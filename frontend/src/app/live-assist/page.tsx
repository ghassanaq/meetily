'use client'

import { type MouseEvent as ReactMouseEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { LogicalSize, type PhysicalSize } from '@tauri-apps/api/dpi'
import {
  ChevronLeft,
  ChevronRight,
  Cloud,
  EyeOff,
  LoaderCircle,
  Lock,
  Mic,
  Radio,
  RefreshCw,
  Sparkles,
  UserRoundCog,
  X,
} from 'lucide-react'
import { ProfessionalIdentitySettings, type SavedProfessionalIdentity } from '@/components/ProfessionalIdentitySettings'

type ExchangeStatus =
  | 'capturing'
  | 'transcribing'
  | 'transcript_only'
  | 'requesting'
  | 'streaming'
  | 'complete'
  | 'interrupted'
  | 'failed'

type AssistExchange = {
  id: string
  ordinal: number
  kind: 'new_question' | 'follow_up'
  parentExchangeId: string | null
  contextGeneration: number
  dataClass: 'private' | 'standard'
  status: ExchangeStatus
  question: string
  answer: string
  answerWordCount: number | null
  answerFormatWarnings: string[]
  detail: string
  detailStatus: ExchangeStatus | null
  detailTruncated: boolean
  detailError: string | null
  error: string | null
  profileId: string | null
  profileVersionHash: string | null
  playbookId: string | null
  identityId: string | null
  identityVersionHash: string | null
  groundingSources: GroundingSource[]
  buildRevision: string
  createdAt: string
  timings: {
    captureMs: number | null
    transcriptionMs: number | null
    requestToFirstTokenMs: number | null
    requestToCompleteMs: number | null
    stopToFirstDeltaMs: number | null
    firstDeltaAtUnixMs: number | null
    firstDeltaToPaintMs: number | null
    stopToVisibleTextMs: number | null
  }
}

type AssistSnapshot = {
  armed: boolean
  receiving: boolean
  stalled: boolean
  levelRms: number
  cloudEnabled: boolean
  providerConfigured: boolean
  providerName: string | null
  modelName: string | null
  streamError: string | null
  selectedProfileId: string | null
  selectedProfileVersionHash: string | null
  selectedPlaybookId: string | null
  selectedIdentityId: string | null
  selectedIdentityVersionHash: string | null
  currentExchangeId: string | null
  capturing: boolean
  contextGeneration: number
  stallCount: number
  exchanges: AssistExchange[]
  captureShortcut: string
  followUpShortcut: string
}

type ProfileChoice = {
  profileId: string
  profileVersionHash: string
  profileName: string
  playbooks: Array<{ id: string; name: string }>
}

type IdentityChoice = {
  identityId: string
  identityVersionHash: string
  identityName: string
  roleTitle: string
}

type GroundingSource = {
  recordId: string
  label: string
  revision: string
  updatedAt: string
}

const EMPTY_SNAPSHOT: AssistSnapshot = {
  armed: false,
  receiving: false,
  stalled: false,
  levelRms: 0,
  cloudEnabled: false,
  providerConfigured: false,
  providerName: null,
  modelName: null,
  streamError: null,
  selectedProfileId: null,
  selectedProfileVersionHash: null,
  selectedPlaybookId: null,
  selectedIdentityId: null,
  selectedIdentityVersionHash: null,
  currentExchangeId: null,
  capturing: false,
  contextGeneration: 0,
  stallCount: 0,
  exchanges: [],
  captureShortcut: 'Ctrl+Alt+Space',
  followUpShortcut: 'Ctrl+Alt+Shift+Space',
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : String(error)
}

function StatePill({ snapshot }: { snapshot: AssistSnapshot }) {
  if (!snapshot.armed) {
    return <span className="whitespace-nowrap rounded-full bg-slate-200 px-2.5 py-1 text-xs font-semibold text-slate-700">Not armed</span>
  }
  if (snapshot.stalled) {
    return <span className="whitespace-nowrap rounded-full bg-red-100 px-2.5 py-1 text-xs font-semibold text-red-700">Audio fault</span>
  }
  if (!snapshot.receiving) {
    return <span className="whitespace-nowrap rounded-full bg-amber-100 px-2.5 py-1 text-xs font-semibold text-amber-700">Armed · waiting</span>
  }
  return <span className="whitespace-nowrap rounded-full bg-emerald-100 px-2.5 py-1 text-xs font-semibold text-emerald-700">Armed · receiving</span>
}

function compactModelName(model: string | null) {
  if (!model) return 'model'
  return model
    .replace(/^deepseek-/i, '')
    .split('-')
    .map(part => part.length <= 3 ? part.toUpperCase() : `${part[0].toUpperCase()}${part.slice(1)}`)
    .join(' ')
}

export default function LiveAssistPage() {
  const [snapshot, setSnapshot] = useState(EMPTY_SNAPSHOT)
  const [profiles, setProfiles] = useState<ProfileChoice[]>([])
  const [identities, setIdentities] = useState<IdentityChoice[]>([])
  const [selectedProfile, setSelectedProfile] = useState('')
  const [selectedIdentity, setSelectedIdentity] = useState('')
  const [booting, setBooting] = useState(true)
  const [actionError, setActionError] = useState<string | null>(null)
  const [identityManagerOpen, setIdentityManagerOpen] = useState(false)
  const paintMeasurementPending = useRef(new Set<string>())
  const previousWindowSize = useRef<PhysicalSize | null>(null)

  const refresh = useCallback(async () => {
    const next = await invoke<AssistSnapshot>('assist_get_snapshot')
    setSnapshot(next)
  }, [])

  useEffect(() => {
    let active = true
    const boot = async () => {
      try {
        const isVisible = await getCurrentWindow().isVisible()
        const [next, availableProfiles, availableIdentities] = await Promise.all([
          invoke<AssistSnapshot>(isVisible ? 'assist_arm' : 'assist_get_snapshot'),
          invoke<ProfileChoice[]>('assist_list_profiles'),
          invoke<IdentityChoice[]>('assist_list_identities'),
        ])
        if (!active) return
        setSnapshot(next)
        setProfiles(availableProfiles)
        setIdentities(availableIdentities)
        if (next.selectedProfileId && next.selectedProfileVersionHash && next.selectedPlaybookId) {
          setSelectedProfile(`${next.selectedProfileId}|${next.selectedProfileVersionHash}|${next.selectedPlaybookId}`)
        }
        if (next.selectedIdentityId && next.selectedIdentityVersionHash) {
          setSelectedIdentity(`${next.selectedIdentityId}|${next.selectedIdentityVersionHash}`)
        }
      } catch (error) {
        if (active) setActionError(errorMessage(error))
      } finally {
        if (active) setBooting(false)
      }
    }
    void boot()
    const unlisten = listen('live-assist://show', () => {
      void invoke<AssistSnapshot>('assist_arm')
        .then(setSnapshot)
        .catch(error => setActionError(errorMessage(error)))
    })
    const interval = window.setInterval(() => {
      void refresh().catch(error => setActionError(errorMessage(error)))
    }, 250)
    return () => {
      active = false
      window.clearInterval(interval)
      void unlisten.then(dispose => dispose())
    }
  }, [refresh])

  useEffect(() => {
    const discardOnEscape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || !snapshot.capturing) return
      event.preventDefault()
      void invoke<AssistSnapshot>('assist_discard_capture')
        .then(setSnapshot)
        .catch(error => setActionError(errorMessage(error)))
    }
    window.addEventListener('keydown', discardOnEscape)
    return () => window.removeEventListener('keydown', discardOnEscape)
  }, [snapshot.capturing])

  useEffect(() => {
    if (snapshot.selectedProfileId && snapshot.selectedProfileVersionHash && snapshot.selectedPlaybookId) {
      setSelectedProfile(`${snapshot.selectedProfileId}|${snapshot.selectedProfileVersionHash}|${snapshot.selectedPlaybookId}`)
    } else {
      setSelectedProfile('')
    }
  }, [snapshot.selectedPlaybookId, snapshot.selectedProfileId, snapshot.selectedProfileVersionHash])

  useEffect(() => {
    if (snapshot.selectedIdentityId && snapshot.selectedIdentityVersionHash) {
      setSelectedIdentity(`${snapshot.selectedIdentityId}|${snapshot.selectedIdentityVersionHash}`)
    } else {
      setSelectedIdentity('')
    }
  }, [snapshot.selectedIdentityId, snapshot.selectedIdentityVersionHash])

  const currentIndex = useMemo(
    () => snapshot.exchanges.findIndex(exchange => exchange.id === snapshot.currentExchangeId),
    [snapshot.currentExchangeId, snapshot.exchanges],
  )
  const current = currentIndex >= 0 ? snapshot.exchanges[currentIndex] : null

  useEffect(() => {
    const timings = current?.timings
    if (!current?.answer
      || timings?.firstDeltaAtUnixMs === null
      || timings?.firstDeltaAtUnixMs === undefined
      || timings.stopToFirstDeltaMs === null
      || timings.firstDeltaToPaintMs !== null
      || paintMeasurementPending.current.has(current.id)) {
      return
    }
    const firstDeltaToPaintMs = Math.max(0, Date.now() - timings.firstDeltaAtUnixMs)
    const stopToVisibleTextMs = timings.stopToFirstDeltaMs + firstDeltaToPaintMs
    paintMeasurementPending.current.add(current.id)
    void invoke<AssistSnapshot>('assist_record_first_paint', {
      exchangeId: current.id,
      firstDeltaToPaintMs,
      stopToVisibleTextMs,
    })
      .then(setSnapshot)
      .catch(() => undefined)
      .finally(() => paintMeasurementPending.current.delete(current.id))
  }, [current])

  const canFollowUp = Boolean(
    current
      && current.contextGeneration === snapshot.contextGeneration
      && ((snapshot.cloudEnabled && current.dataClass === 'standard')
        || (!snapshot.cloudEnabled && current.dataClass === 'private')),
  )
  const level = Math.min(100, Math.max(2, snapshot.levelRms * 550))
  const captureSeconds = current?.status === 'capturing'
    ? Math.max(0, Math.floor((Date.now() - new Date(current.createdAt).getTime()) / 1000))
    : 0

  const run = useCallback(async (work: () => Promise<AssistSnapshot>) => {
    try {
      setActionError(null)
      setSnapshot(await work())
    } catch (error) {
      setActionError(errorMessage(error))
    }
  }, [])

  const openIdentityManager = async () => {
    try {
      const appWindow = getCurrentWindow()
      const size = await appWindow.outerSize()
      previousWindowSize.current = size
      setIdentityManagerOpen(true)
      await appWindow.setSize(new LogicalSize(1080, 760))
    } catch (error) {
      setActionError(errorMessage(error))
    }
  }

  const closeIdentityManager = async () => {
    setIdentityManagerOpen(false)
    const size = previousWindowSize.current
    previousWindowSize.current = null
    if (size) await getCurrentWindow().setSize(size).catch(() => undefined)
  }

  const identitySaved = async (saved: SavedProfessionalIdentity) => {
    const availableIdentities = await invoke<IdentityChoice[]>('assist_list_identities')
    setIdentities(availableIdentities)
    const value = `${saved.identityId}|${saved.versionHash}`
    setSelectedIdentity(value)
    setSnapshot(await invoke<AssistSnapshot>('assist_set_identity', {
      identityId: saved.identityId,
      identityVersionHash: saved.versionHash,
    }))
    await closeIdentityManager()
  }

  const toggleCapture = useCallback((kind: 'new_question' | 'follow_up') => {
    void run(() => invoke<AssistSnapshot>('assist_toggle_capture', { kind }))
  }, [run])

  const selectExchange = (index: number) => {
    const target = snapshot.exchanges[index]
    if (target) void run(() => invoke<AssistSnapshot>('assist_select_exchange', { exchangeId: target.id }))
  }

  const toggleCloud = () => {
    if (!snapshot.cloudEnabled && !snapshot.providerConfigured) {
      setActionError('Cloud is not configured. Launch with the Live Assist API environment variables set.')
      return
    }
    void run(() => invoke<AssistSnapshot>('assist_set_cloud_enabled', { enabled: !snapshot.cloudEnabled }))
  }

  const chooseProfile = (value: string) => {
    setSelectedProfile(value)
    if (!value) {
      void run(() => invoke<AssistSnapshot>('assist_clear_profile'))
      return
    }
    const [profileId, profileVersionHash, playbookId] = value.split('|')
    void run(() => invoke<AssistSnapshot>('assist_set_profile', { profileId, profileVersionHash, playbookId }))
  }

  const chooseIdentity = (value: string) => {
    setSelectedIdentity(value)
    if (!value) {
      void run(() => invoke<AssistSnapshot>('assist_clear_identity'))
      return
    }
    const [identityId, identityVersionHash] = value.split('|')
    void run(() => invoke<AssistSnapshot>('assist_set_identity', { identityId, identityVersionHash }))
  }

  const startWindowDrag = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    const target = event.target as HTMLElement
    if (target.closest('button, select, input, textarea, a, [data-no-drag]')) return
    void getCurrentWindow().startDragging()
  }

  return (
    <main className="relative h-screen overflow-hidden bg-slate-950 text-slate-100">
      <div onMouseDown={startWindowDrag} className="flex h-11 cursor-move select-none items-center gap-3 border-b border-white/10 px-4">
        <Sparkles className="h-4 w-4 text-cyan-300" />
        <span className="whitespace-nowrap text-sm font-semibold">Live Assist</span>
        <StatePill snapshot={snapshot} />
        <div className="h-1.5 w-24 overflow-hidden rounded-full bg-white/10" title="Dedicated Assist audio level">
          <div className="h-full bg-cyan-400 transition-[width] duration-150" style={{ width: `${level}%` }} />
        </div>
        {snapshot.stallCount > 0 && <span className="whitespace-nowrap text-[11px] text-red-300">faults {snapshot.stallCount}</span>}
        <div className="ml-auto flex items-center gap-2">
          <button
            type="button"
            onClick={toggleCloud}
            className={`flex max-w-[230px] items-center gap-1.5 whitespace-nowrap rounded-full px-3 py-1 text-[11px] font-semibold ${snapshot.cloudEnabled ? 'bg-sky-500 text-white' : 'bg-white/10 text-slate-200'}`}
            title={snapshot.cloudEnabled ? `${snapshot.providerName ?? 'Cloud'} · ${snapshot.modelName ?? ''}` : 'Private: transcript stays on this PC'}
          >
            {snapshot.cloudEnabled ? <Cloud className="h-3.5 w-3.5" /> : <Lock className="h-3.5 w-3.5" />}
            {snapshot.cloudEnabled
              ? `Cloud · ${snapshot.providerName ?? 'provider'} · ${compactModelName(snapshot.modelName)}`
              : 'Private'}
          </button>
          <button type="button" onClick={() => void getCurrentWindow().hide()} className="rounded p-1.5 text-slate-400 hover:bg-white/10 hover:text-white" aria-label="Hide Live Assist">
            <EyeOff className="h-4 w-4" />
          </button>
          <button type="button" onClick={() => void getCurrentWindow().close()} className="rounded p-1.5 text-slate-400 hover:bg-red-500/20 hover:text-red-300" aria-label="Close Live Assist">
            <X className="h-4 w-4" />
          </button>
        </div>
      </div>

      <div className="grid h-[calc(100vh-2.75rem)] grid-cols-[210px_1fr]">
        <aside className="border-r border-white/10 p-3">
          <div className="mb-1 flex items-center justify-between gap-2">
            <label className="block text-[10px] font-semibold uppercase tracking-wider text-slate-500">Professional identity</label>
            <button type="button" onClick={() => void openIdentityManager()} className="flex items-center gap-1 text-[10px] font-semibold text-cyan-300 hover:text-cyan-100"><UserRoundCog className="h-3 w-3" />Manage</button>
          </div>
          <select value={selectedIdentity} onChange={event => chooseIdentity(event.target.value)} className="mb-3 w-full rounded-md border border-white/10 bg-slate-900 px-2 py-1.5 text-xs text-slate-200">
            <option value="">No professional identity</option>
            {identities.map(identity => (
              <option key={identity.identityId} value={`${identity.identityId}|${identity.identityVersionHash}`}>
                {identity.identityName} · {identity.roleTitle}
              </option>
            ))}
          </select>
          <label className="mb-1 block text-[10px] font-semibold uppercase tracking-wider text-slate-500">Meeting lens</label>
          <select value={selectedProfile} onChange={event => chooseProfile(event.target.value)} className="mb-3 w-full rounded-md border border-white/10 bg-slate-900 px-2 py-1.5 text-xs text-slate-200">
            <option value="">General guidance</option>
            {profiles.flatMap(profile => profile.playbooks.map(playbook => (
              <option key={`${profile.profileId}-${playbook.id}`} value={`${profile.profileId}|${profile.profileVersionHash}|${playbook.id}`}>
                {profile.profileName} · {playbook.name}
              </option>
            )))}
          </select>

          <button
            type="button"
            disabled={!snapshot.armed || snapshot.stalled || booting}
            onClick={() => toggleCapture('new_question')}
            className={`mb-2 flex w-full select-none items-center justify-center gap-2 rounded-lg px-3 py-3 text-sm font-bold disabled:opacity-40 ${snapshot.capturing ? 'bg-red-400 text-slate-950' : 'bg-cyan-500 text-slate-950'}`}
          >
            {snapshot.capturing ? <Radio className="h-4 w-4 animate-pulse" /> : <Mic className="h-4 w-4" />}
            {snapshot.capturing ? `Stop & answer · ${captureSeconds}s` : 'Start question'}
          </button>
          <button
            type="button"
            disabled={!snapshot.armed || snapshot.stalled || (!snapshot.capturing && !canFollowUp) || booting}
            onClick={() => toggleCapture('follow_up')}
            className="flex w-full select-none items-center justify-center gap-2 rounded-lg bg-violet-500 px-3 py-2.5 text-sm font-bold text-white disabled:opacity-40"
          >
            <RefreshCw className="h-4 w-4" />
            {snapshot.capturing ? 'Stop current capture' : 'Start follow-up'}
          </button>
          {snapshot.capturing && (
            <button
              type="button"
              onClick={() => void run(() => invoke<AssistSnapshot>('assist_restart_capture'))}
              className="mt-2 w-full rounded-md border border-white/10 px-3 py-1.5 text-xs font-semibold text-slate-300 hover:bg-white/10"
            >
              Restart capture
            </button>
          )}
          <p className="mt-3 text-[10px] leading-4 text-slate-500">
            {snapshot.captureShortcut}<br />{snapshot.followUpShortcut}<br />Press once to start and again to submit. Esc discards. Auto-submits at 50 seconds.<br />Follow-up attaches to the exchange selected when capture begins.
          </p>
        </aside>

        <section className="min-w-0 overflow-y-auto p-4">
          {actionError && (
            <div className="mb-3 rounded-md border border-red-400/30 bg-red-400/10 px-3 py-2 text-xs text-red-200">{actionError}</div>
          )}
          {snapshot.streamError && (
            <div className="mb-3 flex items-center justify-between gap-3 rounded-md border border-red-400/30 bg-red-400/10 px-3 py-2 text-xs text-red-200">
              <span>Assist audio stream failed: {snapshot.streamError}</span>
              <button type="button" onClick={() => void run(() => invoke<AssistSnapshot>('assist_arm'))} className="shrink-0 rounded bg-red-200/10 px-2 py-1 font-semibold hover:bg-red-200/20">Retry audio</button>
            </div>
          )}
          {!current ? (
            <div className="flex h-full flex-col items-center justify-center text-center text-slate-400">
              {booting ? <LoaderCircle className="mb-3 h-6 w-6 animate-spin" /> : <Mic className="mb-3 h-7 w-7" />}
              <p className="text-sm font-medium text-slate-300">Start capture when the relevant person begins speaking.</p>
              <p className="mt-1 max-w-md text-xs">Four seconds before your signal are included. Press again when the useful part is complete.</p>
            </div>
          ) : (
            <div>
              <div className="mb-2 flex items-center gap-2 text-xs text-slate-400">
                <button type="button" disabled={currentIndex <= 0} onClick={() => selectExchange(currentIndex - 1)} className="rounded p-1 hover:bg-white/10 disabled:opacity-25"><ChevronLeft className="h-4 w-4" /></button>
                <span>Exchange {current.ordinal} of {snapshot.exchanges.length}</span>
                <button type="button" disabled={currentIndex >= snapshot.exchanges.length - 1} onClick={() => selectExchange(currentIndex + 1)} className="rounded p-1 hover:bg-white/10 disabled:opacity-25"><ChevronRight className="h-4 w-4" /></button>
                <span className="ml-2 rounded bg-white/10 px-2 py-0.5">{current.status.replace('_', ' ')}</span>
                {current.kind === 'follow_up' && <span className="rounded bg-violet-500/20 px-2 py-0.5 text-violet-200">follow-up</span>}
              </div>
              {current.question && <p className="mb-2 text-xs text-slate-400">Heard: “{current.question}”</p>}
              {current.status === 'transcribing' && <p className="flex items-center gap-2 text-sm text-slate-300"><LoaderCircle className="h-4 w-4 animate-spin" />Transcribing locally…</p>}
              {current.status === 'transcript_only' && <p className="rounded-lg bg-amber-300/10 p-3 text-sm text-amber-100">Private mode kept this exchange on your PC. Enable cloud before the next capture if you want a suggestion.</p>}
              {current.error && <p className="rounded-lg bg-red-400/10 p-3 text-sm text-red-200">{current.error}</p>}
              {current.answer && (
                <>
                  <h1 className="mb-1 text-[10px] font-bold uppercase tracking-[0.18em] text-cyan-300">Your response</h1>
                  <p className="max-w-3xl text-lg font-semibold leading-7 text-white">{current.answer}</p>
                  {current.identityId && (
                    <p className="mt-2 max-w-3xl text-[10px] leading-4 text-slate-500">
                      {current.groundingSources.length > 0
                        ? `Grounded in: ${current.groundingSources.map(source => `${source.label} · ${source.revision} · updated ${new Date(source.updatedAt).toLocaleDateString()}`).join(' · ')}`
                        : 'No professional-identity source matched this question.'}
                    </p>
                  )}
                  {!current.profileId && (
                    <button
                      type="button"
                      disabled={current.status !== 'complete' || current.detailStatus === 'requesting' || current.detailStatus === 'streaming'}
                      onClick={() => void run(() => invoke<AssistSnapshot>('assist_request_detail', { exchangeId: current.id }))}
                      className="mt-3 rounded-md bg-white/10 px-3 py-1.5 text-xs font-semibold text-slate-200 hover:bg-white/15 disabled:opacity-40"
                    >
                      {current.detailStatus === 'requesting' || current.detailStatus === 'streaming' ? 'Adding detail…' : current.detail ? 'Refresh detail' : 'More detail'}
                    </button>
                  )}
                </>
              )}
              {!current.profileId && current.detailTruncated && (
                <p className="mt-2 text-xs font-semibold text-amber-300">Partial detail — response limit reached.</p>
              )}
              {!current.profileId && current.detailError && <p className="mt-2 text-xs text-red-300">{current.detailError}</p>}
              {!current.profileId && current.detail && <p className="mt-3 max-w-3xl border-l-2 border-slate-700 pl-3 text-sm leading-6 text-slate-300">{current.detail}</p>}
              {(current.timings.transcriptionMs || current.timings.requestToFirstTokenMs) && (
                <p className="mt-4 text-[10px] text-slate-500">
                  stop → visible {current.timings.stopToVisibleTextMs ?? 'measuring…'} ms · local transcript {current.timings.transcriptionMs ?? '—'} ms · cloud first token {current.timings.requestToFirstTokenMs ?? '—'} ms · complete {current.timings.requestToCompleteMs ?? '—'} ms · build {current.buildRevision}
                </p>
              )}
            </div>
          )}
        </section>
      </div>
      {identityManagerOpen && (
        <div className="absolute inset-0 z-50">
          <ProfessionalIdentitySettings onClose={() => void closeIdentityManager()} onSaved={identitySaved} />
        </div>
      )}
    </main>
  )
}
