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
- Reviewed Mishkat's compose/edit service architecture and adapted its governing boundary rather than porting unrelated editorial code: broad professional introductions now receive an explicit, versioned evidence brief before provider generation, while specific questions retain lexical retrieval.
- Added `professional-introduction/v1` routing for common self-introduction/background phrasings, deterministic CV-first evidence ordering, source provenance, a 7,000-character evidence budget, and a 1,200-character per-record cap that omits content rather than cutting a claim mid-sentence. Contested facts are suppressed from the broad brief; specific lexical questions retain the existing fail-closed conflict error.
- Tightened the plain-text boundary: matching outer emphasis wrappers are removed immediately from the streamed display and normalized at completion with a recorded warning; inline emphasis and code Markdown remain invalid.
- Verified the change with 20 focused Professional Identity tests, 18 focused Live Assist tests, three focused streaming-display tests, the full Rust and frontend suites, TypeScript typechecking, and a production release rebuild.

## Current finding

The active imported context contains the broader career evidence. The narrow-answer defect was in production retrieval: `Tell us about yourself` had little literal overlap with career records. That canonical path now bypasses lexical ranking and supplies a governed career brief. The remaining product question is answer quality on the private corpus and configured providers, not whether the model received the broader evidence.

## Next

1. **Verify without leaking private data.** Run the ignored real-corpus workload and a manual `Tell us about yourself` request against the configured provider. Store no private answer text in tracked files.
2. **Assess the brief, not the provider.** Confirm the grounding-source list spans the expected CV/career sections and that the response covers progression, current strengths, and role relevance without invented transitions.
3. **Extend compose profiles only from observed misses.** Add a separate governed brief for suitability or career-arc questions only if real trials show that the professional-introduction contract cannot serve them.
4. **Run the use loop.** Freeze provider, identity version, lens/depth, and compose profile; run a mock interview and then five real meeting/interview trials. Record question, word count, used/not used, answer fit, continuity correctness, and one-line missing-context notes.

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
