# Meeting Assistant product handoff

## Foundation

The product is now based on the MIT-licensed Meetily project. Preserve its working baseline before customization:

- microphone and system-audio recording;
- live local transcription;
- meeting workspace, notes, history, and recordings;
- local AI summaries and the existing optional provider integrations;
- no meeting bot joining the call.

The previous custom M0/M1 capture implementation is intentionally not migrated. It was a research and validation effort, not the new product foundation.

## Product direction

Extend Meetily with the following capabilities without removing its existing features.

### Deployment assumption

- This product is a single-user personal desktop assistant for the repository owner.
- It is not currently a multi-user, enterprise-managed, or generally distributed service.
- Prefer useful local workflows and recoverable backups over application-specific encryption infrastructure.
- Revisit the threat model before distributing the application to other users or organizations.

### Expert profiles and playbooks

- Keep professional identity and meeting behavior as two explicit, independently versioned layers.
- A **Professional Identity Profile** represents the user for a meeting. It may contain the user's CV, TORs, responsibilities, authority and approval limits, stakeholders, active projects, current commitments, and dated source records. It records authority; it does not grant authority.
- An **Expert Lens / Meeting Playbook** controls how the selected identity responds: objective, perspective, tone, boundaries, response length and form, retrieval policy, and meeting-specific guidance. Initial lenses include Meeting Coach, CEO, Head of Unit/Product, HR Manager, and Head of Mission workshop use.
- The existing phase-one Expert Profile schema remains the declarative lens layer until a reviewed rename/migration separates its terminology in storage and UI.
- Profiles and lenses contain data only: no scripts, shell commands, native libraries, tools, network targets, or hidden permissions.
- Provide create, edit, import, export, activate, version, and delete workflows.
- Identity and lens switches are explicit and should not relabel an answer produced under another selection.

### Evaluation-first customization

- Every active profile/capability must have a nonempty evaluation suite.
- Test role-policy adherence, declared objectives/style/boundaries, grounding, schema compliance, and non-escalation of authority.
- Preserve held-out evaluation data and compare changes against the current baseline.
- A model or prompt change must not activate if it regresses an enabled capability.

### Evidence-linked meeting intelligence

- Detect and support decisions, actions, direct questions, risks, topic changes, and coaching suggestions.
- Link each claim or suggestion to immutable transcript evidence.
- Corrections must invalidate affected derived artifacts; summaries can be regenerated against the corrected transcript.
- Distinguish facts, assumptions, analysis, recommendations, and suggested wording in the UI.

### Private document retrieval

- Start with explicitly selected text PDFs.
- Preserve document revision, page/section, and passage provenance.
- Answers must cite retrieved passages and abstain when evidence is missing, conflicting, stale, or insufficient.
- Treat document and transcript content as untrusted evidence, never as system instructions.
- Add prompt-injection fixtures, deletion/reindex behavior, and retrieval/citation evaluation gates.

### Model customization and fine-tuning readiness

- Use prompts, playbooks, retrieval, and schemas as the default customization mechanisms.
- Support trusted external GGUF and optional LoRA bindings only after compatibility, resource, grounding, schema, and capability-regression evaluations pass.
- Keep the prior verified binding for one-step rollback.
- Record exact model, adapter, tokenizer, template, runtime, and content hashes.
- Do not describe arbitrary third-party model files as sandboxed merely because they are hashed.
- Build an in-app fine-tuning factory only after held-out evaluation proves a stable deficiency that prompting, retrieval, and playbooks cannot solve.

### Privacy and security

- Keep local processing as the default; cloud providers remain explicit opt-in integrations.
- Store meeting data in the user's local workspace and recommend operating-system disk encryption and reliable backups; application-level database/audio encryption is optional and is not a personal-release blocker.
- Store provider API secrets through the operating-system credential facility and return only configured/not-configured metadata over frontend IPC. The Windows Provider Settings implementation now follows this rule.
- Keep meeting/document text out of ordinary logs.
- Model output receives no filesystem, network, shell, or application tools.
- Verify downloaded/imported model artifacts by exact manifest and cryptographic digest before activation.

## Current delivery status — 2026-08-22

The working Meetily baseline, F8/F9 Live Assist capture, Interview lens, production Markdown context import bridge, OpenAI-compatible provider adapters, offline evaluation lifecycle, and Windows Provider Settings UI are implemented and locally committed. Provider Settings supports presets and custom OpenAI-compatible endpoints, secure Windows credential storage, bounded connection testing, explicit activation, safe replacement/removal, and active-provider display. Expert Profile evaluation now binds explicitly to one saved and currently tested Provider Settings record, shows the exact safe binding before a paid run, and persists that binding without storing or hashing the API key.

The Kimi–DeepSeek comparison is complete. Both answers were grounded but too narrow for the broad `Tell us about yourself` prompt, and both returned Markdown formatting. The shared failure indicates a retrieval/composition defect rather than a provider-selection problem: literal token-overlap retrieval does not recognize a generic interview prompt as a request for broad career evidence.

The former dirty implementation tree has been separated into eight coherent local commits without rewriting the six earlier commits. The immediate delivery sequence is now:

1. add deterministic, lens-aware composition for broad interview questions;
2. reject or safely normalize inline Markdown and preserve shared-authority qualifiers;
3. verify the change with provider fixtures and the private imported corpus without tracking private answers; and
4. run a mock interview followed by the real five-use learning loop.

Broad professional introductions, streaming plain-text normalization, and the authority-scope
warning safeguards are now implemented. Authority rules are explicit identity-version data; new
constrained versions evaluate offline by default, and advisory display requires typed activation
for that exact immutable hash. Runtime matching happens locally only after answer completion.
Warnings are non-blocking, highlight only the matched excluded-object span, can be dismissed only
for the current exchange, and expose evidence excerpts only after a separate user action. A clean
indicator means only that no enrolled rule matched.

Detailed completion state and roadmap are maintained in [CURRENT_STATUS_AND_ROADMAP.md](CURRENT_STATUS_AND_ROADMAP.md).

## Delivery order

1. Verify the unchanged Meetily workflow on the reference Windows laptop.
2. Establish automated baseline tests for recording, transcription, workspace persistence, and summaries.
3. Rebrand without altering behavior.
4. Add Expert Profile data models, editor, and evaluation harness.
5. Add evidence-linked meeting intelligence and coaching.
6. Add cited private-document retrieval.
7. Add evaluated model/LoRA binding, activation, and rollback.
8. Consider in-app fine-tuning only when its evaluation trigger is met.

## Repository policy

- `origin` is the user-owned GitHub fork.
- `upstream` is `Zackriya-Solutions/meetily` and must remain fetch-only.
- Keep upstream updates reviewable; avoid mixing generated build artifacts or local model/audio data into Git.
- Retain Meetily's MIT license and required attribution.
