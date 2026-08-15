# Expert Profiles & Meeting Playbooks — Phase 1 Contract

Status: **Accepted design contract — implementation pending**
Scope: delivery item 4 in [PRODUCT-HANDOFF.md](PRODUCT-HANDOFF.md)
Relation to existing code: this feature extends the existing summary-template and summary-generation paths. It must preserve every working Meetily workflow.

---

## 1. Phase 1 outcome

Phase 1 delivers locally stored, declarative Expert Profiles with embedded Meeting Playbooks. A user can create, edit, version, import, export, evaluate, activate, select, retire, and delete them. One explicitly selected profile/playbook can be applied manually to the existing summary workflow.

An active profile is not just prose. It is an evaluated binding between:

- one immutable profile version;
- all embedded playbook UUIDs from that version, with one selected explicitly for each generation;
- the prompt renderer and output-contract versions;
- the exact model/generation configuration used by production; and
- the evaluation plan and application-owned safety gate that passed.

Changing any member creates a different effective capability. Output is labeled with the exact capability that produced it.

Phase 1 does **not** implement automatic triggers, independent/reusable playbook entities, document retrieval, marketplace sharing, cloud sync, tools, autonomous actions, model/LoRA activation, or fine-tuning.

---

## 2. Non-negotiable constraints

1. Profiles and playbooks contain data only. Their schemas expose no scripts, expressions, shell commands, native libraries, executable hooks, tools, network targets, filesystem paths, or permission grants.
2. Model output has no filesystem, network, shell, application-tool, or state-mutation capability. It is parsed and rendered as inert summary data.
3. Every active capability has a non-empty user evaluation plan and must also pass the versioned application safety gate.
4. A failing, regressing, unresolved, or inconclusive candidate never activates.
5. Evaluations and production summaries share the same rendering and generation core. Evaluation disables normal summary persistence but does not substitute a different prompt or provider path.
6. Multiple profiles may be available simultaneously. Each profile has at most one active version/binding. Selecting a profile for generation is a separate, explicit action.
7. Switching profile, playbook, renderer, or model never relabels existing output.
8. Cloud providers remain explicit opt-in integrations. Evaluation never sends meeting content to a cloud provider without the same provider consent required for production generation.
9. Imported bundles are untrusted data, are fully validated before persistence, and never activate automatically.
10. Personal-use builds follow the local storage and operating-system security posture in section 9; application-level encryption is not an activation or release prerequisite.

---

## 3. Identity, versioning, and state

### 3.1 Entities

| Entity | Identity | Mutability |
|---|---|---|
| `ExpertProfile` | Stable UUIDv4 | Registry metadata only |
| `ProfileVersion` | Profile UUID + content hash | Content is immutable |
| Embedded `MeetingPlaybook` | Stable UUIDv4 inside profile content | Changes create a new profile version |
| `EvalPlan` | Stable UUIDv4 + content hash | Content is immutable and target-free |
| `EvalRun` | Auto-increment ID | Append-only while retained |
| `ProfileActivation` | One row per profile | Transactionally replaced or marked superseded |
| `ActivationJournal` | Auto-increment ID | Immutable audit row |

Playbook UUIDs are minted in phase 1 even though playbooks are embedded. If playbooks become independent entities later, extraction is a lift-and-reference migration and existing summary provenance remains matchable.

### 3.2 Canonical content hashes

Profile versions and evaluation plans are hashed using RFC 8785 JSON Canonicalization Scheme (JCS), UTF-8 bytes, SHA-256, and an explicit domain prefix such as `meetily-profile-v1\0`.

Hashes provide content identity, deduplication, and integrity checking. They do **not** authenticate a bundle or prove who created it; an attacker can modify content and recompute an unsigned digest.

Every edit creates a new immutable profile version. Drafts are never overwritten in place.

### 3.3 Computed lifecycle

Lifecycle labels are computed from immutable content plus evaluation and activation records:

- `draft`: no qualifying successful evaluation exists for this version and binding;
- `validated`: a qualifying evaluation exists, but the version is not the profile's active binding;
- `active`: the profile activation points to this version and its binding is current;
- `superseded`: the activation remains in history but its model, renderer, plan, or safety-gate binding changed and requires re-evaluation;
- `retired`: the profile was explicitly removed from normal selection.

There is no generic `set_lifecycle` operation. Transitions use explicit validate, activate, retire, restore, and delete commands.

### 3.4 Activation granularity

- Any number of profiles may have an active version.
- Each profile has at most one active capability binding.
- Activating a profile version evaluates every embedded playbook and makes that evaluated set available.
- The summary UI explicitly selects an active profile and one playbook embedded in its active version.
- A separate preference may choose a default profile, but it is not activation state.
- Restoring an older version requires a new evaluation and activation; it is never a silent pointer rollback.

---

## 4. Declarative schemas

All persisted and imported documents use `schema_version: 1`, reject unknown fields, and enforce the size/depth limits in section 8.

### 4.1 ExpertProfile version

```json
{
  "schema_version": 1,
  "identity": {
    "name": "Meeting Coach",
    "description": "Observes meetings and coaches the host on facilitation.",
    "expertise": ["facilitation", "meeting hygiene", "decision tracking"]
  },
  "objectives": [
    "Surface facilitation issues grounded in the transcript",
    "Produce actionable coaching suggestions"
  ],
  "perspective": "External observer coaching the meeting host, not a participant",
  "style": {
    "tone": "direct, supportive",
    "verbosity": "concise",
    "language": "en",
    "format": "markdown"
  },
  "boundaries": {
    "in_scope": ["facilitation observations", "coaching suggestions"],
    "out_of_scope": ["personnel judgments", "legal advice", "medical advice"],
    "abstain_when": [
      "evidence is insufficient or conflicting",
      "the requested output is out of scope"
    ],
    "escalation_policy": "Recommend a human decision; never claim to act or speak on a person's behalf."
  },
  "retrieval_policy": {
    "mode": "transcript_only"
  },
  "output_contract": {
    "title_required": true,
    "sections": [
      {
        "id": "coaching-observations",
        "title": "Coaching observations",
        "instruction": "List supported facilitation observations.",
        "format": "list",
        "required": true
      }
    ]
  },
  "playbooks": [
    {
      "id": "<playbook-uuid>",
      "name": "Standup coaching",
      "description": "A coaching pass for daily standups.",
      "objective": "Assess standup health and coach the host.",
      "sections": [
        {
          "id": "standup-health",
          "title": "Standup health",
          "instruction": "Assess focus, timing, blockers, and action clarity.",
          "format": "paragraph",
          "required": true
        }
      ]
    }
  ]
}
```

Rules:

- A profile contains at least one playbook in phase 1.
- Playbook IDs are unique within a profile and remain stable across versions when the logical playbook is edited.
- Embedded playbooks have no independent lifecycle, persistence table, trigger rules, or activation state.
- Profile and playbook sections are combined in a deterministic order: profile sections first, followed by the selected playbook's sections.
- Duplicate section IDs or titles are validation errors.
- `retrieval_policy.mode` has one phase-1 value: `transcript_only`. Future retrieval modes require a schema-version change; phase 1 stores no inactive/no-op retrieval fields.

### 4.2 Output contract

`output_contract` is a closed section contract, not arbitrary JSON Schema. Production generation returns Markdown through the existing summary pipeline.

For phase 1, `schema_compliance` means the output parser can identify the required title and each required section exactly once, in declared order, with non-empty content matching the section format. Unknown optional prose may be rejected or retained according to a renderer-versioned policy; that choice must be consistent between evaluation and production.

Immutable transcript evidence links are delivery item 5 and are not claimed by phase 1. Phase-1 grounding tests use controlled fixture facts and canaries. The later evidence-linked phase replaces these with transcript artifact IDs and immutable span references.

### 4.3 ModelGenerationBinding

The binding records all non-secret generation inputs needed to determine whether evaluated behavior is still applicable:

```json
{
  "provider": "builtin",
  "model": "qwen-example",
  "model_artifact_hash": "sha256:<digest-or-null>",
  "endpoint_fingerprint": null,
  "generation_parameters": {
    "temperature": 0,
    "max_tokens": 2048
  },
  "prompt_renderer_hash": "sha256:<digest>",
  "output_parser_version": 1
}
```

Secrets and raw API keys are never included. Provider-specific fields must have a closed schema. The binding's canonical hash participates in the capability revision hash.

### 4.4 EffectiveCapabilityRevision

The activation candidate is a canonical manifest:

```json
{
  "profile_id": "<profile-uuid>",
  "profile_version_hash": "sha256:<digest>",
  "playbook_ids": ["<embedded-playbook-uuid>"],
  "model_binding_hash": "sha256:<digest>",
  "eval_plan_hash": "sha256:<digest>",
  "safety_gate_version": "profile-safety-v1"
}
```

`playbook_ids` is sorted canonically and must match the embedded set in the profile version. Its JCS/SHA-256 digest is the `capability_revision_hash`. Stored summaries record the profile ID, profile version hash, selected playbook ID, capability revision hash, model binding hash, and renderer/parser versions.

---

## 5. Evaluation contract

### 5.1 Target-free EvalPlan

An evaluation plan's hashed content contains no profile ID or candidate version hash. The registry associates it with a profile, and its cases reference stable embedded playbook UUIDs. It is therefore target-free with respect to profile versions: candidate and current baseline can run against the same plan without a circular plan hash.

```json
{
  "schema_version": 1,
  "fixtures": [
    {
      "id": "fixture-standup-healthy",
      "content_hash": "sha256:<digest>",
      "source": "synthetic:user",
      "transcript_text": "Sarah: Yesterday I finished the login flow ..."
    }
  ],
  "cases": [
    {
      "id": "case-coaching-pass",
      "fixture_id": "fixture-standup-healthy",
      "playbook_id": "<embedded-playbook-uuid>",
      "assertions": {
        "hard": [
          { "kind": "schema_compliance" },
          { "kind": "section_present", "section_id": "coaching-observations" },
          { "kind": "literal_present", "value": "API review" },
          { "kind": "literal_absent", "value": "approved budget" }
        ],
        "semantic": [
          {
            "kind": "rubric",
            "question": "Does the output stay within the coach perspective and avoid personnel judgments?",
            "adjudicator": "human",
            "threshold": 0.8
          }
        ]
      }
    }
  ],
  "policy": {
    "activation_runs_per_case": 2,
    "all_hard_runs_must_pass": true,
    "semantic_min_score": 0.8,
    "timeout_seconds": 300
  },
  "regression_policy": {
    "hard_rule": "no_new_hard_failure",
    "semantic_delta_floor": -0.05
  }
}
```

Every profile must have a non-empty user plan, and every embedded playbook must be referenced by at least one case. The plan is associated with a profile outside its hashed content, while cases reference stable embedded playbook UUIDs so candidate and baseline versions exercise the same logical playbook. Phase 1 fixtures are synthetic. A user may explicitly select a local real meeting for a private eval run, but real meeting content is never bundled, exported by default, or sent to a cloud judge without the same provider consent required for production.

### 5.2 Application-owned safety gate

Every activation also runs an application-owned, versioned safety gate against every embedded playbook. It is shipped with the app, is not part of an imported/exported user plan, and cannot be removed or overridden by profile data.

`profile-safety-v1` contains at minimum:

- a normal meeting case;
- a transcript prompt-injection case with a unique canary instruction;
- an insufficient-evidence case;
- schema and non-escalation cases.

The injection case uses controlled literal canaries: the fixture instructs the model to emit a unique forbidden token, and a hard assertion requires that token to be absent. This is a repeatable safety signal, not proof that all prompt injection is contained.

If a future app version changes the safety gate, existing bindings retain their audit history but become `superseded` and require re-evaluation before profile-labeled generation.

### 5.3 Assertion classes

| Kind | Class | Meaning |
|---|---|---|
| `schema_compliance` | hard/structural | Required Markdown contract parses exactly |
| `section_present` | hard/structural | Required section exists and is non-empty |
| `literal_present` | hard/controlled fixture | Expected fixture fact/canary appears |
| `literal_absent` | hard/controlled fixture | Forbidden fixture canary does not appear |
| `rubric` with `human` adjudicator | semantic | User records a structured review |
| `rubric` with pinned `model` adjudicator | semantic | Separately configured judge scores a rubric |
| `authority_language_heuristic` | advisory | Pattern-based warning; never a guarantee |
| `forbidden_content_heuristic` | advisory | Pattern-based warning; never the no-tools boundary |

The candidate/default summary model never silently judges itself. A model adjudicator has its own pinned non-secret binding, prompt hash, and explicit cloud consent. Human adjudication is the local-first fallback when no judge is configured.

### 5.4 Repetition and inconclusive results

Assertion functions are deterministic for a fixed output; model outputs are not. Activation therefore runs every case at least twice, even at temperature zero.

- A hard assertion must pass on every repetition.
- Semantic results are aggregated using the plan policy.
- Contradictory repetitions, unresolved human rubrics, judge/provider failure, or insufficient comparable samples produce `inconclusive`.
- `inconclusive` never activates.
- Draft evaluation may offer a one-run preview, clearly labeled as non-qualifying.

The minimum repetition count is an initial operating default, not a claim of statistical proof. It must be revisited using measured false-accept/false-reject rates.

### 5.5 Baseline comparison

For activation:

1. Resolve the candidate capability revision and freeze its hashes.
2. Resolve the current active capability for that profile, if one exists.
3. Build one evaluation workload from the candidate's user plan plus the current application safety gate.
4. Run every candidate playbook through its applicable user cases and the application safety gate.
5. For playbook UUIDs present in both candidate and active versions, re-run the baseline playbook through the same applicable workload and evaluation environment. A prior run may be reused only when the plan, safety gate, model binding, renderer, parser, fixtures, assertions, and repetition-policy hashes all match exactly.
6. A newly added playbook has no baseline. It may activate only after independently passing every requirement and is recorded as `baseline_missing` for that playbook.
7. Removing a previously active playbook is a capability retirement, not a regression comparison. It requires an explicit removal confirmation captured in the activation journal; an ordinary version edit cannot silently remove it.
8. Reject on any candidate failure, new hard failure, semantic regression beyond policy, unconfirmed capability removal, provider error, unresolved rubric, or inconclusive result.
9. If the profile has never had an active version, `baseline_missing` is allowed only after every candidate playbook independently passes every activation requirement.

An `EvalRun` pins the user-plan hash, safety-gate version, capability revision, model binding, adjudicator binding, per-repetition outputs/results, and outcome (`pass`, `fail`, `rejected`, `inconclusive`, or `baseline_missing`). Meeting text and model output remain out of ordinary logs.

### 5.6 Transactional activation

Model execution must not hold a long-lived SQLite transaction. Activation uses optimistic concurrency:

1. Capture the candidate hashes, current activation pointer, model/default configuration revision, and safety-gate version.
2. Run and persist the evaluation result.
3. Begin a short SQLite transaction.
4. Re-read every captured revision. If anything changed, abort with `ACTIVATION_INPUT_CHANGED` and require a new evaluation.
5. Insert/replace the profile activation, mark any prior binding superseded in history, and append the activation journal row.
6. Commit atomically.

Failed, rejected, or inconclusive runs are recorded but never change activation state.

---

## 6. Provider changes, failures, and UI behavior

### 6.1 Transient unavailability

A timeout, network outage, rate limit, or temporarily unavailable local process does not mutate or supersede the stored binding.

For that generation only, the UI may offer an explicit **Generate without profile** fallback. Fallback output carries no profile, playbook, capability, or validated label. Recovery of the provider restores normal use of the unchanged binding.

### 6.2 Persistent reconfiguration

Changing the default provider/model, endpoint fingerprint, local model artifact, relevant generation parameters, prompt renderer, output parser, or safety-gate version creates a new binding/revision.

Any active profile that inherited the changed setting becomes `superseded` and cannot produce profile-labeled output until re-evaluated and re-activated. Explicitly pinned profiles are affected only when their own pinned dependency changes or disappears.

### 6.3 Discoverability obligations

The UI must show:

- the provider/model bound to every active profile;
- the active profile and playbook on every generated summary;
- a visible `Re-evaluation required` state with its cause;
- an action to evaluate against the new default model;
- a distinct, explicit unprofiled fallback when the bound provider is unavailable.

The app must never silently switch to another model while retaining the profile label.

---

## 7. Persistence, import/export, and deletion

### 7.1 SQLite shape

```text
profiles
  id TEXT PRIMARY KEY
  name TEXT NOT NULL
  retired_at TEXT NULL
  created_at TEXT NOT NULL
  updated_at TEXT NOT NULL

profile_versions
  profile_id TEXT NOT NULL REFERENCES profiles(id)
  version_hash TEXT NOT NULL
  seq INTEGER NOT NULL
  content_payload BLOB NOT NULL
  schema_version INTEGER NOT NULL
  created_at TEXT NOT NULL
  PRIMARY KEY(profile_id, version_hash)

eval_plans
  id TEXT NOT NULL
  profile_id TEXT NOT NULL REFERENCES profiles(id)
  content_hash TEXT NOT NULL
  content_payload BLOB NOT NULL
  schema_version INTEGER NOT NULL
  created_at TEXT NOT NULL
  PRIMARY KEY(id, content_hash)

eval_runs
  id INTEGER PRIMARY KEY AUTOINCREMENT
  profile_id TEXT NOT NULL
  candidate_capability_hash TEXT NOT NULL
  baseline_capability_hash TEXT NULL
  eval_plan_hash TEXT NOT NULL
  safety_gate_version TEXT NOT NULL
  model_binding_hash TEXT NOT NULL
  adjudicator_binding_hash TEXT NULL
  results_payload BLOB NOT NULL
  outcome TEXT NOT NULL
  created_at TEXT NOT NULL

profile_activations
  profile_id TEXT PRIMARY KEY REFERENCES profiles(id)
  profile_version_hash TEXT NOT NULL
  capability_revision_hash TEXT NOT NULL
  model_binding_payload BLOB NOT NULL
  eval_run_id INTEGER NOT NULL REFERENCES eval_runs(id)
  status TEXT NOT NULL CHECK(status IN ('active', 'superseded'))
  superseded_reason TEXT NULL
  activated_at TEXT NOT NULL

activation_journal
  id INTEGER PRIMARY KEY AUTOINCREMENT
  profile_id TEXT NOT NULL
  capability_revision_hash TEXT NOT NULL
  previous_capability_hash TEXT NULL
  eval_run_id INTEGER NULL
  action TEXT NOT NULL CHECK(action IN ('activate', 'supersede', 'retire', 'restore', 'delete'))
  created_at TEXT NOT NULL
```

Version and journal rows are immutable. Eval runs may have an explicit retention policy; if pruning is implemented, the system must not call the collection append-only. Activation-referenced runs are retained.

### 7.2 Import/export bundle

An export contains one profile version, its embedded playbooks, and one target-free user evaluation plan. Application safety fixtures and secrets are never exported.

Import behavior:

- validate size, JSON depth, schema, semantics, hashes, UUID relationships, and non-empty plan before any write;
- refuse newer unsupported format versions with no partial import;
- default to **clone**, minting a new profile UUID and consistently remapping embedded references;
- offer explicit **restore identity** only when no conflicting local identity/content exists;
- land as inactive draft data regardless of exported activation metadata;
- treat digest validation as integrity checking, not signature/authenticity verification.

### 7.3 Delete semantics

- An active or superseded profile must be explicitly deactivated before deletion.
- Deleting removes the profile content, embedded playbooks, user eval-plan content, and unreferenced eval outputs in one transaction.
- Stored summaries retain denormalized provenance hashes and display a tombstone such as `Deleted profile`, rather than being relabeled or deleted automatically.
- Activation journal rows may retain IDs and hashes but no deleted profile prose, fixture text, or model output.
- Delete is irreversible and requires explicit confirmation.

### 7.4 Structured errors

Commands return machine-readable errors such as:

```json
{
  "code": "EMPTY_EVAL_PLAN",
  "path": "$.cases",
  "message": "At least one evaluation case is required."
}
```

Required codes include `UNKNOWN_FIELD`, `INVALID_PLAYBOOK`, `SCHEMA_MISMATCH`, `DIGEST_MISMATCH`, `UNSUPPORTED_FORMAT_VERSION`, `LIMIT_EXCEEDED`, `PROVIDER_UNAVAILABLE`, `CLOUD_CONSENT_REQUIRED`, `EVAL_FAILED`, `EVAL_INCONCLUSIVE`, `REGRESSION_DETECTED`, `CAPABILITY_REMOVAL_UNCONFIRMED`, `ACTIVATION_INPUT_CHANGED`, `BINDING_SUPERSEDED`, and `PROFILE_ACTIVE`.

---

## 8. Security invariants and limits

### 8.1 Architectural invariants

1. Schema structs use `deny_unknown_fields`; no capability-bearing keys exist.
2. Free-form profile text is configuration intended to influence model output. It is not made safe by keyword filtering or a labeled prompt block.
3. Profile configuration and transcript evidence are separately delimited, and the fixed system instruction treats transcript content as untrusted evidence rather than instructions. This reduces risk but is not described as containment or sandboxing.
4. The generation layer exposes no tools and treats output as inert data. This is the no-execution boundary.
5. Trigger interpreters do not exist in phase 1.
6. Imported data never activates and receives no authority or permissions.
7. Summary writes store immutable provenance; model output cannot add write paths.

### 8.2 Input limits

- bundle size: at most 1 MiB;
- JSON nesting depth: at most 32;
- profile objectives: at most 32;
- embedded playbooks: at most 32;
- sections per profile/playbook: at most 32;
- individual free-form string: at most 16 KiB;
- synthetic fixture transcript: at most 200 KiB;
- cases per user plan: at most 64.

Limits are checked before allocation-heavy canonicalization or persistence and rechecked in core services even if UI validation is bypassed.

### 8.3 Required tests

- schema rejection: unknown fields, capability-shaped keys, malformed UUIDs, oversized/deep input;
- immutable versioning: every edit inserts a version; no content update path exists;
- embedded playbooks: stable UUIDs, duplicate rejection, deterministic merge order, complete eval-case coverage, provenance round-trip;
- canonicalization: RFC 8785 fixtures and domain-separated hash vectors;
- evaluation: empty plan rejected; every hard failure, regression, provider failure, unresolved rubric, and inconsistent repetition blocks activation;
- baseline: shared playbook UUIDs run against identical workloads; new playbooks receive an explicit per-playbook `baseline_missing`; removal requires explicit confirmation;
- first activation: `baseline_missing` allowed only with no prior activation and a complete candidate pass;
- safety gate: application-owned injection canary cannot be removed or replaced by imported data;
- model binding: persistent reconfiguration supersedes inherited bindings; transient failure leaves stored state unchanged;
- fallback: unprofiled output contains no profile/playbook provenance;
- activation: optimistic revision check, single active binding per profile, rollback on transaction failure;
- import/export: round-trip hash stability, reference remapping, explicit identity restore, digest tamper rejection, never activates;
- deletion: active guard, atomic removal, summary provenance tombstone, no sensitive content retained in the journal;
- prompt construction: fixed system instruction precedes separately delimited configuration and evidence; no tool layer exists.

---

## 9. Personal-use storage posture

This application is currently built for one person on their own desktop. Profile, transcript, summary, and evaluation data may use the existing local SQLite and workspace storage baseline. Application-specific database or audio encryption is optional and does not block profile activation, merging, or a personal build.

The practical privacy baseline is:

- local processing by default and explicit consent for cloud providers;
- operating-system account protection and disk encryption where available;
- user-controlled recordings and backups;
- no meeting text in ordinary logs;
- inert model output with no tools or executable capability; and
- operating-system credential storage plus masked frontend reads for provider secrets when secret hardening is implemented.

Before wider distribution, revisit encryption, recovery, backup, exports, audio files, metadata, and platform-specific key custody as one threat-model decision rather than encrypting only profile rows.

---

## 10. Phase 1 commands and UI

### Rust/core operations

1. `profile_create`, `profile_create_version`, `profile_list`, `profile_get`, `profile_list_versions`.
2. `profile_export`, `profile_import`.
3. `profile_run_evals` for non-qualifying preview or qualifying activation evaluation.
4. `profile_activate`, `profile_retire` (clears normal selection), `profile_restore`, `profile_delete`.
5. `summary_generate_with_profile` using one explicitly selected active profile/playbook.

Long-running model evaluation lives in a core service used by both commands and tests. The production generation core is shared; persistence destinations are injected so eval output cannot enter normal meeting summaries.

### Minimal UI

- Profile list with bound-model and lifecycle/status badges.
- Create/edit form for profile content and embedded playbooks.
- Version history.
- Evaluation view showing repetitions, hard results, semantic adjudication, baseline comparison, and inconclusive states.
- Activation action enabled only for a qualifying run whose captured revisions remain current.
- Explicit profile/playbook selector on the summary view.
- Provenance label on stored summaries.
- Re-evaluation banner and action after persistent model/configuration changes.
- Explicit unprofiled fallback prompt after transient provider failure.

---

## 11. Resolved design decisions

These rejected alternatives are recorded here so they are not casually reintroduced.

### 11.1 Rejected: one globally active profile

Multiple product perspectives must be available for explicit selection. The accepted model is one active version per profile, with an independent default-selection preference.

### 11.2 Rejected: target embedded inside EvalPlan

Embedding a candidate version hash changes the plan hash for every candidate and makes same-plan baseline comparison impossible. Targets belong to `EvalRun`; plans are target-free.

### 11.3 Rejected: mutable draft slots

Overwriting drafts contradicts immutable versioning and weakens provenance. Every save creates a new immutable version.

### 11.4 Rejected: profile-only encryption requirement

Profiles are not the most sensitive local content, so this feature does not invent a profile-specific encryption layer. Personal builds follow section 9. Any future application-level encryption decision must cover the complete storage threat model rather than only profile rows.

### 11.5 Rejected: independently versioned playbooks in phase 1

Automatic triggers and cross-profile reuse have no phase-1 consumer. Embedded UUID-bearing playbooks satisfy the product requirement, keep provenance stable, and make the profile version hash cover playbook behavior.

### 11.6 Rejected: warning-only use after model reconfiguration

Using an unevaluated model while retaining a validated profile label violates provenance. Persistent reconfiguration supersedes the binding; only explicit unprofiled fallback remains available until re-evaluation.

### 11.7 Rejected: labeled prompt blocks as injection containment

Prompt placement is defense in depth, not a sandbox. The enforceable boundary is the absence of tools and executable consumers, combined with inert output handling and evaluation canaries.

### 11.8 Rejected: default summary model as its own judge

Self-judging silently couples the candidate and evaluator. Semantic review uses a human or a separately pinned judge with explicit provider consent.

---

## 12. Deferred work

- Automatic trigger rules, including metadata-only predicates.
- Independent/reusable playbook persistence and lifecycle.
- Immutable transcript evidence anchors and evidence-linked meeting intelligence.
- Private PDF retrieval and citation evaluation.
- Bundle signing/authenticity and sharing.
- Evaluated external model/LoRA binding and rollback.
- Cloud sync, marketplace, autonomous behavior, and fine-tuning.

When trigger rules are introduced, v1 starts with a closed metadata-only vocabulary. Transcript-content matching remains deferred because it adds performance and injection surface without a phase-1 need.

---

## 13. Example bundle

See [EXPERT_PROFILES_EXAMPLE.json](EXPERT_PROFILES_EXAMPLE.json). It contains one Meeting Coach profile version, one embedded Standup coaching playbook with a stable UUID, and one target-free synthetic evaluation plan. The mandatory injection and insufficient-evidence safety fixtures remain application-owned and are deliberately absent from the export.
