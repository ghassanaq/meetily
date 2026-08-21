# Meeting Assistant current status and roadmap

Updated: 2026-08-22

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
- Reviewed Mishkat's compose/edit service architecture and adapted its governing boundary rather than porting unrelated editorial code: broad professional introductions now receive an explicit, versioned evidence brief before provider generation, while specific questions retain lexical retrieval.
- Added `professional-introduction/v1` routing for common self-introduction/background phrasings, deterministic CV-first evidence ordering, source provenance, a 7,000-character evidence budget, and a 1,200-character per-record cap that omits content rather than cutting a claim mid-sentence. Contested facts are suppressed from the broad brief; specific lexical questions retain the existing fail-closed conflict error.
- Tightened the plain-text boundary: matching outer emphasis wrappers are removed immediately from the streamed display and normalized at completion with a recorded warning; inline emphasis and code Markdown remain invalid.
- Verified the change with 20 focused Professional Identity tests, 18 focused Live Assist tests, three focused streaming-display tests, the full Rust and frontend suites, TypeScript typechecking, and a production release rebuild.
- Added identity schema v2 authority constraints and a pure local scope-expansion matcher derived from explicit Role and authority boundaries. The matcher is closed-world: no match never claims factual verification.
- Completed five private offline authority trials without tracking answer text. All five accepted answers remained true negatives; the private gate reported zero false positives and zero false negatives for the enrolled rules.
- Implemented the approved advisory path with policy state pinned to the exact immutable identity-version hash. New constrained versions default to offline evaluation and require the typed confirmation `ACTIVATE AUTHORITY WARNINGS` before Live Assist can display results.
- Added post-completion, local-only authority diagnostics separate from format warnings; amber excluded-object highlighting; exchange-local dismissal with aggregate counters; and source metadata with excerpts revealed only on demand. Answers, sentences, aliases, and excerpts are never persisted by the policy tables.

## Current finding

Broad professional-introduction composition now works against the active context. The five observed answers preserved the important quantitative and authority qualifiers. The authority-warning engine is intentionally narrower: it detects only claims covered by explicitly enrolled rules and remains an advisory review aid, not a factuality certificate.

## Next

1. **Manually verify advisory display.** Activate warnings for the tested identity version, run one synthetic positive claim and one accepted negative claim, then confirm amber span highlighting, honest clean-state copy, exchange-local dismissal, and evidence-on-demand.
2. **Keep learning from dismissals.** Review aggregate dismissal counts after real use. Change rules only through a new immutable identity version; never turn a dismissal into persisted suppression.
3. **Extend compose profiles only from observed misses.** Add a separate governed brief for suitability or career-arc questions only if real trials show that the professional-introduction contract cannot serve them.
4. **Run the use loop.** Freeze provider, identity version, lens/depth, and compose profile; run a mock interview and then five real meeting/interview trials. Record question, word count, used/not used, answer fit, continuity correctness, warning usefulness, and one-line missing-context notes.

## Later

- Add durable Live Assist sessions with explicit start/end, resume preview, deletion, persisted relationships, immutable selections, and provenance.
- Promote the Markdown import bridge into full Person/Role/Project selection with freshness/conflict preflight, bundle-aware retrieval, and pinned snapshots.
- Continue evidence-linked meeting intelligence and cited private-document retrieval after real-use evidence identifies the highest-value gaps.
- Complete visible rebranding and isolate inherited cleanup in a separate mechanical change.
- Consider evaluated local model/LoRA bindings only after prompts, retrieval, playbooks, and schemas demonstrate a stable unresolved deficiency.

## Still intentionally not done

- OpenAI is not configured; Ghassan will add its key later through Provider Settings.
- Live Assist exchanges are still memory-only.
- Broad composition currently covers professional-introduction/background questions only; suitability, strengths, motivation, and career-arc questions still use lexical retrieval.
- Only a matching outer emphasis wrapper is normalized. Inline emphasis and code Markdown are still rejected so structurally unsafe provider output fails visibly.
- Full production Project Context architecture is not yet implemented.
- The semantic judge is not a runtime safety oracle; Ghassan remains the final live-use gate.
- Authority warnings are not comprehensive verification. Only explicitly enrolled verb/object/context combinations can match, and advisory activation is version-specific.
