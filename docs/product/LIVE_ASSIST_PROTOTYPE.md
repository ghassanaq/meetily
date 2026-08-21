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

The launcher contains no key and never prints one. It first uses inherited process variables. For absent values, it loads only the allowlisted `MEETING_ASSISTANT_LIVE_API_KEY`, `MEETING_ASSISTANT_LIVE_ENDPOINT`, and `MEETING_ASSISTANT_LIVE_MODEL` names from the repository root's ignored `.env` and `.env.provider` files. It ignores every other name and does not execute or interpolate either file as PowerShell. Keeping the API key in `.env` and the non-secret provider binding in `.env.provider` lets secure key setup update the credential without changing the selected endpoint or model.

Required variable:

- `MEETING_ASSISTANT_LIVE_API_KEY`

Optional variables:

- `MEETING_ASSISTANT_LIVE_MODEL` — defaults to `deepseek-v4-pro`. The experiment uses streaming Chat Completions in non-thinking mode and no tools.
- `MEETING_ASSISTANT_LIVE_ENDPOINT` — defaults to DeepSeek's OpenAI-compatible endpoint at `https://api.deepseek.com/chat/completions`.

Configure these in the Windows user environment or keep the key in the ignored root `.env`, build a release binary once, and then double-click `scripts/start-live-assist.cmd` before a meeting. The launcher checks configuration and the release binary without printing the secret.

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

## Deliberate non-goals

- No full-meeting capture or background speaker detection.
- No incremental transcription while a person is still speaking.
- No exchange persistence, cloud history, or formal Live Assist activation/eval tables. Professional Identity versions are stored locally and immutably.
- No local language-model answer fallback.
- No changes to the existing 600 ms recording mixer window; that pipeline is measured separately before any tuning.
- No automatic provider enablement. Cloud access is an explicit per-launch choice.
- No arbitrary document ingestion or fine-tuning. Professional Identity accepts only its closed declarative schema and uses bounded local retrieval; future fields should still be justified by repeated `Missing context:` evidence.
