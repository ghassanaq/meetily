# Authority-Scope Warning Implementation Plan

Status: Complete — Checkpoints A, B, and C passed automated, release, and manual UI verification
Date: 2026-08-21
Design: `AUTHORITY_SCOPE_WARNING_DESIGN.md`

## 1. Goal and fixed boundaries

Implement an advisory, local authority-scope detector that warns only when a completed
first-person answer matches an explicitly enrolled excluded scope. The detector never blocks,
rewrites, delays, or regenerates an answer and never calls a provider.

The implementation is deliberately split by evidence:

1. build and verify the immutable schema and pure matcher without changing Live Assist;
2. run the matcher offline against five unseen real answers while warnings remain absent from
   Live Assist;
3. only after zero false positives and explicit user approval, add production persistence and
   the advisory UI.

No private corpus source, rule file, generated answer, or API key may enter Git. Existing
professional-identity versions remain immutable and selected version-1 profiles do not change
behavior.

## 2. Code facts that constrain the plan

- `ProfessionalIdentityVersion` is a closed serde schema in
  `frontend/src-tauri/src/professional_identity/mod.rs`; its canonical JSON is content-hashed.
- Stored versions already carry `schema_version` and immutable payload bytes, so schema v2
  needs no migration of existing identity rows.
- The Markdown importer resolves every path beneath the selected manifest root and compiles
  source sections into stable UUID records. Authority enrollment must reuse that boundary.
- `RetrievedIdentityContext.prompt_json` is provider evidence. Matcher rules and diagnostics
  must remain outside it.
- Completed answers pass through `validate_completed_answer` before the exchange becomes
  `Complete`; the future production matcher belongs immediately after that normalization.
- `AssistExchange` is memory-only. Persisted activation and dismissal counts therefore require
  dedicated local SQLite tables, but trial answers must remain in an ignored workload.
- The Live Assist frontend currently declares snapshot types locally in
  `frontend/src/app/live-assist/page.tsx`; policy diagnostics must update those types and their
  tests in the same checkpoint.
- New migrations default to CRLF unless `.gitattributes` explicitly pins them to LF. The new
  migration must be pinned before it is staged; existing migration files must not be edited.

## 3. Proposed final data contracts

### 3.1 Immutable identity schema v2

`ProfessionalIdentityVersion` gains:

```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub authority_constraints: Vec<AuthorityConstraint>
```

The empty collection is omitted during serialization so a parsed version-1 payload hashes to
the same bytes and digest it has today. Validation accepts versions 1 and 2, with these rules:

- version 1 must have no authority constraints;
- version 2 may have zero or more validated constraints;
- newly created/imported identities use version 2;
- loading a stored version never upgrades or rewrites it.

Proposed compiled rule:

```rust
pub struct AuthorityConstraint {
    pub id: String,
    pub label: String,
    pub contexts: Vec<String>,
    pub action_families: Vec<AuthorityActionFamily>,
    pub permitted_objects: Vec<String>,
    pub excluded_objects: Vec<String>,
    pub evidence_record_ids: Vec<Uuid>,
}
```

`AuthorityActionFamily` is a closed enum (`manage`, `lead`, `own`, `oversee`,
`responsible_for`, `approve`, `decide`). Each family has a reviewed, finite English form table
in code; the matcher does not invent synonyms. Unrecognized phrasing remains `unknown`.

Initial bounds:

- at most 64 constraints per identity;
- at most 16 contexts, permitted objects, excluded objects, and evidence records per rule;
- at most 128 UTF-8 bytes per alias and 256 bytes per label;
- stable rule IDs restricted to lowercase ASCII letters, digits, `_`, and `-`;
- aliases normalized for comparison and rejected if empty, duplicated, or present in both
  permitted and excluded sets.

### 3.2 Private authoring sidecar

A version-2 context manifest may reference one relative JSON sidecar:

```json
{
  "schema_version": 1,
  "rules": [
    {
      "id": "synthetic-workstream-boundary",
      "label": "Workstream boundary",
      "contexts": [],
      "action_families": ["manage", "lead"],
      "permitted_objects": ["processing workstream"],
      "excluded_objects": ["whole operation"],
      "evidence": [
        {"source_label": "Synthetic experience", "title": "Role and authority"}
      ]
    }
  ]
}
```

The sidecar uses exact source-label/title selectors because a person should not have to author
UUIDs. Import resolves every selector to exactly one compiled record ID. Missing or ambiguous
selectors fail import. The stored identity contains only compiled IDs, never the sidecar path.

Manifest version 1 remains accepted and produces identity schema version 1. Manifest version 2
uses the optional `authority_constraints` path and produces identity schema version 2. The
path receives the same canonicalization, traversal rejection, extension check, and size cap as
the existing bundle graph.

### 3.3 Matcher result

```rust
pub enum AuthorityCheckStatus {
    NotConfigured,
    CheckedNoMatch,
    Warning,
}

pub struct AuthorityPolicyWarning {
    pub code: AuthorityPolicyWarningCode,
    pub rule_id: String,
    pub rule_label: String,
    pub sentence: String,
    pub matched_action: String,
    pub matched_context: Option<String>,
    pub matched_excluded_object: String,
    pub excluded_start_utf16: u32,
    pub excluded_end_utf16: u32,
    pub evidence_record_ids: Vec<Uuid>,
}

pub struct AuthorityCheckResult {
    pub status: AuthorityCheckStatus,
    pub evaluated_rule_count: u32,
    pub warnings: Vec<AuthorityPolicyWarning>,
}
```

`CheckedNoMatch` is not a pass. UI copy and serialized names must never use `verified`,
`supported`, or `safe`.

## 4. Checkpoint A — schema and pure matcher, no behavior change

Stop after this checkpoint for code review. Live Assist output and snapshots must remain
unchanged.

### Task 1 — Add backward-compatible identity schema v2

Files:

- modify `frontend/src-tauri/src/professional_identity/mod.rs`;
- modify `frontend/src-tauri/src/professional_identity/repository.rs` tests;
- modify `frontend/src/types/professional-identity.ts`;
- modify `frontend/src/components/ProfessionalIdentitySettings.tsx` only to preserve v1/v2
  payloads and initialize new blank identities as v2.

Steps:

- [ ] Introduce current/minimum schema constants instead of treating one constant as the only
      accepted version.
- [ ] Add the closed rule types and serde fields with default-plus-empty omission.
- [ ] Validate rule counts, text bounds, enum values, unique IDs, alias disjointness, and
      evidence IDs belonging to records in the same identity version.
- [ ] Reject nonempty constraints on schema v1.
- [ ] Keep existing v1 canonical serialization and a captured v1 digest byte-identical.
- [ ] Ensure the manual editor preserves constraints it does not edit and does not silently
      convert a loaded v1 profile to v2.

Tests:

- [ ] existing stored v1 payload loads and retains its exact pre-change hash;
- [ ] v1 plus constraints fails closed;
- [ ] v2 with a valid rule round-trips and hashes deterministically;
- [ ] changing a rule alias, family, or evidence ID changes the identity hash;
- [ ] unknown fields, duplicate IDs, overlapping aliases, and foreign evidence IDs fail;
- [ ] repository immutability and content-digest tests remain green.

Suggested commit: `feat: add versioned authority constraints`

### Task 2 — Compile the private rule sidecar during import

Files:

- modify `frontend/src-tauri/src/professional_identity/markdown_import.rs`;
- extend its temporary-file fixtures only; do not add a real or private rule file.

Steps:

- [ ] Accept manifest schemas 1 and 2 with closed, version-specific validation.
- [ ] Add a bounded `authority_constraints` relative path only for manifest v2.
- [ ] Parse a closed sidecar schema, resolve source-label/title selectors after Markdown
      sections are loaded, and compile them to record UUIDs.
- [ ] Reject absolute paths, traversal, non-JSON extensions, oversized files, unknown fields,
      empty rule sets where a sidecar is declared, and ambiguous evidence selectors.
- [ ] Build schema-v1 identities for old manifests and schema-v2 identities for new manifests.
- [ ] Confirm re-import creates a new immutable hash and does not select or mutate a stored
      version except through the existing explicit import action.

Tests:

- [ ] existing manifest-v1 fixture remains byte/hash compatible;
- [ ] manifest-v2 compiles an anonymised rule to the expected stable record ID;
- [ ] missing, ambiguous, cross-root, and traversal selectors fail before persistence;
- [ ] no filesystem path survives in serialized `ProfessionalIdentityVersion`.

Suggested commit: `feat: compile authority constraints during import`

### Task 3 — Implement the pure enrolled-rule matcher

Files:

- add `frontend/src-tauri/src/professional_identity/authority_scope.rs`;
- register the private module in `professional_identity/mod.rs`;
- keep the API pure and independent of Tauri, SQLite, providers, and `prompt_json`.

Steps:

- [ ] Normalize case, punctuation, and whitespace while retaining a mapping to original UTF-16
      offsets.
- [ ] Split completed plain text into sentences, then into conservative clause windows for
      first-person, prospective, and negation checks.
- [ ] Match a closed action-family form, optional context, and excluded-object alias in the
      same autobiographical clause.
- [ ] Treat context-free rules as the default path for vague claims.
- [ ] Scope negation to the matched action/object pair so contrastive clauses behave correctly.
- [ ] Deduplicate equivalent matches and order warnings by sentence offset, then rule ID.
- [ ] Return `NotConfigured` for no rules and `CheckedNoMatch` only after at least one rule was
      evaluated.

Required synthetic tests:

- [ ] bounded-team management: no warning;
- [ ] bounded-workstream leadership: no warning;
- [ ] excluded whole-operation object: warning;
- [ ] excluded decision class: warning;
- [ ] shared responsibility expanded to enrolled sole ownership: warning;
- [ ] prospective and hypothetical claims: no warning;
- [ ] simple and contrastive negation: no warning;
- [ ] negation attached to a different object: warning on the affirmative excluded object;
- [ ] multi-context compound claim: one warning with the exact excluded span;
- [ ] context-free vague claim: warning without an event/location token;
- [ ] unenrolled paraphrase: no warning, represented as no enrolled match rather than pass;
- [ ] non-ASCII text before the match yields correct UTF-16 offsets;
- [ ] repeated aliases and overlapping rules produce deterministic deduplicated output.

Suggested commit: `feat: detect enrolled authority scope expansion`

Checkpoint A verification:

```text
cargo test --lib professional_identity::authority_scope::tests -- --nocapture
cargo test --lib professional_identity::markdown_import::tests -- --nocapture
cargo test --lib professional_identity::repository::tests -- --nocapture
corepack pnpm typecheck
cargo test --lib
```

Acceptance: all checks pass, existing identity hashes are pinned, and no Live Assist model,
snapshot, command, prompt, UI, or runtime path has changed.

## 5. Checkpoint B — offline private gate, still no Live Assist warning

This checkpoint may run provider-backed evaluation but does not alter production exchanges or
the Live Assist panel.

### Task 4 — Extend the ignored harness for authority trials

Files:

- modify `frontend/src-tauri/src/live_assist/voice_harness.rs`;
- update `docs/product/LIVE_ASSIST_PROTOTYPE.md` with invocation and privacy rules;
- keep the real workload and results under ignored `experiments/` or `target/` paths.

Steps:

- [ ] Add an ignored authority-trial workload loader that accepts five or more answer cases,
      their selected immutable identity version, and human adjudication.
- [ ] Permit answers captured from the real Live Assist app to be copied only into the ignored
      private workload; never serialize them into tracked fixtures or documentation.
- [ ] Run `evaluate_authority_scope` locally against each answer.
- [ ] Persist only case ID, answer hash, identity-version hash, matched rule IDs, warning codes,
      TP/TN/FP/FN adjudication, and timestamp in the ignored result ledger.
- [ ] Reject duplicate answer hashes so author-selected fixtures cannot be counted again as
      unseen live trials.
- [ ] Report precision and recall counts without claiming statistical proof.
- [ ] Fail the activation gate unless there are at least five distinct unseen live answers and
      zero false positives. A rule revision changes the identity hash and resets the count.

Tracked tests:

- [ ] anonymised workload parsing rejects raw corpus paths outside its root;
- [ ] duplicate answer hashes do not increase the trial count;
- [ ] four trials cannot satisfy the gate;
- [ ] one false positive fails the gate;
- [ ] five distinct zero-false-positive trials satisfy only the offline evidence gate, not
      runtime activation.

Private execution:

```text
cargo test --lib live_assist::voice_harness::authority_scope_private_trials -- --ignored --nocapture
```

Suggested commit: `test: add offline authority warning gate`

Checkpoint B stop condition:

- report the five-trial confusion counts and rule/identity hashes without private answer text;
- do not implement or expose runtime warnings;
- obtain explicit approval before Checkpoint C.

Measured 2026-08-22: five distinct private Live Assist answers produced TP=0, TN=5, FP=0,
and FN=0 for identity hash `sha256:6480011cef0620d35f4c8899ce2f99ee457d81d0807e51c96705c7bf4b3426f3`
and rule-set hash `sha256:4cc45b5c01ecb5d472185597ec783708cf25f932561f925781a03e2a122c4dd7`.
The offline evidence gate passed. Precision and recall were undefined because this batch contained
no human-expected positive case; sensitivity remains covered synthetically rather than measured
by this private batch. Runtime activation remained false.

## 6. Checkpoint C — production advisory path, separately authorised

Implementation note (2026-08-22): Ghassan explicitly approved this checkpoint. The
version-bound policy tables, post-completion diagnostics, exact-exchange dismissal and evidence
commands, honest three-state UI, and focused persistence/presentation tests are implemented.
Warnings remain advisory, local, and opt-in per immutable identity-version hash.

Do not begin this checkpoint merely because A and B pass. It requires a new explicit user go
signal after the five-trial report.

### Task 5 — Persist per-version mode and dismissal feedback

Files:

- add `frontend/src-tauri/migrations/20260821120000_add_authority_scope_feedback.sql`;
- modify `.gitattributes` to pin that exact migration to `eol=lf` before staging it;
- add `frontend/src-tauri/src/professional_identity/authority_scope_repository.rs`;
- modify `professional_identity/mod.rs` and repository tests as needed.

Tables:

- `authority_scope_policy_state`: composite identity/version key, mode constrained to
  `offline` or `advisory`, activation timestamp;
- `authority_scope_rule_feedback`: composite identity/version/rule key, dismissal count and
  last-dismissed timestamp.

Foreign keys reference `(identity_id, version_hash)` in `professional_identity_versions`.
No table stores answers, excerpts, matched sentences, or aliases.

Tests:

- [ ] a new constrained identity defaults to offline;
- [ ] activation binds one exact immutable version and does not carry to a newer hash;
- [ ] dismissal increments atomically and never changes policy mode;
- [ ] deleting an identity cascades policy and feedback rows;
- [ ] existing migration checksums remain unchanged on the real database.

Suggested commit: `feat: persist authority warning policy state`

### Task 6 — Attach local diagnostics after answer normalization

Files:

- modify `frontend/src-tauri/src/professional_identity/mod.rs` so
  `RetrievedIdentityContext` carries matcher constraints outside `prompt_json`;
- modify `frontend/src-tauri/src/live_assist/models.rs`;
- modify focused construction paths and tests in `frontend/src-tauri/src/live_assist/mod.rs`;
- modify `frontend/src-tauri/src/live_assist/voice_harness.rs` constructors.

Steps:

- [ ] Add `authority_check` to `AssistExchange`, separate from `answer_format_warnings`.
- [ ] Carry selected-version constraints alongside retrieval without serializing them to the
      provider prompt or grounding-source list.
- [ ] Run the pure matcher only after `validate_completed_answer` succeeds.
- [ ] Preserve generation-ID and interruption guards so stale completions cannot attach a
      warning to another exchange.
- [ ] In offline mode retain diagnostics only for review; in advisory mode expose them through
      the normal snapshot.
- [ ] Confirm no-provider, private-mode, failed, interrupted, and transcript-only exchanges do
      not claim a completed authority check.

Tests:

- [ ] `prompt_json` is identical with and without equivalent matcher diagnostics;
- [ ] v1/no-rule identity produces `not_configured` after a completed answer;
- [ ] configured clean result is `checked_no_match`, never `verified`;
- [ ] matched warning contains exact UTF-16 span and evidence IDs;
- [ ] interrupted/stale generations cannot publish a check result;
- [ ] matcher execution makes no provider call and adds no request latency measurement.

Suggested commit: `feat: attach authority diagnostics to live assist`

### Task 7 — Add explicit activation, dismissal, and evidence commands

Files:

- add focused commands under `professional_identity/authority_scope_repository.rs` or a
  dedicated `authority_scope_commands.rs`;
- register commands in `frontend/src-tauri/src/lib.rs`;
- test authorization by exact identity/version/exchange/rule tuple.

Commands:

- read policy/rule-review status for one immutable version;
- activate advisory mode only after an explicit confirmation value;
- dismiss one warning on one current exchange and increment feedback;
- inspect evidence for a warning by resolving only its enrolled record IDs from the exact
  selected version.

The evidence command returns source metadata by default and content only when the caller sets
an explicit `includeExcerpt` flag. It never contacts a provider and rejects arbitrary record
IDs not referenced by the warning.

Suggested commit: `feat: manage authority warning review state`

### Task 8 — Add the honest passive indicator and warning UI

Files:

- modify `frontend/src/app/live-assist/page.tsx`;
- add a focused pure UI/model helper under `frontend/src/lib/`;
- add Vitest coverage under `frontend/tests/lib/`;
- modify `frontend/src/components/ProfessionalIdentitySettings.tsx` for offline/advisory
  status, trial result summary, and dismissal counts.

Live Assist behavior after advisory activation:

- `Authority rules not configured` for v1/no-rule identities;
- `Authority rules checked · no enrolled match` in neutral styling with a tooltip that says it
  is not comprehensive verification;
- amber `Authority wording needs review` for matches;
- highlight only the excluded-object UTF-16 span;
- dismiss affects only the visible exchange while incrementing local feedback;
- evidence source metadata is compact; excerpt loading is an explicit post-hoc action.

During offline mode, no match/no-match result is shown under the answer. Settings may show
`Offline trial · warnings hidden` so activation state is not confused with an unconfigured
identity.

Tests:

- [ ] not-configured and checked-no-match render differently;
- [ ] checked-no-match never uses success color or verification language;
- [ ] matched span highlighting preserves supported neighboring text and non-ASCII offsets;
- [ ] dismiss hides only the current warning and leaves a later exchange unchanged;
- [ ] excerpt content is absent until explicitly requested;
- [ ] offline mode renders no Live Assist warning outcome.

Suggested commit: `feat: surface advisory authority warnings`

### Task 9 — Final verification and documentation

Files:

- update `AUTHORITY_SCOPE_WARNING_DESIGN.md` status only after measured activation;
- update `CURRENT_STATUS_AND_ROADMAP.md`, `LIVE_ASSIST_PROTOTYPE.md`, and the current session
  handoff without copying private trial data.

Verification:

```text
rustfmt --edition 2021 --config skip_children=true --check <changed Rust files>
cargo test --lib professional_identity::authority_scope::tests -- --nocapture
cargo test --lib live_assist::tests -- --nocapture
cargo test --lib
corepack pnpm test
corepack pnpm typecheck
corepack pnpm build
cargo build --release --features custom-protocol
```

Then stop the exact running Meetily process only after verifying its executable path, launch
the rebuilt authoritative binary, and manually verify the three UI states. Private trial
answers remain ignored and are reported only as counts.

Suggested commit: `docs: record authority warning activation evidence`

## 7. Review checkpoints

| Checkpoint | Included | Explicitly excluded | Required decision |
| --- | --- | --- | --- |
| A | Schema v2, importer, pure matcher | Runtime, DB, commands, UI | Approve matcher behavior |
| B | Ignored private gate and five unseen trials | Live warning display | Review FP/FN counts |
| C | Policy/feedback persistence, runtime diagnostics, commands, UI | Blocking or rewriting | Explicitly approve advisory activation |

Each checkpoint must leave the worktree testable and independently reviewable. Checkpoint C
cannot be approved prospectively from this plan; it depends on Checkpoint B's measured unseen
answers.

## 8. Known limitations retained deliberately

- Exact enrolled matching misses unenrolled paraphrases.
- English clause and negation handling is conservative, not general semantic parsing.
- Five trials are a product gate, not statistical proof.
- A clean indicator means only that no enrolled rule matched.
- Human review remains the final authority before the user speaks an answer.
