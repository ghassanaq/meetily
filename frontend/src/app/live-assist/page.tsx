'use client'

import { type MouseEvent as ReactMouseEvent, useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
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
  X,
} from 'lucide-react'

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
  detail: string
  detailStatus: ExchangeStatus | null
  detailError: string | null
  error: string | null
  createdAt: string
  timings: {
    captureMs: number | null
    transcriptionMs: number | null
    requestToFirstTokenMs: number | null
    requestToCompleteMs: number | null
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

const EMPTY_SNAPSHOT: AssistSnapshot = {
  armed: false,
  receiving: false,
  stalled: false,
  levelRms: 0,
  cloudEnabled: false,
  providerConfigured: false,
  providerName: null,
  modelName: null,
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
    return <span className="rounded-full bg-slate-200 px-2.5 py-1 text-xs font-semibold text-slate-700">Not armed</span>
  }
  if (snapshot.stalled) {
    return <span className="rounded-full bg-red-100 px-2.5 py-1 text-xs font-semibold text-red-700">Stalled</span>
  }
  if (!snapshot.receiving) {
    return <span className="rounded-full bg-amber-100 px-2.5 py-1 text-xs font-semibold text-amber-700">Armed · waiting</span>
  }
  return <span className="rounded-full bg-emerald-100 px-2.5 py-1 text-xs font-semibold text-emerald-700">Armed · receiving</span>
}

export default function LiveAssistPage() {
  const [snapshot, setSnapshot] = useState(EMPTY_SNAPSHOT)
  const [profiles, setProfiles] = useState<ProfileChoice[]>([])
  const [selectedProfile, setSelectedProfile] = useState('')
  const [booting, setBooting] = useState(true)
  const [actionError, setActionError] = useState<string | null>(null)
  const holding = useRef(false)

  const refresh = useCallback(async () => {
    const next = await invoke<AssistSnapshot>('assist_get_snapshot')
    setSnapshot(next)
  }, [])

  useEffect(() => {
    let active = true
    const boot = async () => {
      try {
        const isVisible = await getCurrentWindow().isVisible()
        const [next, availableProfiles] = await Promise.all([
          invoke<AssistSnapshot>(isVisible ? 'assist_arm' : 'assist_get_snapshot'),
          invoke<ProfileChoice[]>('assist_list_profiles'),
        ])
        if (!active) return
        setSnapshot(next)
        setProfiles(availableProfiles)
        const firstProfile = availableProfiles.find(profile => profile.playbooks.length > 0)
        const firstPlaybook = firstProfile?.playbooks[0]
        if (firstProfile && firstPlaybook) {
          const key = `${firstProfile.profileId}|${firstProfile.profileVersionHash}|${firstPlaybook.id}`
          setSelectedProfile(key)
          setSnapshot(await invoke<AssistSnapshot>('assist_set_profile', {
            profileId: firstProfile.profileId,
            profileVersionHash: firstProfile.profileVersionHash,
            playbookId: firstPlaybook.id,
          }))
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

  const currentIndex = useMemo(
    () => snapshot.exchanges.findIndex(exchange => exchange.id === snapshot.currentExchangeId),
    [snapshot.currentExchangeId, snapshot.exchanges],
  )
  const current = currentIndex >= 0 ? snapshot.exchanges[currentIndex] : null
  const canFollowUp = Boolean(
    current
      && current.contextGeneration === snapshot.contextGeneration
      && ((snapshot.cloudEnabled && current.dataClass === 'standard')
        || (!snapshot.cloudEnabled && current.dataClass === 'private')),
  )
  const level = Math.min(100, Math.max(2, snapshot.levelRms * 550))

  const run = useCallback(async (work: () => Promise<AssistSnapshot>) => {
    try {
      setActionError(null)
      setSnapshot(await work())
    } catch (error) {
      setActionError(errorMessage(error))
    }
  }, [])

  const startCapture = useCallback((kind: 'new_question' | 'follow_up') => {
    if (holding.current) return
    holding.current = true
    void run(() => invoke<AssistSnapshot>('assist_start_capture', { kind }))
  }, [run])

  const stopCapture = useCallback(() => {
    if (!holding.current) return
    holding.current = false
    void run(() => invoke<AssistSnapshot>('assist_stop_capture'))
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
    if (!value) return
    const [profileId, profileVersionHash, playbookId] = value.split('|')
    void run(() => invoke<AssistSnapshot>('assist_set_profile', { profileId, profileVersionHash, playbookId }))
  }

  const startWindowDrag = (event: ReactMouseEvent<HTMLDivElement>) => {
    if (event.button !== 0) return
    const target = event.target as HTMLElement
    if (target.closest('button, select, input, textarea, a, [data-no-drag]')) return
    void getCurrentWindow().startDragging()
  }

  return (
    <main className="h-screen overflow-hidden bg-slate-950 text-slate-100">
      <div onMouseDown={startWindowDrag} className="flex h-11 cursor-move select-none items-center gap-3 border-b border-white/10 px-4">
        <Sparkles className="h-4 w-4 text-cyan-300" />
        <span className="text-sm font-semibold">Live Assist</span>
        <StatePill snapshot={snapshot} />
        <div className="h-1.5 w-24 overflow-hidden rounded-full bg-white/10" title="Dedicated Assist audio level">
          <div className="h-full bg-cyan-400 transition-[width] duration-150" style={{ width: `${level}%` }} />
        </div>
        <span className="text-[11px] text-slate-400">stalls {snapshot.stallCount}</span>
        <div className="ml-auto flex items-center gap-2">
          <button
            type="button"
            onClick={toggleCloud}
            className={`flex items-center gap-1.5 rounded-full px-3 py-1 text-xs font-semibold ${snapshot.cloudEnabled ? 'bg-sky-500 text-white' : 'bg-white/10 text-slate-200'}`}
            title={snapshot.cloudEnabled ? `${snapshot.providerName ?? 'Cloud'} · ${snapshot.modelName ?? ''}` : 'Private: transcript stays on this PC'}
          >
            {snapshot.cloudEnabled ? <Cloud className="h-3.5 w-3.5" /> : <Lock className="h-3.5 w-3.5" />}
            {snapshot.cloudEnabled ? 'Cloud on' : 'Private'}
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
            onPointerDown={event => { event.currentTarget.setPointerCapture(event.pointerId); startCapture('new_question') }}
            onPointerUp={stopCapture}
            onPointerCancel={stopCapture}
            className="mb-2 flex w-full select-none items-center justify-center gap-2 rounded-lg bg-cyan-500 px-3 py-3 text-sm font-bold text-slate-950 disabled:opacity-40"
          >
            {snapshot.capturing ? <Radio className="h-4 w-4 animate-pulse" /> : <Mic className="h-4 w-4" />}
            Hold for question
          </button>
          <button
            type="button"
            disabled={!snapshot.armed || snapshot.stalled || !canFollowUp || booting}
            onPointerDown={event => { event.currentTarget.setPointerCapture(event.pointerId); startCapture('follow_up') }}
            onPointerUp={stopCapture}
            onPointerCancel={stopCapture}
            className="flex w-full select-none items-center justify-center gap-2 rounded-lg bg-violet-500 px-3 py-2.5 text-sm font-bold text-white disabled:opacity-40"
          >
            <RefreshCw className="h-4 w-4" />
            Hold for follow-up
          </button>
          <p className="mt-3 text-[10px] leading-4 text-slate-500">
            {snapshot.captureShortcut}<br />{snapshot.followUpShortcut}<br />Follow-up attaches to the exchange selected when capture begins.
          </p>
        </aside>

        <section className="min-w-0 overflow-y-auto p-4">
          {actionError && (
            <div className="mb-3 rounded-md border border-red-400/30 bg-red-400/10 px-3 py-2 text-xs text-red-200">{actionError}</div>
          )}
          {!current ? (
            <div className="flex h-full flex-col items-center justify-center text-center text-slate-400">
              {booting ? <LoaderCircle className="mb-3 h-6 w-6 animate-spin" /> : <Mic className="mb-3 h-7 w-7" />}
              <p className="text-sm font-medium text-slate-300">Hold the question button while the person is speaking.</p>
              <p className="mt-1 max-w-md text-xs">Four seconds before your signal are included. Release when the useful part is complete.</p>
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
                  <button
                    type="button"
                    disabled={current.status !== 'complete' || current.detailStatus === 'requesting' || current.detailStatus === 'streaming'}
                    onClick={() => void run(() => invoke<AssistSnapshot>('assist_request_detail', { exchangeId: current.id }))}
                    className="mt-3 rounded-md bg-white/10 px-3 py-1.5 text-xs font-semibold text-slate-200 hover:bg-white/15 disabled:opacity-40"
                  >
                    {current.detailStatus === 'requesting' || current.detailStatus === 'streaming' ? 'Adding detail…' : current.detail ? 'Refresh detail' : 'More detail'}
                  </button>
                </>
              )}
              {current.detailError && <p className="mt-2 text-xs text-red-300">{current.detailError}</p>}
              {current.detail && <p className="mt-3 max-w-3xl border-l-2 border-slate-700 pl-3 text-sm leading-6 text-slate-300">{current.detail}</p>}
              {(current.timings.transcriptionMs || current.timings.requestToFirstTokenMs) && (
                <p className="mt-4 text-[10px] text-slate-500">
                  local transcript {current.timings.transcriptionMs ?? '—'} ms · first cloud token {current.timings.requestToFirstTokenMs ?? '—'} ms · complete {current.timings.requestToCompleteMs ?? '—'} ms
                </p>
              )}
            </div>
          )}
        </section>
      </div>
    </main>
  )
}
