# Broad-Question Retrieval and Composition Design

Status: superseded as the immediate implementation design; retained as an expansion reference
Date: 2026-08-21
Scope: `professional_identity` retrieval, `live_assist` policy derivation
Complements: `PROJECT_CONTEXT_DESIGN.md`

> Implementation note (2026-08-21): after reviewing Mishkat's production compose/edit
> services, the first shipped fix uses the smaller service boundary already proven there:
> an explicit versioned compose brief before generation and deterministic validation after
> generation. `professional-introduction/v1` handles canonical self-introduction/background
> questions with CV-first ordering and fixed evidence budgets. The configurable dimensions,
> schema discriminator, runtime override, diagnostics UI, and migration described below were
> not implemented. They remain possible extensions only if real-use evidence requires them.

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

The discriminator belongs on the **profile**, not the playbook. Interview is the lens;
Junior, Mid-level, and Expert are depth playbooks *within* a lens. Putting the gate on
`MeetingPlaybook` would force every depth variant to re-declare it and would let one
profile hold playbooks that disagree about whether they are an interview.

`ExpertProfileVersion` therefore gains one optional discriminator:

```rust
#[serde(default)]
pub kind: Option<ProfileKind>,   // Interview | Other
```

This is additive and backward compatible under `deny_unknown_fields`: absent in
existing stored versions it deserialises to `None`, and `None` maps to `LexicalOnly`.

**Upgrading existing Interview profiles.** Profile versions are immutable and
content-hashed, so `kind` cannot be backfilled in place — writing it would change the
version hash of a version that other records already reference.

- Stored versions are **never mutated**.
- The lens is **never inferred from a profile name.** A profile called "Interview
  Coach" does not become `ProfileKind::Interview` by virtue of its name; silent
  inference would turn a rename into a behaviour change.
- An existing Interview profile is upgraded by creating a **new immutable version**
  carrying `kind: Some(Interview)`, with a new hash, which the user then explicitly
  selects. Until that selection happens the profile keeps running under
  `LexicalOnly` — the current behaviour, and a safe default.

This is the only schema touch in the design.

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

**Broad-question evidence.** Computed from `informative_tokens()` (section 5), never
from `normalize_phrase()`. Present when the question is self-referential — it targets
the person or career as a whole (`yourself`, `your background`, `your career`,
`your experience`, `about you`) — **and** retains no informative domain term, where a
term is informative if its corpus document frequency falls below the configured
threshold.

| Condition | Outcome |
| --- | --- |
| Named broad intent matches (via `normalize_phrase()`) | Composed package for that intent, using that intent's anchor |
| No intent, signal Low, broad evidence present | Composed package from the declared `fallback` block, using **its own** anchor |
| No intent, signal Low, broad evidence absent | Retain lexical result if non-empty; abstain if empty |
| No intent, signal Strong | Lexical path, unchanged |
| Policy is `LexicalOnly` | Lexical path, unchanged, regardless of the above |

The general-background fallback is a first-class composed package, not an
unstructured catch-all: it declares its own anchor and its own dimension list, and
anchor sufficiency (section 10) applies to it exactly as it does to a named intent.

`"What was your authority over budget approvals?"` carries `authority`, `budget`, and
`approvals` as informative terms, so it is never routed to composition even when its
lexical score is weak.

## 5. Two separate normalisers

Filler terms are **not** added to the global `STOP_WORDS`. Changing that list would
alter scoring for every question and risk regressions far outside this feature.

Intent detection needs two *different* transformations, and conflating them breaks the
feature. A single filler-stripping normaliser applied to named patterns would reduce
`"tell me about yourself"` — the canonical pattern this design exists to serve — to the
**empty string**, since every one of its tokens is filler. An empty pattern then
matches unpredictably. The two functions are therefore kept distinct and are used for
different things.

**`normalize_phrase()` — named-pattern matching.** Lowercase, strip punctuation,
collapse internal whitespace, trim. **No token removal.** Both the configured pattern
and the incoming question pass through it, and matching compares the results.
`"Tell us about yourself?"` normalises to `tell us about yourself` and matches the
configured pattern intact.

**`informative_tokens()` — filler removal and broadness detection.** Drops a filler
set (`tell`, `us`, `me`, `about`, `yourself`, `walk`, `through`, `describe`, `bit`,
`little`, `just`, `quick`, `give`) and returns the remaining tokens, from which
corpus document frequency determines which are informative. Used **only** for the
broad-question evidence test in section 4. It is never applied to configured patterns.

**Validation rejects any configured pattern that is empty after `normalize_phrase()`.**
A pattern that normalises to nothing is a configuration error, caught at load, not a
wildcard discovered at question time.

`tokenize()`, `lexical_score()`, and `STOP_WORDS` are untouched, so the lexical path is
unaffected by either function.

## 6. Configuration

Two layers, resolved once at load.

**Shipped default.** `composition.default.json`, embedded via `include_str!`, carrying
an integer `config_version`. Dimensions are deliberately **career-neutral**:
`career_core`, `scope_and_scale`, `leadership`, `domain_practice`, `role_fit`. No
humanitarian, sector, or persona-specific dimension ships in the product.

**Local override.** Optional file in the app data directory, loaded and validated once
at startup, not per question. On any validation failure the loader falls back to the
embedded default and the failure is **visible in two places**:

- **Settings** — a detailed, persistent warning naming the override path and the
  specific validation failure, so the problem can actually be fixed.
- **Live Assist** — a compact, non-blocking badge indicating that shipped defaults are
  in force. It never interrupts a live answer; it only makes the degraded state
  legible while the meeting is running.

Both are backed by `config_status: "override_invalid"` in diagnostics and a logged
reason. A bad override degrades to shipped behaviour; it never silently half-applies
and never fails a live answer.

Sector-specific dimensions such as `emergency_regional` belong in the local override.

```json
{
  "config_version": 1,
  "max_arbitrary_ties": 8,
  "budget": { "total_evidence_chars": 7000, "per_record_chars": 1200 },
  "intents": [{
    "name": "self_introduction",
    "patterns": ["tell me about yourself", "tell us about yourself",
                 "walk me through your background", "introduce yourself"],
    "anchor": "career_core",
    "dimensions": ["career_core", "scope_and_scale", "leadership",
                   "domain_practice", "role_fit"]
  }],
  "fallback": {
    "anchor": "career_core",
    "dimensions": ["career_core", "scope_and_scale", "role_fit"]
  },
  "dimensions": [{
    "name": "career_core", "priority": 1, "quota_chars": 2000,
    "match_category": ["cv"], "match_any_tag": ["cv", "experience"],
    "match_title": ["^Experience", "Summary"]
  }]
}
```

### Validation

Performed once at load, against both the embedded default and any override. Every
rule below is a load-time failure, never a question-time surprise.

- A pattern that is empty after `normalize_phrase()` is **rejected** (section 5).
- `match_title` entries are **bounded and precompiled**: each is length-capped, the
  count per dimension is capped, and all are compiled to regexes during validation.
  Compilation failures are rejected at load, and no regex is ever compiled on the
  question path. This keeps a malformed or pathological override from becoming a
  latency or backtracking hazard during a live meeting.
- A dimension declaring no selectors is rejected (section 7).
- Duplicate dimension priorities are rejected (section 7).
- Every `anchor` — for each intent **and** for `fallback` — must name a dimension that
  exists and is listed in that same block's `dimensions`.
- `fallback` is required; a config without it is rejected.

## 7. Selector semantics and deterministic assignment

Selectors draw only on signals that exist today — `category`, document-level `tags`,
and the heading breadcrumb stored in `title` — so no identity migration is required.

Note that `tags` are document-level: every record parsed from one file inherits that
file's tags. Tags separate documents into facets; they cannot discriminate within a
document. `match_title` covers intra-document selection; its patterns are bounded and
precompiled at load (section 6), never compiled while answering a question.

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

**The budget counts evidence content only** — the characters of record `content` that
are actually admitted into the package. It excludes JSON structure, field names,
identity header, record metadata, and the prompt template. Defining it this way keeps
the number stable when serialisation changes: adding a metadata field alters
`prompt_json` size but must not silently shrink the evidence the model receives.

- Total evidence budget: 7,000 characters.
- Per-record cap: 1,200 characters.
- Each dimension draws up to its quota in priority order; unused quota redistributes
  downward by priority.
- Ordering within a dimension: document order, then `id`.

**Total serialised `prompt_json` size is recorded separately** in diagnostics
(section 11). Evidence budget governs selection; serialised size is what actually
drives cost and latency, and the two must be observable independently.

Both figures are **provisional experimental defaults.** Before retuning, measure all
four of: admitted evidence characters, serialised `prompt_json` size, first-token
latency, and completion latency.

**Truncation ladder.** Qualifiers are load-bearing here.
`SPECIALIZED_PERSONAL_FACT_POLICY` depends on "approximately", "shared
responsibility", and exact quantities surviving intact. A mid-sentence cut could strip
the very qualifier the anti-fabrication rule relies on. So:

1. Whole record if it fits.
2. Otherwise the largest prefix of complete **paragraphs** that fits.
3. Otherwise the largest prefix of complete **sentences** that fits.
4. If not even one complete sentence fits, **omit the record entirely.** A sentence is
   never cut mid-way.

Partial records carry `truncated: true`, and that flag **carries a prompt rule**, not
merely an annotation. The prompt states that for any record marked truncated, the
omitted content is *unknown*: it must never be inferred, assumed absent, treated as
containing nothing further, or completed from plausible continuation. Absence of a
qualifier, a caveat, or a subsequent step in a truncated record is evidence of
nothing.

Without that rule the flag is actively dangerous — a truncated authority record could
read as an unqualified one, which is exactly the fabrication
`SPECIALIZED_PERSONAL_FACT_POLICY` exists to prevent.

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

Every composed package declares one anchor dimension it cannot answer without — each
named intent, **and the general-background `fallback` block equally.** For
`self_introduction` the anchor is `career_core`. Composition never proceeds without a
declared anchor; there is no unanchored path.

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
- `anchor`: the anchor dimension applied, and whether it survived
- per record: assigned `dimension`, `original_chars`, `admitted_chars`
- `suppressed[]`: `conflict_key`, ids, revisions, reason
- `omitted[]`: records dropped by the truncation ladder
- `evidence_chars_used` / `evidence_chars_total` — the section 8 budget
- `prompt_json_bytes` — total serialised prompt size, recorded independently of the
  evidence budget so cost and latency stay observable when serialisation changes
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
9. Config validation fails at load, not at question time, for each of: duplicate
   dimension priorities; a dimension with no selectors; a pattern that is empty after
   `normalize_phrase()`; an invalid or over-long `match_title` regex; an `anchor`
   naming a dimension absent from its own block; and a missing `fallback` block.
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
16. `"tell me about yourself"` survives `normalize_phrase()` as a non-empty phrase and
    matches its configured pattern. This is the regression test for the normaliser
    blocker: under a single filler-stripping normaliser the phrase reduces to empty.
    Asserted for every shipped pattern, not just this one.
17. `informative_tokens()` is never applied to configured patterns — asserted
    structurally, so the two normalisers cannot be reconnected by a later edit.
18. The `fallback` block abstains when **its own** anchor is empty, proving anchor
    sufficiency is enforced for fallback composition and not only for named intents.
19. `prompt_json_bytes` and `evidence_chars_used` move independently: adding a record
    metadata field changes the former and leaves the latter unchanged.
20. A record marked `truncated: true` is accompanied by the omitted-content prompt
    rule, asserted on the rendered prompt.
21. A profile version with no `kind` resolves to `LexicalOnly`; a profile *named*
    "Interview Coach" but carrying no `kind` also resolves to `LexicalOnly`, proving
    no name-based inference.
22. Upgrading a profile to `ProfileKind::Interview` produces a new version hash and
    leaves the prior stored version byte-identical.

**Fixtures.** The private corpus must never enter Git. `experiments/` is already
gitignored and currently has zero tracked files. Tests therefore use:

- a tracked, anonymised, career-neutral synthetic fixture exercising every dimension,
  conflict case, and oversize record, living under the Rust test tree — not under
  `experiments/`, which is ignored and would be invisible to CI; and
- an `#[ignore]` gate reading the real corpus from the ignored path, run manually as a
  pre-release check.

## 13. Decisions and remaining work

Resolved:

- **Schema.** Approved as `ExpertProfileVersion::kind: Option<ProfileKind>`, not on
  `MeetingPlaybook`. Interview is the lens; Junior, Mid-level, and Expert are depth
  playbooks. Existing Interview profiles upgrade via an explicit new immutable
  version, with no name-based inference and no mutation of stored versions
  (section 3).
- **Budgets.** 7,000 total evidence characters and 1,200 per record are approved as
  provisional experimental defaults. Retuning requires measuring admitted evidence
  size, serialised prompt size, first-token latency, and completion latency
  (section 8).
- **Warnings.** Both surfaces: a detailed persistent warning in Settings and a
  compact non-blocking badge in Live Assist (section 6).
- **Conflict policy.** Approved for both the composed and lexical paths (section 9).

Remaining before implementation:

- Review of this revision.
- An implementation plan, to be written only after that review.
