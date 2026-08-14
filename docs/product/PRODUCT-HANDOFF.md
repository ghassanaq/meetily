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

### Expert profiles and playbooks

- Add schema-validated, declarative Expert Profiles.
- Initial perspectives: Meeting Coach, CEO, Head of Unit/Product, and HR Manager.
- A profile controls objectives, perspective, style, boundaries, retrieval policy, playbooks, and output schema. It grants no real-world authority.
- Profiles contain data only: no scripts, shell commands, native libraries, tools, or hidden permissions.
- Provide create, edit, import, export, activate, version, and delete workflows.
- Profile switches are explicit and should not relabel an answer produced under another profile.

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
- Encrypt transcripts, summaries, document passages, embeddings/indexes, profiles, and settings at rest.
- Keep meeting/document text out of ordinary logs.
- Model output receives no filesystem, network, shell, or application tools.
- Verify downloaded/imported model artifacts by exact manifest and cryptographic digest before activation.

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
