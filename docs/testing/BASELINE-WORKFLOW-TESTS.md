# Baseline workflow tests

These checks protect the existing Meetily workflows while the Meeting Assistant
product is added. They are local-first: tests use temporary SQLite databases,
local fixtures, and a local HTTP mock. They do not call live AI providers.

## Automated checks

The normal locked workspace suite includes:

- meeting, transcript, search, summary, and cascade-delete persistence against a
  fully migrated temporary SQLite database;
- Custom OpenAI summary orchestration against a local mock server, including
  database-backed provider configuration, request shaping, response parsing,
  and completed-summary persistence.

Run all Rust tests:

```powershell
cargo test --workspace --locked
```

Focused commands:

```powershell
cargo test --locked -p meetily --lib database::tests
cargo test --locked -p meetily --lib summary::workflow_tests
```

## Checked-in audio import smoke test

Cloud CI skips this test deliberately. Run it on the reference laptop after
audio decoder, FFmpeg, resampling, or VAD changes.

PowerShell from the repository root:

```powershell
$env:TEST_AUDIO_PATH = (Resolve-Path "frontend/src-tauri/tests/fixtures/jfk.wav").Path
cargo test --locked -p meetily --lib audio::import::tests::test_import_pipeline_decode_vad -- --ignored --nocapture
```

Bash from the repository root:

```bash
TEST_AUDIO_PATH="$PWD/frontend/src-tauri/tests/fixtures/jfk.wav" \
  cargo test --locked -p meetily --lib \
  audio::import::tests::test_import_pipeline_decode_vad -- --ignored --nocapture
```

A passing run must decode the fixture, convert it to 16 kHz mono, and find at
least one non-empty speech segment with valid timestamps for both configured VAD
redemption windows.

This check does **not** load a Whisper or Parakeet model and therefore does not
prove transcription inference. It also does not open a live input/output device.
Those remain separate manual workflow checks; do not describe this test as
decode-to-transcription coverage.

## Hardware-gated checks

The following ignored tests require actual audio devices and should be run on
the relevant reference machine after capture or device-discovery changes:

```powershell
cargo test --locked -p meetily --lib audio::playback_monitor::tests::test_get_output_device -- --ignored --nocapture
cargo test --locked -p meetily --lib audio::system_detector::tests::test_system_audio_detector -- --ignored --nocapture
```

On macOS, also run:

```bash
cargo test --locked -p meetily --lib audio::capture::core_audio::tests::test_core_audio_capture -- --ignored --nocapture
```

Record the operating system, selected devices, transcription provider/model,
and pass/fail result in the PR that changes the affected workflow.
