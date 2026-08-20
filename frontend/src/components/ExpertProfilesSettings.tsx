'use client';

import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Textarea } from '@/components/ui/textarea';
import type {
  EvaluationReport,
  ExpertProfileSummary,
  ProfileActivationView,
  ProfileEvalResponse,
  StoredEvalPlan,
  StoredProfileVersion,
} from '@/types/expert-profiles';

interface SemanticScore {
  target: 'candidate' | 'baseline';
  caseId: string;
  repetition: number;
  assertionIndex: number;
  score: string;
}

export function ExpertProfilesSettings() {
  const [profiles, setProfiles] = useState<ExpertProfileSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [versions, setVersions] = useState<StoredProfileVersion[]>([]);
  const [plans, setPlans] = useState<StoredEvalPlan[]>([]);
  const [selectedVersion, setSelectedVersion] = useState('');
  const [selectedPlan, setSelectedPlan] = useState('');
  const [activation, setActivation] = useState<ProfileActivationView | null>(null);
  const [profileJson, setProfileJson] = useState('');
  const [planJson, setPlanJson] = useState('');
  const [bundleJson, setBundleJson] = useState('');
  const [cloudConsent, setCloudConsent] = useState(false);
  const [busy, setBusy] = useState(false);
  const [evaluation, setEvaluation] = useState<ProfileEvalResponse | null>(null);
  const [scores, setScores] = useState<SemanticScore[]>([]);
  const [confirmedRemovedPlaybooks, setConfirmedRemovedPlaybooks] = useState<string[]>([]);

  const selectedProfile = useMemo(
    () => profiles.find(profile => profile.id === selectedId) ?? null,
    [profiles, selectedId],
  );

  const refreshProfiles = useCallback(async () => {
    const rows = await invoke<ExpertProfileSummary[]>('profile_list');
    setProfiles(rows);
    setSelectedId(current => current ?? rows[0]?.id ?? null);
  }, []);

  const refreshSelection = useCallback(async (profileId: string) => {
    const [versionRows, planRows, active] = await Promise.all([
      invoke<StoredProfileVersion[]>('profile_list_versions', { profileId }),
      invoke<StoredEvalPlan[]>('profile_list_eval_plans', { profileId }),
      invoke<ProfileActivationView | null>('profile_get_activation', { profileId }),
    ]);
    setVersions(versionRows);
    setPlans(planRows);
    setSelectedVersion(current =>
      versionRows.some(row => row.version_hash === current)
        ? current
        : versionRows[0]?.version_hash ?? '',
    );
    setSelectedPlan(current =>
      planRows.some(row => row.content_hash === current)
        ? current
        : planRows[0]?.content_hash ?? '',
    );
    setActivation(active);
  }, []);

  useEffect(() => {
    refreshProfiles().catch(error => toast.error(formatError(error)));
  }, [refreshProfiles]);

  useEffect(() => {
    if (!selectedId) return;
    refreshSelection(selectedId).catch(error => toast.error(formatError(error)));
  }, [refreshSelection, selectedId]);

  useEffect(() => {
    if (!selectedId || !selectedVersion) return;
    invoke<unknown>('profile_get', {
      profileId: selectedId,
      versionHash: selectedVersion,
    })
      .then(profile => setProfileJson(JSON.stringify(profile, null, 2)))
      .catch(error => toast.error(formatError(error)));
  }, [selectedId, selectedVersion]);

  useEffect(() => {
    if (!selectedPlan) return;
    const plan = plans.find(row => row.content_hash === selectedPlan);
    if (!plan) return;
    invoke<unknown>('profile_get_eval_plan', {
      planId: plan.id,
      planHash: plan.content_hash,
    })
      .then(content => setPlanJson(JSON.stringify(content, null, 2)))
      .catch(error => toast.error(formatError(error)));
  }, [plans, selectedPlan]);

  const run = async (operation: () => Promise<void>) => {
    setBusy(true);
    try {
      await operation();
    } catch (error) {
      toast.error(formatError(error));
    } finally {
      setBusy(false);
    }
  };

  const createProfile = () => run(async () => {
    if (!profileJson.trim() || !planJson.trim()) {
      throw new Error('Profile and evaluation plan JSON are both required.');
    }
    const created = await invoke<{ profile_id: string }>('profile_create', {
      profileJson,
      evalPlanJson: planJson,
    });
    await refreshProfiles();
    setSelectedId(created.profile_id);
    toast.success('Expert profile created as an inactive draft.');
  });

  const createInterviewLens = () => run(async () => {
    const created = await invoke<{ profile_id: string }>('profile_create_interview_preset');
    await refreshProfiles();
    setSelectedId(created.profile_id);
    toast.success('Interview lens created with Junior, Mid-level, and Expert depth playbooks.');
  });

  const saveVersion = () => run(async () => {
    if (!selectedId) return;
    const version = await invoke<StoredProfileVersion>('profile_create_version', {
      profileId: selectedId,
      profileJson,
    });
    await refreshSelection(selectedId);
    setSelectedVersion(version.version_hash);
    toast.success(`Saved immutable version ${version.seq}.`);
  });

  const storePlan = () => run(async () => {
    if (!selectedId || !selectedVersion || !planJson.trim()) return;
    const plan = await invoke<StoredEvalPlan>('profile_store_eval_plan', {
      profileId: selectedId,
      profileVersionHash: selectedVersion,
      planId: crypto.randomUUID(),
      evalPlanJson: planJson,
    });
    await refreshSelection(selectedId);
    setSelectedPlan(plan.content_hash);
    toast.success('Evaluation plan stored immutably.');
  });

  const runEvaluation = () => run(async () => {
    if (!selectedId || !selectedVersion || !selectedPlan) return;
    const plan = plans.find(row => row.content_hash === selectedPlan);
    if (!plan) return;
    const result = await invoke<ProfileEvalResponse>('profile_run_evals', {
      args: {
        profile_id: selectedId,
        profile_version_hash: selectedVersion,
        plan_id: plan.id,
        plan_hash: selectedPlan,
        qualifying: true,
        confirmed_removed_playbooks: confirmedRemovedPlaybooks,
        adjudications: [],
        cloud_consent: cloudConsent,
      },
    });
    setEvaluation(result);
    setScores(semanticScoreRows(result.report));
    toast.success(`Evaluation completed: ${result.report.outcome}.`);
  });

  const adjudicate = () => run(async () => {
    if (!selectedId || !selectedPlan || !evaluation) return;
    const plan = plans.find(row => row.content_hash === selectedPlan);
    if (!plan) return;
    const adjudications = scores
      .filter(item => item.score.trim() !== '')
      .map(item => ({
        target: item.target,
        case_id: item.caseId,
        repetition: item.repetition,
        assertion_index: item.assertionIndex,
        score: Number(item.score),
      }));
    const result = await invoke<ProfileEvalResponse>('profile_adjudicate_eval', {
      args: {
        profile_id: selectedId,
        source_eval_run_id: evaluation.run.id,
        plan_id: plan.id,
        plan_hash: plan.content_hash,
        adjudications,
      },
    });
    setEvaluation(result);
    setScores(semanticScoreRows(result.report));
    toast.success(`Adjudication completed: ${result.report.outcome}.`);
  });

  const activate = () => run(async () => {
    if (!selectedId || !evaluation) return;
    await invoke('profile_activate', {
      profileId: selectedId,
      evalRunId: evaluation.run.id,
      expectedPreviousCapabilityHash: activation?.activation.capability_revision_hash ?? null,
      cloudConsent,
    });
    await refreshSelection(selectedId);
    toast.success('Profile activated with the evaluated model binding.');
  });

  const exportSelected = () => run(async () => {
    if (!selectedId || !selectedVersion || !selectedPlan) return;
    const plan = plans.find(row => row.content_hash === selectedPlan);
    if (!plan) return;
    const bundle = await invoke<string>('profile_export', {
      profileId: selectedId,
      versionHash: selectedVersion,
      planId: plan.id,
      planHash: plan.content_hash,
    });
    setBundleJson(bundle);
    await navigator.clipboard.writeText(bundle);
    toast.success('Bundle copied to the clipboard.');
  });

  const importProfile = () => run(async () => {
    const imported = await invoke<{ profile_id: string }>('profile_import', {
      bundleJson,
      identityMode: 'clone',
    });
    await refreshProfiles();
    setSelectedId(imported.profile_id);
    toast.success('Bundle imported as a new inactive profile.');
  });

  const retireProfile = () => run(async () => {
    if (!selectedId) return;
    await invoke('profile_retire', { profileId: selectedId });
    await refreshProfiles();
    await refreshSelection(selectedId);
    toast.success('Profile retired. Re-activation requires a new qualifying evaluation.');
  });

  const restoreProfile = () => run(async () => {
    if (!selectedId) return;
    await invoke('profile_restore', { profileId: selectedId });
    await refreshProfiles();
    toast.success('Profile restored as an inactive draft.');
  });

  const deleteProfile = () => run(async () => {
    if (!selectedId || !window.confirm('Permanently delete this inactive profile and its evaluation data?')) return;
    await invoke('profile_delete', { profileId: selectedId });
    setSelectedId(null);
    setProfileJson('');
    setPlanJson('');
    setEvaluation(null);
    setConfirmedRemovedPlaybooks([]);
    await refreshProfiles();
    toast.success('Profile deleted. Existing summary provenance remains as a tombstone.');
  });

  return (
    <div className="grid gap-6 py-6 lg:grid-cols-[18rem_1fr]">
      <aside className="space-y-3">
        <div>
          <h2 className="text-lg font-semibold">Meeting Lenses</h2>
          <p className="text-sm text-gray-600">Lenses control how deeply the selected identity answers. They cannot run tools or scripts.</p>
        </div>
        <Button
          variant="outline"
          className="w-full"
          onClick={() => {
            setSelectedId(null);
            setProfileJson('');
            setPlanJson('');
            setVersions([]);
            setPlans([]);
            setActivation(null);
            setEvaluation(null);
            setConfirmedRemovedPlaybooks([]);
          }}
        >
          New custom lens
        </Button>
        <Button
          variant="outline"
          className="w-full"
          onClick={createInterviewLens}
          disabled={busy || profiles.some(profile => profile.name.toLowerCase() === 'interview' && !profile.retired_at)}
        >
          Create Interview lens
        </Button>
        <div className="space-y-2">
          {profiles.map(profile => (
            <button
              key={profile.id}
              type="button"
              onClick={() => {
                setSelectedId(profile.id);
                setEvaluation(null);
                setConfirmedRemovedPlaybooks([]);
              }}
              className={`w-full rounded-md border p-3 text-left text-sm ${selectedId === profile.id ? 'border-blue-500 bg-blue-50' : 'bg-white hover:bg-gray-50'}`}
            >
              <span className="font-medium">{profile.name}</span>
              <span className="mt-1 block text-xs text-gray-500">{profile.id}</span>
            </button>
          ))}
          {profiles.length === 0 && <p className="text-sm text-gray-500">No local profiles yet.</p>}
        </div>
        <label className="flex items-start gap-2 rounded-md border bg-white p-3 text-sm">
          <input
            type="checkbox"
            checked={cloudConsent}
            onChange={event => setCloudConsent(event.target.checked)}
            className="mt-1"
          />
          <span>Allow this run to send synthetic evaluation text to the configured remote provider.</span>
        </label>
      </aside>

      <main className="min-w-0 space-y-6">
        {selectedProfile && (
          <section className="rounded-lg border bg-white p-4">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div>
                <h3 className="font-semibold">{selectedProfile.name}</h3>
                {activation ? (
                  <p className="text-sm text-gray-600">
                    {activation.activation.status} · {activation.binding.provider}/{activation.binding.model}
                    {activation.activation.superseded_reason ? ` · ${activation.activation.superseded_reason}` : ''}
                  </p>
                ) : (
                  <p className="text-sm text-amber-700">Inactive draft — evaluation and activation required.</p>
                )}
              </div>
              <div className="flex gap-2">
                <select
                  value={selectedVersion}
                  onChange={event => setSelectedVersion(event.target.value)}
                  className="rounded-md border px-2 py-1 text-sm"
                >
                  {versions.map(version => (
                    <option key={version.version_hash} value={version.version_hash}>Version {version.seq}</option>
                  ))}
                </select>
                <select
                  value={selectedPlan}
                  onChange={event => setSelectedPlan(event.target.value)}
                  className="max-w-52 rounded-md border px-2 py-1 text-sm"
                >
                  {plans.map((plan, index) => (
                    <option key={`${plan.id}:${plan.content_hash}`} value={plan.content_hash}>Eval plan {plans.length - index}</option>
                  ))}
                </select>
              </div>
            </div>
            <div className="mt-3 flex flex-wrap gap-2">
              {selectedProfile.retired_at ? (
                <Button variant="outline" size="sm" onClick={restoreProfile} disabled={busy}>Restore draft</Button>
              ) : (
                <Button variant="outline" size="sm" onClick={retireProfile} disabled={busy}>Retire</Button>
              )}
              <Button variant="outline" size="sm" onClick={deleteProfile} disabled={busy || !!activation} className="text-red-700">
                Delete inactive profile
              </Button>
            </div>
          </section>
        )}

        <section className="space-y-3 rounded-lg border bg-white p-4">
          <div>
            <h3 className="font-semibold">Profile content</h3>
            <p className="text-sm text-gray-600">Saving an edit always creates a new immutable version.</p>
          </div>
          <Textarea value={profileJson} onChange={event => setProfileJson(event.target.value)} rows={18} className="font-mono text-xs" placeholder="Paste strict ExpertProfile JSON" />
          <div className="flex flex-wrap gap-2">
            <Button onClick={selectedId ? saveVersion : createProfile} disabled={busy || !profileJson.trim()}>
              {selectedId ? 'Save new version' : 'Create profile'}
            </Button>
          </div>
        </section>

        <section className="space-y-3 rounded-lg border bg-white p-4">
          <div>
            <h3 className="font-semibold">Evaluation plan</h3>
            <p className="text-sm text-gray-600">A non-empty synthetic suite is mandatory. Application safety fixtures are added automatically.</p>
          </div>
          <Textarea value={planJson} onChange={event => setPlanJson(event.target.value)} rows={14} className="font-mono text-xs" placeholder="Paste strict target-free EvalPlan JSON" />
          <div className="flex flex-wrap gap-2">
            {!selectedId && <Button onClick={createProfile} disabled={busy || !profileJson.trim() || !planJson.trim()}>Create profile and plan</Button>}
            {selectedId && <Button variant="outline" onClick={storePlan} disabled={busy || !planJson.trim()}>Store new eval plan</Button>}
            {selectedId && <Button onClick={runEvaluation} disabled={busy || !selectedVersion || !selectedPlan}>Run qualifying evaluation</Button>}
          </div>
        </section>

        {evaluation && (
          <EvaluationView
            evaluation={evaluation}
            scores={scores}
            setScores={setScores}
            onAdjudicate={adjudicate}
            onActivate={activate}
            confirmedRemovedPlaybooks={confirmedRemovedPlaybooks}
            setConfirmedRemovedPlaybooks={setConfirmedRemovedPlaybooks}
            onRerun={runEvaluation}
            busy={busy}
          />
        )}

        <section className="space-y-3 rounded-lg border bg-white p-4">
          <div>
            <h3 className="font-semibold">Import / export</h3>
            <p className="text-sm text-gray-600">Imports are digest-checked, cloned by default, and never activate automatically.</p>
          </div>
          <Textarea value={bundleJson} onChange={event => setBundleJson(event.target.value)} rows={10} className="font-mono text-xs" placeholder="Paste or export a meetily-profile bundle" />
          <div className="flex gap-2">
            <Button variant="outline" onClick={importProfile} disabled={busy || !bundleJson.trim()}>Import as clone</Button>
            <Button variant="outline" onClick={exportSelected} disabled={busy || !selectedId || !selectedPlan}>Export selected</Button>
          </div>
        </section>
      </main>
    </div>
  );
}

function EvaluationView({
  evaluation,
  scores,
  setScores,
  onAdjudicate,
  onActivate,
  confirmedRemovedPlaybooks,
  setConfirmedRemovedPlaybooks,
  onRerun,
  busy,
}: {
  evaluation: ProfileEvalResponse;
  scores: SemanticScore[];
  setScores: (scores: SemanticScore[]) => void;
  onAdjudicate: () => void;
  onActivate: () => void;
  confirmedRemovedPlaybooks: string[];
  setConfirmedRemovedPlaybooks: (ids: string[]) => void;
  onRerun: () => void;
  busy: boolean;
}) {
  const report = evaluation.report;
  const canActivate = report.outcome === 'pass' || report.outcome === 'baseline_missing';
  return (
    <section className="space-y-4 rounded-lg border bg-white p-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h3 className="font-semibold">Evaluation: {report.outcome}</h3>
          <p className="text-sm text-gray-600">{report.safety_gate_version} · {report.repetitions.length} generated samples</p>
        </div>
        <div className="flex gap-2">
          {scores.length > 0 && <Button variant="outline" onClick={onAdjudicate} disabled={busy}>Save human scores</Button>}
          <Button onClick={onActivate} disabled={busy || !canActivate}>Activate exact binding</Button>
        </div>
      </div>
      {report.reasons.length > 0 && (
        <ul className="list-disc pl-5 text-sm text-amber-800">
          {report.reasons.map(reason => <li key={reason}>{reason}</li>)}
        </ul>
      )}
      {report.removed_playbooks.length > 0 && (
        <div className="space-y-2 rounded-md border border-amber-300 bg-amber-50 p-3">
          <p className="text-sm font-medium text-amber-900">
            This version removes playbooks from the active baseline. Confirm each removal explicitly, then rerun the evaluation.
          </p>
          {report.removed_playbooks.map(playbookId => (
            <label key={playbookId} className="flex items-start gap-2 text-sm text-amber-900">
              <input
                type="checkbox"
                checked={confirmedRemovedPlaybooks.includes(playbookId)}
                onChange={event => {
                  setConfirmedRemovedPlaybooks(event.target.checked
                    ? [...confirmedRemovedPlaybooks, playbookId]
                    : confirmedRemovedPlaybooks.filter(id => id !== playbookId));
                }}
                className="mt-1"
              />
              <span>Confirm removal of playbook <code>{playbookId}</code></span>
            </label>
          ))}
          <Button
            variant="outline"
            onClick={onRerun}
            disabled={busy || report.removed_playbooks.some(id => !confirmedRemovedPlaybooks.includes(id))}
          >
            Rerun with confirmed removals
          </Button>
        </div>
      )}
      <div className="max-h-[36rem] space-y-3 overflow-y-auto">
        {report.repetitions.map((result, resultIndex) => (
          <details key={`${result.target}:${result.case_id}:${result.repetition}`} className="rounded-md border p-3">
            <summary className="cursor-pointer text-sm font-medium">
              {result.target} · {result.case_id} · run {result.repetition + 1}
            </summary>
            {result.generation_error && <p className="mt-2 text-sm text-red-700">{result.generation_error}</p>}
            {result.output_markdown && <pre className="mt-3 whitespace-pre-wrap rounded bg-gray-50 p-3 text-xs">{result.output_markdown}</pre>}
            <div className="mt-3 space-y-1 text-sm">
              {result.hard.map(assertion => (
                <p key={assertion.assertion} className={assertion.passed ? 'text-green-700' : 'text-red-700'}>
                  {assertion.passed ? 'Pass' : 'Fail'} · {assertion.assertion}
                </p>
              ))}
            </div>
            {result.semantic.map(assertion => {
              const scoreIndex = scores.findIndex(score =>
                score.target === result.target
                && score.caseId === result.case_id
                && score.repetition === result.repetition
                && score.assertionIndex === assertion.assertion_index,
              );
              if (assertion.adjudicator === 'model') {
                return <p key={assertion.assertion_index} className="mt-3 text-sm text-amber-700">Pinned model adjudicator is not configured; this rubric remains inconclusive.</p>;
              }
              return (
                <label key={assertion.assertion_index} className="mt-3 flex items-center gap-2 text-sm">
                  Human score (0–1, threshold {assertion.threshold})
                  <input
                    type="number"
                    min="0"
                    max="1"
                    step="0.05"
                    value={scoreIndex >= 0 ? scores[scoreIndex].score : ''}
                    onChange={event => {
                      if (scoreIndex < 0) return;
                      const next = [...scores];
                      next[scoreIndex] = { ...next[scoreIndex], score: event.target.value };
                      setScores(next);
                    }}
                    className="w-20 rounded border px-2 py-1"
                  />
                </label>
              );
            })}
          </details>
        ))}
      </div>
    </section>
  );
}

function semanticScoreRows(report: EvaluationReport): SemanticScore[] {
  return report.repetitions.flatMap(result => result.semantic
    .filter(assertion => assertion.adjudicator === 'human')
    .map(assertion => ({
    target: result.target,
    caseId: result.case_id,
    repetition: result.repetition,
    assertionIndex: assertion.assertion_index,
    score: assertion.score?.toString() ?? '',
    })));
}

function formatError(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String((error as { message: unknown }).message);
  }
  return error instanceof Error ? error.message : String(error);
}
