# Broad-Question Retrieval and Composition Design

Status: approved for documentation; implementation pending review
Date: 2026-08-21
Scope: `professional_identity` retrieval, `live_assist` policy derivation
Complements: `PROJECT_CONTEXT_DESIGN.md`

## 1. Problem

Live Assist answered "Tell us about yourself" with a narrow ~47-word reply covering a
single five-year role, identically across two unrelated providers. The cause is
retrieval, not provider choice.

`tokenize()` drops words shorter than three characters and a 21-word stopword list.
`tell`, `about`, and `yourself` are all absent from that list, so the query becomes
literally `{tell, about, yourself}`. `lexical_score()` is an unweighted set
intersection, and candidates scoring zero are discarded.

Measured against the working corpus (275 records, 267,758 characters):

| Source | Occurrences of tell/about/yourself |
| --- | --- |
| `projects/application-interview/primary-questions-and-answers.md` | 9 |
| `projects/professional-evidence/compliance-management-tool.md` | 8 |
| `projects/professional-evidence/joint-mhd-rmm-directive-133.md` | 6 |
| `identity/professional-experience-stories.md` | 1 |
| `identity/cv-current.md` | **0** |

Retrieval therefore selects records containing the filler word "about", not
biography. The CV scores zero on every section and is excluded outright, while
compliance SOPs rank above it. Because scores are small integers, most survivors tie
at 1 and the sort tie-breaks on `left.id.cmp(&right.id)` — arbitrary UUID order.

What the models actually received was the always-included `ProfessionalIdentityHeader`
plus up to eight near-random records. Both providers converged because both were given
the same header-shaped evidence. The header was the answer.

The defect is not thin retrieval. It is actively misleading retrieval.

## 2. Scope and non-goals

In scope: intent detection, composed evidence packages, character budgeting,
conflict resolution, abstention, provenance, tests.

Not in scope, tracked separately:

- **Importer monster records.** `markdown_import.rs` splits on headings, so the four
  corpus files with no headings collapse into one record each; the largest is 45,244
  characters. The budget makes this survivable, but the real fix is size-based
  splitting.
- **Inline-Markdown validator gap.** Both providers wrapped answers in Markdown bold
  against a plain-text contract. That lives in answer validation, not retrieval.

## 3. Composition is gated by the active lens

Composition applies only under an Interview lens. Retrieval must not import lens
types, so the gate is resolved upstream and passed down as a plain value.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalPolicy {
    /// Lexical retrieval only. Current behaviour, unchanged.
    LexicalOnly,
    /// Named intents and broad-question composition permitted.
    CompositionEnabled,
}

pub fn retrieve_identity_context(
    profile: &ProfessionalIdentityVersion,
    question: &str,
    policy: RetrievalPolicy,
    now: DateTime<Utc>,
) -> Result<RetrievedIdentityContext>
```

`live_assist` derives the policy from the selected playbook and passes it in. The
`professional_identity` module never sees `ExpertProfileVersion`, a playbook, or a
lens. The layers stay separate.

`MeetingPlaybook` gains one optional discriminator so the gate is typed rather than
name-matched:

```rust
#[serde(default)]
pub kind: Option<PlaybookKind>,   // Interview | Other
```

This is additive and backward compatible under `deny_unknown_fields`: absent in
existing stored profiles, it deserialises to `None`, and `None` maps to `LexicalOnly`.
**This is the one schema touch in the design and needs explicit approval.**

## 4. Three-outcome routing

Degenerate lexical retrieval does not by itself prove a broad question. Two
independent signals are computed, and routing uses both.

**Lexical signal.** `Low` when either holds, otherwise `Strong`:

- `max_score <= 1` — no record matched more than a single query term, so every
  survivor is a coin-flip; or
- the number of records tied at `max_score` exceeds `max_arbitrary_ties` (config,
  default 8) — the set is larger than any package could carry, so which records
  survive would be decided by the `id` tie-break rather than by relevance.

The second condition is expressed as a record count, not a share of the character
budget, so it stays independent of record sizes and of budget retuning.

**Broad-question evidence.** Present when, after intent-local normalisation, the
question is self-referential (it targets the person or career as a whole:
`yourself`, `your background`, `your career`, `your experience`, `about you`) **and**
contains no informative domain term, where a term is informative if its corpus
document frequency falls below the configured threshold.

| Condition | Outcome |
| --- | --- |
| Named broad intent matches | Composed package for that intent |
| No intent, signal Low, broad evidence present | General background package |
| No intent, signal Low, broad evidence absent | Retain lexical result if non-empty; abstain if empty |
| No intent, signal Strong | Lexical path, unchanged |
| Policy is `LexicalOnly` | Lexical path, unchanged, regardless of the above |

`"What was your authority over budget approvals?"` carries `authority`, `budget`, and
`approvals` as informative terms, so it is never routed to composition even when its
lexical score is weak.

## 5. Intent-local normalisation

Filler terms are **not** added to the global `STOP_WORDS`. Changing that list would
alter scoring for every question and risk regressions far outside this feature.

Instead `normalize_for_intent()` is private to intent detection: lowercase, strip
punctuation, and drop a filler set (`tell`, `us`, `me`, `about`, `yourself`, `walk`,
`through`, `describe`, `bit`, `little`, `just`, `quick`, `give`). It feeds pattern
matching and broad-evidence detection only. `tokenize()`, `lexical_score()`, and
`STOP_WORDS` are untouched, so the lexical path is unaffected.

## 6. Configuration

Two layers, resolved once at load.

**Shipped default.** `composition.default.json`, embedded via `include_str!`, carrying
an integer `config_version`. Dimensions are deliberately **career-neutral**:
`career_core`, `scope_and_scale`, `leadership`, `domain_practice`, `role_fit`. No
humanitarian, sector, or persona-specific dimension ships in the product.

**Local override.** Optional file in the app data directory, loaded and validated once
at startup, not per question. On any validation failure the loader falls back to the
embedded default and the failure is **visible**: a warning surfaced in the UI, a
logged reason, and `config_status: "override_invalid"` recorded in diagnostics. A bad
override degrades to shipped behaviour; it never silently half-applies and never
fails a live answer.

Sector-specific dimensions such as `emergency_regional` belong in the local override.

```json
{
  "config_version": 1,
  "intents": [{
    "name": "self_introduction",
    "patterns": ["tell me about yourself", "tell us about yourself",
                 "walk me through your background", "introduce yourself"],
    "anchor": "career_core",
    "dimensions": ["career_core", "scope_and_scale", "leadership",
                   "domain_practice", "role_fit"]
  }],
  "dimensions": [{
    "name": "career_core", "priority": 1, "quota_chars": 2000,
    "match_category": ["cv"], "match_any_tag": ["cv", "experience"],
    "match_title": ["^Experience", "Summary"]
  }]
}
```

## 7. Selector semantics and deterministic assignment

Selectors draw only on signals that exist today — `category`, document-level `tags`,
and the heading breadcrumb stored in `title` — so no identity migration is required.

Note that `tags` are document-level: every record parsed from one file inherits that
file's tags. Tags separate documents into facets; they cannot discriminate within a
document. `match_title` covers intra-document selection.

**Matching.** Within one selector kind, entries are OR-ed. Across different selector
kinds that are present, results are AND-ed. Absent kinds are ignored rather than
treated as a failed match. A dimension declaring no selectors matches nothing and is
**rejected at validation**, so a catch-all cannot be created by omission.

**Assignment and deduplication.** A record belongs to exactly one dimension. Among all
dimensions it matches, it is assigned to the one with the lowest `priority`.
Validation rejects duplicate priorities, so this is always unambiguous. Assignment is
computed once, before budget allocation, and an assigned record is never reconsidered
for another dimension. A record can therefore never appear twice in one package.

## 8. Budget and record limits

`MAX_RETRIEVED_SOURCES = 8` is retired as the limiter. Eight records could mean 3k or
100k characters against this corpus — a 30x swing in prompt size and latency on a live
path.

- Total budget: 7,000 characters.
- Per-record cap: 1,200 characters.
- Each dimension draws up to its quota in priority order; unused quota redistributes
  downward by priority.
- Ordering within a dimension: document order, then `id`.

**Truncation ladder.** Qualifiers are load-bearing here.
`SPECIALIZED_PERSONAL_FACT_POLICY` depends on "approximately", "shared
responsibility", and exact quantities surviving intact. A mid-sentence cut could strip
the very qualifier the anti-fabrication rule relies on. So:

1. Whole record if it fits.
2. Otherwise the largest prefix of complete **paragraphs** that fits.
3. Otherwise the largest prefix of complete **sentences** that fits.
4. If not even one complete sentence fits, **omit the record entirely.** A sentence is
   never cut mid-way.

Partial records carry `truncated: true` so the model knows it is seeing part of a
record and does not read a missing qualifier as an absent fact.

## 9. Conflict resolution

This replaces today's behaviour, which returns `Err` and aborts retrieval whenever any
`conflict_key` appears on more than one live record. That converts a data-curation
problem into a mid-meeting outage, and composition — which deliberately widens the
candidate set — would make broad questions the most exposed.

Records are grouped by `conflict_key`. For each group:

- Keep one record **only** if its `updated_at` is strictly newer than every other
  record in the group. Suppressed ids and revisions are recorded as `superseded`.
- If freshness ties, is missing, or is unparseable, **suppress every record in the
  group**, reason `ambiguous_freshness`.

No arbitrary tie-breaker is used anywhere. The system never chooses between equally
current conflicting sources.

**This policy applies to both retrieval paths, not only to composition.** The lexical
path today aborts on conflicting current sources; under this design it resolves or
suppresses them by the same rules. That is a deliberate behaviour change on the
existing path: it removes a total-failure mode and is the one respect in which
lexical retrieval does not remain bit-for-bit identical to today.

## 10. Sufficiency and local abstention

Each intent declares one anchor dimension it cannot answer without. For
`self_introduction` the anchor is `career_core`.

After conflict suppression:

- Anchor has surviving records: answer.
- Anchor empty: **abstain**.

Abstention short-circuits locally. `live_assist` returns the exact abstention string
without calling the provider — no token spend, no latency, and no chance of a model
paraphrasing the contract. The literal is a single shared constant referenced by both
the prompt template and the short-circuit, so the two cannot drift.

Thin *secondary* dimensions do not trigger abstention. The package is emitted from
what survived, and the existing sparse-evidence rule shortens the answer.

## 11. Provenance

Diagnostics are kept out of the model's view. `prompt_json` carries only what the
model needs in order to answer:

- identity header
- per record: `id`, `category`, `title`, `content`, `source_label`,
  `source_revision`, `updated_at`
- `truncated: true`, emitted only when true

Everything else moves to a new `diagnostics` field on `RetrievedIdentityContext`, and
is never serialised into the prompt:

- `selection_mode`: `lexical` | `intent:<name>` | `broad_fallback`
- per record: assigned `dimension`, `original_chars`
- `suppressed[]`: `conflict_key`, ids, revisions, reason
- `omitted[]`: records dropped by the truncation ladder
- `budget_used` / `budget_total`
- `config_status`: `default` | `override_applied` | `override_invalid`
- `abstained`, with reason

## 12. Test plan

1. `"Tell us about yourself"` against the real corpus resolves to
   `intent:self_introduction`, `career_core` is non-empty, and CV records are present.
   This is the regression test for the reported defect; today it returns compliance
   SOPs.
2. A filler-only question yields broad evidence and never UUID-ordered selection.
3. `"What was your authority over budget approvals?"` keeps `selection_mode: lexical`
   and returns the same selected evidence as today, for a profile carrying no
   contested `conflict_key`. Where a conflict does exist, section 9 applies and the
   result is expected to differ from today, which aborts.
4. Low lexical signal without broad evidence retains lexical behaviour, or abstains
   when the lexical result is empty. It never composes.
5. `RetrievalPolicy::LexicalOnly` never composes, whatever the question.
6. A 45,244-character record respects the total budget, is marked truncated, and does
   not starve other dimensions.
7. Truncation lands on a paragraph boundary; a record whose first sentence exceeds the
   cap is omitted rather than cut.
8. A record matching several dimensions is assigned once, to the lowest priority, and
   appears exactly once.
9. Duplicate dimension priorities, and dimensions with no selectors, fail validation
   at load.
10. A strictly newer record under a shared `conflict_key` wins, and the older is
    recorded as `superseded`.
11. Equal `updated_at` under a shared `conflict_key` suppresses both, reason
    `ambiguous_freshness`.
12. Conflict suppression that empties the anchor abstains locally, with no provider
    call.
13. Conflict suppression that empties only a secondary dimension still answers.
14. An invalid local override falls back to the shipped default and reports
    `config_status: "override_invalid"`.
15. Determinism: the same profile, question, policy, and `now` produce **identical
    selected lexical evidence** — the same ordered record ids and the same emitted
    content. Byte-identical `prompt_json` is not asserted, because additive provenance
    changes serialisation.

**Fixtures.** The private corpus must never enter Git. `experiments/` is already
gitignored and currently has zero tracked files. Tests therefore use:

- a tracked, anonymised, career-neutral synthetic fixture exercising every dimension,
  conflict case, and oversize record, living under the Rust test tree — not under
  `experiments/`, which is ignored and would be invisible to CI; and
- an `#[ignore]` gate reading the real corpus from the ignored path, run manually as a
  pre-release check.

## 13. Open items

- Approval for the additive `MeetingPlaybook::kind` field (section 3).
- Confirm the 7,000 and 1,200 character budgets against measured live latency.
- Decide whether `config_status` warnings surface in Settings, the Live Assist panel,
  or both.
