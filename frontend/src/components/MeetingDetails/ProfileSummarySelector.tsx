'use client';

import { invoke } from '@tauri-apps/api/core';
import { UserRoundCog } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogTitle, DialogTrigger } from '@/components/ui/dialog';
import type { ExpertProfileSummary, ProfileActivationView } from '@/types/expert-profiles';

interface Playbook {
  id: string;
  name: string;
  description: string;
}

interface ActiveProfile {
  summary: ExpertProfileSummary;
  activation: ProfileActivationView;
}

export interface ProfileSummaryGenerationResponse {
  meeting_id: string;
  markdown: string;
  english_markdown: string;
  chunk_count: number;
  provenance: {
    profile_id: string;
    profile_version_hash: string;
    playbook_id: string;
    capability_revision_hash: string;
    model_binding_hash: string;
  };
}

export function ProfileSummarySelector({
  meetingId,
  transcriptText,
  additionalUserContext,
  summaryLanguage,
  disabled,
  onGenerated,
}: {
  meetingId: string;
  transcriptText: string;
  additionalUserContext: string;
  summaryLanguage: string | null;
  disabled: boolean;
  onGenerated: (result: ProfileSummaryGenerationResponse) => void;
}) {
  const [open, setOpen] = useState(false);
  const [profiles, setProfiles] = useState<ActiveProfile[]>([]);
  const [selectedProfileId, setSelectedProfileId] = useState('');
  const [playbooks, setPlaybooks] = useState<Playbook[]>([]);
  const [selectedPlaybookId, setSelectedPlaybookId] = useState('');
  const [cloudConsent, setCloudConsent] = useState(false);
  const [busy, setBusy] = useState(false);

  const selected = useMemo(
    () => profiles.find(profile => profile.summary.id === selectedProfileId),
    [profiles, selectedProfileId],
  );

  useEffect(() => {
    if (!open) return;
    invoke<ExpertProfileSummary[]>('profile_list')
      .then(async rows => {
        const active = await Promise.all(rows.map(async summary => ({
          summary,
          activation: await invoke<ProfileActivationView | null>('profile_get_activation', {
            profileId: summary.id,
          }),
        })));
        const usable = active.filter((item): item is ActiveProfile => item.activation?.activation.status === 'active');
        setProfiles(usable);
        setSelectedProfileId(current => current || usable[0]?.summary.id || '');
      })
      .catch(error => toast.error(formatError(error)));
  }, [open]);

  useEffect(() => {
    if (!selected) {
      setPlaybooks([]);
      setSelectedPlaybookId('');
      return;
    }
    invoke<{ playbooks: Playbook[] }>('profile_get', {
      profileId: selected.summary.id,
      versionHash: selected.activation.activation.profile_version_hash,
    })
      .then(profile => {
        setPlaybooks(profile.playbooks);
        setSelectedPlaybookId(current =>
          profile.playbooks.some(playbook => playbook.id === current)
            ? current
            : profile.playbooks[0]?.id ?? '',
        );
      })
      .catch(error => toast.error(formatError(error)));
  }, [selected]);

  const generate = async () => {
    if (!selectedProfileId || !selectedPlaybookId) return;
    setBusy(true);
    try {
      const result = await invoke<ProfileSummaryGenerationResponse>('summary_generate_with_profile', {
        args: {
          meeting_id: meetingId,
          transcript_text: transcriptText,
          profile_id: selectedProfileId,
          playbook_id: selectedPlaybookId,
          additional_user_context: additionalUserContext || null,
          summary_language: summaryLanguage,
          detected_transcript_language: null,
          cloud_consent: cloudConsent,
        },
      });
      onGenerated(result);
      setOpen(false);
      toast.success('Generated with the selected evaluated profile.');
    } catch (error) {
      toast.error(formatError(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="outline" size="sm" disabled={disabled} title="Generate with an active Expert Profile">
          <UserRoundCog size={18} />
          <span className="hidden lg:inline">Expert</span>
        </Button>
      </DialogTrigger>
      <DialogContent aria-describedby={undefined}>
        <DialogTitle>Generate with Expert Profile</DialogTitle>
        <div className="space-y-4">
          {profiles.length === 0 ? (
            <p className="text-sm text-amber-700">No active profile is available. Evaluate and activate one in Settings → Experts.</p>
          ) : (
            <>
              <label className="block space-y-1 text-sm">
                <span className="font-medium">Active profile</span>
                <select value={selectedProfileId} onChange={event => setSelectedProfileId(event.target.value)} className="w-full rounded-md border p-2">
                  {profiles.map(profile => (
                    <option key={profile.summary.id} value={profile.summary.id}>
                      {profile.summary.name} · {profile.activation.binding.provider}/{profile.activation.binding.model}
                    </option>
                  ))}
                </select>
              </label>
              <label className="block space-y-1 text-sm">
                <span className="font-medium">Meeting playbook</span>
                <select value={selectedPlaybookId} onChange={event => setSelectedPlaybookId(event.target.value)} className="w-full rounded-md border p-2">
                  {playbooks.map(playbook => <option key={playbook.id} value={playbook.id}>{playbook.name}</option>)}
                </select>
              </label>
              <label className="flex items-start gap-2 rounded-md border p-3 text-sm">
                <input type="checkbox" checked={cloudConsent} onChange={event => setCloudConsent(event.target.checked)} className="mt-1" />
                <span>Allow this generation to send the meeting transcript to the configured remote provider. Local providers do not require this.</span>
              </label>
              <p className="text-xs text-gray-500">The stored summary will retain the exact profile, playbook, capability, and model-binding hashes.</p>
              <Button onClick={generate} disabled={busy || !selectedPlaybookId} className="w-full">
                {busy ? 'Generating…' : 'Generate profiled summary'}
              </Button>
            </>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}

function formatError(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String((error as { message: unknown }).message);
  }
  return error instanceof Error ? error.message : String(error);
}
