# Live Assist Prototype

## Purpose

Live Assist is a disposable, personal-use experiment. It tests whether a ready-to-speak response is useful while a meeting is still moving.

It does not replace or modify Meetily's existing recording, transcription, workspace, summary, Expert Profile, or evidence workflows. It uses a separate Windows loopback stream and keeps its exchanges in memory only.

## Interaction contract

- The dedicated Assist stream retains the latest 60 seconds of system audio in RAM.
- Pressing **F8** once starts a new question capture. Four seconds before the press are included. Pressing **F8** again closes the clip and starts local transcription.
- Pressing **F9** once starts a follow-up. Pressing **F9** again closes the clip and starts local transcription. Its parent is the exchange displayed when capture starts; later navigation cannot retarget it.
- F8 and F9 are registered only while the Live Assist overlay is armed. Hiding or closing the overlay disarms audio and releases both system-wide keys; reopening and successfully arming the overlay reserves them again.
- **Escape** discards an active capture. Restart is explicit and never silently replaces or submits the wrong clip. A capture auto-submits at 50 seconds, before the 60-second RAM window can be exhausted.
- **General guidance** streams a two-or-three-sentence first-person response written as the user's own ready-to-speak words. It contains no coaching labels or instructions. Longer detail remains a separate, on-demand provider request for this general mode.
- An explicitly selected **specialized lens** streams one continuous first-person plain-text paragraph of 200–300 words. Its first two sentences are prompted as a 40–70-word complete lead, followed by a natural expansion in the same paragraph. Headings, bullets, numbered lists, line breaks, Markdown, coaching labels, and assistant meta-language are forbidden. The legacy detail request is hidden and rejected for specialized responses.
- A primary answer must finish normally and pass its applicable deterministic safety checks. Unsafe or incomplete primary streams are discarded. Specialized whitespace is normalized, while word-count drift is retained and recorded for prompt tuning rather than destroying a usable answer. General-mode detail stopped explicitly by the provider's token limit may remain visible only with a **Partial detail** warning. Unknown, malformed, or network-failed streams are discarded.
- Previous and next navigation allow an interruption to be handled and the earlier answer to be revisited.
- A new capture interrupts an unfinished provider stream. Generation IDs prevent late chunks from replacing the current result.

The capture intentionally records only signaled turns. It does not identify speakers automatically and it does not transcribe the full meeting.

## Privacy rail

Every app launch starts in **Private** mode. A Private exchange is transcribed locally and never sent to a provider. Turning cloud access on or off starts a new ephemeral context. Exchanges captured while Private are never eligible for later cloud context.

The privacy state is always visible in the overlay. If the configured provider is unavailable, the app keeps the transcript and reports the failure; there is no local-LLM fallback in this prototype.

The existing Expert Profiles and Meeting Playbooks are optional, explicitly selected expert lenses. They shape objectives, style, boundaries, and playbook guidance; they are not treated as the user's identity, biography, authority, or meeting history. They remain declarative data and cannot execute tools or scripts. Provider tool-call output is rejected.

Professional identity is a separate product layer defined in [PROFESSIONAL_IDENTITY_AND_LENSES_DESIGN.md](PROFESSIONAL_IDENTITY_AND_LENSES_DESIGN.md). A selected immutable identity version supplies locally retrieved CV, TOR, project, authority, stakeholder, or commitment records. The passive grounding line names only the actual local sources and freshness metadata used for that exchange; it is never model-authored.

## Local configuration

Use **Settings → Providers** for normal configuration. It supports DeepSeek, Kimi/Moonshot, OpenAI, and custom OpenAI-compatible providers. A provider is saved first, tested with a bounded tool-free request, and then explicitly activated. Changing its kind, endpoint, model, or key invalidates the prior test and deactivates it until it passes again. The active provider cannot be deleted until another provider is activated.

API keys are stored in Windows Credential Manager. SQLite stores only provider metadata and a credential revision. The frontend receives only whether a key is configured; it never receives or displays the saved key. Leaving the key field blank while editing preserves the existing key, while entering a replacement safely advances the credential revision.

The launcher contains no key and never prints one. Before any UI-managed provider exists, it can bootstrap from inherited process variables or the allowlisted `MEETING_ASSISTANT_LIVE_API_KEY`, `MEETING_ASSISTANT_LIVE_ENDPOINT`, and `MEETING_ASSISTANT_LIVE_MODEL` names in ignored `.env` and `.env.provider` files. It ignores every other name and does not execute or interpolate either file as PowerShell. Once the first UI-managed provider is saved, this environment fallback is disabled so an old key cannot silently become active.

Required variable:

- `MEETING_ASSISTANT_LIVE_API_KEY`

Optional variables:

- `MEETING_ASSISTANT_LIVE_MODEL` — defaults to `deepseek-v4-pro`. The experiment uses streaming Chat Completions in non-thinking mode and no tools.
- `MEETING_ASSISTANT_LIVE_ENDPOINT` — defaults to DeepSeek's OpenAI-compatible endpoint at `https://api.deepseek.com/chat/completions`.

Kimi K3 is also supported through `https://api.moonshot.ai/v1/chat/completions` with model `kimi-k3`. Because K3 always reasons, its adapter uses `reasoning_effort: low`, omits unsupported sampling parameters, and reserves a separate hidden-reasoning token allowance without changing the visible-answer contract. Live Assist consumes only final-answer `content` deltas and continues to reject tool calls.

The environment variables above are compatibility/bootstrap settings, not the preferred switching workflow. Build a release binary once and then double-click `scripts/start-live-assist.cmd` before a meeting. The launcher checks configuration and the release binary without printing a secret.

Build a standalone executable through Tauri from `frontend/` with `pnpm exec tauri build --no-bundle`. Do not use plain `cargo build --release` for a launchable app: it retains `build.devUrl` and expects the Next development server on localhost. Direct Cargo builds must explicitly enable `--features custom-protocol` after the frontend export exists.

## Preflight and experiment

A meeting counts as an evaluated meeting only when the overlay says **Armed · receiving** before it begins. The meter reflects the dedicated Assist loopback stream, not the ordinary recording meter.

Run the prototype in five real meetings:

1. Treat meeting one as calibration for learning the signal gesture.
2. In meetings two through five, capture only turns where a suggestion could affect what you say next.
3. After each meeting, record whether the response was useful, whether the timing was acceptable, and one `Missing context:` note for every inadequate exchange.
4. Use the built-in per-exchange timings for stop-to-visible text, local transcription, cloud first token, and cloud completion. Stop-to-visible includes provider streaming, the 250 ms snapshot polling interval, React rendering, and paint acknowledgement; component timings remain available for diagnosis.
5. Every exchange displays the embedded Git build revision so trial results remain attributable to the exact executable that produced them.
6. Silence is not a fault. If the CPAL stream reports **Audio fault**, treat that interval as unevaluable and the meeting as only partially evaluated.

Proceed only if the suggestions are actually used. Revise delivery if the content is useful but late or distracting. Stop the Live Assist slice if the answers are consistently ignored even when correctly captured.

Provider behavior is measured separately from deterministic CI. Run `scripts/test-live-assist-voice.cmd` on the reference PC to exercise the production prompt and streaming provider path against the synthetic fixtures. The ignored harness rejects coaching prefixes and assistant meta-language, verifies that an unspoken draft is not converted into meeting history, and appends timestamped prompt/model/latency results under the ignored `target/` directory without recording the API key. A separate private run may set `MEETING_ASSISTANT_LIVE_HARNESS_PROFILE_PATH` to an ignored workload JSON file. That file may reference only relative manifest, bundle, and Markdown paths below its own corpus root; the harness rejects absolute paths and traversal and preserves Markdown sections as separately attributable identity records. Its ignored JSONL result stores the questions, generated answers, audits, and retrieved IDs, but does not serialize the raw identity object or source documents.

Authority-scope warnings remain offline during their evidence gate. Put a private authority workload under ignored `experiments/` or `target/`, set `MEETING_ASSISTANT_AUTHORITY_TRIAL_PATH` to it, and run `cargo test --lib live_assist::voice_harness::authority_scope_private_trials -- --ignored --nocapture` from `frontend/src-tauri`. The closed workload schema requires schema version 1, a relative version-2 context manifest, the immutable identity-version hash expected from that manifest, and at least five cases. Each case contains a unique ID, an answer copied from the real Live Assist app, `captured_from_live_assist: true`, and human adjudication of either `warning_expected` or `no_warning_expected`. Never put the private workload, its answers, corpus paths, sidecar, or API credentials in Git or documentation.

The authority trial ledger defaults to ignored `target/authority-scope-private-trials.jsonl` and can be redirected with `MEETING_ASSISTANT_AUTHORITY_TRIAL_OUTPUT`. It stores only case ID, answer hash, immutable identity-version hash, matched rule IDs, warning codes, TP/TN/FP/FN outcome, and timestamp. Duplicate answer hashes are not counted again. The printed report contains counts plus identity/rule hashes, precision and recall when defined, and an offline-gate boolean; five trials are a product gate rather than statistical proof. Even a passing report keeps runtime activation false, and no warning is exposed in Live Assist until a separately approved production checkpoint.

The first private gate completed on 2026-08-22 with five distinct answers: TP=0, TN=5, FP=0, FN=0. The zero-false-positive offline gate passed for identity hash `sha256:6480011cef0620d35f4c8899ce2f99ee457d81d0807e51c96705c7bf4b3426f3` and rule-set hash `sha256:4cc45b5c01ecb5d472185597ec783708cf25f932561f925781a03e2a122c4dd7`. Precision and recall were undefined because all five human adjudications expected no warning; this is not evidence of real-answer sensitivity, which remains synthetically tested. Runtime activation remained false.

For a real trial, open the Professional Identity manager and choose `Import Markdown context`, then select the private `meeting-assistant.context.json`. Production uses the same bounded parser as the private harness, copies the resulting records into one immutable local identity version, selects that exact version in Live Assist, and retains no link or watcher to the source folder. Re-import the manifest deliberately after editing the private Markdown corpus.

## Deliberate non-goals

- No full-meeting capture or background speaker detection.
- No incremental transcription while a person is still speaking.
- No exchange persistence, cloud history, or formal Live Assist activation/eval tables. Professional Identity versions are stored locally and immutably.
- No local language-model answer fallback.
- No changes to the existing 600 ms recording mixer window; that pipeline is measured separately before any tuning.
- No automatic provider enablement. Cloud access is an explicit per-launch choice.
- No arbitrary document ingestion, folder crawling, live file watching, or fine-tuning. Professional Identity imports only an explicitly selected closed-schema context manifest whose relative JSON/Markdown dependencies remain below its corpus root, then uses bounded local retrieval; future fields should still be justified by repeated `Missing context:` evidence.
