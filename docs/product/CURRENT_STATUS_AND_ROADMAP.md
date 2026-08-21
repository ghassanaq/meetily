# Meeting Assistant current status and roadmap

Updated: 2026-08-21

Authoritative implementation worktree: `C:\Users\ghass\.codex\worktrees\0db3\Meeting-Assistant`

Branch: `codex/project-context-idf-diversity`

This file is the concise operational view of what is done and what comes next. Product principles remain in `PRODUCT-HANDOFF.md`; implementation detail and safety constraints remain in `SESSION_HANDOFF_2026-08-21.md`.

## Done

- Preserved the Meetily desktop baseline: recording, local transcription, workspace, notes, history, recordings, summaries, and no meeting bot.
- Added on-demand Live Assist capture with F8 for a new question, F9 for a follow-up, Escape to discard, pre-signal buffering, auto-submit, and a private-by-default overlay.
- Added Professional Identity and Interview lens selection with immutable version grounding and provenance.
- Merged the initial Project Context retrieval experiments and documented why global score-floor tuning does not solve upstream ranking dominance.
- Added a bounded production Markdown context importer shared with the private voice harness. It creates an immutable local identity snapshot and keeps private corpus files out of Git.
- Added DeepSeek and Kimi/Moonshot request adapters while preserving provider-specific parameters and rejecting tool calls.
- Hardened the interview prompt and offline evaluator around invented experience, quantities, dates, authority, shared responsibility, and cross-story contamination.
- Completed the same-context Kimi–DeepSeek comparison. Both answers were grounded but narrow; neither provider solved broad career composition. Both also violated the plain-text contract with Markdown.
- Added Settings → Providers with DeepSeek, Kimi/Moonshot, OpenAI, and custom OpenAI-compatible presets.
- Added secure Windows Credential Manager storage. Saved keys are never returned to the frontend; only key status is exposed.
- Added save/edit, bounded Test Connection, explicit activation, replacement, active-provider display, and guarded deletion.
- Disabled silent `.env` fallback after the first UI-managed provider is saved. Environment configuration remains only a bootstrap fallback before managed mode begins.
- Verified the Provider Settings implementation with focused Rust tests, `cargo check`, frontend typecheck, frontend tests, a production Next build, a release build with `custom-protocol`, database migration inspection, relaunch behavior, and Ghassan's successful manual UI check.
- Separated the former dirty implementation tree into eight coherent local commits: migration line endings, provider adapters, Markdown importer, evaluator hardening, provider metadata schema, secure Provider Settings, launcher behavior, and product documentation. The six earlier local commits were preserved without rewriting history.

## Current finding

The active imported context contains the broader career evidence. The narrow answers come from production retrieval: a generic question such as `Tell us about yourself` has little literal overlap with career-section records, so only a small high-overlap slice reaches either model. The next fix belongs in retrieval/composition, not in API-key or provider switching.

## Next

1. **Implement broad interview composition.** Detect broad career-introduction questions within the Interview lens and assemble a bounded evidence set containing career overview/progression plus representative frontline, leadership/emergency or regional, and current-role-fit evidence.
2. **Enforce answer form.** Deterministically reject or safely normalize inline Markdown and retain exact shared-authority and quantitative qualifiers.
3. **Verify without leaking private data.** Run focused unit/integration fixtures and the ignored real-corpus workload against the configured providers. Store no private answer text in tracked files.
4. **Run the use loop.** Freeze provider, identity version, lens/depth, and retrieval configuration; run a mock interview and then five real meeting/interview trials. Record question, word count, used/not used, answer fit, continuity correctness, and one-line missing-context notes.

## Later

- Add durable Live Assist sessions with explicit start/end, resume preview, deletion, persisted relationships, immutable selections, and provenance.
- Promote the Markdown import bridge into full Person/Role/Project selection with freshness/conflict preflight, bundle-aware retrieval, and pinned snapshots.
- Continue evidence-linked meeting intelligence and cited private-document retrieval after real-use evidence identifies the highest-value gaps.
- Complete visible rebranding and isolate inherited cleanup in a separate mechanical change.
- Consider evaluated local model/LoRA bindings only after prompts, retrieval, playbooks, and schemas demonstrate a stable unresolved deficiency.

## Still intentionally not done

- OpenAI is not configured; Ghassan will add its key later through Provider Settings.
- Live Assist exchanges are still memory-only.
- Generic interview prompts do not yet trigger broad career composition.
- Inline Markdown is not yet deterministically blocked or normalized.
- Full production Project Context architecture is not yet implemented.
- The semantic judge is not a runtime safety oracle; Ghassan remains the final live-use gate.
