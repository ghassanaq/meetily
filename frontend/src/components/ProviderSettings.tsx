'use client'

import { invoke } from '@tauri-apps/api/core'
import { CheckCircle2, KeyRound, LoaderCircle, Plus, Save, ShieldCheck, Trash2, Wifi } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { toast } from 'sonner'

type ProviderKind = 'deepseek' | 'kimi' | 'openai' | 'custom'

type ProviderSummary = {
  id: string
  displayName: string
  providerKind: ProviderKind
  endpoint: string
  model: string
  isActive: boolean
  keyConfigured: boolean
  lastTestedAt: string | null
  testCurrent: boolean
}

type ProviderForm = {
  id: string | null
  displayName: string
  providerKind: ProviderKind
  endpoint: string
  model: string
}

const PRESETS: Record<ProviderKind, Omit<ProviderForm, 'id'>> = {
  deepseek: {
    displayName: 'DeepSeek',
    providerKind: 'deepseek',
    endpoint: 'https://api.deepseek.com/chat/completions',
    model: 'deepseek-v4-pro',
  },
  kimi: {
    displayName: 'Kimi / Moonshot',
    providerKind: 'kimi',
    endpoint: 'https://api.moonshot.ai/v1/chat/completions',
    model: 'kimi-k3',
  },
  openai: {
    displayName: 'OpenAI',
    providerKind: 'openai',
    endpoint: 'https://api.openai.com/v1/chat/completions',
    model: 'gpt-5.2',
  },
  custom: {
    displayName: 'Custom provider',
    providerKind: 'custom',
    endpoint: 'https://',
    model: '',
  },
}

const fieldClass = 'w-full rounded-md border border-gray-300 bg-white px-3 py-2 text-sm text-gray-900 outline-none transition focus:border-blue-500 focus:ring-2 focus:ring-blue-100 disabled:bg-gray-100'
const labelClass = 'mb-1.5 block text-xs font-semibold text-gray-700'

function formatError(error: unknown) {
  if (error instanceof Error) return error.message
  return String(error)
}

function formFromProvider(provider: ProviderSummary): ProviderForm {
  return {
    id: provider.id,
    displayName: provider.displayName,
    providerKind: provider.providerKind,
    endpoint: provider.endpoint,
    model: provider.model,
  }
}

function newForm(kind: ProviderKind = 'deepseek'): ProviderForm {
  return { id: null, ...PRESETS[kind] }
}

export function ProviderSettings() {
  const [providers, setProviders] = useState<ProviderSummary[]>([])
  const [form, setForm] = useState<ProviderForm>(() => newForm())
  const [apiKey, setApiKey] = useState('')
  const [busyAction, setBusyAction] = useState<string | null>(null)

  const selected = providers.find(provider => provider.id === form.id) ?? null
  const dirty = useMemo(() => {
    if (!selected) return true
    return apiKey.length > 0
      || selected.displayName !== form.displayName
      || selected.providerKind !== form.providerKind
      || selected.endpoint !== form.endpoint
      || selected.model !== form.model
  }, [apiKey, form, selected])

  const refresh = useCallback(async (preferredId?: string) => {
    const rows = await invoke<ProviderSummary[]>('live_assist_provider_list')
    setProviders(rows)
    const next = rows.find(provider => provider.id === preferredId)
      ?? rows.find(provider => provider.isActive)
      ?? rows[0]
    if (next) setForm(formFromProvider(next))
  }, [])

  useEffect(() => {
    void refresh().catch(error => toast.error(formatError(error)))
  }, [refresh])

  const run = async (action: string, operation: () => Promise<void>) => {
    setBusyAction(action)
    try {
      await operation()
    } catch (error) {
      toast.error(formatError(error))
    } finally {
      setBusyAction(null)
    }
  }

  const save = () => run('save', async () => {
    const saved = await invoke<ProviderSummary>('live_assist_provider_save', {
      request: {
        id: form.id,
        displayName: form.displayName,
        providerKind: form.providerKind,
        endpoint: form.endpoint,
        model: form.model,
        apiKey: apiKey.trim() || null,
      },
    })
    setApiKey('')
    await refresh(saved.id)
    toast.success(saved.keyConfigured ? 'Provider saved. The API key is stored securely.' : 'Provider saved without an API key.')
  })

  const test = () => {
    if (!selected) return
    void run('test', async () => {
      const tested = await invoke<ProviderSummary>('live_assist_provider_test', { providerId: selected.id })
      await refresh(tested.id)
      toast.success('Connection successful. This provider can now be activated.')
    })
  }

  const activate = () => {
    if (!selected) return
    void run('activate', async () => {
      const activated = await invoke<ProviderSummary>('live_assist_provider_activate', { providerId: selected.id })
      await refresh(activated.id)
      toast.success(`${activated.displayName} is now active for Live Assist.`)
    })
  }

  const remove = () => {
    if (!selected || selected.isActive) return
    if (!window.confirm(`Remove ${selected.displayName}? Its saved API key will also be deleted from Windows Credential Manager.`)) return
    void run('delete', async () => {
      await invoke('live_assist_provider_delete', { providerId: selected.id })
      const rows = await invoke<ProviderSummary[]>('live_assist_provider_list')
      setProviders(rows)
      const next = rows.find(provider => provider.isActive) ?? rows[0]
      setForm(next ? formFromProvider(next) : newForm())
      setApiKey('')
      toast.success('Provider and saved key removed.')
    })
  }

  const choosePreset = (providerKind: ProviderKind) => {
    setForm(current => ({ id: current.id, ...PRESETS[providerKind] }))
  }

  const busy = busyAction !== null

  return (
    <div className="mt-6 grid gap-6 lg:grid-cols-[320px_minmax(0,1fr)]">
      <section className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm">
        <div className="mb-4 flex items-start gap-3">
          <div>
            <h2 className="text-lg font-semibold text-gray-900">Live Assist providers</h2>
            <p className="mt-1 text-xs leading-5 text-gray-500">Only the active provider receives cloud-mode questions and bounded context.</p>
          </div>
          <button
            type="button"
            onClick={() => { setForm(newForm()); setApiKey('') }}
            className="ml-auto rounded-md bg-blue-600 p-2 text-white hover:bg-blue-700"
            aria-label="Add provider"
          >
            <Plus className="h-4 w-4" />
          </button>
        </div>

        <div className="space-y-2">
          {providers.map(provider => (
            <button
              key={provider.id}
              type="button"
              onClick={() => { setForm(formFromProvider(provider)); setApiKey('') }}
              className={`w-full rounded-lg border p-3 text-left transition ${form.id === provider.id ? 'border-blue-500 bg-blue-50' : 'border-gray-200 hover:bg-gray-50'}`}
            >
              <div className="flex items-center gap-2">
                <span className="min-w-0 flex-1 truncate text-sm font-semibold text-gray-900">{provider.displayName}</span>
                {provider.isActive && <span className="rounded-full bg-emerald-100 px-2 py-0.5 text-[10px] font-bold uppercase tracking-wide text-emerald-700">Active</span>}
              </div>
              <p className="mt-1 truncate text-xs text-gray-500">{provider.model}</p>
              <div className="mt-2 flex items-center gap-3 text-[11px]">
                <span className={provider.keyConfigured ? 'text-emerald-700' : 'text-amber-700'}>{provider.keyConfigured ? 'Key saved securely' : 'No key saved'}</span>
                {provider.testCurrent && <span className="flex items-center gap-1 text-blue-700"><ShieldCheck className="h-3 w-3" />Tested</span>}
              </div>
            </button>
          ))}
          {providers.length === 0 && (
            <div className="rounded-lg border border-dashed border-gray-300 p-5 text-center text-xs text-gray-500">No UI-managed provider yet. Add one to replace `.env` switching.</div>
          )}
        </div>
      </section>

      <section className="rounded-xl border border-gray-200 bg-white p-6 shadow-sm">
        <div className="mb-5 flex items-start gap-3">
          <div>
            <h2 className="text-lg font-semibold text-gray-900">{selected ? 'Edit provider' : 'Add provider'}</h2>
            <p className="mt-1 text-xs text-gray-500">Saved keys never return to this screen. Leave the key field empty to keep the existing key.</p>
          </div>
          {selected?.isActive && <div className="ml-auto flex items-center gap-1.5 rounded-full bg-emerald-100 px-3 py-1 text-xs font-semibold text-emerald-700"><CheckCircle2 className="h-4 w-4" />Active for Live Assist</div>}
        </div>

        <div className="grid gap-4 md:grid-cols-2">
          <label>
            <span className={labelClass}>Provider preset</span>
            <select value={form.providerKind} onChange={event => choosePreset(event.target.value as ProviderKind)} className={fieldClass}>
              <option value="deepseek">DeepSeek</option>
              <option value="kimi">Kimi / Moonshot</option>
              <option value="openai">OpenAI</option>
              <option value="custom">Custom OpenAI-compatible</option>
            </select>
          </label>
          <label>
            <span className={labelClass}>Display name</span>
            <input value={form.displayName} onChange={event => setForm(current => ({ ...current, displayName: event.target.value }))} className={fieldClass} />
          </label>
          <label className="md:col-span-2">
            <span className={labelClass}>API endpoint</span>
            <input value={form.endpoint} onChange={event => setForm(current => ({ ...current, endpoint: event.target.value }))} className={fieldClass} spellCheck={false} />
            <span className="mt-1 block text-[11px] text-gray-500">HTTPS is required except for a provider running on this PC.</span>
          </label>
          <label>
            <span className={labelClass}>Model</span>
            <input value={form.model} onChange={event => setForm(current => ({ ...current, model: event.target.value }))} className={fieldClass} spellCheck={false} />
          </label>
          <label>
            <span className={labelClass}>{selected?.keyConfigured ? 'Replace API key' : 'API key'}</span>
            <div className="relative">
              <KeyRound className="pointer-events-none absolute left-3 top-2.5 h-4 w-4 text-gray-400" />
              <input
                type="password"
                value={apiKey}
                onChange={event => setApiKey(event.target.value)}
                className={`${fieldClass} pl-9`}
                placeholder={selected?.keyConfigured ? 'Leave empty to keep saved key' : 'Paste key, then save'}
                autoComplete="new-password"
                spellCheck={false}
              />
            </div>
          </label>
        </div>

        {selected && dirty && selected.isActive && (
          <p className="mt-4 rounded-md border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800">Saving a changed endpoint, model, preset, or key deactivates this provider until Test Connection succeeds again.</p>
        )}

        <div className="mt-6 flex flex-wrap items-center gap-2 border-t border-gray-100 pt-5">
          <button type="button" disabled={busy} onClick={() => void save()} className="flex items-center gap-2 rounded-md bg-blue-600 px-4 py-2 text-sm font-semibold text-white hover:bg-blue-700 disabled:opacity-50">
            {busyAction === 'save' ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Save className="h-4 w-4" />}
            Save securely
          </button>
          <button type="button" disabled={busy || !selected || dirty || !selected.keyConfigured} onClick={test} className="flex items-center gap-2 rounded-md border border-gray-300 px-4 py-2 text-sm font-semibold text-gray-700 hover:bg-gray-50 disabled:opacity-40" title={dirty ? 'Save changes before testing' : undefined}>
            {busyAction === 'test' ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <Wifi className="h-4 w-4" />}
            Test Connection
          </button>
          <button type="button" disabled={busy || !selected || dirty || !selected.testCurrent || selected.isActive} onClick={activate} className="flex items-center gap-2 rounded-md border border-emerald-300 px-4 py-2 text-sm font-semibold text-emerald-700 hover:bg-emerald-50 disabled:opacity-40">
            {busyAction === 'activate' ? <LoaderCircle className="h-4 w-4 animate-spin" /> : <CheckCircle2 className="h-4 w-4" />}
            Activate
          </button>
          {selected && (
            <button type="button" disabled={busy || selected.isActive} onClick={remove} className="ml-auto flex items-center gap-2 rounded-md border border-red-200 px-4 py-2 text-sm font-semibold text-red-700 hover:bg-red-50 disabled:opacity-40" title={selected.isActive ? 'Activate another provider before removing this one' : undefined}>
              <Trash2 className="h-4 w-4" />Remove
            </button>
          )}
        </div>
      </section>
    </div>
  )
}
