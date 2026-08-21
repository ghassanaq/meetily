'use client'

import { invoke } from '@tauri-apps/api/core'
import { FileUp, Plus, RotateCcw, Save, Trash2, X } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'
import type { ProfessionalIdentitySummary, ProfessionalIdentityVersion, StoredProfessionalIdentityVersion } from '@/types/professional-identity'

type IdentityRecord = ProfessionalIdentityVersion['records'][number]
type IdentityRecordCategory = IdentityRecord['category']
type IdentityProject = ProfessionalIdentityVersion['projects'][number]
type IdentityProjectFact = IdentityProject['facts'][number]

export type SavedProfessionalIdentity = { identityId: string; versionHash: string; displayName: string }

type ImportedProfessionalIdentity = StoredProfessionalIdentityVersion & {
  display_name: string
  context_name: string
  record_count: number
}

type Props = {
  onClose: () => void
  onSaved?: (identity: SavedProfessionalIdentity) => void | Promise<void>
}

const CATEGORY_LABELS: Record<IdentityRecordCategory, string> = {
  cv: 'CV & experience',
  terms_of_reference: 'Terms of Reference',
  authority: 'Authority & limits',
  stakeholder: 'Stakeholders',
  commitment: 'Commitments',
  operating_practice: 'Ways of working',
  other: 'Other context',
}

const fieldClass = 'w-full rounded-md border border-white/10 bg-slate-950 px-3 py-2 text-sm text-slate-100 outline-none placeholder:text-slate-600 focus:border-cyan-400/70'
const labelClass = 'mb-1 block text-[10px] font-semibold uppercase tracking-wider text-slate-400'
const timestamp = () => new Date().toISOString()

function blankIdentity(): ProfessionalIdentityVersion {
  return {
    schema_version: 1,
    identity: { display_name: '', role_title: '', organization: '', professional_summary: '' },
    records: [],
    projects: [],
  }
}

function newRecord(category: IdentityRecordCategory): IdentityRecord {
  return {
    id: crypto.randomUUID(),
    category,
    title: CATEGORY_LABELS[category],
    content: '',
    source: { label: CATEGORY_LABELS[category], revision: 'current' },
    updated_at: timestamp(),
    valid_until: null,
    conflict_key: null,
    tags: [],
  }
}

function newProject(): IdentityProject {
  return {
    id: crypto.randomUUID(),
    name: '',
    role: '',
    status: '',
    source: { label: 'Project information', revision: 'current' },
    updated_at: timestamp(),
    valid_until: null,
    tags: [],
    facts: [],
  }
}

function newProjectFact(project: IdentityProject): IdentityProjectFact {
  return {
    id: crypto.randomUUID(),
    content: '',
    source: { ...project.source },
    conflict_key: null,
    tags: [],
  }
}

const tagsToText = (tags: string[]) => tags.join(', ')
const textToTags = (value: string) => value.split(',').map(tag => tag.trimStart())
const cleanTags = (tags: string[]) => [...new Set(tags.map(tag => tag.trim()).filter(Boolean))]
const dateInput = (value: string | null) => value?.slice(0, 10) ?? ''
const expiryFromInput = (value: string) => value ? new Date(`${value}T23:59:59.999Z`).toISOString() : null

function message(error: unknown) {
  if (typeof error === 'object' && error !== null && 'message' in error) return String((error as { message: unknown }).message)
  return error instanceof Error ? error.message : String(error)
}

export function ProfessionalIdentitySettings({ onClose, onSaved }: Props) {
  const [identities, setIdentities] = useState<ProfessionalIdentitySummary[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [versions, setVersions] = useState<StoredProfessionalIdentityVersion[]>([])
  const [selectedVersion, setSelectedVersion] = useState('')
  const [form, setForm] = useState<ProfessionalIdentityVersion>(blankIdentity)
  const [recordKind, setRecordKind] = useState<IdentityRecordCategory>('terms_of_reference')
  const [busy, setBusy] = useState(false)

  const refresh = useCallback(async (preferredId?: string) => {
    const rows = await invoke<ProfessionalIdentitySummary[]>('identity_list')
    setIdentities(rows)
    setSelectedId(current => preferredId ?? current ?? rows[0]?.id ?? null)
  }, [])

  useEffect(() => { void refresh().catch(error => toast.error(message(error))) }, [refresh])

  useEffect(() => {
    if (!selectedId) return
    void invoke<StoredProfessionalIdentityVersion[]>('identity_list_versions', { identityId: selectedId })
      .then(rows => { setVersions(rows); setSelectedVersion(rows[0]?.version_hash ?? '') })
      .catch(error => toast.error(message(error)))
  }, [selectedId])

  useEffect(() => {
    if (!selectedId || !selectedVersion) return
    void invoke<ProfessionalIdentityVersion>('identity_get', { identityId: selectedId, versionHash: selectedVersion })
      .then(setForm)
      .catch(error => toast.error(message(error)))
  }, [selectedId, selectedVersion])

  const run = async (operation: () => Promise<void>) => {
    setBusy(true)
    try { await operation() } catch (error) { toast.error(message(error)) } finally { setBusy(false) }
  }

  const startNew = () => {
    setSelectedId(null)
    setVersions([])
    setSelectedVersion('')
    setForm(blankIdentity())
  }

  const updateRecord = (id: string, update: (record: IdentityRecord) => IdentityRecord) => {
    setForm(current => ({ ...current, records: current.records.map(record => record.id === id ? { ...update(record), updated_at: timestamp() } : record) }))
  }

  const updateProject = (id: string, update: (project: IdentityProject) => IdentityProject) => {
    setForm(current => ({ ...current, projects: current.projects.map(project => project.id === id ? { ...update(project), updated_at: timestamp() } : project) }))
  }

  const save = () => run(async () => {
    const normalizedForm: ProfessionalIdentityVersion = {
      ...form,
      records: form.records.map(record => ({ ...record, tags: cleanTags(record.tags) })),
      projects: form.projects.map(project => ({
        ...project,
        tags: cleanTags(project.tags),
        facts: project.facts.map(fact => ({ ...fact, tags: cleanTags(fact.tags) })),
      })),
    }
    const identityJson = JSON.stringify(normalizedForm)
    const result = selectedId
      ? await invoke<StoredProfessionalIdentityVersion>('identity_create_version', { identityId: selectedId, identityJson })
      : await invoke<StoredProfessionalIdentityVersion>('identity_create', { identityJson })
    await refresh(result.identity_id)
    setSelectedId(result.identity_id)
    setSelectedVersion(result.version_hash)
    toast.success(selectedId ? `Saved identity version ${result.seq}.` : 'Professional identity created locally.')
    await onSaved?.({ identityId: result.identity_id, versionHash: result.version_hash, displayName: form.identity.display_name })
  })

  const importContext = () => run(async () => {
    const result = await invoke<ImportedProfessionalIdentity | null>('identity_import_context_manifest')
    if (!result) return
    await refresh(result.identity_id)
    setSelectedId(result.identity_id)
    setSelectedVersion(result.version_hash)
    toast.success(`Imported ${result.record_count} sections from ${result.context_name}.`)
    await onSaved?.({ identityId: result.identity_id, versionHash: result.version_hash, displayName: result.display_name })
  })

  const selected = identities.find(identity => identity.id === selectedId) ?? null

  return (
    <div className="flex h-full min-h-0 flex-col bg-slate-950 text-slate-100">
      <header className="flex shrink-0 items-start gap-3 border-b border-white/10 px-5 py-4">
        <div>
          <h2 className="text-lg font-semibold">Professional identity</h2>
          <p className="mt-0.5 text-xs text-slate-400">The facts Live Assist can use when answering as you. Everything stays local until a question is sent.</p>
        </div>
        <button type="button" disabled={busy} onClick={importContext} className="ml-auto flex items-center gap-1.5 rounded-md border border-cyan-400/30 px-3 py-1.5 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/10 disabled:opacity-40"><FileUp className="h-3.5 w-3.5" />Import Markdown context</button>
        <button type="button" onClick={startNew} className="rounded-md border border-white/10 px-3 py-1.5 text-xs font-semibold hover:bg-white/10">New identity</button>
        <button type="button" onClick={onClose} className="rounded p-1.5 text-slate-400 hover:bg-white/10 hover:text-white" aria-label="Close identity manager"><X className="h-5 w-5" /></button>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        {identities.length > 0 && (
          <div className="mb-5">
            <span className={labelClass}>Saved identities</span>
            <div className="flex flex-wrap gap-2">
              {identities.map(identity => (
                <button key={identity.id} type="button" onClick={() => { setVersions([]); setSelectedVersion(''); setSelectedId(identity.id) }} className={`rounded-md border px-3 py-2 text-left text-xs ${selectedId === identity.id ? 'border-cyan-400 bg-cyan-400/10 text-cyan-100' : 'border-white/10 text-slate-300 hover:bg-white/5'}`}>
                  <span className="font-semibold">{identity.name}</span>{identity.retired_at && <span className="ml-2 text-amber-300">retired</span>}
                </button>
              ))}
            </div>
            {selected && versions.length > 0 && (
              <select value={selectedVersion} onChange={event => setSelectedVersion(event.target.value)} className={`${fieldClass} mt-2 max-w-48 text-xs`}>
                {versions.map(version => <option key={version.version_hash} value={version.version_hash}>Version {version.seq}</option>)}
              </select>
            )}
          </div>
        )}

        <section className="mb-5 rounded-lg border border-white/10 bg-slate-900/60 p-4">
          <h3 className="mb-3 text-sm font-semibold text-cyan-200">Who you are</h3>
          <div className="grid gap-3 md:grid-cols-3">
            <label><span className={labelClass}>Your name</span><input value={form.identity.display_name} onChange={event => setForm(current => ({ ...current, identity: { ...current.identity, display_name: event.target.value } }))} className={fieldClass} placeholder="Ghassan Aqrabawi" /></label>
            <label><span className={labelClass}>Role title</span><input value={form.identity.role_title} onChange={event => setForm(current => ({ ...current, identity: { ...current.identity, role_title: event.target.value } }))} className={fieldClass} placeholder="Head of Mission" /></label>
            <label><span className={labelClass}>Organization</span><input value={form.identity.organization} onChange={event => setForm(current => ({ ...current, identity: { ...current.identity, organization: event.target.value } }))} className={fieldClass} placeholder="Organization or mission" /></label>
          </div>
          <label className="mt-3 block"><span className={labelClass}>Professional summary</span><textarea value={form.identity.professional_summary} onChange={event => setForm(current => ({ ...current, identity: { ...current.identity, professional_summary: event.target.value } }))} rows={4} className={fieldClass} placeholder="Describe your role, experience, responsibilities, and operating context in your own words." /></label>
        </section>

        <section className="mb-5">
          <div className="mb-2 flex flex-wrap items-end gap-2">
            <div className="mr-auto"><h3 className="text-sm font-semibold text-cyan-200">Role knowledge</h3><p className="text-xs text-slate-500">Add the parts of your CV, TOR, authority, stakeholders, and commitments that should shape answers.</p></div>
            <select value={recordKind} onChange={event => setRecordKind(event.target.value as IdentityRecordCategory)} className={`${fieldClass} w-auto text-xs`}>{Object.entries(CATEGORY_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select>
            <button type="button" onClick={() => setForm(current => ({ ...current, records: [...current.records, newRecord(recordKind)] }))} className="flex items-center gap-1 rounded-md bg-cyan-500 px-3 py-2 text-xs font-bold text-slate-950"><Plus className="h-3.5 w-3.5" />Add section</button>
          </div>
          <div className="space-y-3">
            {form.records.map(record => (
              <article key={record.id} className="rounded-lg border border-white/10 bg-slate-900/60 p-4">
                <div className="grid gap-3 md:grid-cols-[180px_1fr_auto]">
                  <label><span className={labelClass}>Section</span><select value={record.category} onChange={event => updateRecord(record.id, item => ({ ...item, category: event.target.value as IdentityRecordCategory }))} className={fieldClass}>{Object.entries(CATEGORY_LABELS).map(([value, label]) => <option key={value} value={value}>{label}</option>)}</select></label>
                  <label><span className={labelClass}>Title</span><input value={record.title} onChange={event => updateRecord(record.id, item => ({ ...item, title: event.target.value }))} className={fieldClass} /></label>
                  <button type="button" onClick={() => setForm(current => ({ ...current, records: current.records.filter(item => item.id !== record.id) }))} className="mt-5 rounded p-2 text-slate-500 hover:bg-red-500/10 hover:text-red-300" aria-label={`Remove ${record.title}`}><Trash2 className="h-4 w-4" /></button>
                </div>
                <label className="mt-3 block"><span className={labelClass}>What Live Assist should know</span><textarea value={record.content} onChange={event => updateRecord(record.id, item => ({ ...item, content: event.target.value }))} rows={4} className={fieldClass} placeholder="Paste or summarize the relevant facts. Be explicit about limits and decision authority." /></label>
                <div className="mt-3 grid gap-3 md:grid-cols-4">
                  <label><span className={labelClass}>Source</span><input value={record.source.label} onChange={event => updateRecord(record.id, item => ({ ...item, source: { ...item.source, label: event.target.value } }))} className={fieldClass} placeholder="CV, TOR, policy…" /></label>
                  <label><span className={labelClass}>Revision</span><input value={record.source.revision} onChange={event => updateRecord(record.id, item => ({ ...item, source: { ...item.source, revision: event.target.value } }))} className={fieldClass} placeholder="current" /></label>
                  <label><span className={labelClass}>Tags</span><input value={tagsToText(record.tags)} onChange={event => updateRecord(record.id, item => ({ ...item, tags: textToTags(event.target.value) }))} className={fieldClass} placeholder="staff, budget, duty of care" /></label>
                  <label><span className={labelClass}>Valid until (optional)</span><input type="date" value={dateInput(record.valid_until)} onChange={event => updateRecord(record.id, item => ({ ...item, valid_until: expiryFromInput(event.target.value) }))} className={fieldClass} /></label>
                </div>
              </article>
            ))}
            {form.records.length === 0 && <p className="rounded-lg border border-dashed border-white/10 px-4 py-5 text-center text-xs text-slate-500">No role knowledge added yet.</p>}
          </div>
        </section>

        <section>
          <div className="mb-2 flex items-end gap-3"><div className="mr-auto"><h3 className="text-sm font-semibold text-cyan-200">Current projects</h3><p className="text-xs text-slate-500">Add only facts you want the model to rely on in meetings. Dates and amounts should come from a named source.</p></div><button type="button" onClick={() => setForm(current => ({ ...current, projects: [...current.projects, newProject()] }))} className="flex items-center gap-1 rounded-md bg-violet-500 px-3 py-2 text-xs font-bold text-white"><Plus className="h-3.5 w-3.5" />Add project</button></div>
          <div className="space-y-3">
            {form.projects.map(project => (
              <article key={project.id} className="rounded-lg border border-violet-400/20 bg-slate-900/60 p-4">
                <div className="grid gap-3 md:grid-cols-[1fr_1fr_1fr_auto]">
                  <label><span className={labelClass}>Project</span><input value={project.name} onChange={event => updateProject(project.id, item => ({ ...item, name: event.target.value }))} className={fieldClass} placeholder="Project Atlas" /></label>
                  <label><span className={labelClass}>Your role</span><input value={project.role} onChange={event => updateProject(project.id, item => ({ ...item, role: event.target.value }))} className={fieldClass} placeholder="Project sponsor" /></label>
                  <label><span className={labelClass}>Current status</span><input value={project.status} onChange={event => updateProject(project.id, item => ({ ...item, status: event.target.value }))} className={fieldClass} placeholder="On track / delayed / planning" /></label>
                  <button type="button" onClick={() => setForm(current => ({ ...current, projects: current.projects.filter(item => item.id !== project.id) }))} className="mt-5 rounded p-2 text-slate-500 hover:bg-red-500/10 hover:text-red-300" aria-label={`Remove ${project.name || 'project'}`}><Trash2 className="h-4 w-4" /></button>
                </div>
                <div className="mt-3 grid gap-3 md:grid-cols-4">
                  <label><span className={labelClass}>Source</span><input value={project.source.label} onChange={event => updateProject(project.id, item => ({ ...item, source: { ...item.source, label: event.target.value } }))} className={fieldClass} /></label>
                  <label><span className={labelClass}>Revision</span><input value={project.source.revision} onChange={event => updateProject(project.id, item => ({ ...item, source: { ...item.source, revision: event.target.value } }))} className={fieldClass} /></label>
                  <label><span className={labelClass}>Tags</span><input value={tagsToText(project.tags)} onChange={event => updateProject(project.id, item => ({ ...item, tags: textToTags(event.target.value) }))} className={fieldClass} placeholder="delivery, partner" /></label>
                  <label><span className={labelClass}>Valid until (optional)</span><input type="date" value={dateInput(project.valid_until)} onChange={event => updateProject(project.id, item => ({ ...item, valid_until: expiryFromInput(event.target.value) }))} className={fieldClass} /></label>
                </div>
                <div className="mt-4 space-y-2">
                  {project.facts.map((fact, factIndex) => (
                    <div key={fact.id} className="rounded-md border border-white/10 p-3">
                      <div className="flex items-start gap-2"><label className="flex-1"><span className={labelClass}>Project fact {factIndex + 1}</span><textarea value={fact.content} onChange={event => updateProject(project.id, item => ({ ...item, facts: item.facts.map(candidate => candidate.id === fact.id ? { ...candidate, content: event.target.value } : candidate) }))} rows={2} className={fieldClass} placeholder="State one current, verifiable fact." /></label><button type="button" onClick={() => updateProject(project.id, item => ({ ...item, facts: item.facts.filter(candidate => candidate.id !== fact.id) }))} className="mt-5 rounded p-2 text-slate-500 hover:text-red-300"><Trash2 className="h-4 w-4" /></button></div>
                      <div className="mt-2 grid gap-2 md:grid-cols-3"><input value={fact.source.label} onChange={event => updateProject(project.id, item => ({ ...item, facts: item.facts.map(candidate => candidate.id === fact.id ? { ...candidate, source: { ...candidate.source, label: event.target.value } } : candidate) }))} className={fieldClass} placeholder="Source label" /><input value={fact.source.revision} onChange={event => updateProject(project.id, item => ({ ...item, facts: item.facts.map(candidate => candidate.id === fact.id ? { ...candidate, source: { ...candidate.source, revision: event.target.value } } : candidate) }))} className={fieldClass} placeholder="Revision" /><input value={tagsToText(fact.tags)} onChange={event => updateProject(project.id, item => ({ ...item, facts: item.facts.map(candidate => candidate.id === fact.id ? { ...candidate, tags: textToTags(event.target.value) } : candidate) }))} className={fieldClass} placeholder="Tags, comma separated" /></div>
                    </div>
                  ))}
                  <button type="button" onClick={() => updateProject(project.id, item => ({ ...item, facts: [...item.facts, newProjectFact(item)] }))} className="flex items-center gap-1 rounded-md border border-white/10 px-3 py-1.5 text-xs font-semibold text-slate-300 hover:bg-white/5"><Plus className="h-3.5 w-3.5" />Add project fact</button>
                </div>
              </article>
            ))}
            {form.projects.length === 0 && <p className="rounded-lg border border-dashed border-white/10 px-4 py-5 text-center text-xs text-slate-500">No current projects added yet.</p>}
          </div>
        </section>
      </div>

      <footer className="flex shrink-0 items-center gap-2 border-t border-white/10 bg-slate-950 px-5 py-3">
        <p className="mr-auto text-[10px] text-slate-500">Saving creates an immutable version. Expired facts are automatically excluded from answers.</p>
        {selectedId && selected && !selected.retired_at && <button type="button" disabled={busy} onClick={() => void run(async () => { await invoke('identity_retire', { identityId: selectedId }); await refresh() })} className="flex items-center gap-1 rounded-md border border-white/10 px-3 py-2 text-xs font-semibold text-slate-300 hover:bg-white/5 disabled:opacity-40"><Trash2 className="h-3.5 w-3.5" />Retire</button>}
        {selectedId && selected?.retired_at && <button type="button" disabled={busy} onClick={() => void run(async () => { await invoke('identity_restore', { identityId: selectedId }); await refresh() })} className="flex items-center gap-1 rounded-md border border-white/10 px-3 py-2 text-xs font-semibold text-slate-300 hover:bg-white/5 disabled:opacity-40"><RotateCcw className="h-3.5 w-3.5" />Restore</button>}
        <button type="button" onClick={save} disabled={busy} className="flex items-center gap-1.5 rounded-md bg-cyan-500 px-4 py-2 text-xs font-bold text-slate-950 disabled:opacity-40"><Save className="h-3.5 w-3.5" />{busy ? 'Saving…' : selectedId ? 'Save new version' : 'Create & use identity'}</button>
      </footer>
    </div>
  )
}
