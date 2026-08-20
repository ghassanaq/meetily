# Meeting Assistant session handoff

Date: 2026-08-20  
Repository: `C:\Users\ghass\Projects\Meeting-Assistant`  
Owner and sole intended user: Ghassan Aqrabawi  
Status: implementation handoff; read-only orientation until Ghassan explicitly says **go**

## 0. Mandatory instruction for the next session

Read `docs/product/PRODUCT-HANDOFF.md` completely first and treat it as product direction. Then read this document completely before taking any action.

After reading and inspecting the current repository state:

1. report that the handoff is understood;
2. summarize the current Git state in a few lines;
3. state the first proposed action; and
4. **wait for Ghassan's explicit go signal**.

Do not edit files, change branches, commit, push, open or merge a pull request, launch a build, or begin any implementation before that signal. Read-only inspection is allowed. Do not ask Ghassan to repeat decisions already recorded here.

## 1. Product identity and target experience

This is a fresh personal Meeting Assistant built on Meetily. It is not a restoration of the previous M0/M1 implementation. Preserve all working Meetily functionality:

- microphone and system-audio recording;
- local transcription;
- meeting workspace, notes, history, recordings, and summaries;
- the existing optional provider integrations; and
- the no-bot architecture: no meeting bot joins a call.

The primary new product is **Live Assist**, a private companion Ghassan uses during long meetings, workshops, and interviews. It should feel like an extension of Ghassan rather than a coach speaking to him.

The intended interaction is:

- Live Assist listens through a dedicated local Windows loopback buffer but transcribes only a turn Ghassan explicitly signals.
- Ghassan starts and stops a capture with one easy key, not a key combination and not click-and-hold.
- The captured question is transcribed locally.
- When cloud mode is explicitly enabled, the question and the smallest relevant context are sent to the configured provider.
- The response streams quickly in first-person, ready-to-speak language.
- The assistant carries context through lengthy meetings, understands elaborations and follow-ups, and builds on the conversation without inventing what Ghassan said.
- Professional knowledge comes from explicit local identity, role, and project sources. The selected meeting lens controls reasoning depth and response form.

The product priorities are, in order:

1. ease of use while Ghassan is actively participating in a meeting;
2. useful first-person answers;
3. context building and continuity;
4. trustworthy grounding in Ghassan's own current material;
5. low visible latency; and
6. preservation of the Meetily baseline.

Do not steer this requirement toward a different interaction based on generic UX theory. In particular, Ghassan explicitly wants complete prose answers rather than bullet-point briefing notes.

## 2. Decisions that must be preserved

### 2.1 Capture and answer behavior

- Live Assist is on-demand, not full-meeting transcription and not automatic speaker identification.
- `F8` is the single-key toggle for a new question: press once to start, press again to submit.
- `F9` is the single-key toggle for a follow-up.
- `Escape` discards an active capture.
- Capture includes four seconds of buffered audio before the signal and auto-submits at 50 seconds, before the 60-second buffer can be exhausted.
- Starting another capture must not silently destroy an active capture. Restart/discard remains explicit.
- The overlay must be draggable, hideable, closeable, and single-instance. These behaviors were fixed during the prototype work.
- A launcher at `scripts/start-live-assist.cmd` starts the standalone release build. Do not launch a second instance to restore a hidden overlay; the application has single-instance handling.

### 2.2 Voice and output form

- All primary responses are written as Ghassan's own first-person answer.
- Never emit coaching wrappers such as “You can say,” “Say this,” or “Tell them.”
- Never mention “the assistant,” “the generated answer,” “the previous suggestion,” or similar internal vocabulary in the response region.
- General Guidance remains concise: typically two or three sentences. Its optional detail path currently remains available.
- A specialized lens produces one continuously streamed plain-text paragraph.
- Specialized output has a complete 40–70-word opening lead, then expands naturally in the same paragraph.
- It must not become headings, bullets, numbered talking points, Markdown, or a teleprompter outline.
- Specialized word ranges are soft format targets. Safe completed output is not destroyed merely because it is slightly short or long.
- Coaching/meta-language and unsafe or incompletely terminated primary output remain hard failures.

### 2.3 Lens depth

Professional Identity answers **who is speaking**. A Meeting Lens answers **how to reason and respond in this meeting**. Do not merge these layers.

The first context preset is Interview because it exercises the hardest answer patterns. It has three depth playbooks:

- **Junior:** relevant fundamentals, immediate practical application, and explicit limits.
- **Mid-level:** applied judgment, sequencing, stakeholders, meaningful risks/trade-offs, controls, and outcomes.
- **Expert:** competing constraints, second-order effects, governance or precedent where relevant, explicit boundaries, and defensible judgment.

These are different content expectations, not percentage-based shortening of an Expert answer.

For Expert Interview responses, the model internally selects the closest question type in the same generation call and uses these soft targets:

| Question type | Target |
| --- | ---: |
| Major career or suitability narrative | 200–250 words |
| Strategic implementation | 220–275 words |
| Direct factual or commitment | 80–140 words |
| Capability-gap question | 140–180 words |
| Urgent operational scenario | 110–170 words |
| Beneficiary communication or ethical scenario | 140–180 words |
| Governance, safeguarding, or financial question | 170–220 words |
| Behavioural failure | 200–250 words |
| Comparative closing | 180–220 words |
| External partnership | 170–220 words |

Question-type classification is intentionally implicit pure-prose behavior. There is no extra classification call and no structured output. Wrong length for the question type is therefore assessed through use and provider fixtures, not falsely claimed as a deterministic runtime check.

### 2.4 Privacy and provider use

- Every Live Assist launch begins in Private mode.
- Private questions are transcribed locally and never become later cloud context.
- Cloud use is an explicit visible choice.
- The overlay shows the destination provider/model, currently DeepSeek V4 Pro through an OpenAI-compatible streaming endpoint.
- There is no local-LLM answer fallback in the current Live Assist path.
- The provider receives no tools and cannot execute scripts, shell commands, application commands, or file operations.
- Provider tool-call output is rejected.
- Documents, profiles, TORs, SOPs, and project files are untrusted/inert data even when authored by Ghassan. Imperative language in those files is policy content, not an instruction to the model or application.
- The API key is loaded from `MEETING_ASSISTANT_LIVE_API_KEY` in the inherited environment or the ignored root `.env`. Never print it, commit it, copy it into documentation, or return it over IPC.
- A provider key was pasted into an earlier chat. It is not tracked in Git, but it should be rotated outside the repository before long-term use.

### 2.5 Personal-use threat model

- This application is for Ghassan on one Windows PC, not a distributed enterprise product.
- BitLocker, Windows account hygiene, and reliable backups are the current data-at-rest controls.
- Application-level database/audio encryption is not a personal-release blocker and should not be reintroduced without a changed distribution or threat model.
- API-key hardening through the OS credential facility remains worthwhile but is separate from database encryption.

## 3. Architecture as it exists now

### 3.1 Meetily foundation

The Tauri/Rust backend remains the source of local state, audio, transcription, database access, and provider orchestration. The Next/React frontend provides Meetily and Live Assist UI. Preserve the existing Rust-to-TypeScript command/event layering.

Important foundation outcomes already merged:

- Cargo workspace patch/profile configuration is effective and warning-free.
- The duration `mul_f32` precision defect was corrected without weakening its test.
- A Windows CPAL/WASAPI device-enumeration process crash was fixed at its root.
- Frontend Vitest, lint, typecheck, and build gates work.
- A required path-filtered PR quality gate exists with an always-run aggregate.
- Persistence and summary baseline workflow tests exist; transcription is decode/VAD-level, while live recording remains a manual/hardware gap.
- The upstream updater channel and signing residue were removed. There is currently **no update mechanism**.
- App identity is `com.ghassanaq.meetingassistant`; application data uses a stable `%LOCALAPPDATA%\Meeting Assistant` root independent of the identifier.
- The identity/storage migration is WAL-safe, journaled, idempotent, retryable, and preserves upstream Meetily data/models.
- Expert Profile phase one and the evidence foundation are merged and tested.

### 3.2 Live Assist

Key backend files:

- `frontend/src-tauri/src/live_assist/mod.rs` — state machine, Tauri commands, prompt rendering, validation, capture/provider orchestration, identity/lens wiring.
- `frontend/src-tauri/src/live_assist/capture.rs` — dedicated Assist audio capture/buffer.
- `frontend/src-tauri/src/live_assist/provider.rs` — streaming cloud provider path.
- `frontend/src-tauri/src/live_assist/models.rs` — exchange and timing models.
- `frontend/src-tauri/src/live_assist/voice_harness.rs` — credentialed reference-PC provider checks.
- `frontend/src/app/live-assist/page.tsx` — overlay UI.
- `scripts/start-live-assist.cmd` and `.ps1` — safe local launcher.
- `scripts/test-live-assist-voice.cmd` and `.ps1` — credentialed voice/provider fixture launcher.

Current production-path behavior includes:

- dedicated loopback health that distinguishes real audio faults from ordinary silence;
- toggle capture and explicit discard/restart semantics;
- stream generation IDs so late chunks cannot overwrite a newer answer;
- deterministic hard rejection of coaching/meta-language, structural markers, tool calls, and invalid completion;
- harmless whitespace normalization;
- soft word-count format warnings;
- first-person prompt contract;
- local transcription and streaming DeepSeek output; and
- per-exchange timing and build-revision telemetry.

Observed reference-PC behavior from hands-on tests:

- local transcription around 713 ms for a test clip;
- first cloud token around 1,487 ms after request;
- roughly 2.2 seconds for those measured components combined; and
- responses were judged useful and timely by Ghassan.

Do not generalize these measurements beyond the reference PC or conceal orchestration/UI time. The app has more complete stop-to-visible telemetry for trials.

### 3.3 Professional Identity

Key files:

- `frontend/src-tauri/src/professional_identity/mod.rs`
- `frontend/src-tauri/src/professional_identity/repository.rs`
- `frontend/src-tauri/src/professional_identity/commands.rs`
- `frontend/src/components/ProfessionalIdentitySettings.tsx`
- migration `frontend/src-tauri/migrations/20260818000000_add_professional_identities.sql`

The merged implementation provides:

- strict, declarative, immutable/versioned identity data;
- a friendly settings UI rather than requiring raw JSON editing;
- explicit Live Assist identity selection and pinning;
- bounded deterministic local retrieval rather than sending the entire profile;
- expiry filtering;
- fail-closed blocking of relevant current `conflict_key` collisions before the provider call;
- a local grounding line derived from selected source metadata, never authored by the model; and
- preservation of authority boundaries.

Ghassan has manually confirmed that the profile affects Live Assist responses.

### 3.4 Expert Profile / Meeting Lens foundation

Key files:

- `frontend/src-tauri/src/expert_profiles/`
- `frontend/src-tauri/src/database/repositories/expert_profile.rs`
- `frontend/src/components/ExpertProfilesSettings.tsx`
- migration `frontend/src-tauri/migrations/20260815000000_add_expert_profiles.sql`
- design `docs/product/EXPERT_PROFILES_DESIGN.md`

The phase-one “Expert Profile” storage is currently the lens/playbook layer despite its historical name. It is declarative, versioned, immutable, evaluated, and activated. Do not treat it as Ghassan's biography or professional authority.

The summary evaluator and Live Assist are different generation paths:

- summary evaluation calls the production summary processor;
- Live Assist calls `live_assist::provider::stream_chat` with a different prompt and validator.

A summary-capability evaluation must never be presented as proof that a Live Assist lens works.

### 3.5 Evidence foundation

Design and code for immutable transcript/audio-time evidence, citation resolution, retranscription invalidation, and closed document locators are merged. This remains a deferred track. Do not delete it, and do not force the current Live Assist flow back into constrained structured generation.

The structured-generation spike showed that full enumerated GBNF was prohibitively slow on the exact tested CPU configuration. Its measurements and sampler fix were preserved on `codex/structured-generation-spike`. That does not establish a universal GPU requirement, but it redirected the personal product toward the much more useful on-demand Live Assist flow.

## 4. Current Git state — do not blur these layers

At handoff creation:

- checked-out branch: `codex/single-key-live-assist`;
- local base/tracking ref: `origin/main` at `7c58ca2`;
- branch is two commits ahead of `origin/main`;
- working tree is intentionally not clean because the Project Context spike is uncommitted;
- `.claude/` is user-local state and must remain untouched and untracked.

The two committed but not yet merged commits are:

1. `31dcc1b feat: use single-key Live Assist capture shortcuts`
2. `fa35824 feat: add interview depth lens preset`

The uncommitted Project Context spike consists of:

- modified `Cargo.lock`;
- modified `frontend/src-tauri/Cargo.toml` adding test-only `serde_yaml_ng = "0.10"`;
- untracked `docs/product/PROJECT_CONTEXT_DESIGN.md`;
- untracked `docs/product/PROJECT_CONTEXT_SPIKE_RESULTS.md`;
- untracked `frontend/src-tauri/tests/project_context_retrieval.rs`; and
- untracked synthetic fixtures under `frontend/src-tauri/tests/fixtures/project_context/`.

This handoff file is also newly added and uncommitted.

Do not mix the two existing Live Assist commits with the Project Context spike in one PR. Do not discard the spike. Do not add `.claude/`.

The repository remotes are intentionally:

- `origin`: Ghassan's fork, fetch and push;
- `upstream`: Zackriya-Solutions/meetily, fetch-only with push disabled.

## 5. Project Context design and spike

The latest product request is to let Ghassan point the app at local project/role material, similar to a project context file, instead of manually recreating all context in Settings.

The agreed layering is:

- **Person Identity:** who Ghassan is — CV, experience, qualifications.
- **Role Context:** role-wide TOR, responsibility, authority, approval limits, reporting, policies.
- **Project Context:** project-specific status, commitments, deadlines, risks, stakeholders, references.
- **Meeting Lens:** reasoning depth, priorities, style, and answer form.
- **Session Memory:** continuity within the current meeting.

A session selects exactly one Person bundle, exactly one Role bundle, and zero or more Project bundles. Identity and role material must not be duplicated into every project.

The uncommitted design specifies:

- a strict JSON selection manifest;
- explicit relative file paths under a selected context root;
- rejection of absolute paths, traversal, URLs, globs, executables, and scripts;
- canonical user-authored Markdown with strict typed YAML frontmatter;
- bundle-derived passage kinds: `person_fact`, `role_policy`, `project_fact`;
- deterministic heading/paragraph chunking and content hashes;
- provenance, revision, update, expiry, and conflict metadata;
- deterministic local weighted lexical retrieval;
- expiry before scoring;
- relevant explicit conflict blocking before provider use;
- a score floor so a result limit is a ceiling rather than a quota;
- an immutable context snapshot pinned to a session; and
- capability hashes that cover renderer/parser/chunker/retrieval policy versions but not the current identity/project content hash.

The provider-free CI-eligible spike uses an anonymized synthetic Person bundle, Role bundle, and Atlas/Beacon projects. It tests ten questions at result limits 3, 5, and 8.

Measured spike result:

- all ten cases passed at all three limits;
- expected passage rank was first or second;
- zero irrelevant passages were selected;
- 1–2 passages and 30–78 words were selected per question;
- zero single-project topic bleed;
- the cross-project question selected one correct passage from each project;
- expired content was excluded;
- a relevant current conflict blocked before selection;
- an unrelated conflict did not block or enter the results; and
- the selector preserved imperative role policy as typed inert data.

Provisional maximum result limit: 3. The score floor made limits 3, 5, and 8 select the same useful set.

The spike corrected two real design defects:

1. generic body-word overlap was too broad for determining conflict relevance; conflict relevance now uses meaningful terms from the explicit conflict key;
2. filling every available result slot introduced irrelevant material; candidates below half the strongest score are removed before the maximum limit is applied.

This proves mechanics only on representative fixtures. It does **not** prove lexical retrieval will be sufficient for Ghassan's real CV, TOR, guides, and project material.

Latest verification completed before handoff:

- `cargo metadata --locked --no-deps` passed;
- focused Project Context integration tests passed 6/6;
- exact-current-tree `cargo test --workspace --locked` passed;
- Meetily library: 314 passed, 4 ignored;
- llama helper: 2 passed;
- Project Context integration: 6 passed;
- doc tests: 1 passed; and
- no production Live Assist code was changed by the spike.

## 6. Session memory and context-awareness contract — designed, not implemented

Ghassan's key reframe is that this assistant is his meeting companion during lengthy discussions. Context and continuity are core product behavior, not optional polish.

The intended durable session model is:

- a session has an explicit start and explicit end;
- an interrupted/restarted app offers a resume preview showing the meeting start and recognizable first question;
- stale sessions require confirmation rather than silently contaminating a new meeting;
- deleting a session removes its locally persisted exchange/context state;
- questions and explicit relationship edges are ground truth;
- inferred “meeting direction” is weak, disposable, and rebuilt from persisted questions/relationships rather than stored as truth;
- ambient cloud context uses only eligible questions, not generated suggestions;
- an explicit follow-up freezes its `parent_exchange_id` when capture begins;
- a prior generated suggestion may be passed only as clearly labelled unspoken draft context;
- current question and explicit parent dominate inferred direction;
- elaboration requests should understand verbs such as elaborate, clarify, explain why, give an example, compare, apply, and expand;
- a user can explicitly **Adopt position**, which carries a substantive stance forward without claiming the exact words were spoken;
- adopted positions are session-scoped by default;
- promoting a position to Professional Identity creates a new immutable identity version with provenance/expiry/conflict handling; and
- changing identity data must not invalidate a Live Assist lens activation. Capability hashes cover renderer/retrieval policy, not retrieved content instances.

No implicit signal such as display time, scrolling, or navigation may be treated as proof that Ghassan spoke or adopted a suggestion.

## 7. Ordered next steps after Ghassan says go

### Step 1 — separate and preserve the current Git work

Goal: make both workstreams independently reviewable without losing anything.

1. Re-inspect status and confirm the hashes/state above.
2. Preserve `.claude/` untouched.
3. Keep `31dcc1b` and `fa35824` together as the single-key shortcuts + Interview lens PR.
4. Put the Project Context design, spike tests, fixtures, manifest dependency, lockfile, results document, and this handoff into a separate branch/commit/PR.
5. Rebase the Project Context work onto current `main` after the Live Assist PR merges if necessary.
6. Run the appropriate required gates for each PR.

Do not combine these into a large PR.

### Step 2 — build and privately validate Ghassan's real context corpus

Goal: test the design against the material that will actually govern answers before changing production Live Assist.

1. Create a local private context root outside Git, preferably under `%LOCALAPPDATA%\Meeting Assistant\contexts` or another folder Ghassan selects.
2. Convert real source material into canonical Markdown:
   - one Person bundle from the CV/professional history;
   - one Role bundle from the current TOR, authority, responsibilities, reporting, and policies;
   - one or more Project bundles containing current status, commitments, deadlines, risks, stakeholders, and references.
3. Never commit real CV, TOR, confidential project, or meeting content.
4. Create 10–20 representative questions from real meetings with expected passage IDs.
5. Run the same provider-free recall/precision tests and record:
   - rank of each expected passage;
   - irrelevant passages returned;
   - topic bleed across projects;
   - expired-source exclusion;
   - conflict behavior; and
   - selected context size.
6. Keep the anonymized suite as the permanent CI mechanics test.
7. If lexical retrieval fails, preserve the corpus/provenance contract and change only the retrieval mechanism.

### Step 3 — implement durable meeting sessions

Goal: context must survive a crash/restart during a long meeting without leaking into the wrong meeting.

Implement and test:

- session lifecycle and explicit End session;
- restart/resume preview and stale-session confirmation;
- local persistence for questions, answers, relationships, selections, adopted positions, and provenance;
- session deletion;
- immutable pinned identity/role/project/lens snapshot references;
- context-generation boundaries when privacy, identity, lens, or project selection changes; and
- reconstruction of weak direction inference from durable ground truth.

This is deliberately before production context-aware generation because later steps depend on durable state.

### Step 4 — integrate Project Context into production Live Assist

Goal: Ghassan points Live Assist at local declarative context files and receives grounded answers without uploading the whole corpus.

Implement and test:

- pre-meeting selection of one Person, one Role, and zero+ Project bundles;
- validation and preflight summary of sources, revisions, age, expiry, and conflicts;
- immutable session snapshot pinning;
- bounded local retrieval for the current question and explicit follow-up context;
- provider payload containing only selected typed passages and compact metadata;
- passive grounding display under the answer with source/revision/freshness;
- fail-closed conflict handling before any provider request;
- expired-source exclusion;
- no path traversal, URLs, executables, scripts, tools, or hidden capabilities; and
- no automatic ingestion of arbitrary folders.

### Step 5 — add meeting continuity and context awareness

Goal: the assistant can answer expansions and hard follow-ups immediately while keeping the current question dominant.

Implement and test:

- explicit parent/relationship edges;
- relevant prior-question selection based on meaning, not only recency;
- questions-only ambient history;
- elaboration intent handling;
- thread navigation without retargeting in-flight work;
- Adopt position and promote-to-identity flows;
- weak inferred direction with visible/inspectable boundaries;
- no fabricated speech history; and
- latency telemetry split between first-in-thread and deep-session elaborations.

Do not treat the captured subset of hard questions as a complete unbiased transcript of the meeting. Direction inference must remain subordinate to the current question and explicit context.

### Step 6 — add a real Live Assist capability evaluation path

Goal: evaluation must qualify the path users actually run.

After the production prompt/context design stabilizes:

- add a `capability` discriminator to the existing Expert Profile eval/activation structures rather than creating parallel assist tables;
- preserve existing `meeting_summary` activations through the SQLite table rebuild with a migration test;
- make CI test the real Live Assist prompt renderer, selected playbook, and deterministic validators without credentials;
- keep provider behavior in the credentialed reference-PC harness;
- require real provider evidence fields for any activation-qualifying run, such as provider/model, endpoint host, finish reason, usage, request identifier/fingerprint, prompt-template hash, timestamp, and output;
- make it structurally impossible for deterministic CI/mock output to qualify a provider capability; and
- never use the summary generation evaluator as evidence for Live Assist.

### Step 7 — use and refine

Run real long-meeting trials and record only signals Ghassan can realistically maintain:

- Was the answer useful/used?
- Was question-type fit too short, appropriate, or too long?
- Was continuity correct, wrong, or unnecessarily repetitive?
- `Missing context:` one line for each inadequate exchange.
- Was latency acceptable for first-in-thread and deep-session elaboration?

Use repeated missing-context evidence to improve retrieval and visible fields. Do not continuously expand the schema based on one imagined scenario.

## 8. Known gaps and debt

These are real but are not permission to derail the ordered plan:

- Live Assist session exchanges are still in memory only.
- Production Project Context file selection/retrieval is not implemented; only the design and provider-free spike exist.
- The Interview lens and F8/F9 commits are not merged at this handoff.
- Live Assist has no capability-specific activation/eval path yet.
- The PR Rust gate is Windows-only; macOS/Linux `cfg` paths are not compiled on PRs.
- Live recording hardware behavior remains a manual test gap.
- The updater is intentionally disabled; shipped builds currently have no automatic update path.
- `productName` may still display Meetily even though the identifier/storage identity changed. Treat visible rebranding as separate from storage identity.
- The API key is still environment/ignored-file based rather than OS-credential-backed.
- A real identity/storage migration run against Ghassan's actual existing profile, with a backup, remains advisable before a release.
- `live_assist/mod.rs` is large and combines several responsibilities. Refactor only when a concrete feature boundary requires it; do not make refactoring an open-ended prerequisite.
- The repository still contains inherited dead files/artifacts identified in earlier review, including old Rust backups/modules and a tracked `vs_buildtools.exe`. Cleanup should be a bounded mechanical PR, not mixed with product work.
- Line-ending warnings and the user-global ignore warning can generate noise. Do not normalize or reformat the whole repository during feature work. A deliberate `.gitattributes`/cleanup PR can handle that later.

## 9. Verification commands and expectations

Before changing anything, inspect:

```powershell
git status --short --branch
git log --oneline --decorate -15
git diff --stat
git diff --name-only
```

Rust baseline:

```powershell
cargo metadata --locked --no-deps
cargo test --workspace --locked
```

Focused current spike:

```powershell
cargo test -p meetily --test project_context_retrieval
```

Frontend gates from `frontend/` use the scripts pinned in `package.json`/CI: lint, typecheck, Vitest, and build. Use the repository's pnpm version from `packageManager`; do not separately pin a conflicting workflow version.

A launchable desktop binary must be built through Tauri with the exported frontend and custom protocol. Do not rely on plain `cargo build --release`, which can retain the localhost development URL.

## 10. Files to read for decisions, in order

1. `docs/product/PRODUCT-HANDOFF.md`
2. `docs/product/SESSION_HANDOFF_2026-08-20.md`
3. `docs/product/LIVE_ASSIST_PROTOTYPE.md`
4. `docs/product/PROFESSIONAL_IDENTITY_AND_LENSES_DESIGN.md`
5. `docs/product/PROJECT_CONTEXT_DESIGN.md`
6. `docs/product/PROJECT_CONTEXT_SPIKE_RESULTS.md`
7. `docs/product/EXPERT_PROFILES_DESIGN.md`
8. `docs/product/EVIDENCE_LINKED_INTELLIGENCE_DESIGN.md`

## 11. Final instruction

The next session is not being asked to start implementation immediately. It is being asked to inherit the product accurately.

Read, inspect, report readiness, propose only the first action from Step 1, and wait for Ghassan to say **go**.
