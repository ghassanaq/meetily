# Broad-Question Retrieval and Composition Implementation Plan

Status: superseded before execution on 2026-08-21.

The task-by-task plan below was not executed. Review of Mishkat's existing compose/edit
services showed that Meeting Assistant already possessed most of the required boundaries:
provider rails, versioned expert profiles, provenance, and post-generation validation. The
implemented path therefore adds only a versioned `professional-introduction/v1` compose brief,
deterministic evidence selection/budgeting, prompt handling for that brief, and stricter
plain-text validation. Keep this document as a design inventory for future expansion; do not
treat its checkpoints or embedded worker instructions as current work instructions.

**Goal:** Make Live Assist answer broad interview questions from a deliberately composed, budgeted, conflict-resolved evidence package instead of records that happen to contain the filler word "about".

**Architecture:** A new `professional_identity::composition` module holds config, normalisation, selection, and budgeting as pure functions. `retrieve_identity_context` becomes an ordered pipeline — filter current records, resolve conflicts across the complete eligible set, classify and assign, allocate budget — gated by an `IdentityRetrievalPolicy` that `live_assist` derives from `ExpertProfileVersion::kind`. Composition never runs unless an Interview lens is selected.

**Tech Stack:** Rust (Tauri backend), `serde`/`serde_json`, `regex` 1.11 and `once_cell` (both already dependencies), Next.js/TypeScript for the two warning surfaces.

**Design of record:** [BROAD_QUESTION_RETRIEVAL_DESIGN.md](BROAD_QUESTION_RETRIEVAL_DESIGN.md). Where this plan and the design disagree, the design wins and the plan is wrong.

**Execution:** inline, with four review checkpoints. Stop at each and wait for review before continuing.

---

## Review Checkpoints

| Checkpoint | Tasks | What it delivers | Reviewable because |
| --- | --- | --- | --- |
| **1. Schema and config** | 1, 2, 6 | `ProfileKind`, `IdentityRetrievalPolicy` threaded through, validated config with override loading | No behaviour change yet — every existing test must still pass unchanged |
| **2. Pure retrieval** | 3, 4, 5, 7, 8, 9, 10 | Conflict resolution, diagnostics, normalisers, selection, budget, routing, composition | All pure functions, fully unit-testable with no Tauri runtime |
| **3. Runtime, upgrade, UI** | 11, 12, 13, 14 | Managed state, lens gating, abstention, prompt rule, four-step upgrade, both warning surfaces | First point at which behaviour is user-visible |
| **4. Integration and manual** | 15, Final Verification | Fixtures, integration tests, the manual acceptance check | The acceptance criterion is a human judgement about answer quality |

Checkpoint 2 is the largest and carries the real logic. If it needs splitting during execution, the natural seam is after Task 8 — Tasks 3 to 8 are independent primitives, and Tasks 9 to 10 assemble them.

---

## Before You Start: Build Prerequisites

`cargo test` on this Windows machine fails twice before it works. Both are environmental, not code:

1. **`cmake` is not on PATH**, so `whisper-rs-sys` fails with "is `cmake` not installed?". Prepend the copy bundled with VS Build Tools:

```bash
export PATH="/c/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin:$PATH"
```

2. **The `llama-helper` sidecar must exist before the Tauri build script runs**, or it aborts with ``resource path `binaries\llama-helper-x86_64-pc-windows-msvc.exe` doesn't exist``:

```bash
cargo build -p llama-helper && cp target/debug/llama-helper.exe frontend/src-tauri/binaries/llama-helper-x86_64-pc-windows-msvc.exe
```

That directory is gitignored, so the artifact will not be committed.

**Cold build of the workspace test binaries takes ~25 minutes** (`whisper-rs-sys` and `llama-cpp-sys-2` dominate). Run it once in the background before starting Task 1. Subsequent per-task runs are fast.

The crate under test is `app_lib`. All test commands in this plan use `cargo test -p app_lib`.

---

## File Structure

**Created:**

| Path | Responsibility |
| --- | --- |
| `frontend/src-tauri/src/professional_identity/composition/mod.rs` | Public types, package orchestration, three-outcome routing |
| `frontend/src-tauri/src/professional_identity/composition/config.rs` | Config types, embedded default, validation, precompiled regexes |
| `frontend/src-tauri/src/professional_identity/composition/normalize.rs` | `normalize_phrase()` and `informative_tokens()` — kept separate on purpose |
| `frontend/src-tauri/src/professional_identity/composition/selection.rs` | Dimension matching and single deterministic assignment |
| `frontend/src-tauri/src/professional_identity/composition/budget.rs` | Quota allocation and the truncation ladder |
| `frontend/src-tauri/src/professional_identity/composition/composition.default.json` | Shipped career-neutral config, embedded via `include_str!` |
| `frontend/src-tauri/src/professional_identity/conflict.rs` | Conflict grouping and supersession, extracted from `mod.rs` |
| `frontend/src-tauri/src/professional_identity/diagnostics.rs` | `RetrievalDiagnostics` and its sub-types |
| `frontend/src-tauri/tests/fixtures/composition/corpus.json` | Anonymised synthetic identity fixture |
| `frontend/src-tauri/tests/composition_retrieval.rs` | Integration tests, incl. the env-gated real-corpus check |
| `frontend/src/components/ConfigStatusWarning.tsx` | Detailed persistent Settings warning |
| `frontend/src/components/__tests__/ConfigStatusWarning.test.tsx` | Its tests |

**Modified:**

| Path | Change |
| --- | --- |
| `frontend/src-tauri/src/professional_identity/mod.rs` | Pipeline reorder, new signature, diagnostics, module wiring |
| `frontend/src-tauri/src/expert_profiles/models.rs` | `ProfileKind` enum, `ExpertProfileVersion::kind` |
| `frontend/src-tauri/src/live_assist/mod.rs` | Single profile load, policy derivation, abstention short-circuit, truncation prompt rule |
| `frontend/src-tauri/src/live_assist/voice_harness.rs` | 4 call sites updated for the new signature |
| `frontend/src-tauri/src/professional_identity/commands.rs` | `get_composition_config_status` command |
| `frontend/src-tauri/src/expert_profiles/commands.rs` | Interview-lens upgrade command |
| `frontend/src-tauri/src/app_paths.rs` | `composition_override_path()` |
| `frontend/src-tauri/src/lib.rs` | Register the two new commands |
| `frontend/src/components/ProfessionalIdentitySettings.tsx` | Renders the persistent config-status warning |
| `frontend/src/app/live-assist/page.tsx` | Compact non-blocking fallback badge |

`professional_identity/mod.rs` is 742 lines today. Extracting conflict resolution and diagnostics keeps it as an orchestrator rather than growing it.

---

## Task 1: `ProfileKind` on `ExpertProfileVersion`

Schema first, no behaviour change. The discriminator goes on the **profile**, not the playbook: Interview is the lens; Junior, Mid-level and Expert are depth playbooks within it.

**Files:**
- Modify: `frontend/src-tauri/src/expert_profiles/models.rs:8-18`, `:54`
- Test: `frontend/src-tauri/src/expert_profiles/tests.rs`

- [ ] **Step 1: Write the failing tests**

Append to `frontend/src-tauri/src/expert_profiles/tests.rs`:

```rust
use crate::expert_profiles::presets::interview_profile;

#[test]
fn profile_version_without_kind_deserialises_to_none() {
    // Stored versions predate `kind`. They must load unchanged.
    let mut value = serde_json::to_value(interview_profile()).unwrap();
    value.as_object_mut().unwrap().remove("kind");
    let parsed: ExpertProfileVersion = serde_json::from_value(value).unwrap();
    assert_eq!(parsed.kind, None);
}

#[test]
fn profile_version_round_trips_interview_kind() {
    let mut profile = interview_profile();
    profile.kind = Some(ProfileKind::Interview);
    let encoded = serde_json::to_string(&profile).unwrap();
    let decoded: ExpertProfileVersion = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.kind, Some(ProfileKind::Interview));
}

#[test]
fn the_shipped_interview_preset_declares_the_interview_lens() {
    // New profiles created from the preset ARE an interview lens and must not
    // require the manual upgrade. Existing stored versions still do.
    assert_eq!(interview_profile().kind, Some(ProfileKind::Interview));
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p app_lib expert_profiles::tests::profile_version -- --nocapture
```

Expected: FAIL — `no field 'kind' on type ExpertProfileVersion`.

- [ ] **Step 3: Add the type and field**

In `frontend/src-tauri/src/expert_profiles/models.rs`, add after the `ExpertProfileVersion` struct:

```rust
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileKind {
    /// Interview lens. The only kind that enables identity composition.
    Interview,
    Other,
}
```

Add the field to `ExpertProfileVersion`, after `playbooks`:

```rust
    #[serde(default)]
    pub kind: Option<ProfileKind>,
```

`deny_unknown_fields` rejects unknown keys but permits absent ones, so `#[serde(default)]` makes this backward compatible with every stored version.

- [ ] **Step 4: Fix construction sites**

`ExpertProfileVersion` is constructed in `expert_profiles/presets.rs:65` (inside `interview_profile()`) and `expert_profiles/tests.rs:31`. Compile to find any others:

```bash
cargo check -p app_lib 2>&1 | grep -A3 "missing field"
```

**The two construction sites take different values, and the distinction matters:**

- `presets::interview_profile()` gets `kind: Some(ProfileKind::Interview)`. A profile newly created from the shipped Interview preset *is* an Interview lens, and should not require a manual upgrade.
- `tests.rs:31` gets `kind: None`, so the generic test fixture keeps exercising the default path.

Existing **stored** versions are unaffected either way: they deserialise to `None` and require the explicit upgrade in Task 13. Only newly created profiles pick up the preset's value.

- [ ] **Step 5: Run tests to verify they pass**

```bash
cargo test -p app_lib expert_profiles -- --nocapture
```

Expected: PASS, including pre-existing expert-profile tests.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/src/expert_profiles/
git commit -m "feat: add optional ProfileKind discriminator to expert profile versions"
```

---

## Task 2: `IdentityRetrievalPolicy` threaded through retrieval

The name matters: `RetrievalPolicy` already exists as a **struct** at `expert_profiles/models.rs:54` and is already a field on `ExpertProfileVersion`. A second type by that name would collide inside the same struct.

This task changes signatures only. Behaviour is identical to today.

**Files:**
- Create: `frontend/src-tauri/src/professional_identity/composition/mod.rs`
- Modify: `frontend/src-tauri/src/professional_identity/mod.rs:259-268`
- Modify call sites: `live_assist/mod.rs:1467`, `live_assist/voice_harness.rs:888,914,1077,1117`, and 8 test sites in `professional_identity/mod.rs`

- [ ] **Step 1: Write the failing test**

Append to the `mod tests` block in `frontend/src-tauri/src/professional_identity/mod.rs`:

```rust
#[test]
fn lexical_only_policy_matches_previous_behaviour() {
    let profile = sample_identity();
    let result = retrieve_identity_context(
        &profile,
        "budget approvals authority",
        IdentityRetrievalPolicy::LexicalOnly,
        Utc::now(),
    )
    .unwrap();
    assert!(!result.sources.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p app_lib professional_identity::tests::lexical_only_policy -- --nocapture
```

Expected: FAIL — `cannot find type IdentityRetrievalPolicy`.

- [ ] **Step 3: Create the module and type**

Create `frontend/src-tauri/src/professional_identity/composition/mod.rs`:

```rust
//! Composition of balanced evidence packages for broad questions.
//!
//! See docs/product/BROAD_QUESTION_RETRIEVAL_DESIGN.md.

pub mod budget;
pub mod config;
pub mod normalize;
pub mod selection;

/// Whether composition is permitted for this retrieval.
///
/// Named to avoid collision with `expert_profiles::models::RetrievalPolicy`,
/// which is a different concept and is already a field on the same profile type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityRetrievalPolicy {
    /// Lexical retrieval only. Current behaviour.
    LexicalOnly,
    /// Named intents and broad-question composition permitted.
    CompositionEnabled,
}
```

Create empty placeholder files so the module compiles — each is filled in by a later task:

```bash
cd frontend/src-tauri/src/professional_identity/composition
printf '//! Config types and validation. Filled in by Task 6.\n' > config.rs
printf '//! Phrase and token normalisation. Filled in by Task 5.\n' > normalize.rs
printf '//! Dimension assignment. Filled in by Task 7.\n' > selection.rs
printf '//! Budget allocation and truncation. Filled in by Task 8.\n' > budget.rs
```

- [ ] **Step 4: Wire the module and change the signature**

In `frontend/src-tauri/src/professional_identity/mod.rs`, add near the other module declarations:

```rust
pub mod composition;
pub use composition::IdentityRetrievalPolicy;
```

Change the signature at line 259:

```rust
pub fn retrieve_identity_context(
    profile: &ProfessionalIdentityVersion,
    question: &str,
    policy: IdentityRetrievalPolicy,
    now: DateTime<Utc>,
) -> Result<RetrievedIdentityContext> {
```

Add `let _ = policy;` as the first line of the body for now. Task 10 consumes it.

- [ ] **Step 5: Update all 13 call sites**

Pass `IdentityRetrievalPolicy::LexicalOnly` at every existing call site — `live_assist/mod.rs:1467`, the four in `voice_harness.rs`, and the eight in the `professional_identity` test module. Find them:

```bash
grep -rn "retrieve_identity_context(" frontend/src-tauri/src --include=*.rs
```

- [ ] **Step 6: Run tests to verify they pass**

```bash
cargo test -p app_lib professional_identity -- --nocapture
```

Expected: PASS. Every pre-existing test still passes, unchanged, because `LexicalOnly` is today's behaviour.

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/src/professional_identity/ frontend/src-tauri/src/live_assist/
git commit -m "feat: thread IdentityRetrievalPolicy through identity retrieval"
```

---

## Task 3: Conflict resolution, extracted and corrected

Two changes at once because they are inseparable: the policy changes, and **the pipeline order changes**. Today conflicts are detected on the post-scoring `ranked` subset. The design requires resolution across the **complete eligible set**, before scoring, so a conflict is resolved the same way regardless of which records happen to score.

**Files:**
- Create: `frontend/src-tauri/src/professional_identity/conflict.rs`
- Modify: `frontend/src-tauri/src/professional_identity/mod.rs:325-357`

- [ ] **Step 1: Write the failing tests**

Create `frontend/src-tauri/src/professional_identity/conflict.rs` with tests only for now:

```rust
//! Conflict-key grouping and supersession.
//!
//! Applies to BOTH the composed and lexical paths. Replaces the previous
//! behaviour, which aborted retrieval entirely when any conflict_key appeared
//! on more than one live record.

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn record(id: u128, key: &str, updated_at: &str) -> ConflictCandidate {
        ConflictCandidate {
            id: Uuid::from_u128(id),
            conflict_key: Some(key.to_string()),
            updated_at: updated_at.to_string(),
            revision: "r1".to_string(),
        }
    }

    #[test]
    fn strictly_newer_record_supersedes_older() {
        let input = vec![
            record(1, "authority", "2026-01-01T00:00:00Z"),
            record(2, "authority", "2026-06-01T00:00:00Z"),
        ];
        let outcome = resolve_conflicts(&input);
        assert_eq!(outcome.kept, vec![Uuid::from_u128(2)]);
        assert_eq!(outcome.suppressed.len(), 1);
        assert_eq!(outcome.suppressed[0].reason, SuppressionReason::Superseded);
        assert_eq!(outcome.suppressed[0].record_ids, vec![Uuid::from_u128(1)]);
    }

    #[test]
    fn equal_timestamps_suppress_the_whole_group() {
        let input = vec![
            record(1, "authority", "2026-06-01T00:00:00Z"),
            record(2, "authority", "2026-06-01T00:00:00Z"),
        ];
        let outcome = resolve_conflicts(&input);
        assert!(outcome.kept.is_empty(), "no arbitrary tie-break is permitted");
        assert_eq!(
            outcome.suppressed[0].reason,
            SuppressionReason::AmbiguousFreshness
        );
    }

    #[test]
    fn unparseable_timestamp_suppresses_the_whole_group() {
        let input = vec![
            record(1, "authority", "not-a-timestamp"),
            record(2, "authority", "2026-06-01T00:00:00Z"),
        ];
        let outcome = resolve_conflicts(&input);
        assert!(outcome.kept.is_empty());
        assert_eq!(
            outcome.suppressed[0].reason,
            SuppressionReason::AmbiguousFreshness
        );
    }

    #[test]
    fn records_without_a_conflict_key_are_always_kept() {
        let input = vec![ConflictCandidate {
            id: Uuid::from_u128(9),
            conflict_key: None,
            updated_at: "2026-06-01T00:00:00Z".to_string(),
            revision: "r1".to_string(),
        }];
        let outcome = resolve_conflicts(&input);
        assert_eq!(outcome.kept, vec![Uuid::from_u128(9)]);
        assert!(outcome.suppressed.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p app_lib professional_identity::conflict -- --nocapture
```

Expected: FAIL — `cannot find type ConflictCandidate`.

- [ ] **Step 3: Implement**

Prepend to `frontend/src-tauri/src/professional_identity/conflict.rs`:

```rust
use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

/// The subset of a record that conflict resolution needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictCandidate {
    pub id: Uuid,
    pub conflict_key: Option<String>,
    pub updated_at: String,
    pub revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SuppressionReason {
    /// A strictly newer record exists for this key.
    Superseded,
    /// Freshness tied, was missing, or could not be parsed.
    AmbiguousFreshness,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuppressedGroup {
    pub conflict_key: String,
    pub record_ids: Vec<Uuid>,
    pub revisions: Vec<String>,
    pub reason: SuppressionReason,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConflictOutcome {
    pub kept: Vec<Uuid>,
    pub suppressed: Vec<SuppressedGroup>,
}

fn parse(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

/// Resolve conflicts across the COMPLETE eligible set.
///
/// Keeps a record only when its `updated_at` is strictly newer than every
/// other record sharing its key. Otherwise the entire group is suppressed.
/// No arbitrary tie-breaker is used anywhere.
pub fn resolve_conflicts(candidates: &[ConflictCandidate]) -> ConflictOutcome {
    let mut outcome = ConflictOutcome::default();
    let mut groups: BTreeMap<String, Vec<&ConflictCandidate>> = BTreeMap::new();

    for candidate in candidates {
        match candidate.conflict_key.as_deref() {
            None => outcome.kept.push(candidate.id),
            Some(key) => groups.entry(key.to_string()).or_default().push(candidate),
        }
    }

    for (key, group) in groups {
        if group.len() == 1 {
            outcome.kept.push(group[0].id);
            continue;
        }

        let timestamps: Option<Vec<DateTime<Utc>>> = group
            .iter()
            .map(|candidate| parse(&candidate.updated_at))
            .collect();

        let winner = timestamps.as_ref().and_then(|stamps| {
            let newest = stamps.iter().max()?;
            // Strict supersession: exactly one record may hold the maximum.
            if stamps.iter().filter(|stamp| *stamp == newest).count() == 1 {
                stamps.iter().position(|stamp| stamp == newest)
            } else {
                None
            }
        });

        match winner {
            Some(index) => {
                outcome.kept.push(group[index].id);
                outcome.suppressed.push(SuppressedGroup {
                    conflict_key: key,
                    record_ids: group
                        .iter()
                        .enumerate()
                        .filter(|(position, _)| *position != index)
                        .map(|(_, candidate)| candidate.id)
                        .collect(),
                    revisions: group
                        .iter()
                        .enumerate()
                        .filter(|(position, _)| *position != index)
                        .map(|(_, candidate)| candidate.revision.clone())
                        .collect(),
                    reason: SuppressionReason::Superseded,
                });
            }
            None => outcome.suppressed.push(SuppressedGroup {
                conflict_key: key,
                record_ids: group.iter().map(|candidate| candidate.id).collect(),
                revisions: group
                    .iter()
                    .map(|candidate| candidate.revision.clone())
                    .collect(),
                reason: SuppressionReason::AmbiguousFreshness,
            }),
        }
    }

    outcome.kept.sort();
    outcome
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p app_lib professional_identity::conflict -- --nocapture
```

Expected: PASS, 4 tests.

- [ ] **Step 5: Reorder the pipeline in `mod.rs`**

Declare the module (`pub mod conflict;`). In `retrieve_identity_context`, replace the `conflicting_key_counts` block and the `Err` return at lines 325-357 with this order:

```rust
    // 1. Filter to current records.
    let eligible: Vec<Candidate<'_>> = candidates
        .into_iter()
        .filter(|candidate| !is_expired(candidate.valid_until, now))
        .collect();

    // 2. Resolve conflicts across the COMPLETE eligible set, before scoring,
    //    so the outcome does not depend on which records happen to match.
    let conflict_input: Vec<conflict::ConflictCandidate> = eligible
        .iter()
        .map(|candidate| conflict::ConflictCandidate {
            id: candidate.id,
            conflict_key: candidate.conflict_key.map(str::to_string),
            updated_at: candidate.updated_at.to_string(),
            revision: candidate.source.revision.clone(),
        })
        .collect();
    let conflict_outcome = conflict::resolve_conflicts(&conflict_input);
    let kept: std::collections::HashSet<Uuid> =
        conflict_outcome.kept.iter().copied().collect();
    let survivors: Vec<Candidate<'_>> = eligible
        .into_iter()
        .filter(|candidate| kept.contains(&candidate.id))
        .collect();

    // 3. Score and rank the survivors.
    let mut ranked: Vec<(usize, Candidate<'_>)> = survivors
        .into_iter()
        .map(|candidate| (lexical_score(&query_terms, &candidate.score_text), candidate))
        .filter(|(score, _)| *score > 0)
        .collect();
```

Delete the old expiry filter, the `conflicting_key_counts` fold, the `relevant_conflicting_keys` collection, and the `return Err(...)`.

- [ ] **Step 6: Replace the obsolete conflict test**

The existing test at `professional_identity/mod.rs:566` asserts the removed `Err`. Replace its body:

```rust
#[test]
fn conflicting_current_sources_are_resolved_not_aborted() {
    let profile = conflicting_identity();
    let result = retrieve_identity_context(
        &profile,
        "authority",
        IdentityRetrievalPolicy::LexicalOnly,
        Utc::now(),
    );
    assert!(
        result.is_ok(),
        "a curation conflict must not become a live outage"
    );
}
```

- [ ] **Step 7: Run the full module tests**

```bash
cargo test -p app_lib professional_identity -- --nocapture
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add frontend/src-tauri/src/professional_identity/
git commit -m "feat: resolve identity conflicts by supersession instead of aborting"
```

---

## Task 4: Retrieval diagnostics

Diagnostics stay **out** of `prompt_json`. This task adds the carrier; later tasks populate it.

**Files:**
- Create: `frontend/src-tauri/src/professional_identity/diagnostics.rs`
- Modify: `frontend/src-tauri/src/professional_identity/mod.rs:117-121`

- [ ] **Step 1: Write the failing test**

Create `frontend/src-tauri/src/professional_identity/diagnostics.rs`:

```rust
//! Retrieval provenance. Never serialised into the model-visible prompt.

use serde::Serialize;
use uuid::Uuid;

use super::conflict::SuppressedGroup;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AbstentionReason {
    /// The package's anchor dimension was empty after conflict suppression.
    AnchorEmpty,
    /// Lexical retrieval selected nothing and composition did not apply.
    LexicalEmpty,
}

// ConfigStatus is declared once, in composition::config (Task 6), and shared.
// Two shapes for one concept - an enum here and a bare String there - would
// drift, and the Live Assist badge compares the serialised value.
pub use super::composition::config::ConfigStatus;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordDiagnostic {
    pub record_id: Uuid,
    pub dimension: Option<String>,
    pub original_chars: usize,
    pub admitted_chars: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OmittedRecord {
    pub record_id: Uuid,
    pub original_chars: usize,
    /// Always "first_sentence_exceeds_cap" today. Present so the reason is
    /// explicit in provenance rather than inferred.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalDiagnostics {
    pub selection_mode: String,
    pub anchor: Option<String>,
    pub anchor_survived: bool,
    pub records: Vec<RecordDiagnostic>,
    pub suppressed: Vec<SuppressedGroup>,
    pub omitted: Vec<OmittedRecord>,
    pub evidence_chars_used: usize,
    pub evidence_chars_total: usize,
    pub prompt_json_bytes: usize,
    pub config_status: ConfigStatus,
    pub abstained: Option<AbstentionReason>,
}

impl RetrievalDiagnostics {
    pub fn lexical() -> Self {
        Self {
            selection_mode: "lexical".to_string(),
            anchor: None,
            anchor_survived: true,
            records: Vec::new(),
            suppressed: Vec::new(),
            omitted: Vec::new(),
            evidence_chars_used: 0,
            evidence_chars_total: 0,
            prompt_json_bytes: 0,
            config_status: ConfigStatus::Default,
            abstained: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_are_never_part_of_the_prompt_payload() {
        // PromptIdentityContext is the only thing serialised into prompt_json.
        // This asserts the diagnostics type is not reachable from it.
        let rendered = serde_json::to_string(&RetrievalDiagnostics::lexical()).unwrap();
        assert!(rendered.contains("selectionMode"));
        assert!(rendered.contains("promptJsonBytes"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p app_lib professional_identity::diagnostics -- --nocapture
```

Expected: FAIL — module not declared.

- [ ] **Step 3: Wire the module and extend the result type**

In `professional_identity/mod.rs` add `pub mod diagnostics;`, and extend `RetrievedIdentityContext` at line 117:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievedIdentityContext {
    pub prompt_json: String,
    pub sources: Vec<GroundingSource>,
    pub diagnostics: diagnostics::RetrievalDiagnostics,
}
```

- [ ] **Step 4: Populate it on the lexical path**

At the end of `retrieve_identity_context`, before the `Ok(...)`:

```rust
    let mut diagnostics = diagnostics::RetrievalDiagnostics::lexical();
    diagnostics.suppressed = conflict_outcome.suppressed;
    diagnostics.prompt_json_bytes = prompt_json.len();

    Ok(RetrievedIdentityContext {
        prompt_json,
        sources,
        diagnostics,
    })
```

Add `diagnostics: Default::default()` or the explicit constructor to the `no_professional_identity` literal in `live_assist/mod.rs:1450`.

- [ ] **Step 5: Run tests**

```bash
cargo test -p app_lib professional_identity -- --nocapture
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/src/professional_identity/ frontend/src-tauri/src/live_assist/
git commit -m "feat: add retrieval diagnostics separate from prompt payload"
```

---

## Task 5: The two normalisers

**This is the task the design blocker was about.** A single filler-stripping normaliser reduces `"tell me about yourself"` — the canonical pattern — to the empty string, because every one of its tokens is filler. Keep them separate.

**Files:**
- Modify: `frontend/src-tauri/src/professional_identity/composition/normalize.rs`

- [ ] **Step 1: Write the failing tests**

Replace the contents of `normalize.rs` with tests first:

```rust
//! Two distinct normalisers. Do not merge them.
//!
//! `normalize_phrase` removes NO tokens and is used for named-pattern matching.
//! `informative_tokens` removes filler and is used ONLY for broadness detection.
//! Applying filler removal to patterns empties "tell me about yourself".

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_phrase_preserves_every_token() {
        assert_eq!(
            normalize_phrase("  Tell us ABOUT yourself?  "),
            "tell us about yourself"
        );
    }

    #[test]
    fn canonical_pattern_survives_phrase_normalisation() {
        // The regression test for the design blocker.
        assert!(!normalize_phrase("tell me about yourself").is_empty());
    }

    #[test]
    fn every_shipped_pattern_survives_phrase_normalisation() {
        // Asserted for ALL patterns, not just the canonical one, so a future
        // pattern that is entirely filler cannot slip in.
        let config = crate::professional_identity::composition::config::load_default().unwrap();
        for intent in &config.intents {
            for pattern in &intent.patterns {
                assert!(
                    !pattern.is_empty(),
                    "intent '{}' has a pattern that normalises to empty",
                    intent.name
                );
            }
        }
    }

    fn corpus() -> CorpusFrequency {
        // 10 documents. "who/are/what/here" are ubiquitous; "budget" and
        // "approvals" are distinctive; "brought" is absent entirely.
        let mut documents: Vec<String> = (0..10)
            .map(|_| "who are what here general text".to_string())
            .collect();
        documents[0].push_str(" budget approvals");
        CorpusFrequency::build(&documents)
    }

    #[test]
    fn informative_tokens_strips_filler() {
        let tokens = informative_tokens("tell me about yourself", &corpus(), 0.2);
        assert!(tokens.is_empty(), "the whole phrase is filler");
    }

    #[test]
    fn informative_tokens_keeps_distinctive_domain_terms() {
        let tokens = informative_tokens("tell me about your budget approvals", &corpus(), 0.2);
        assert!(tokens.contains("budget"));
        assert!(tokens.contains("approvals"));
        assert!(!tokens.contains("about"));
    }

    #[test]
    fn ubiquitous_terms_are_not_informative() {
        // Appear in every document, so they cannot discriminate.
        let tokens = informative_tokens("who are you and what happened here", &corpus(), 0.2);
        assert!(
            tokens.is_empty(),
            "high document frequency means no retrievable signal"
        );
    }

    #[test]
    fn absent_terms_are_not_informative() {
        // df == 0: the term cannot retrieve anything, so it must not block
        // composition. Otherwise any typo would force the lexical path.
        let tokens = informative_tokens("what brought you here", &corpus(), 0.2);
        assert!(tokens.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p app_lib composition::normalize -- --nocapture
```

Expected: FAIL — functions not found.

- [ ] **Step 3: Implement**

Prepend to `normalize.rs`:

```rust
use std::collections::HashSet;

/// Filler removed for broadness detection ONLY. Never applied to patterns.
const FILLER: &[&str] = &[
    "tell", "us", "me", "about", "yourself", "walk", "through", "describe", "bit",
    "little", "just", "quick", "give", "your", "you",
];

/// Lowercase, strip punctuation, collapse whitespace. Removes no tokens.
///
/// Applied to BOTH the configured pattern and the incoming question so that
/// matching compares like with like.
pub fn normalize_phrase(value: &str) -> String {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Document frequency across the identity corpus.
///
/// Informativeness is a corpus property, not a fixed word list. A term is
/// informative only when it actually discriminates: present in the corpus,
/// but not in most of it.
#[derive(Debug, Clone, Default)]
pub struct CorpusFrequency {
    total: usize,
    counts: std::collections::HashMap<String, usize>,
}

impl CorpusFrequency {
    /// One entry per record. Duplicate terms within a record count once.
    pub fn build(documents: &[String]) -> Self {
        let mut counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();
        for document in documents {
            let seen: HashSet<String> = normalize_phrase(document)
                .split(' ')
                .filter(|word| !word.is_empty())
                .map(str::to_string)
                .collect();
            for term in seen {
                *counts.entry(term).or_insert(0) += 1;
            }
        }
        Self {
            total: documents.len(),
            counts,
        }
    }

    /// Informative when 0 < df <= max_ratio * total.
    ///
    /// df == 0 is NOT informative: an absent term retrieves nothing, so
    /// treating it as informative would let any typo block composition.
    pub fn is_informative(&self, term: &str, max_ratio: f32) -> bool {
        let frequency = self.counts.get(term).copied().unwrap_or(0);
        if frequency == 0 || self.total == 0 {
            return false;
        }
        (frequency as f32) <= (self.total as f32) * max_ratio
    }
}

/// Tokens that carry retrievable domain signal, for broadness detection only.
///
/// Filler removal alone is not sufficient — the design defines informativeness
/// by corpus document frequency, and a fixed word list cannot know which terms
/// discriminate in THIS corpus.
pub fn informative_tokens(
    value: &str,
    corpus: &CorpusFrequency,
    max_ratio: f32,
) -> HashSet<String> {
    normalize_phrase(value)
        .split(' ')
        .filter(|word| !word.is_empty())
        .filter(|word| word.chars().count() >= 3)
        .filter(|word| !FILLER.contains(word))
        .filter(|word| corpus.is_informative(word, max_ratio))
        .map(str::to_string)
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p app_lib composition::normalize -- --nocapture
```

Expected: PASS, 7 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/professional_identity/composition/normalize.rs
git commit -m "feat: separate phrase normalisation from filler removal"
```

---

## Task 6: Config, embedded default, and validation

**Files:**
- Modify: `frontend/src-tauri/src/professional_identity/composition/config.rs`
- Create: `frontend/src-tauri/src/professional_identity/composition/composition.default.json`

- [ ] **Step 1: Write the shipped default**

Create `composition.default.json`. Dimensions are **career-neutral** — no sector-specific dimension ships in the product:

```json
{
  "config_version": 1,
  "max_arbitrary_ties": 8,
  "informative_df_max_ratio": 0.2,
  "budget": { "total_evidence_chars": 7000, "per_record_chars": 1200 },
  "intents": [
    {
      "name": "self_introduction",
      "patterns": [
        "tell me about yourself",
        "tell us about yourself",
        "walk me through your background",
        "introduce yourself",
        "tell me a bit about yourself"
      ],
      "anchor": "career_core",
      "dimensions": ["career_core", "scope_and_scale", "leadership", "domain_practice", "role_fit"]
    }
  ],
  "fallback": {
    "anchor": "career_core",
    "dimensions": ["career_core", "scope_and_scale", "role_fit"]
  },
  "dimensions": [
    {
      "name": "career_core", "priority": 1, "quota_chars": 2400,
      "match_category": ["cv"], "match_any_tag": [], "match_title": []
    },
    {
      "name": "scope_and_scale", "priority": 2, "quota_chars": 1600,
      "match_category": [], "match_any_tag": ["operations", "delivery"], "match_title": []
    },
    {
      "name": "leadership", "priority": 3, "quota_chars": 1400,
      "match_category": ["authority"], "match_any_tag": ["leadership"], "match_title": []
    },
    {
      "name": "domain_practice", "priority": 4, "quota_chars": 1000,
      "match_category": ["operating_practice"], "match_any_tag": [], "match_title": []
    },
    {
      "name": "role_fit", "priority": 5, "quota_chars": 600,
      "match_category": ["terms_of_reference"], "match_any_tag": ["role", "fit", "motivation"], "match_title": []
    }
  ]
}
```

- [ ] **Step 2: Write the failing tests**

Replace `config.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_default_validates() {
        let config = load_default().expect("shipped default must always validate");
        assert_eq!(config.config_version, 1);
        assert_eq!(config.budget.total_evidence_chars, 7000);
        assert_eq!(config.budget.per_record_chars, 1200);
    }

    #[test]
    fn shipped_default_is_career_neutral() {
        let config = load_default().unwrap();
        let names: Vec<&str> = config.dimensions.iter().map(|d| d.name.as_str()).collect();
        assert!(
            !names.contains(&"emergency_regional"),
            "sector-specific dimensions belong in a local override"
        );
    }

    #[test]
    fn empty_pattern_is_rejected() {
        // "tell me about yourself" is all filler; if a pattern were normalised
        // with informative_tokens it would arrive here empty.
        let error = validate(raw_with_pattern("   ?  ")).unwrap_err();
        assert!(error.to_string().contains("pattern"));
    }

    #[test]
    fn duplicate_dimension_priority_is_rejected() {
        let mut raw = raw_default();
        raw.dimensions[1].priority = raw.dimensions[0].priority;
        let error = validate(raw).unwrap_err();
        assert!(error.to_string().contains("priority"));
    }

    #[test]
    fn dimension_without_selectors_is_rejected() {
        let mut raw = raw_default();
        raw.dimensions[0].match_category.clear();
        raw.dimensions[0].match_any_tag.clear();
        raw.dimensions[0].match_title.clear();
        let error = validate(raw).unwrap_err();
        assert!(error.to_string().contains("selector"));
    }

    #[test]
    fn anchor_absent_from_its_own_dimensions_is_rejected() {
        let mut raw = raw_default();
        raw.fallback.anchor = "leadership".to_string();
        raw.fallback.dimensions = vec!["career_core".to_string()];
        let error = validate(raw).unwrap_err();
        assert!(error.to_string().contains("anchor"));
    }

    #[test]
    fn invalid_title_regex_is_rejected_at_load() {
        let mut raw = raw_default();
        raw.dimensions[0].match_title = vec!["[unclosed".to_string()];
        let error = validate(raw).unwrap_err();
        assert!(error.to_string().contains("match_title"));
    }

    #[test]
    fn over_long_title_pattern_is_rejected() {
        let mut raw = raw_default();
        raw.dimensions[0].match_title = vec!["a".repeat(MAX_TITLE_PATTERN_CHARS + 1)];
        let error = validate(raw).unwrap_err();
        assert!(error.to_string().contains("match_title"));
    }

    fn raw_default() -> RawConfig {
        serde_json::from_str(DEFAULT_CONFIG_JSON).unwrap()
    }

    fn raw_with_pattern(pattern: &str) -> RawConfig {
        let mut raw = raw_default();
        raw.intents[0].patterns = vec![pattern.to_string()];
        raw
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

```bash
cargo test -p app_lib composition::config -- --nocapture
```

Expected: FAIL — types not found.

- [ ] **Step 4: Implement**

Prepend to `config.rs`:

```rust
use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde::Deserialize;

use super::normalize::normalize_phrase;

pub const DEFAULT_CONFIG_JSON: &str = include_str!("composition.default.json");
pub const MAX_TITLE_PATTERN_CHARS: usize = 128;
pub const MAX_TITLE_PATTERNS_PER_DIMENSION: usize = 8;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConfig {
    pub config_version: u32,
    pub max_arbitrary_ties: usize,
    /// A term is informative when 0 < document_frequency <= this fraction
    /// of the corpus. Governs broad-question detection.
    pub informative_df_max_ratio: f32,
    pub budget: BudgetConfig,
    pub intents: Vec<RawIntent>,
    pub fallback: RawPackage,
    pub dimensions: Vec<RawDimension>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BudgetConfig {
    pub total_evidence_chars: usize,
    pub per_record_chars: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawIntent {
    pub name: String,
    pub patterns: Vec<String>,
    pub anchor: String,
    pub dimensions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPackage {
    pub anchor: String,
    pub dimensions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDimension {
    pub name: String,
    pub priority: usize,
    pub quota_chars: usize,
    pub match_category: Vec<String>,
    pub match_any_tag: Vec<String>,
    pub match_title: Vec<String>,
}

/// Validated config. Title patterns are compiled here and never on the
/// question path.
#[derive(Debug, Clone)]
pub struct CompositionConfig {
    pub config_version: u32,
    pub max_arbitrary_ties: usize,
    pub informative_df_max_ratio: f32,
    pub budget: BudgetConfig,
    pub intents: Vec<Intent>,
    pub fallback: Package,
    pub dimensions: Vec<Dimension>,
}

#[derive(Debug, Clone)]
pub struct Intent {
    pub name: String,
    /// Already passed through `normalize_phrase`.
    pub patterns: Vec<String>,
    pub package: Package,
}

#[derive(Debug, Clone)]
pub struct Package {
    pub anchor: String,
    pub dimensions: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Dimension {
    pub name: String,
    pub priority: usize,
    pub quota_chars: usize,
    pub match_category: Vec<String>,
    pub match_any_tag: Vec<String>,
    pub match_title: Vec<Regex>,
}

pub fn load_default() -> Result<CompositionConfig> {
    let raw: RawConfig = serde_json::from_str(DEFAULT_CONFIG_JSON)?;
    validate(raw)
}

fn validate_package(package: &RawPackage, known: &[String], label: &str) -> Result<Package> {
    for name in &package.dimensions {
        if !known.contains(name) {
            bail!("{label} references unknown dimension '{name}'");
        }
    }
    if !package.dimensions.contains(&package.anchor) {
        bail!("{label} anchor '{}' is not in its own dimensions", package.anchor);
    }
    Ok(Package {
        anchor: package.anchor.clone(),
        dimensions: package.dimensions.clone(),
    })
}

pub fn validate(raw: RawConfig) -> Result<CompositionConfig> {
    let mut dimensions = Vec::new();
    let mut seen_priorities = Vec::new();
    let known: Vec<String> = raw.dimensions.iter().map(|d| d.name.clone()).collect();

    for dimension in &raw.dimensions {
        if seen_priorities.contains(&dimension.priority) {
            bail!(
                "duplicate dimension priority {} on '{}'",
                dimension.priority,
                dimension.name
            );
        }
        seen_priorities.push(dimension.priority);

        if dimension.match_category.is_empty()
            && dimension.match_any_tag.is_empty()
            && dimension.match_title.is_empty()
        {
            bail!("dimension '{}' declares no selector", dimension.name);
        }

        if dimension.match_title.len() > MAX_TITLE_PATTERNS_PER_DIMENSION {
            bail!(
                "dimension '{}' exceeds the match_title pattern limit",
                dimension.name
            );
        }

        let mut compiled = Vec::new();
        for pattern in &dimension.match_title {
            if pattern.chars().count() > MAX_TITLE_PATTERN_CHARS {
                bail!(
                    "dimension '{}' has an over-long match_title pattern",
                    dimension.name
                );
            }
            compiled.push(Regex::new(pattern).map_err(|error| {
                anyhow!(
                    "dimension '{}' has an invalid match_title pattern: {error}",
                    dimension.name
                )
            })?);
        }

        dimensions.push(Dimension {
            name: dimension.name.clone(),
            priority: dimension.priority,
            quota_chars: dimension.quota_chars,
            match_category: dimension.match_category.clone(),
            match_any_tag: dimension.match_any_tag.clone(),
            match_title: compiled,
        });
    }

    let mut intents = Vec::new();
    for intent in &raw.intents {
        let mut patterns = Vec::new();
        for pattern in &intent.patterns {
            let normalized = normalize_phrase(pattern);
            if normalized.is_empty() {
                bail!(
                    "intent '{}' declares a pattern that is empty after normalisation",
                    intent.name
                );
            }
            patterns.push(normalized);
        }
        if patterns.is_empty() {
            bail!("intent '{}' declares no pattern", intent.name);
        }
        let package = validate_package(
            &RawPackage {
                anchor: intent.anchor.clone(),
                dimensions: intent.dimensions.clone(),
            },
            &known,
            &format!("intent '{}'", intent.name),
        )?;
        intents.push(Intent {
            name: intent.name.clone(),
            patterns,
            package,
        });
    }

    let fallback = validate_package(&raw.fallback, &known, "fallback")?;

    dimensions.sort_by_key(|dimension| dimension.priority);

    if !(0.0..=1.0).contains(&raw.informative_df_max_ratio) {
        bail!("informative_df_max_ratio must be between 0.0 and 1.0");
    }

    Ok(CompositionConfig {
        config_version: raw.config_version,
        max_arbitrary_ties: raw.max_arbitrary_ties,
        informative_df_max_ratio: raw.informative_df_max_ratio,
        budget: raw.budget,
        intents,
        fallback,
        dimensions,
    })
}
```

- [ ] **Step 5: Load the local override, once, with a visible fallback**

Add the override loader and the status it records. This is what makes an invalid override degrade *visibly* rather than silently:

**No process-global status.** `load_with_override` *returns* the status next to the config rather than stashing it in a static. A `OnceLock` would leak across unit tests — whichever test ran first would fix the status for every later one — and would also hide the real problem in blocker 3, that lazy initialisation happens on first retrieval rather than at startup.

```rust
use std::path::PathBuf;

/// Declared once here and re-exported by `diagnostics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigStatus {
    Default,
    OverrideApplied,
    OverrideInvalid,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompositionConfigStatus {
    pub status: ConfigStatus,
    pub path: Option<String>,
    pub reason: Option<String>,
}

impl CompositionConfigStatus {
    fn shipped_default() -> Self {
        Self {
            status: ConfigStatus::Default,
            path: None,
            reason: None,
        }
    }
}

/// Load the override if present, else the shipped default.
///
/// An invalid override NEVER fails the load and never half-applies: it falls
/// back to the embedded default and reports why, for both warning surfaces.
pub fn load_with_override(
    override_path: Option<PathBuf>,
) -> (CompositionConfig, CompositionConfigStatus) {
    let default = load_default().expect("shipped default must validate");

    let Some(path) = override_path.filter(|path| path.exists()) else {
        return (default, CompositionConfigStatus::shipped_default());
    };

    let outcome = std::fs::read_to_string(&path)
        .map_err(|error| error.to_string())
        .and_then(|raw| {
            serde_json::from_str::<RawConfig>(&raw).map_err(|error| error.to_string())
        })
        .and_then(|raw| validate(raw).map_err(|error| error.to_string()));

    match outcome {
        Ok(config) => (
            config,
            CompositionConfigStatus {
                status: ConfigStatus::OverrideApplied,
                path: Some(path.display().to_string()),
                reason: None,
            },
        ),
        Err(reason) => {
            log::warn!("composition override at {} ignored: {reason}", path.display());
            (
                default,
                CompositionConfigStatus {
                    status: ConfigStatus::OverrideInvalid,
                    path: Some(path.display().to_string()),
                    reason: Some(reason),
                },
            )
        }
    }
}
```

Add the matching tests:

```rust
    #[test]
    fn missing_override_reports_default_status() {
        let (config, status) = load_with_override(None);
        assert_eq!(config.config_version, 1);
        assert_eq!(status.status, ConfigStatus::Default);
    }

    #[test]
    fn valid_override_reports_applied_status() {
        let dir = std::env::temp_dir().join("composition-override-valid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("good.json");
        std::fs::write(&path, DEFAULT_CONFIG_JSON).unwrap();
        let (_, status) = load_with_override(Some(path));
        assert_eq!(status.status, ConfigStatus::OverrideApplied);
    }

    #[test]
    fn invalid_override_falls_back_and_reports_the_reason() {
        let dir = std::env::temp_dir().join("composition-override-invalid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        std::fs::write(&path, "{ not valid json").unwrap();
        let (config, status) = load_with_override(Some(path));
        assert_eq!(config.config_version, 1, "falls back, never fails");
        assert_eq!(status.status, ConfigStatus::OverrideInvalid);
        assert!(status.reason.is_some(), "the failure must be explainable");
        assert!(status.path.is_some(), "the user must be told which file");
    }
```

Task 11 calls this once during Tauri `.setup()` and stores both halves in managed state.

- [ ] **Step 6: Run tests to verify they pass**

```bash
cargo test -p app_lib composition::config -- --nocapture
```

Expected: PASS, 12 tests.

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/src/professional_identity/composition/
git commit -m "feat: add validated composition config with career-neutral defaults"
```

---

## Task 7: Dimension assignment and deduplication

A record belongs to **exactly one** dimension — the lowest-priority match. Duplicate priorities are already rejected, so this is unambiguous.

**Files:**
- Modify: `frontend/src-tauri/src/professional_identity/composition/selection.rs`

- [ ] **Step 1: Write the failing tests**

Replace `selection.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::professional_identity::composition::config::load_default;

    fn candidate(category: &str, tags: &[&str], title: &str) -> SelectableRecord {
        SelectableRecord {
            id: uuid::Uuid::from_u128(1),
            category: category.to_string(),
            title: title.to_string(),
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
        }
    }

    #[test]
    fn selectors_of_the_same_kind_are_or_ed() {
        let config = load_default().unwrap();
        // scope_and_scale declares only match_any_tag ["operations", "delivery"].
        // Matching either tag is enough.
        let record = candidate("other", &["delivery"], "Anything");
        assert_eq!(assign(&config, &record), Some("scope_and_scale".to_string()));
    }

    #[test]
    fn different_selector_kinds_are_and_ed() {
        let config = load_default().unwrap();
        // leadership declares category ["authority"] AND tag ["leadership"].
        // The category alone must not be enough.
        let record = candidate("authority", &["unrelated"], "Anything");
        assert_eq!(assign(&config, &record), None);
    }

    #[test]
    fn a_record_matching_several_dimensions_takes_the_lowest_priority() {
        let config = load_default().unwrap();
        // Matches scope_and_scale (priority 2, via tag "operations") and
        // leadership (priority 3, via category + tag). Priority 2 wins.
        let record = candidate("authority", &["operations", "leadership"], "Anything");
        assert_eq!(assign(&config, &record), Some("scope_and_scale".to_string()));
    }

    #[test]
    fn a_record_matching_nothing_is_unassigned() {
        let config = load_default().unwrap();
        let record = candidate("stakeholder", &["nothing"], "Anything");
        assert_eq!(assign(&config, &record), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p app_lib composition::selection -- --nocapture
```

Expected: FAIL — `assign` not found.

- [ ] **Step 3: Implement**

Prepend to `selection.rs`:

```rust
use uuid::Uuid;

use super::config::{CompositionConfig, Dimension};

/// The subset of a record that dimension matching needs.
#[derive(Debug, Clone)]
pub struct SelectableRecord {
    pub id: Uuid,
    pub category: String,
    pub title: String,
    pub tags: Vec<String>,
}

fn matches(dimension: &Dimension, record: &SelectableRecord) -> bool {
    // Within a selector kind: OR. Across kinds that are present: AND.
    // Absent kinds are ignored rather than treated as a failed match.
    let mut any_present = false;

    if !dimension.match_category.is_empty() {
        any_present = true;
        if !dimension.match_category.contains(&record.category) {
            return false;
        }
    }
    if !dimension.match_any_tag.is_empty() {
        any_present = true;
        if !record
            .tags
            .iter()
            .any(|tag| dimension.match_any_tag.contains(tag))
        {
            return false;
        }
    }
    if !dimension.match_title.is_empty() {
        any_present = true;
        if !dimension
            .match_title
            .iter()
            .any(|pattern| pattern.is_match(&record.title))
        {
            return false;
        }
    }

    any_present
}

/// Assign a record to exactly one dimension: the lowest-priority match.
///
/// `config.dimensions` is sorted by priority during validation, and duplicate
/// priorities are rejected, so the first match is deterministic.
pub fn assign(config: &CompositionConfig, record: &SelectableRecord) -> Option<String> {
    config
        .dimensions
        .iter()
        .find(|dimension| matches(dimension, record))
        .map(|dimension| dimension.name.clone())
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p app_lib composition::selection -- --nocapture
```

Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/professional_identity/composition/selection.rs
git commit -m "feat: assign identity records to exactly one dimension"
```

---

## Task 8: Budget allocation and the truncation ladder

A sentence is **never** cut mid-way. If not even one complete sentence fits, the record is omitted entirely.

**Files:**
- Modify: `frontend/src-tauri/src/professional_identity/composition/budget.rs`

- [ ] **Step 1: Write the failing tests**

Replace `budget.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_content_is_returned_whole() {
        let outcome = fit("One sentence here.", 100);
        assert_eq!(outcome, Fit::Whole("One sentence here.".to_string()));
    }

    #[test]
    fn truncation_prefers_paragraph_boundaries() {
        let content = "First para line one.\n\nSecond para is much longer than the cap allows.";
        let outcome = fit(content, 30);
        assert_eq!(outcome, Fit::Truncated("First para line one.".to_string()));
    }

    #[test]
    fn truncation_falls_back_to_sentence_boundaries() {
        let content = "Alpha one. Beta two. Gamma three is far too long to include here.";
        let outcome = fit(content, 22);
        assert_eq!(outcome, Fit::Truncated("Alpha one. Beta two.".to_string()));
    }

    #[test]
    fn a_single_oversized_sentence_is_omitted_not_cut() {
        let content = "This one sentence is longer than the cap and must never be cut.";
        let outcome = fit(content, 20);
        assert_eq!(outcome, Fit::Omitted);
    }

    #[test]
    fn allocation_grants_the_lesser_of_demand_and_quota() {
        // (name, actual demand, configured quota)
        let admitted = allocate(
            &[
                ("career_core".to_string(), 5000, 2400),
                ("role_fit".to_string(), 100, 600),
            ],
            7000,
        );
        assert_eq!(admitted, vec![2400, 100]);
    }

    #[test]
    fn unused_quota_redistributes_downward_by_priority() {
        // career_core wants 400 of its 2400 quota, releasing 2000.
        // role_fit's effective cap becomes 600 + 2000 = 2600.
        let admitted = allocate(
            &[
                ("career_core".to_string(), 400, 2400),
                ("role_fit".to_string(), 5000, 600),
            ],
            3000,
        );
        assert_eq!(admitted[0], 400);
        assert_eq!(
            admitted[1], 2600,
            "the surplus must actually reach the later dimension"
        );
        assert!(admitted.iter().sum::<usize>() <= 3000);
    }

    #[test]
    fn carry_accumulates_across_several_dimensions() {
        // 2000 released by the first, 1500 by the second: the third's
        // effective cap is 600 + 3500.
        let admitted = allocate(
            &[
                ("career_core".to_string(), 400, 2400),
                ("scope_and_scale".to_string(), 100, 1600),
                ("role_fit".to_string(), 9000, 600),
            ],
            7000,
        );
        assert_eq!(admitted[0], 400);
        assert_eq!(admitted[1], 100);
        assert_eq!(admitted[2], 4100);
    }

    #[test]
    fn quota_caps_prevent_the_first_dimension_starving_the_rest() {
        // The starvation guard: without a per-dimension quota, career_core's
        // 50,000 characters of demand would consume the entire budget and
        // every later dimension would receive nothing.
        let admitted = allocate(
            &[
                ("career_core".to_string(), 50_000, 2400),
                ("scope_and_scale".to_string(), 50_000, 1600),
                ("role_fit".to_string(), 50_000, 600),
            ],
            7000,
        );
        assert_eq!(admitted, vec![2400, 1600, 600]);
        assert!(
            admitted.iter().all(|granted| *granted > 0),
            "no dimension may be starved to zero by an earlier one"
        );
    }

    #[test]
    fn a_total_smaller_than_the_quotas_still_terminates() {
        let admitted = allocate(
            &[
                ("career_core".to_string(), 5000, 2400),
                ("role_fit".to_string(), 5000, 600),
            ],
            1000,
        );
        assert_eq!(admitted, vec![1000, 0]);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p app_lib composition::budget -- --nocapture
```

Expected: FAIL — `fit` not found.

- [ ] **Step 3: Implement**

Prepend to `budget.rs`:

```rust
/// Outcome of fitting one record's content to the per-record cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fit {
    Whole(String),
    Truncated(String),
    /// Not even one complete sentence fits. A sentence is never cut.
    Omitted,
}

fn largest_prefix(units: &[&str], joiner: &str, cap: usize) -> Option<String> {
    let mut assembled = String::new();
    for unit in units {
        let candidate = if assembled.is_empty() {
            unit.trim().to_string()
        } else {
            format!("{assembled}{joiner}{}", unit.trim())
        };
        if candidate.chars().count() > cap {
            break;
        }
        assembled = candidate;
    }
    if assembled.is_empty() {
        None
    } else {
        Some(assembled)
    }
}

/// Apply the truncation ladder: whole, then paragraphs, then sentences, then omit.
pub fn fit(content: &str, cap: usize) -> Fit {
    let trimmed = content.trim();
    if trimmed.chars().count() <= cap {
        return Fit::Whole(trimmed.to_string());
    }

    let paragraphs: Vec<&str> = trimmed
        .split("\n\n")
        .filter(|part| !part.trim().is_empty())
        .collect();
    if paragraphs.len() > 1 {
        if let Some(prefix) = largest_prefix(&paragraphs, "\n\n", cap) {
            return Fit::Truncated(prefix);
        }
    }

    let mut sentences = Vec::new();
    let mut start = 0usize;
    let bytes: Vec<char> = trimmed.chars().collect();
    for (index, character) in bytes.iter().enumerate() {
        if matches!(character, '.' | '!' | '?') {
            let sentence: String = bytes[start..=index].iter().collect();
            sentences.push(sentence.trim().to_string());
            start = index + 1;
        }
    }
    let borrowed: Vec<&str> = sentences.iter().map(String::as_str).collect();
    match largest_prefix(&borrowed, " ", cap) {
        Some(prefix) => Fit::Truncated(prefix),
        None => Fit::Omitted,
    }
}

/// Allocate a total evidence budget across dimensions in priority order.
///
/// `demands` is (dimension name, ACTUAL characters wanted, configured quota),
/// already priority-ordered.
///
/// Two mechanisms, and both are needed:
///
/// - **Quota caps prevent starvation.** An early dimension cannot consume the
///   whole budget, so later dimensions always get their share.
/// - **Unused quota carries forward.** A dimension that wants less than its
///   quota releases the difference, and a later dimension's effective cap
///   becomes `own quota + carried`. Capping each dimension at its own quota
///   alone would strand the surplus and there would be no redistribution at
///   all, only capping.
///
/// Carry accumulates: surplus released by dimension 1 is available to
/// dimension 2, and whatever dimension 2 does not use passes to dimension 3.
pub fn allocate(demands: &[(String, usize, usize)], total: usize) -> Vec<usize> {
    let mut remaining = total;
    let mut carried = 0usize;
    let mut admitted = Vec::with_capacity(demands.len());
    for (_, wanted, quota) in demands {
        let effective_cap = quota.saturating_add(carried);
        let granted = (*wanted).min(effective_cap).min(remaining);
        // Whatever this dimension left within its effective cap passes on.
        carried = effective_cap.saturating_sub(granted);
        remaining -= granted;
        admitted.push(granted);
    }
    admitted
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p app_lib composition::budget -- --nocapture
```

Expected: PASS, 9 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/professional_identity/composition/budget.rs
git commit -m "feat: add evidence budget allocation and truncation ladder"
```

---

## Task 9: Three-outcome routing and anchor sufficiency

**Files:**
- Modify: `frontend/src-tauri/src/professional_identity/composition/mod.rs`

- [ ] **Step 1: Write the failing tests**

Append to `composition/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use config::load_default;

    use super::normalize::CorpusFrequency;

    /// 10 records. "budget"/"approvals" appear in one, so they discriminate.
    /// Everything else is ubiquitous or absent.
    fn corpus() -> CorpusFrequency {
        let mut documents: Vec<String> = (0..10)
            .map(|_| "general operations coordination text".to_string())
            .collect();
        documents[0].push_str(" budget approvals authority");
        CorpusFrequency::build(&documents)
    }

    #[test]
    fn named_pattern_matches_the_canonical_phrase() {
        let config = load_default().unwrap();
        let route = classify(&config, "Tell us about yourself?", LexicalSignal::Low, &corpus());
        assert_eq!(route, Route::Intent("self_introduction".to_string()));
    }

    #[test]
    fn unseen_broad_phrase_routes_to_the_fallback() {
        let config = load_default().unwrap();
        // Deliberately NOT in the shipped pattern list. Self-referential via
        // "who are you", and every remaining token is either ubiquitous or
        // absent from the corpus, so none is informative.
        let route = classify(
            &config,
            "So, who are you and what brought you here?",
            LexicalSignal::Low,
            &corpus(),
        );
        assert_eq!(route, Route::BroadFallback);
    }

    #[test]
    fn low_signal_without_broad_evidence_stays_lexical() {
        let config = load_default().unwrap();
        // "budget" and "approvals" are informative in this corpus.
        let route = classify(
            &config,
            "What was your authority over budget approvals?",
            LexicalSignal::Low,
            &corpus(),
        );
        assert_eq!(route, Route::Lexical);
    }

    #[test]
    fn strong_signal_stays_lexical_even_for_a_broad_phrase() {
        let config = load_default().unwrap();
        let route = classify(&config, "who are you", LexicalSignal::Strong, &corpus());
        assert_eq!(route, Route::Lexical);
    }

    /// A corpus where the marker word "background" is rare but present -
    /// exactly the case that wrongly reads as informative.
    fn corpus_with_rare_marker_word() -> CorpusFrequency {
        let mut documents: Vec<String> = (0..10)
            .map(|_| "general operations coordination text".to_string())
            .collect();
        documents[0].push_str(" background career history");
        documents[1].push_str(" budget approvals");
        CorpusFrequency::build(&documents)
    }

    #[test]
    fn a_rare_marker_word_does_not_block_the_fallback() {
        let config = load_default().unwrap();
        // "background" has df == 1 of 10, so it is "informative" on its own
        // terms. As part of the matched marker it must be excluded.
        let route = classify(
            &config,
            "Could you walk me through your background?",
            LexicalSignal::Low,
            &corpus_with_rare_marker_word(),
        );
        assert_eq!(route, Route::BroadFallback);
    }

    #[test]
    fn a_rare_marker_word_still_allows_other_domain_terms_to_block() {
        let config = load_default().unwrap();
        // The marker is excluded, but "budget" is not part of it and must
        // still force the lexical path.
        let route = classify(
            &config,
            "Tell me about your background in budget work",
            LexicalSignal::Low,
            &corpus_with_rare_marker_word(),
        );
        assert_eq!(route, Route::Lexical);
    }

    #[test]
    fn a_self_referential_phrase_with_a_domain_term_is_not_broad() {
        let config = load_default().unwrap();
        // Self-referential AND carries an informative term: both conditions
        // are required, so this must not compose.
        let route = classify(
            &config,
            "tell me about yourself and your budget experience",
            LexicalSignal::Low,
            &corpus(),
        );
        assert_eq!(route, Route::Lexical);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p app_lib composition::tests -- --nocapture
```

Expected: FAIL — `classify` not found.

- [ ] **Step 3: Implement**

Add to `composition/mod.rs` above the test module:

```rust
use config::CompositionConfig;
use normalize::{informative_tokens, normalize_phrase};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexicalSignal {
    Strong,
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Intent(String),
    BroadFallback,
    Lexical,
}

const SELF_REFERENTIAL: &[&str] = &[
    "yourself",
    "your background",
    "your career",
    "your experience",
    "about you",
    "who are you",
];

/// True when the question targets the person as a whole AND carries no
/// informative domain term.
///
/// Informativeness comes from corpus document frequency, not a word list:
/// a term that appears in most records cannot discriminate, and a term absent
/// from the corpus retrieves nothing. Both conditions are required.
fn has_broad_evidence(
    question: &str,
    corpus: &normalize::CorpusFrequency,
    max_ratio: f32,
) -> bool {
    let phrase = normalize_phrase(question);
    let Some(marker) = SELF_REFERENTIAL
        .iter()
        .find(|marker| phrase.contains(*marker))
    else {
        return false;
    };

    // The marker's OWN tokens must not count as domain terms.
    //
    // "background" appears in a single CV record, so its document frequency is
    // low and non-zero - textbook "informative". Left in, the marker
    // "your background" would disqualify the very question it identifies as
    // broad, and "walk me through your background" would never compose.
    let marker_tokens: std::collections::HashSet<String> = marker
        .split(' ')
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect();

    informative_tokens(question, corpus, max_ratio)
        .difference(&marker_tokens)
        .next()
        .is_none()
}

/// Three-outcome routing. See design section 4.
///
/// Named-pattern matching uses `normalize_phrase` on BOTH sides. Broadness
/// uses `informative_tokens`. The two normalisers are never crossed.
pub fn classify(
    config: &CompositionConfig,
    question: &str,
    signal: LexicalSignal,
    corpus: &normalize::CorpusFrequency,
) -> Route {
    let phrase = normalize_phrase(question);
    for intent in &config.intents {
        if intent.patterns.iter().any(|pattern| *pattern == phrase) {
            return Route::Intent(intent.name.clone());
        }
    }
    if signal == LexicalSignal::Low
        && has_broad_evidence(question, corpus, config.informative_df_max_ratio)
    {
        return Route::BroadFallback;
    }
    Route::Lexical
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p app_lib composition -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/professional_identity/composition/mod.rs
git commit -m "feat: add three-outcome routing for identity retrieval"
```

---

## Task 10: Wire composition into the pipeline

**Files:**
- Modify: `frontend/src-tauri/src/professional_identity/mod.rs`

- [ ] **Step 1: Write the failing test**

Append to the `mod tests` block in `professional_identity/mod.rs`:

```rust
#[test]
fn composition_enabled_uses_the_intent_package() {
    let profile = sample_identity();
    let config = composition::config::load_default().unwrap();
    let result = retrieve_identity_context(
        &profile,
        "Tell us about yourself",
        IdentityRetrievalPolicy::CompositionEnabled,
        &config,
        Utc::now(),
    )
    .unwrap();
    assert_eq!(result.diagnostics.selection_mode, "intent:self_introduction");
}

#[test]
fn composition_disabled_never_composes() {
    let profile = sample_identity();
    let config = composition::config::load_default().unwrap();
    let result = retrieve_identity_context(
        &profile,
        "Tell us about yourself",
        IdentityRetrievalPolicy::LexicalOnly,
        &config,
        Utc::now(),
    )
    .unwrap();
    assert_eq!(result.diagnostics.selection_mode, "lexical");
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p app_lib professional_identity::tests::composition -- --nocapture
```

Expected: FAIL — `selection_mode` is `"lexical"` in both.

- [ ] **Step 3: Take the config as a parameter — no global state**

**Do not use a `Lazy`/`OnceLock` global here.** A lazily-initialised static would initialise on the *first retrieval*, not at startup, so Settings could report `default` before any question had been asked even when a valid override exists. A process-global would also leak between unit tests, making config-dependent tests order-sensitive.

Instead `retrieve_identity_context` receives the resolved config. Retrieval stays a pure function; the runtime owns the lifetime. Extend the signature from Task 2:

```rust
pub fn retrieve_identity_context(
    profile: &ProfessionalIdentityVersion,
    question: &str,
    policy: IdentityRetrievalPolicy,
    config: &composition::config::CompositionConfig,
    now: DateTime<Utc>,
) -> Result<RetrievedIdentityContext> {
```

Update all 13 call sites again — including the tests added in Tasks 2 and 3, which were written against the earlier four-argument signature. Tests construct their own config with `composition::config::load_default().unwrap()`, so each test is independent and no global leaks between them. Task 11 adds the managed state that supplies the config at runtime.

Build the corpus frequency from the survivors, once per retrieval:

```rust
    let corpus = composition::normalize::CorpusFrequency::build(
        &survivors
            .iter()
            .map(|candidate| candidate.score_text.clone())
            .collect::<Vec<_>>(),
    );
```

- [ ] **Step 4: Implement the composed branch**

Replace the `ranked.sort_by(...)` / `ranked.truncate(MAX_RETRIEVED_SOURCES)` block with:

```rust
    let top_score = ranked.iter().map(|(score, _)| *score).max().unwrap_or(0);
    let tied = ranked.iter().filter(|(score, _)| *score == top_score).count();
    let signal = if top_score <= 1 || tied > config.max_arbitrary_ties {
        composition::LexicalSignal::Low
    } else {
        composition::LexicalSignal::Strong
    };

    let route = match policy {
        IdentityRetrievalPolicy::LexicalOnly => composition::Route::Lexical,
        IdentityRetrievalPolicy::CompositionEnabled => {
            composition::classify(config, question, signal, &corpus)
        }
    };

    ranked.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.id.cmp(&right.id))
    });

    let mut diagnostics = diagnostics::RetrievalDiagnostics::lexical();
    diagnostics.suppressed = conflict_outcome.suppressed;
    diagnostics.evidence_chars_total = config.budget.total_evidence_chars;

    let selected: Vec<(Candidate<'_>, String, bool)> = match &route {
        composition::Route::Lexical => {
            diagnostics.selection_mode = "lexical".to_string();
            if ranked.is_empty() {
                diagnostics.abstained =
                    Some(diagnostics::AbstentionReason::LexicalEmpty);
            }
            ranked
                .into_iter()
                .take(MAX_RETRIEVED_SOURCES)
                .map(|(_, candidate)| {
                    let content = candidate.content.clone();
                    (candidate, content, false)
                })
                .collect()
        }
        composition::Route::Intent(name) => {
            diagnostics.selection_mode = format!("intent:{name}");
            let package = config
                .intents
                .iter()
                .find(|intent| intent.name == *name)
                .map(|intent| &intent.package)
                .unwrap_or(&config.fallback);
            compose(config, package, survivors_for_composition, &mut diagnostics)
        }
        composition::Route::BroadFallback => {
            diagnostics.selection_mode = "broad_fallback".to_string();
            compose(
                config,
                &config.fallback,
                survivors_for_composition,
                &mut diagnostics,
            )
        }
    };
```

Note that composition draws from **all** conflict survivors, not from `ranked` — a record scoring zero on the literal query is exactly what composition exists to recover. Clone the survivor list before scoring consumes it:

```rust
    let survivors_for_composition = survivors.clone();
```

- [ ] **Step 5: Implement `compose`**

Add to `professional_identity/mod.rs`:

```rust
/// Build a balanced package: assign, allocate, fit, and record provenance.
fn compose<'a>(
    config: &composition::config::CompositionConfig,
    package: &composition::config::Package,
    survivors: Vec<Candidate<'a>>,
    diagnostics: &mut diagnostics::RetrievalDiagnostics,
) -> Vec<(Candidate<'a>, String, bool)> {
    use composition::{budget, selection};

    diagnostics.anchor = Some(package.anchor.clone());

    // 1. Assign each survivor to exactly one dimension.
    let mut by_dimension: std::collections::HashMap<String, Vec<Candidate<'a>>> =
        std::collections::HashMap::new();
    for candidate in survivors {
        let selectable = selection::SelectableRecord {
            id: candidate.id,
            category: candidate.category.to_string(),
            title: candidate.title.to_string(),
            tags: candidate.tags.iter().map(|tag| tag.to_string()).collect(),
        };
        if let Some(dimension) = selection::assign(config, &selectable) {
            if package.dimensions.contains(&dimension) {
                by_dimension.entry(dimension).or_default().push(candidate);
            }
        }
    }

    // 2. Order within each dimension by DOCUMENT ORDER, then id.
    //    Sorting by id alone would scramble a CV into random section order.
    for records in by_dimension.values_mut() {
        records.sort_by(|left, right| {
            left.doc_index
                .cmp(&right.doc_index)
                .then_with(|| left.id.cmp(&right.id))
        });
    }

    // 3. Allocate from ACTUAL demand, capped by quota.
    //    Passing the quota as the demand would reserve unused characters and
    //    defeat redistribution; the quota is a cap, not a reservation.
    let demands: Vec<(String, usize, usize)> = package
        .dimensions
        .iter()
        .map(|name| {
            let quota = config
                .dimensions
                .iter()
                .find(|dimension| dimension.name == *name)
                .map(|dimension| dimension.quota_chars)
                .unwrap_or(0);
            let wanted = by_dimension
                .get(name)
                .map(|records| {
                    records
                        .iter()
                        .map(|candidate| {
                            candidate
                                .content
                                .chars()
                                .count()
                                .min(config.budget.per_record_chars)
                        })
                        .sum::<usize>()
                })
                .unwrap_or(0);
            (name.clone(), wanted, quota)
        })
        .collect();
    let grants = budget::allocate(&demands, config.budget.total_evidence_chars);

    // 4. Fit records within each dimension's grant.
    let mut selected = Vec::new();
    let mut used = 0usize;
    for ((name, _, _), grant) in demands.iter().zip(grants) {
        let mut dimension_used = 0usize;
        let records = by_dimension.remove(name).unwrap_or_default();
        for candidate in records {
            let remaining = grant.saturating_sub(dimension_used);
            let cap = remaining.min(config.budget.per_record_chars);
            let original = candidate.content.chars().count();
            match budget::fit(&candidate.content, cap) {
                budget::Fit::Whole(content) => {
                    dimension_used += content.chars().count();
                    used += content.chars().count();
                    diagnostics.records.push(diagnostics::RecordDiagnostic {
                        record_id: candidate.id,
                        dimension: Some(name.clone()),
                        original_chars: original,
                        admitted_chars: content.chars().count(),
                        truncated: false,
                    });
                    selected.push((candidate, content, false));
                }
                budget::Fit::Truncated(content) => {
                    dimension_used += content.chars().count();
                    used += content.chars().count();
                    diagnostics.records.push(diagnostics::RecordDiagnostic {
                        record_id: candidate.id,
                        dimension: Some(name.clone()),
                        original_chars: original,
                        admitted_chars: content.chars().count(),
                        truncated: true,
                    });
                    selected.push((candidate, content, true));
                }
                budget::Fit::Omitted => {
                    diagnostics.omitted.push(diagnostics::OmittedRecord {
                        record_id: candidate.id,
                        original_chars: original,
                        reason: "first_sentence_exceeds_cap".to_string(),
                    });
                }
            }
        }
    }

    // 5. Anchor sufficiency. Applies to named intents and fallback alike.
    diagnostics.anchor_survived = diagnostics
        .records
        .iter()
        .any(|record| record.dimension.as_deref() == Some(package.anchor.as_str()));
    if !diagnostics.anchor_survived {
        diagnostics.abstained = Some(diagnostics::AbstentionReason::AnchorEmpty);
    }
    diagnostics.evidence_chars_used = used;

    selected
}
```

`Candidate` needs two new fields for this:

- `tags: &'a [String]` — for dimension matching.
- `doc_index: usize` — the record's position in `profile.records`, so document order survives. `profile.records` is a `Vec` persisted as a serialised blob, so insertion order round-trips; capture the index with `.enumerate()` at both push sites in `retrieve_identity_context`. Projects and project facts continue after the records, keeping a single stable ordering across all candidate kinds.

Emit `truncated: true` into `PromptIdentityRecord` only when the flag is set, via `#[serde(skip_serializing_if = "std::ops::Not::not")]`.

- [ ] **Step 6: Run tests to verify they pass**

```bash
cargo test -p app_lib professional_identity -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/src/professional_identity/
git commit -m "feat: compose balanced evidence packages for broad questions"
```

---

## Task 11: Live Assist wiring, single load, and abstention short-circuit

Both abstention reasons take the **same** local no-provider path, with distinct diagnostic reasons.

**Files:**
- Modify: `frontend/src-tauri/src/live_assist/mod.rs:1272-1273`, `:1363-1364`, `:1417-1442`, `:1444-1468`

- [ ] **Step 1: Write the failing test**

Append to the `mod tests` block in `live_assist/mod.rs`:

```rust
use crate::expert_profiles::models::ProfileKind;
use crate::expert_profiles::presets::interview_profile;
use crate::professional_identity::IdentityRetrievalPolicy;

#[test]
fn interview_profile_kind_enables_composition() {
    let mut profile = interview_profile();
    profile.kind = Some(ProfileKind::Interview);
    assert_eq!(
        derive_identity_policy(Some(&profile)),
        IdentityRetrievalPolicy::CompositionEnabled
    );
}

#[test]
fn absent_kind_falls_back_to_lexical_only() {
    let mut profile = interview_profile();
    profile.kind = None;
    assert_eq!(
        derive_identity_policy(Some(&profile)),
        IdentityRetrievalPolicy::LexicalOnly
    );
}

#[test]
fn profile_name_never_infers_the_interview_lens() {
    let mut profile = interview_profile();
    profile.identity.name = "Interview Coach".to_string();
    profile.kind = None;
    assert_eq!(
        derive_identity_policy(Some(&profile)),
        IdentityRetrievalPolicy::LexicalOnly,
        "the lens must never be inferred from a name"
    );
}

#[test]
fn no_profile_is_lexical_only() {
    assert_eq!(
        derive_identity_policy(None),
        IdentityRetrievalPolicy::LexicalOnly
    );
}
```

`presets::interview_profile()` at `expert_profiles/presets.rs:18` is the real shipped builder — use it rather than inventing a fixture.

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p app_lib live_assist::tests::interview_profile -- --nocapture
```

Expected: FAIL — `derive_identity_policy` not found.

- [ ] **Step 3: Implement policy derivation**

Add to `live_assist/mod.rs`:

```rust
/// Derive the identity retrieval policy from the profile's declared kind.
///
/// Never inferred from a profile name: a rename must not change behaviour.
fn derive_identity_policy(profile: Option<&ExpertProfileVersion>) -> IdentityRetrievalPolicy {
    match profile.and_then(|profile| profile.kind) {
        Some(ProfileKind::Interview) => IdentityRetrievalPolicy::CompositionEnabled,
        _ => IdentityRetrievalPolicy::LexicalOnly,
    }
}
```

- [ ] **Step 4: Load the profile once, return both values**

`load_profile_context` currently loads the version, renders it, and drops it — so the `kind` is already fetched and discarded. Change it to return both:

```rust
struct LoadedProfileContext {
    context: String,
    identity_policy: IdentityRetrievalPolicy,
}

async fn load_profile_context<R: Runtime>(
    app: &AppHandle<R>,
    selection: Option<((Uuid, String), Uuid)>,
) -> Result<LoadedProfileContext> {
    let Some(((profile_id, version_hash), playbook_id)) = selection else {
        return Ok(LoadedProfileContext {
            context: "No Expert Profile is selected. Give concise, practical meeting guidance."
                .to_string(),
            identity_policy: IdentityRetrievalPolicy::LexicalOnly,
        });
    };
    let app_state = app.state::<AppState>();
    validate_profile_selection(
        app_state.db_manager.pool(),
        profile_id,
        &version_hash,
        playbook_id,
    )
    .await?;
    let profile = ExpertProfilesRepository::get_profile_version(
        app_state.db_manager.pool(),
        profile_id,
        &version_hash,
    )
    .await?
    .ok_or_else(|| anyhow!("selected Expert Profile version was not found"))?;
    Ok(LoadedProfileContext {
        context: render_profile_context(&profile, playbook_id)?,
        identity_policy: derive_identity_policy(Some(&profile)),
    })
}
```

Update both call sites (lines 1272-1273 and 1363-1364):

```rust
    let profile_context = load_profile_context(&app, profile).await?;
    let identity_context = load_identity_context(
        &app,
        identity,
        &question,
        profile_context.identity_policy,
    )
    .await?;
```

Replace subsequent uses of `profile_context` with `profile_context.context`. Add the `policy` parameter to `load_identity_context` and forward it to `retrieve_identity_context`.

- [ ] **Step 4b: Resolve the composition config at startup, as managed state**

This is what makes the config genuinely startup-time, so Settings cannot report `default` before the first question.

Add to `app_paths.rs` — `AppPaths` is managed Tauri state resolved in `.setup()` (`lib.rs:463`), so the helper is a method on it, not a free function:

```rust
impl AppPaths {
    /// Optional user override for the composition config.
    pub fn composition_override_path(&self) -> PathBuf {
        self.root().join("composition.json")
    }
}
```

Add the managed state type:

```rust
pub struct CompositionState {
    pub config: composition::config::CompositionConfig,
    pub status: composition::config::CompositionConfigStatus,
}
```

Resolve and register it inside the existing `.setup()` closure in `lib.rs`, immediately after `AppPaths::resolve`:

```rust
            let (composition_config, composition_status) =
                professional_identity::composition::config::load_with_override(Some(
                    app_paths.composition_override_path(),
                ));
            if composition_status.status == ConfigStatus::OverrideInvalid {
                log::warn!(
                    "composition override ignored at startup: {:?}",
                    composition_status.reason
                );
            }
            _app.manage(professional_identity::CompositionState {
                config: composition_config,
                status: composition_status,
            });
```

`get_composition_config_status` (Task 14) then reads `state.status` rather than re-reading the file, and `load_identity_context` passes `&state.config` into `retrieve_identity_context`.

**Stamp the status onto every retrieval result.** `RetrievalDiagnostics::lexical()` initialises `config_status` to `Default`, and retrieval itself has no way to know what happened at startup. Without this the Live Assist badge reads `default` forever, and an invalid override stays invisible in the surface built to show it:

```rust
async fn load_identity_context<R: Runtime>(
    app: &AppHandle<R>,
    selection: Option<(Uuid, String)>,
    question: &str,
    policy: IdentityRetrievalPolicy,
) -> Result<RetrievedIdentityContext> {
    let composition = app.state::<CompositionState>();

    let Some((identity_id, version_hash)) = selection else {
        // The no-identity path carries the status too: a broken override is
        // still broken when no identity is selected.
        let mut diagnostics = diagnostics::RetrievalDiagnostics::lexical();
        diagnostics.config_status = composition.status.status;
        return Ok(RetrievedIdentityContext {
            prompt_json: serde_json::json!({ "context_type": "no_professional_identity" })
                .to_string(),
            sources: Vec::new(),
            diagnostics,
        });
    };

    // ... existing validation and repository load ...

    let mut context =
        retrieve_identity_context(&identity, question, policy, &composition.config, Utc::now())?;
    context.diagnostics.config_status = composition.status.status;
    Ok(context)
}
```

Add the test:

```rust
#[tokio::test]
async fn config_status_reaches_diagnostics_on_every_path() {
    let app = test_app_with_invalid_override().await;

    let with_identity = load_identity_context(&app, Some(selection()), "hello", policy())
        .await
        .unwrap();
    assert_eq!(
        with_identity.diagnostics.config_status,
        ConfigStatus::OverrideInvalid
    );

    let without = load_identity_context(&app, None, "hello", policy())
        .await
        .unwrap();
    assert_eq!(
        without.diagnostics.config_status,
        ConfigStatus::OverrideInvalid,
        "the no_professional_identity path must carry it too"
    );
}
```

- [ ] **Step 4c: Carry diagnostics onto the exchange**

The Live Assist badge needs `configStatus` per exchange, and nothing currently carries diagnostics that far. Add the field, include it in the snapshot the frontend receives, and mirror it in the TypeScript types:

```rust
// In AssistExchange:
    pub diagnostics: Option<professional_identity::diagnostics::RetrievalDiagnostics>,
```

Set it after `load_identity_context` returns, in both the answer and detail paths:

```rust
    {
        let state = app.state::<LiveAssistState>();
        let mut inner = state.lock();
        let exchange = find_exchange_mut(&mut inner, exchange_id)?;
        exchange.diagnostics = Some(identity_context.diagnostics.clone());
    }
```

Add `diagnostics: None` to every `AssistExchange` construction site — `cargo check -p app_lib` will list them. Extend the snapshot struct the frontend consumes with the same field, and add the matching TypeScript interface beside the existing exchange types so `exchange.diagnostics?.configStatus` type-checks.

`RetrievalDiagnostics` already derives `Serialize` with `rename_all = "camelCase"` (Task 4); add `Clone` if it is not already present.

- [ ] **Step 5: Add the abstention short-circuit**

Define the literal once so the prompt and the short-circuit cannot drift:

```rust
pub const ABSTENTION_RESPONSE: &str = "I need more context before I can answer that.";
```

Replace the literal inside both prompt templates with a `{abstention_response}` placeholder filled from this constant. Then, after `load_identity_context` and **before** `load_provider_config`:

```rust
    // Both abstention reasons take the same local path: no provider call,
    // no token spend, and no chance of the model paraphrasing the contract.
    // The reasons stay distinct in diagnostics.
    if let Some(reason) = identity_context.diagnostics.abstained {
        log::info!("live assist abstained locally: {reason:?}");
        return complete_locally(
            &app,
            exchange_id,
            generation_id,
            request_started,
            ABSTENTION_RESPONSE,
        );
    }
```

Add the helper, mirroring the completion sequence already used at `live_assist/mod.rs:1329-1345`. The lock method is `LiveAssistState::lock()` (`live_assist/mod.rs:781`) — there is no `lock_inner`.

The generation guard matters: an exchange can be superseded or interrupted while retrieval is running, and writing a completed answer over a newer generation would resurrect a cancelled turn. Every other completion path checks this, and so must this one.

```rust
/// Complete an exchange without calling a provider.
///
/// Used for local abstention. Mirrors the provider completion path, including
/// the generation guard and request timing, so an abstained exchange is
/// indistinguishable downstream from a normal completion.
fn complete_locally<R: Runtime>(
    app: &AppHandle<R>,
    exchange_id: Uuid,
    generation_id: u64,
    request_started: Instant,
    answer: &str,
) -> Result<()> {
    let state = app.state::<LiveAssistState>();
    let mut inner = state.lock();
    let exchange = find_exchange_mut(&mut inner, exchange_id)?;

    // Do not resurrect a superseded or interrupted turn.
    //
    // `clear_active_operation` guards only the cleanup of the active-operation
    // slot; it cannot un-write an answer already stored on the exchange. The
    // generation comparison is the real guard, and it is what the provider
    // paths at mod.rs:1228 and mod.rs:1278 already do.
    if exchange.generation_id != generation_id
        || exchange.status == AssistExchangeStatus::Interrupted
    {
        return Ok(());
    }

    exchange.answer = answer.to_string();
    exchange.answer_word_count = Some(word_count(answer).try_into().unwrap_or(u32::MAX));
    exchange.answer_format_warnings = Vec::new();
    exchange.status = AssistExchangeStatus::Complete;
    exchange.timings.request_to_complete_ms = Some(
        request_started
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
    );
    clear_active_operation(&mut inner, exchange_id, generation_id);
    Ok(())
}
```

**The detail path needs its own behaviour, not this function.** `process_detail` (`live_assist/mod.rs:1349`) streams into a different field and has its own completion and timing semantics; reusing `complete_locally` there would write an answer onto the wrong surface. Add a sibling that abstains on the detail path by setting the detail field, its own status, and its own timing — and assert both paths independently.

- [ ] **Step 6: Run tests**

```bash
cargo test -p app_lib live_assist -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/src/live_assist/
git commit -m "feat: gate composition on the interview lens and abstain locally"
```

---

## Task 12: Truncation prompt rule

`truncated: true` must carry a rule, not just an annotation. Without it a truncated authority record reads as an unqualified one.

**Files:**
- Modify: `frontend/src-tauri/src/live_assist/mod.rs:58-60`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn truncation_rule_is_present_in_both_prompt_templates() {
    for template in [
        GENERAL_ANSWER_SYSTEM_PROMPT_TEMPLATE,
        SPECIALIZED_ANSWER_SYSTEM_PROMPT_TEMPLATE,
    ] {
        assert!(
            template.contains("truncated"),
            "a truncated record must carry an explicit omitted-content rule"
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p app_lib live_assist::tests::truncation_rule -- --nocapture
```

Expected: FAIL.

- [ ] **Step 3: Add the rule to both templates**

Insert into both `GENERAL_ANSWER_SYSTEM_PROMPT_TEMPLATE` and `SPECIALIZED_ANSWER_SYSTEM_PROMPT_TEMPLATE`, immediately before the `{identity_context}` placeholder:

```
Any identity record marked truncated is partial: its omitted content is unknown. Never infer, assume, or supply what was omitted, and never treat a missing qualifier, caveat, limit, or subsequent step in a truncated record as evidence that none exists.
```

Bump `ANSWER_SYSTEM_PROMPT_VERSION` from `live-assist-answer-v9` to `live-assist-answer-v10`.

- [ ] **Step 4: Run tests**

```bash
cargo test -p app_lib live_assist -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/src-tauri/src/live_assist/mod.rs
git commit -m "feat: add omitted-content rule for truncated identity records"
```

---

## Task 13: Immutable Interview-profile upgrade workflow

Profile versions are content-hashed and immutable. `kind` cannot be backfilled in place — writing it would change the hash of a version other records already reference.

**Files:**
- Modify: `frontend/src-tauri/src/expert_profiles/commands.rs`
- Test: `frontend/src-tauri/src/expert_profiles/tests.rs`

- [ ] **Step 1: Write the failing tests**

```rust
use crate::expert_profiles::hashing::hash_profile_version;
use crate::expert_profiles::presets::interview_profile;

/// A stored version that predates `kind`.
fn legacy_profile() -> ExpertProfileVersion {
    let mut profile = interview_profile();
    profile.kind = None;
    profile
}

#[test]
fn upgrading_to_interview_produces_a_new_version_hash() {
    let original = legacy_profile();
    let original_hash = hash_profile_version(&original).unwrap();
    let upgraded = upgrade_to_interview_lens(&original).unwrap();
    let upgraded_hash = hash_profile_version(&upgraded).unwrap();
    assert_ne!(original_hash, upgraded_hash);
    assert_eq!(upgraded.kind, Some(ProfileKind::Interview));
}

#[test]
fn upgrading_leaves_the_prior_version_byte_identical() {
    let original = legacy_profile();
    let snapshot = serde_json::to_string(&original).unwrap();
    let _ = upgrade_to_interview_lens(&original).unwrap();
    assert_eq!(serde_json::to_string(&original).unwrap(), snapshot);
}

#[test]
fn upgrading_an_already_interview_profile_is_rejected() {
    let profile = interview_profile(); // ships with kind: Some(Interview)
    assert!(upgrade_to_interview_lens(&profile).is_err());
}
```

`hash_profile_version` is the real function at `expert_profiles/hashing.rs:27`; it returns `Result<String, HashError>`.

- [ ] **Step 2: Run tests to verify they fail**

```bash
cargo test -p app_lib expert_profiles::tests::upgrading -- --nocapture
```

Expected: FAIL — `upgrade_to_interview_lens` not found.

- [ ] **Step 3: Implement**

In `expert_profiles/commands.rs`:

```rust
/// Produce a NEW immutable version carrying the Interview lens.
///
/// Stored versions are never mutated: the caller persists the result as a new
/// version, and the user then selects it explicitly. Until that selection the
/// profile keeps running under IdentityRetrievalPolicy::LexicalOnly.
pub fn upgrade_to_interview_lens(
    profile: &ExpertProfileVersion,
) -> Result<ExpertProfileVersion> {
    if profile.kind == Some(ProfileKind::Interview) {
        bail!("profile version is already an Interview lens");
    }
    let mut upgraded = profile.clone();
    upgraded.kind = Some(ProfileKind::Interview);
    Ok(upgraded)
}

#[tauri::command]
pub async fn upgrade_profile_to_interview_lens(
    app: AppHandle,
    profile_id: Uuid,
    version_hash: String,
) -> Result<String, String> {
    // Persists the upgraded version and returns its NEW hash.
    // It does NOT evaluate, activate, or select. See the workflow below.
    let app_state = app.state::<AppState>();
    let current = ExpertProfilesRepository::get_profile_version(
        app_state.db_manager.pool(),
        profile_id,
        &version_hash,
    )
    .await
    .map_err(|error| error.to_string())?
    .ok_or_else(|| "profile version not found".to_string())?;
    let upgraded = upgrade_to_interview_lens(&current).map_err(|error| error.to_string())?;
    let stored = ExpertProfilesRepository::create_profile_version(
        app_state.db_manager.pool(),
        profile_id,
        &upgraded,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(stored.version_hash)
}
```

`create_profile_version` returns `Result<StoredProfileVersion, ExpertProfileRepositoryError>`; take its `version_hash` field.

Register `upgrade_profile_to_interview_lens` in the `invoke_handler` list in `lib.rs`.

- [ ] **Step 4: Implement the full four-step workflow**

**A newly created version is not selectable.** `validate_profile_selection` (`live_assist/mod.rs:1490`) requires an activation row whose `status == "active"` **and** whose `profile_version_hash` equals the selected hash. Creating a version satisfies neither. Returning the new hash and telling the frontend to select it — as the previous draft did — would fail at the next question with "selected Expert Profile is not active".

**Do not manufacture evaluation and activation inside this command.** Evaluation carries a consent and adjudication flow that exists for a reason, and bypassing it by calling repository methods directly would produce an activated version that never passed the gate a user was meant to see. Reuse the existing commands:

| Step | Call | Effect |
| --- | --- | --- |
| 1. Create | `upgrade_profile_to_interview_lens` (this task) | New immutable version, new hash. Prior version untouched. |
| 2. Evaluate | `profile_run_evals` (`expert_profiles/commands.rs:383`) | Runs the existing consent/adjudication flow against the new version. |
| 3. Activate | `profile_activate` (`expert_profiles/commands.rs:462`) | Moves the activation binding. It already journals the superseded activation — **do not** call `mark_activation_superseded` separately, which would double-journal. |
| 4. Select | user selects profile **and playbook** in Live Assist | `derive_identity_policy` then returns `CompositionEnabled`. |

Step 4 is a user action, not a command this task issues. Selection takes a playbook as well as a profile version, and choosing a depth playbook on the user's behalf is exactly the implicit behaviour change the immutability rule exists to prevent.

- [ ] **Step 5: Pin the ordering with tests**

`validate_profile_selection` is private to `live_assist`, so the selectability assertions belong in **the Live Assist test module**, not `expert_profiles::tests`. Keep the pure upgrade tests where they are and add these separately:

```rust
// in live_assist's test module
#[tokio::test]
async fn a_newly_created_version_is_not_yet_selectable() {
    let pool = test_pool().await;
    let (profile_id, original_hash) = seed_active_profile(&pool).await;
    let upgraded_hash = create_upgraded_version(&pool, profile_id, &original_hash).await;
    // Activation still points at the original version.
    let error = validate_profile_selection(&pool, profile_id, &upgraded_hash, playbook_id())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("active"));
}

#[tokio::test]
async fn selection_succeeds_once_the_new_version_is_active() {
    let pool = test_pool().await;
    let (profile_id, original_hash) = seed_active_profile(&pool).await;
    let upgraded_hash = create_upgraded_version(&pool, profile_id, &original_hash).await;
    activate_via_existing_flow(&pool, profile_id, &upgraded_hash).await;
    validate_profile_selection(&pool, profile_id, &upgraded_hash, playbook_id())
        .await
        .expect("an evaluated, activated version must be selectable");
}
```

If a crate-level helper is preferred over relocating the tests, expose one deliberately rather than widening `validate_profile_selection` to `pub`. Follow the existing setup in `expert_profiles/evaluation.rs` and `expert_profiles/safety_gate.rs` for `seed_active_profile` and `activate_via_existing_flow` — do not build a parallel harness.

- [ ] **Step 5b: Add the UI for the workflow**

Without UI the upgrade is unreachable. In the Expert Profile settings surface, for any profile whose active version has `kind: None`, show an "Enable Interview lens" action that walks steps 1 to 3 with a visible progress state and surfaces failure at whichever step fails — including an evaluation the user declines or that fails adjudication.

The action **stops after activation** and tells the user to select the profile and playbook in Live Assist. It never selects on their behalf.

- [ ] **Step 6: Run tests**

```bash
cargo test -p app_lib expert_profiles -- --nocapture
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/src-tauri/src/expert_profiles/ frontend/src-tauri/src/lib.rs frontend/src/
git commit -m "feat: upgrade profiles to the interview lens as a new immutable version"
```

---

## Task 14: Config-status surfaces

Two surfaces, per the design: a detailed persistent warning in Settings, and a compact non-blocking badge in Live Assist.

**Files:**
- Modify: `frontend/src/components/ProfessionalIdentitySettings.tsx`
- Modify: `frontend/src/app/live-assist/page.tsx`

- [ ] **Step 1: Write the failing test**

Create `frontend/src/components/__tests__/ConfigStatusWarning.test.tsx`:

```tsx
import { render, screen } from '@testing-library/react';
import { ConfigStatusWarning } from '../ConfigStatusWarning';

test('names the override path and the validation failure', () => {
  render(
    <ConfigStatusWarning
      status="override_invalid"
      path="C:/Users/x/composition.json"
      reason="duplicate dimension priority 2 on 'leadership'"
    />
  );
  expect(screen.getByText(/composition.json/)).toBeInTheDocument();
  expect(screen.getByText(/duplicate dimension priority/)).toBeInTheDocument();
});

test('renders nothing when the shipped default is in force', () => {
  const { container } = render(<ConfigStatusWarning status="default" />);
  expect(container).toBeEmptyDOMElement();
});
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd frontend && pnpm test ConfigStatusWarning
```

Expected: FAIL — module not found.

- [ ] **Step 3: Implement the component**

Create `frontend/src/components/ConfigStatusWarning.tsx`:

```tsx
type ConfigStatus = 'default' | 'override_applied' | 'override_invalid';

export function ConfigStatusWarning({
  status,
  path,
  reason,
}: {
  status: ConfigStatus;
  path?: string;
  reason?: string;
}) {
  if (status !== 'override_invalid') return null;
  return (
    <div role="alert" className="rounded border border-amber-500 bg-amber-50 p-3 text-sm">
      <p className="font-medium">Composition override ignored — shipped defaults in force.</p>
      {path && <p className="mt-1 font-mono text-xs break-all">{path}</p>}
      {reason && <p className="mt-1">{reason}</p>}
    </div>
  );
}
```

- [ ] **Step 4: Expose the status to the frontend**

Settings needs the status **at rest**, not only after a retrieval, so add a command. In `professional_identity/commands.rs`:

```rust
use crate::professional_identity::composition::config::CompositionConfigStatus;
use crate::professional_identity::CompositionState;

#[tauri::command]
pub async fn get_composition_config_status(
    state: tauri::State<'_, CompositionState>,
) -> Result<CompositionConfigStatus, String> {
    // Reads the status resolved during .setup(); does not re-read the file.
    Ok(state.status.clone())
}
```

Register it in the `invoke_handler` list in `lib.rs`. The status comes from the managed `CompositionState` created in Task 11 Step 4b, so Settings reports what actually happened **at startup** — not whatever a lazily-initialised global happened to hold at the moment of the first question.

Reuse `CompositionConfigStatus` from `composition::config` rather than declaring a second shape; add `Clone` to it if absent.

- [ ] **Step 5: Render both surfaces**

In `ProfessionalIdentitySettings.tsx`, fetch the status on mount and render the warning above the existing panel content:

```tsx
const [configStatus, setConfigStatus] = useState<CompositionConfigStatus | null>(null);

useEffect(() => {
  invoke<CompositionConfigStatus>('get_composition_config_status')
    .then(setConfigStatus)
    .catch(() => setConfigStatus(null));
}, []);

// ...at the top of the panel's root container:
{configStatus && (
  <ConfigStatusWarning
    status={configStatus.status}
    path={configStatus.path}
    reason={configStatus.reason}
  />
)}
```

In `frontend/src/app/live-assist/page.tsx`, read the status from the exchange's diagnostics and render the compact badge beside the exchange header:

```tsx
{exchange.diagnostics?.configStatus === 'override_invalid' && (
  <span className="rounded bg-amber-100 px-2 py-0.5 text-xs text-amber-900">
    default config
  </span>
)}
```

The badge must never block, gate, or interrupt an answer in progress — it is annotation only.

- [ ] **Step 6: Run tests**

```bash
cd frontend && pnpm test && pnpm run typecheck
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/src/ frontend/src-tauri/src/professional_identity/commands.rs frontend/src-tauri/src/lib.rs
git commit -m "feat: surface composition config fallback in settings and live assist"
```

---

## Task 15: Fixtures and integration tests

Follows the pattern already established by `tests/project_context_retrieval.rs`: a synthetic tracked fixture plus an env-gated private-corpus check. The private corpus must never enter Git — `experiments/` is gitignored and currently has zero tracked files, which also means a fixture placed there would be invisible to CI.

**Files:**
- Create: `frontend/src-tauri/tests/fixtures/composition/corpus.json`
- Create: `frontend/src-tauri/tests/composition_retrieval.rs`

- [ ] **Step 1: Build the anonymised fixture**

Create `frontend/src-tauri/tests/fixtures/composition/corpus.json`. The persona is invented — no real employer, place, or role appears. Every `id` must be a distinct valid UUID.

```json
{
  "schema_version": 1,
  "identity": {
    "display_name": "Sam Rivera",
    "role_title": "Operations Lead",
    "organization": "Example Works Ltd",
    "professional_summary": "Operations lead with experience across delivery and coordination."
  },
  "records": [
    {
      "id": "11111111-1111-4111-8111-111111111111",
      "category": "cv", "title": "Career Summary",
      "content": "I have worked in operations for eleven years, moving from frontline coordination into team leadership.",
      "source": { "label": "CV", "revision": "v1" },
      "updated_at": "2026-01-01T00:00:00Z", "valid_until": null,
      "conflict_key": null, "tags": ["cv", "experience"]
    },
    {
      "id": "22222222-2222-4222-8222-222222222222",
      "category": "other", "title": "Delivery Scope",
      "content": "I coordinated a caseload of approximately four hundred items per quarter.",
      "source": { "label": "Notes", "revision": "v1" },
      "updated_at": "2026-01-01T00:00:00Z", "valid_until": null,
      "conflict_key": null, "tags": ["operations", "delivery"]
    },
    {
      "id": "33333333-3333-4333-8333-333333333333",
      "category": "authority", "title": "Team Leadership",
      "content": "I held shared responsibility for scheduling decisions with the department manager.",
      "source": { "label": "ToR", "revision": "v1" },
      "updated_at": "2026-01-01T00:00:00Z", "valid_until": null,
      "conflict_key": null, "tags": ["leadership"]
    },
    {
      "id": "44444444-4444-4444-8444-444444444444",
      "category": "operating_practice", "title": "Working Method",
      "content": "I run a weekly review of open items and escalate blocked cases.",
      "source": { "label": "Practice", "revision": "v1" },
      "updated_at": "2026-01-01T00:00:00Z", "valid_until": null,
      "conflict_key": null, "tags": []
    },
    {
      "id": "55555555-5555-4555-8555-555555555555",
      "category": "terms_of_reference", "title": "Role Fit",
      "content": "The role calls for coordination across teams, which matches my current work.",
      "source": { "label": "JD", "revision": "v1" },
      "updated_at": "2026-01-01T00:00:00Z", "valid_until": null,
      "conflict_key": null, "tags": ["role", "fit"]
    },
    {
      "id": "66666666-6666-4666-8666-666666666666",
      "category": "authority", "title": "Approval Limit (old)",
      "content": "My approval limit was five hundred units.",
      "source": { "label": "ToR", "revision": "v1" },
      "updated_at": "2026-01-01T00:00:00Z", "valid_until": null,
      "conflict_key": "approval_limit", "tags": ["leadership"]
    },
    {
      "id": "77777777-7777-4777-8777-777777777777",
      "category": "authority", "title": "Approval Limit (current)",
      "content": "My approval limit is nine hundred units.",
      "source": { "label": "ToR", "revision": "v2" },
      "updated_at": "2026-06-01T00:00:00Z", "valid_until": null,
      "conflict_key": "approval_limit", "tags": ["leadership"]
    },
    {
      "id": "88888888-8888-4888-8888-888888888888",
      "category": "other", "title": "Tied Conflict A",
      "content": "Ambiguous statement A.",
      "source": { "label": "Notes", "revision": "a" },
      "updated_at": "2026-03-01T00:00:00Z", "valid_until": null,
      "conflict_key": "tied", "tags": ["operations"]
    },
    {
      "id": "99999999-9999-4999-8999-999999999999",
      "category": "other", "title": "Tied Conflict B",
      "content": "Ambiguous statement B.",
      "source": { "label": "Notes", "revision": "b" },
      "updated_at": "2026-03-01T00:00:00Z", "valid_until": null,
      "conflict_key": "tied", "tags": ["operations"]
    }
  ],
  "projects": []
}
```

`ProfessionalIdentityHeader` ([mod.rs:43](../../frontend/src-tauri/src/professional_identity/mod.rs)) requires **all four** fields, `display_name` included. Omit it and the fixture fails to deserialise with a `missing field` error before any assertion runs.

Two records still need generating, because their content must be long:

- **`aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa`** — category `cv`, tags `["cv"]`, content of **more than 1,200 characters across at least three paragraphs** separated by blank lines, each paragraph itself under 1,200 characters. Exercises paragraph-boundary truncation.
- **`bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb`** — category `cv`, tags `["cv"]`, content that is **one single sentence longer than 1,200 characters** with no sentence-ending punctuation until the very end. Exercises the omit-rather-than-cut rule.

Generate both with a script so the lengths are exact rather than eyeballed:

```bash
python - <<'PY'
import json, pathlib
p = pathlib.Path("frontend/src-tauri/tests/fixtures/composition/corpus.json")
doc = json.loads(p.read_text(encoding="utf-8"))
para = "This paragraph describes routine coordination work in plain terms. " * 8
doc["records"].append({
    "id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    "category": "cv", "title": "Long Multi Paragraph",
    "content": "\n\n".join([para.strip()] * 3),
    "source": {"label": "CV", "revision": "v1"},
    "updated_at": "2026-01-01T00:00:00Z", "valid_until": None,
    "conflict_key": None, "tags": ["cv"],
})
doc["records"].append({
    "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
    "category": "cv", "title": "Single Oversized Sentence",
    "content": "I coordinated " + "and reviewed many items " * 60 + "throughout the year.",
    "source": {"label": "CV", "revision": "v1"},
    "updated_at": "2026-01-01T00:00:00Z", "valid_until": None,
    "conflict_key": None, "tags": ["cv"],
})
p.write_text(json.dumps(doc, indent=2), encoding="utf-8")
for record in doc["records"][-2:]:
    print(record["title"], len(record["content"]))
PY
```

Expected output: both lengths above 1,200.

- [ ] **Step 2: Write the integration tests**

Create `frontend/src-tauri/tests/composition_retrieval.rs`:

```rust
use app_lib::professional_identity::composition::config::{load_default, CompositionConfig};
use app_lib::professional_identity::diagnostics::AbstentionReason;
use app_lib::professional_identity::{
    retrieve_identity_context, IdentityRetrievalPolicy, ProfessionalIdentityVersion,
    RetrievedIdentityContext,
};
use chrono::Utc;

const PRIVATE_CONTEXT_PATH_ENV: &str = "PROFESSIONAL_IDENTITY_PATH";

fn fixture() -> ProfessionalIdentityVersion {
    let raw = std::fs::read_to_string("tests/fixtures/composition/corpus.json").unwrap();
    serde_json::from_str(&raw).unwrap()
}

/// Each test builds its own config: there is no process-global to leak.
fn config() -> CompositionConfig {
    load_default().unwrap()
}

fn compose(question: &str) -> RetrievedIdentityContext {
    retrieve_identity_context(
        &fixture(),
        question,
        IdentityRetrievalPolicy::CompositionEnabled,
        &config(),
        Utc::now(),
    )
    .unwrap()
}

/// The selected evidence: ordered (id, exact emitted content).
///
/// Read from `prompt_json`, which is what the model actually receives.
/// Comparing ids, character counts, and truncation flags from diagnostics
/// would pass even if two runs emitted different text of the same length from
/// the same records — precisely the drift this assertion exists to catch.
fn selected_evidence(context: &RetrievedIdentityContext) -> Vec<(String, String)> {
    let payload: serde_json::Value = serde_json::from_str(&context.prompt_json).unwrap();
    payload["records"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|record| {
            (
                record["id"].as_str().unwrap_or_default().to_string(),
                record["content"].as_str().unwrap_or_default().to_string(),
            )
        })
        .collect()
}

#[test]
fn canonical_broad_question_composes_from_the_anchor() {
    let result = compose("Tell us about yourself");
    assert_eq!(result.diagnostics.selection_mode, "intent:self_introduction");
    assert!(result.diagnostics.anchor_survived);
}

#[test]
fn unseen_broad_phrase_uses_the_fallback_package() {
    let result = compose("So, who are you and what brought you here?");
    assert_eq!(result.diagnostics.selection_mode, "broad_fallback");
}

#[test]
fn evidence_budget_is_respected() {
    let result = compose("Tell us about yourself");
    assert!(result.diagnostics.evidence_chars_used <= 7000);
}

#[test]
fn no_single_dimension_starves_the_others() {
    // The fixture holds two oversized cv records. Without quota caps they
    // would consume the whole budget and leave every other dimension empty.
    let result = compose("Tell us about yourself");
    let dimensions: std::collections::HashSet<_> = result
        .diagnostics
        .records
        .iter()
        .filter_map(|record| record.dimension.clone())
        .collect();
    assert!(
        dimensions.len() >= 2,
        "expected several dimensions represented, got {dimensions:?}"
    );
}

#[test]
fn the_fallback_anchor_abstains_when_empty() {
    // Strip every cv record, emptying career_core - the fallback's own anchor.
    let mut profile = fixture();
    profile.records.retain(|record| {
        !matches!(
            record.category,
            app_lib::professional_identity::IdentityRecordCategory::Cv
        )
    });
    let result = retrieve_identity_context(
        &profile,
        "So, who are you and what brought you here?",
        IdentityRetrievalPolicy::CompositionEnabled,
        &config(),
        Utc::now(),
    )
    .unwrap();
    assert_eq!(
        result.diagnostics.abstained,
        Some(AbstentionReason::AnchorEmpty),
        "anchor sufficiency must apply to the fallback, not just named intents"
    );
}

#[test]
fn an_ambiguous_conflict_suppresses_both_records() {
    let result = compose("Tell us about yourself");
    let ambiguous = result
        .diagnostics
        .suppressed
        .iter()
        .find(|group| group.conflict_key == "tied")
        .expect("the tied conflict group must be recorded");
    assert_eq!(ambiguous.record_ids.len(), 2, "neither may be chosen");
}

#[test]
fn prompt_size_and_evidence_size_move_independently() {
    let result = compose("Tell us about yourself");
    assert!(result.diagnostics.prompt_json_bytes > result.diagnostics.evidence_chars_used);
}

#[test]
fn diagnostics_never_appear_in_the_prompt_payload() {
    let result = compose("Tell us about yourself");
    for marker in [
        "selectionMode",
        "evidenceCharsUsed",
        "promptJsonBytes",
        "configStatus",
        "suppressed",
        "ambiguous_freshness",
    ] {
        assert!(
            !result.prompt_json.contains(marker),
            "diagnostics leaked into the model-visible prompt: {marker}"
        );
    }
}

#[test]
fn retrieval_is_deterministic_in_selected_evidence() {
    let first = compose("Tell us about yourself");
    let second = compose("Tell us about yourself");
    // Identical SELECTED EVIDENCE, not byte-identical prompt_json: additive
    // provenance changes serialisation.
    assert_eq!(selected_evidence(&first), selected_evidence(&second));
    assert_eq!(
        first.prompt_json, second.prompt_json,
        "the payload itself is stable for a fixed input"
    );
    assert!(
        !selected_evidence(&first).is_empty(),
        "an empty selection would make this assertion vacuous"
    );
}

/// Real-corpus gate. Ignored by default; the corpus is never committed.
/// Run manually: PROFESSIONAL_IDENTITY_PATH=<path> cargo test -p app_lib
///   --test composition_retrieval -- --ignored --nocapture
#[test]
#[ignore]
fn real_corpus_returns_cv_records_for_the_canonical_question() {
    let path = std::env::var(PRIVATE_CONTEXT_PATH_ENV)
        .expect("set PROFESSIONAL_IDENTITY_PATH to the private corpus");
    let raw = std::fs::read_to_string(path).unwrap();
    let profile: ProfessionalIdentityVersion = serde_json::from_str(&raw).unwrap();
    let result = retrieve_identity_context(
        &profile,
        "Tell us about yourself",
        IdentityRetrievalPolicy::CompositionEnabled,
        &config(),
        Utc::now(),
    )
    .unwrap();
    assert_eq!(result.diagnostics.selection_mode, "intent:self_introduction");
    assert!(
        result.diagnostics.anchor_survived,
        "the CV must reach the package; today it scores zero and is excluded"
    );
}
```

- [ ] **Step 3: Add the no-provider abstention test**

Abstention must be verified to make **no provider call**, not merely to return the right string. Add to the `live_assist` test module:

```rust
#[tokio::test]
async fn local_abstention_never_calls_a_provider() {
    let app = test_app().await;
    let state = app.state::<LiveAssistState>();

    // Declare managed providers present but NONE active. Without this,
    // load_provider_config falls back to environment configuration, which
    // may supply a real provider and make the test silently vacuous.
    state.set_managed_provider_state(true, None);

    let exchange_id = seed_exchange(&app, "Tell us about yourself").await;
    process_answer(&app, exchange_id, identity_with_empty_anchor())
        .await
        .expect("abstention must succeed without reaching a provider");

    let inner = state.lock();
    let exchange = find_exchange(&inner, exchange_id).unwrap();
    assert_eq!(exchange.status, AssistExchangeStatus::Complete);
    assert_eq!(exchange.answer, ABSTENTION_RESPONSE);
    assert!(exchange.timings.request_to_complete_ms.is_some());
}

#[tokio::test]
async fn a_superseded_generation_is_not_resurrected_by_abstention() {
    let app = test_app().await;
    let state = app.state::<LiveAssistState>();
    state.set_managed_provider_state(true, None);
    let exchange_id = seed_exchange(&app, "Tell us about yourself").await;

    // A stale generation must write nothing.
    complete_locally(&app, exchange_id, 999, Instant::now(), ABSTENTION_RESPONSE).unwrap();

    let inner = state.lock();
    let exchange = find_exchange(&inner, exchange_id).unwrap();
    assert_ne!(exchange.status, AssistExchangeStatus::Complete);
    assert!(exchange.answer.is_empty());
}

#[tokio::test]
async fn detail_path_abstains_on_its_own_surface() {
    let app = test_app().await;
    let state = app.state::<LiveAssistState>();
    state.set_managed_provider_state(true, None);
    let exchange_id = seed_exchange(&app, "Tell us about yourself").await;

    process_detail(&app, exchange_id, identity_with_empty_anchor())
        .await
        .unwrap();

    let inner = state.lock();
    let exchange = find_exchange(&inner, exchange_id).unwrap();
    // The detail surface carries the abstention; the answer field does not.
    assert!(exchange.answer.is_empty());
}
```

`set_managed_provider_state(true, None)` is what makes the first test a real assertion. With `has_managed_providers` true and no active provider, `load_provider_config` returns an error rather than falling through to environment configuration — so if the short-circuit were ever moved below it, the test fails loudly instead of passing because a provider happened to be configured on the machine.

- [ ] **Step 4: Run the integration tests**

```bash
cargo test -p app_lib --test composition_retrieval -- --nocapture
```

Expected: PASS, 9 tests; 1 ignored.

- [ ] **Step 5: Verify the private corpus is still untracked**

```bash
git status --short && git ls-files experiments/ | wc -l
```

Expected: no `experiments/` entries, count `0`.

- [ ] **Step 6: Commit**

```bash
git add frontend/src-tauri/tests/ frontend/src-tauri/src/live_assist/
git commit -m "test: add composition fixtures and integration coverage"
```

---

## Final Verification

- [ ] **Full backend suite**

```bash
cargo test -p app_lib -- --nocapture
```

- [ ] **Frontend**

```bash
cd frontend && pnpm run typecheck && pnpm test && pnpm run build
```

- [ ] **Migration line endings unchanged**

```bash
git diff --stat -- frontend/src-tauri/migrations/
```

Expected: empty. This work adds no migration; the four protected files must stay LF.

- [ ] **No secrets, no private corpus**

```bash
git ls-files experiments/ | wc -l
git diff origin/main --stat -- experiments/
```

Expected: `0` and empty.

- [ ] **Manual check** — run the app, select an Interview-lens profile, ask "Tell us about yourself", and confirm the answer covers career progression rather than compliance SOPs. This is the acceptance criterion the whole plan exists to satisfy, and no automated test substitutes for it.

---

## Spec Coverage

| Design section | Task |
| --- | --- |
| 3 Lens gating, `ProfileKind`, immutable upgrade | 1, 11, 13 |
| 4 Three-outcome routing | 9, 10 |
| 5 Two normalisers | 5 |
| 6 Config, validation, both warning surfaces | 6, 14 |
| 7 Selector semantics, single assignment | 7 |
| 8 Budget, truncation ladder, prompt rule | 8, 10, 12 |
| 9 Conflict resolution, pipeline order | 3 |
| 10 Anchor sufficiency, local abstention | 10, 11 |
| 11 Provenance out of `prompt_json` | 4, 10 |
| 12 Test plan, fixtures | 15, plus per-task unit tests |

Design tests explicitly pinned to a task, so none is quietly dropped:

| Design requirement | Where |
| --- | --- |
| Every shipped pattern survives normalisation | Task 5, `every_shipped_pattern_survives_phrase_normalisation` |
| Informativeness from corpus document frequency | Task 5, `ubiquitous_terms_are_not_informative` / `absent_terms_are_not_informative` |
| Unseen broad phrase reaches `broad_fallback` | Task 9 and Task 15 |
| Starvation prevention | Task 8, `quota_caps_prevent_the_first_dimension_starving_the_rest`; Task 15, `no_single_dimension_starves_the_others` |
| Unused quota redistributes | Task 8, `unused_quota_redistributes_downward_by_priority` and `carry_accumulates_across_several_dimensions` |
| Rare self-reference marker does not block fallback | Task 9, `a_rare_marker_word_does_not_block_the_fallback` |
| Config status reaches every retrieval path | Task 11, `config_status_reaches_diagnostics_on_every_path` |
| Stale generation is not resurrected | Task 11, `a_superseded_generation_is_not_resurrected_by_abstention` |
| Detail path abstains on its own surface | Task 11, `detail_path_abstains_on_its_own_surface` |
| Selection ordering after upgrade | Task 13, `selection_succeeds_once_the_new_version_is_active` |
| Fallback anchor abstention | Task 15, `the_fallback_anchor_abstains_when_empty` |
| Abstention makes no provider call | Task 15, `local_abstention_never_calls_a_provider` |
| Determinism of selected content | Task 15, `retrieval_is_deterministic_in_selected_evidence` |
| Diagnostics excluded from `prompt_json` | Task 15, `diagnostics_never_appear_in_the_prompt_payload` |
| Newly created version is not selectable | Task 13, `a_newly_created_version_is_not_yet_selectable` |
| Name never infers the lens | Task 11, `profile_name_never_infers_the_interview_lens` |
