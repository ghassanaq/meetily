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
- Manually verified advisory activation on the rebuilt app: five-rule clean-state reporting, exact excluded-object highlighting on a controlled positive claim, metadata-first evidence inspection, excerpts only after explicit request, exchange-local dismissal, and warning recurrence on a later exchange all passed. A camelCase snapshot-contract defect found during this check was fixed in `a93c797` and pinned by a regression test.
- Connected Expert Profile evaluation to Provider Settings explicitly. Every evaluation now requires one saved, currently tested provider record and a visible pre-run confirmation showing the exact endpoint, model, configuration/key revision, effective generation parameters, last test, and binding digest. The backend rejects stale or untested records before any provider call, activation rechecks the evaluated binding, and each report persists safe provider provenance without the API key.
- Implemented Interview Evaluation v2 tasks 2–4. Version-2 fixtures carry an independent answer shape, a non-empty duplicate-free evidence-contract set, controlled anonymized evidence records, required elements, forbidden expansions, and mandatory-dimension applicability; the fixture digest covers the complete controlled input rather than the question alone.
- Split the evaluator into a safety-first path and an evidence-backed depth path. Injection-canary and structured-output failures stop the run before depth calls; authority-language findings remain visible advisory diagnostics, and the exact safety-suite hash is part of the capability revision. Grounding, authority, past-versus-prospective framing, and directness are mandatory and non-offsetting; depth and concision are aggregated.
- Replaced the unsatisfiable three-case Interview preset with six anonymized questions across all three playbooks (18 depth cases), including documented biography, authority-bounded operations, pressure leadership, a best-practice hypothetical, conditional commitment, and capability-gap reasoning. Junior and Mid-level prompts are now shape-aware. Existing Interview profiles can create an immutable v2 profile-and-plan version through **Upgrade to evaluation v2**; nothing activates automatically.

## Current finding

Broad professional-introduction composition now works against the active context. The five observed answers preserved the important quantitative and authority qualifiers. The authority-warning engine is intentionally narrower: it detects only claims covered by explicitly enrolled rules and remains an advisory review aid, not a factuality certificate. Interview evaluation v1 results cannot rank providers because the old evaluator ignored Provider Settings. Evaluation v2 is now structurally satisfiable and provider-bound, but has not yet had its first non-qualifying tuning run.

## Next

1. **Upgrade the existing Interview lens to evaluation v2.** Use the explicit upgrade action; select the new immutable profile version and v2 eval plan. The prior version remains available and no activation changes automatically.
2. **Run one non-qualifying v2 tuning pass.** Use an explicitly confirmed provider binding and one repetition while tuning; do not reinterpret or compare the unattributable v1 provider runs.
3. **Review all six human dimensions.** Mandatory grounding, authority, past/prospective framing, and directness cannot be offset by high style scores; depth and concision are the weighted quality pair.
4. **Run the qualifying comparison only after the tuning pass is satisfiable.** Use the configured two repetitions, compare providers only by the recorded binding tuple, and activate only the exact passing capability revision.
5. **Continue the real-use loop.** Keep provider, identity version, lens/depth, and compose profile fixed during each trial; record answer fit, continuity, warning usefulness, and missing-context notes.

## Later

- Add durable Live Assist sessions with explicit start/end, resume preview, deletion, persisted relationships, immutable selections, and provenance.
- Promote the Markdown import bridge into full Person/Role/Project selection with freshness/conflict preflight, bundle-aware retrieval, and pinned snapshots.
- Continue evidence-linked meeting intelligence and cited private-document retrieval after real-use evidence identifies the highest-value gaps.
- Complete visible rebranding and isolate inherited cleanup in a separate mechanical change.
- Consider evaluated local model/LoRA bindings only after prompts, retrieval, playbooks, and schemas demonstrate a stable unresolved deficiency.

## Still intentionally not done

- Interview Evaluation v2 has not yet completed a non-qualifying tuning run or a qualifying provider comparison.
- Live Assist exchanges are still memory-only.
- Broad composition currently covers professional-introduction/background questions only; suitability, strengths, motivation, and career-arc questions still use lexical retrieval.
- Only a matching outer emphasis wrapper is normalized. Inline emphasis and code Markdown are still rejected so structurally unsafe provider output fails visibly.
- Full production Project Context architecture is not yet implemented.
- The semantic judge is not a runtime safety oracle; Ghassan remains the final live-use gate.
- Authority warnings are not comprehensive verification. Only explicitly enrolled verb/object/context combinations can match, and advisory activation is version-specific.
