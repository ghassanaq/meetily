# Live Assist Prototype

## Purpose

Live Assist is a disposable, personal-use experiment. It tests one question: is a short suggestion useful while a meeting is still moving?

It does not replace or modify Meetily's existing recording, transcription, workspace, summary, Expert Profile, or evidence workflows. It uses a separate Windows loopback stream and keeps its exchanges in memory only.

## Interaction contract

- The dedicated Assist stream retains the latest 60 seconds of system audio in RAM.
- Holding **Ctrl+Alt+Space** starts a new question capture. Four seconds before the press are included. Releasing closes the clip and starts local transcription.
- Holding **Ctrl+Alt+Shift+Space** captures a follow-up. Its parent is the exchange displayed when capture starts; later navigation cannot retarget it.
- The overlay streams a two-or-three-sentence **Say this** suggestion. Longer detail is a separate, on-demand provider request.
- Previous and next navigation allow an interruption to be handled and the earlier answer to be revisited.
- A new capture interrupts an unfinished provider stream. Generation IDs prevent late chunks from replacing the current result.

The capture intentionally records only signaled turns. It does not identify speakers automatically and it does not transcribe the full meeting.

## Privacy rail

Every app launch starts in **Private** mode. A Private exchange is transcribed locally and never sent to a provider. Turning cloud access on or off starts a new ephemeral context. Exchanges captured while Private are never eligible for later cloud context.

The privacy state is always visible in the overlay. If the configured provider is unavailable, the app keeps the transcript and reports the failure; there is no local-LLM fallback in this prototype.

Expert Profiles and Meeting Playbooks only shape the prompt. They remain declarative data and cannot execute tools or scripts. Provider tool-call output is rejected.

## Local configuration

The launcher contains no key and never prints one. It first uses an inherited process variable; if that is absent, it loads only `MEETING_ASSISTANT_LIVE_API_KEY` from the repository root's ignored `.env` file. It does not execute or interpolate the file as PowerShell.

Required variable:

- `MEETING_ASSISTANT_LIVE_API_KEY`

Optional variables:

- `MEETING_ASSISTANT_LIVE_MODEL` — defaults to `gpt-5.6-luna`, OpenAI's current cost-sensitive, high-volume model. The experiment uses streaming Chat Completions and no tools.
- `MEETING_ASSISTANT_LIVE_ENDPOINT` — defaults to the OpenAI-compatible chat-completions endpoint at `https://api.openai.com/v1/chat/completions`.

Configure these in the Windows user environment or keep the key in the ignored root `.env`, build a release binary once, and then double-click `scripts/start-live-assist.cmd` before a meeting. The launcher checks configuration and the release binary without printing the secret.

## Preflight and experiment

A meeting counts as an evaluated meeting only when the overlay says **Armed · receiving** before it begins. The meter reflects the dedicated Assist loopback stream, not the ordinary recording meter.

Run the prototype in five real meetings:

1. Treat meeting one as calibration for learning the signal gesture.
2. In meetings two through five, capture only turns where a suggestion could affect what you say next.
3. After each meeting, record one short usefulness note and whether the timing was acceptable.
4. Use the built-in per-exchange timings for local transcription, cloud first token, and cloud completion.
5. If the stream enters **Stalled**, treat that interval as unevaluable and the meeting as only partially evaluated.

Proceed only if the suggestions are actually used. Revise delivery if the content is useful but late or distracting. Stop the Live Assist slice if the answers are consistently ignored even when correctly captured.

## Deliberate non-goals

- No full-meeting capture or background speaker detection.
- No incremental transcription while a person is still speaking.
- No database persistence, cloud history, or formal Live Assist activation/eval tables.
- No local language-model answer fallback.
- No changes to the existing 600 ms recording mixer window; that pipeline is measured separately before any tuning.
- No automatic provider enablement. Cloud access is an explicit per-launch choice.
