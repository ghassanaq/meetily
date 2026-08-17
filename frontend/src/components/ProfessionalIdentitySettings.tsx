'use client'

import { invoke } from '@tauri-apps/api/core'
import { useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import type {
  ProfessionalIdentitySummary,
  ProfessionalIdentityVersion,
  StoredProfessionalIdentityVersion,
} from '@/types/professional-identity'

function blankIdentity(): ProfessionalIdentityVersion {
  const now = new Date().toISOString()
  return {
    schema_version: 1,
    identity: {
      display_name: 'Your name',
      role_title: 'Your role',
      organization: 'Your organization',
      professional_summary: 'Describe your professional role, responsibilities, and operating context.',
    },
    records: [{
      id: crypto.randomUUID(),
      category: 'terms_of_reference',
      title: 'Core responsibilities',
      content: 'Describe the responsibilities and limits that should shape answers in meetings.',
      source: { label: 'Terms of Reference', revision: 'current' },
      updated_at: now,
      valid_until: null,
      conflict_key: null,
      tags: ['responsibilities'],
    }],
    projects: [],
  }
}

function message(error: unknown) {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String((error as { message: unknown }).message)
  }
  return error instanceof Error ? error.message : String(error)
}

export function ProfessionalIdentitySettings() {
  const [identities, setIdentities] = useState<ProfessionalIdentitySummary[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [versions, setVersions] = useState<StoredProfessionalIdentityVersion[]>([])
  const [selectedVersion, setSelectedVersion] = useState('')
  const [identityJson, setIdentityJson] = useState(() => JSON.stringify(blankIdentity(), null, 2))
  const [busy, setBusy] = useState(false)

  const refresh = useCallback(async () => {
    const rows = await invoke<ProfessionalIdentitySummary[]>('identity_list')
    setIdentities(rows)
    setSelectedId(current => current ?? rows[0]?.id ?? null)
  }, [])

  useEffect(() => {
    void refresh().catch(error => toast.error(message(error)))
  }, [refresh])

  useEffect(() => {
    if (!selectedId) return
    void invoke<StoredProfessionalIdentityVersion[]>('identity_list_versions', { identityId: selectedId })
      .then(rows => {
        setVersions(rows)
        setSelectedVersion(rows[0]?.version_hash ?? '')
      })
      .catch(error => toast.error(message(error)))
  }, [selectedId])

  useEffect(() => {
    if (!selectedId || !selectedVersion) return
    void invoke<ProfessionalIdentityVersion>('identity_get', {
      identityId: selectedId,
      versionHash: selectedVersion,
    })
      .then(value => setIdentityJson(JSON.stringify(value, null, 2)))
      .catch(error => toast.error(message(error)))
  }, [selectedId, selectedVersion])

  const run = async (operation: () => Promise<void>) => {
    setBusy(true)
    try {
      await operation()
    } catch (error) {
      toast.error(message(error))
    } finally {
      setBusy(false)
    }
  }

  const save = () => run(async () => {
    const result = selectedId
      ? await invoke<StoredProfessionalIdentityVersion>('identity_create_version', { identityId: selectedId, identityJson })
      : await invoke<StoredProfessionalIdentityVersion>('identity_create', { identityJson })
    await refresh()
    setSelectedId(result.identity_id)
    const rows = await invoke<StoredProfessionalIdentityVersion[]>('identity_list_versions', { identityId: result.identity_id })
    setVersions(rows)
    setSelectedVersion(result.version_hash)
    toast.success(selectedId ? `Saved immutable identity version ${result.seq}.` : 'Professional identity created locally.')
  })

  const selected = identities.find(identity => identity.id === selectedId) ?? null

  return (
    <section className="space-y-4 rounded-lg border bg-white p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-lg font-semibold">Professional Identity</h2>
          <p className="text-sm text-gray-600">Your CV, TOR, authority, stakeholders, commitments, and current project facts. Stored locally as inert, versioned data.</p>
        </div>
        <Button
          variant="outline"
          onClick={() => {
            setSelectedId(null)
            setVersions([])
            setSelectedVersion('')
            setIdentityJson(JSON.stringify(blankIdentity(), null, 2))
          }}
        >
          New identity
        </Button>
      </div>
      <div className="flex flex-wrap gap-2">
        {identities.map(identity => (
          <button
            key={identity.id}
            type="button"
            onClick={() => setSelectedId(identity.id)}
            className={`rounded-md border px-3 py-2 text-left text-sm ${selectedId === identity.id ? 'border-blue-500 bg-blue-50' : 'hover:bg-gray-50'}`}
          >
            <span className="font-medium">{identity.name}</span>
            {identity.retired_at && <span className="ml-2 text-xs text-amber-700">retired</span>}
          </button>
        ))}
      </div>
      {selected && versions.length > 0 && (
        <select value={selectedVersion} onChange={event => setSelectedVersion(event.target.value)} className="rounded-md border px-2 py-1 text-sm">
          {versions.map(version => <option key={version.version_hash} value={version.version_hash}>Version {version.seq}</option>)}
        </select>
      )}
      <Textarea value={identityJson} onChange={event => setIdentityJson(event.target.value)} rows={20} className="font-mono text-xs" />
      <div className="flex flex-wrap gap-2">
        <Button onClick={save} disabled={busy || !identityJson.trim()}>{selectedId ? 'Save new version' : 'Create identity'}</Button>
        {selectedId && selected && !selected.retired_at && (
          <Button variant="outline" disabled={busy} onClick={() => void run(async () => { await invoke('identity_retire', { identityId: selectedId }); await refresh() })}>Retire</Button>
        )}
        {selectedId && selected?.retired_at && (
          <Button variant="outline" disabled={busy} onClick={() => void run(async () => { await invoke('identity_restore', { identityId: selectedId }); await refresh() })}>Restore</Button>
        )}
      </div>
      <p className="text-xs text-gray-500">Saving never mutates an existing version. Expired records are excluded from Live Assist retrieval.</p>
    </section>
  )
}
