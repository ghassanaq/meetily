# Authority-Scope Warning Design

Status: reviewed design; no implementation authorised
Date: 2026-08-21
Scope: advisory detection of autobiographical authority expansion in Live Assist
Complements: `PROFESSIONAL_IDENTITY_AND_LENSES_DESIGN.md`

## 1. Decision

Authority enforcement begins as a visible, non-blocking warning after answer completion.
It does not block generation, delay or hide the stream, rewrite the answer, disable copy,
or make a second provider call on the live path.

The checker is object-scoped, not verb-scoped. A verb such as `managed` may be supported
for a team or workstream and unsupported for the wider operation containing it. A warning
therefore requires evidence that the answer joined an authority-bearing action to an
explicitly excluded object or scope.

This is an advisory detector, not a factuality certificate. No warning means only that no
enrolled rule matched; it never means the answer's authority was verified.

## 2. Why this boundary is necessary

The observed failure was not an unsupported action word in isolation. It expanded the
object of a supported action from a bounded workstream to the whole surrounding operation.
The Professional Identity already carries prose that distinguishes duties inside the
workstream from decisions or mission authority outside it.

Free-form prose cannot be converted into a deterministic subject-action-object policy with
a trustworthy false-positive rate. Doing so would invent correspondence between evidence
and answer claims. The design therefore follows the same honesty rule used by Mishkat's
compose service:

- constraints must be explicitly enrolled by a human;
- deterministic verification may claim only that an enrolled rule matched;
- ambiguous semantic coverage stays a human judgment;
- findings are visible and auditable, never hidden mutations.

## 3. Non-goals

The first version does not:

- infer authority rules automatically from all imported Markdown;
- warn merely because an answer contains `managed`, `led`, `owned`, or `oversaw`;
- judge general answer quality or quantitative qualifiers;
- detect every paraphrase of a scope expansion;
- use the answer provider as its own semantic judge;
- regenerate or silently repair an answer;
- claim that an unflagged answer is supported.

## 4. Enrolled authority constraint

Runtime warnings require immutable structured rules selected with the Professional Identity
version. The proposed schema version 2 adds an `authority_constraints` collection to
`ProfessionalIdentityVersion`; the collection participates in the existing content hash.
The private context manifest may reference a bounded local rule file, but import compiles the
rules into the immutable snapshot so runtime never depends on a moving sidecar. Existing
version-1 identities continue unchanged and produce no authority warning. Enabling the
feature requires a newly imported and explicitly selected identity version; stored versions
are never mutated or silently upgraded.

Proposed closed shape:

```json
{
  "id": "stable-local-id",
  "label": "Emergency-processing workstream boundary",
  "contexts": ["named field event"],
  "action_families": ["manage", "lead", "own", "oversee", "be responsible for"],
  "permitted_objects": ["processing workstream", "assigned processing team"],
  "excluded_objects": ["whole mission", "clinical decisions", "entire operation"],
  "evidence_record_ids": ["local-record-uuid"]
}
```

Rules contain aliases deliberately enrolled for matching, not embeddings or generated
synonyms. Each rule must reference at least one record in the same immutable identity
version. Unknown record IDs, empty alias lists, duplicate IDs, oversized values, or an
object present in both permitted and excluded sets fail import.

`contexts` is optional and narrows a rule; it is not a completeness requirement. A
context-specific rule reduces collisions where the same object words mean different things,
while a context-free rule is the intended safety net for vague claims that omit the event,
location, or project name. Enrollment guidance must therefore default to context-free rules
for general capability boundaries and add a context only when it is necessary to avoid a
demonstrated false positive. A narrow event rule may coexist with a context-free capability
rule, but duplicate effective matches collapse to one warning.

The private rule file stays beside the private corpus and remains Git-ignored. Tracked tests
use anonymised synthetic constraints only.

## 5. Detection contract

The checker runs only after the existing completion and plain-text validation succeeds. It
splits the normalized answer into sentences and emits a warning only when one sentence
satisfies every condition below:

1. It is a first-person past or present autobiographical claim.
2. It contains an enrolled authority action or alias.
3. It contains the rule's context when the rule declares one.
4. It contains an enrolled excluded object or alias.
5. The matched action-and-object claim is not negated and is not explicitly prospective or
   hypothetical.

Permitted-object matches do not cancel an excluded-object match in the same sentence. This
is required for compound claims that accurately name a bounded responsibility and then
expand it to the containing operation.

The detector records the matched aliases, rule ID, and the excluded-object span in the exact
generated sentence. Span offsets use UTF-16 code units so the Rust snapshot and TypeScript UI
share one unambiguous indexing contract. It does not record a synthetic subject-action-object
parse and does not report unsupported objects that were never enrolled.

### 5.1 Language handling

Version 1 is English-only because the current interview answer contract is English and the
aliases are manually enrolled. Matching is case-insensitive, punctuation-insensitive, and
whitespace-normalized. It does not stem arbitrary words or generate synonyms.

### 5.2 Negation and prospective guards

The following forms must not warn:

- `I did not manage the whole operation.`
- `I was not responsible for the final decision.`
- `I managed the workstream, not the whole operation.`
- `I would manage the operation through...`
- `If appointed, I would oversee...`

Negation is local to the matched action-and-object claim, not sentence-wide. `I did not manage
the team, but I managed the whole operation` still warns because the excluded-object claim is
affirmative. The guards are deliberately conservative. An ambiguous sentence produces no
deterministic warning and remains available to the offline semantic harness.

## 6. Warning model and UI

Authority findings are separate from `answer_format_warnings`. Add a dedicated collection,
provisionally `answer_policy_warnings`, to the exchange snapshot. A warning contains:

- stable code `authority_scope_expansion`;
- rule ID and human-readable label;
- the exact generated sentence;
- matched action, context, and excluded-object aliases;
- excluded-object start and end offsets in UTF-16 code units;
- evidence record IDs for local provenance.

The Live Assist panel always exposes one of three passive states after completion:

- `Authority rules not configured` for a version-1 identity or an identity with no enrolled
  rules;
- `Authority rules checked · no enrolled match` when at least one rule was evaluated and none
  matched; its tooltip states that this is not comprehensive factual verification;
- an amber `Authority wording needs review` indicator when a rule matched.

This distinction carries the honesty contract into the UI: `not checked` and `checked against
enrolled rules` never collapse into the same silent state. The clean state must not use a
green success treatment or wording such as `verified`, `safe`, or `supported`.

Expanding a warning highlights only the matched excluded-object span inside the sentence,
rather than presenting supported neighboring clauses as suspect. It also shows a neutral
explanation such as:

> This sentence may describe the wider operation, while your enrolled evidence limits the
> documented responsibility to a workstream.

The UI offers dismiss and inspect-evidence actions. Dismissal affects only the current
memory-only exchange. It does not alter the identity rule, rewrite the answer, train a model,
or suppress the same rule later. The app does persist a local feedback count keyed by rule ID
and identity-version hash, with the last-dismissed timestamp. Repeated dismissals are review
evidence that a rule or alias may be wrong; they never change runtime enforcement. Rule review
shows this count, and only a human-authored new immutable version can revise the constraint.
Copy remains available because this phase advises rather than blocks.

`Inspect evidence` is primarily a post-hoc review affordance, not part of the live-answer
path. Its collapsed state shows only the rule label and source metadata. On explicit request
it may load and show the full local source excerpt from the immutable identity version. That
operation need not meet the answer-stream latency budget and never sends the excerpt to a
provider.

Warnings appear only after completion. Streaming partial text is not repeatedly classified;
that would create flickering false alarms from incomplete sentences.

## 7. Runtime and privacy properties

- The check is local and deterministic for a fixed answer, identity version, and rule set.
- It makes no network or provider call.
- It does not add rule diagnostics to `prompt_json`.
- The exchange records rule IDs and matched aliases, not additional private source content.
- Dismissal feedback is local metadata and never suppresses future findings.
- Grounding remains based on the records actually supplied to generation.
- No private rule, answer, or corpus text enters tracked fixtures or documentation.

## 8. Evaluation before runtime activation

The checker ships disabled until the synthetic suite and ignored private gate are reviewed.
The tracked suite must include at least:

1. supported management of a bounded team: no warning;
2. supported leadership of a named workstream: no warning;
3. management of an explicitly excluded whole operation: warning;
4. authority over an explicitly excluded decision class: warning;
5. shared responsibility phrased as sole ownership: warning when separately enrolled;
6. a prospective statement using the same verb and object: no warning;
7. a negated statement using the same verb and object: no warning;
8. a compound multi-context claim with one supported and one excluded scope: warning;
9. an unenrolled paraphrase: no warning and explicitly classified as `unknown`, not `pass`;
10. malformed or cross-version rule references: import failure.
11. a context-free rule matching a vague claim that omits event and location: warning;
12. contrastive negation of the excluded object: no warning;
13. sentence-level negation attached to a different object: warning for the affirmative
    excluded-object clause.

The ignored private gate stores only case IDs, answer hashes, matched rule IDs, warning codes,
and human adjudication. Raw professional evidence and generated answers stay untracked.

Passing author-selected fixtures demonstrates internal consistency, not field precision.
After those fixtures reach zero false positives, the detector remains offline-only for five
live interview trials containing previously unseen generated answers. Each completed answer
is human-adjudicated for whether a warning would have been correct, missed, or unnecessary;
the warning is not shown in Live Assist during this period. Advisory runtime activation
requires zero false positives across both the accepted controls and those five trials, plus
explicit user approval. Recall is measured and reported but is not disguised as complete
coverage. Any false positive returns the rule set to enrollment review and restarts the live
trial count after a new immutable version is selected.

## 9. Relationship to the semantic claim audit

The existing credentialed harness already decomposes claims and checks source attribution.
It remains the right place to evaluate unenrolled paraphrases, compound-story contamination,
and whether the structured rule vocabulary is missing aliases.

Semantic audit findings may propose a new alias or rule, but they never activate one. The
human reviews the evidence and enrolls the constraint in a new immutable identity version.
This is the improvement loop:

```text
observed miss -> offline candidate -> human evidence review -> new version -> deterministic warning
```

An asynchronous runtime semantic judge is explicitly deferred. If evaluated later, its
findings must remain separately labelled, non-blocking, and distinguishable from deterministic
enrolled-rule matches.

## 10. Implementation boundary

A future implementation plan should be split into independently reviewable changes:

1. schema/import validation for immutable authority constraints, with no runtime behavior;
2. pure local matcher and synthetic tests;
3. exchange diagnostics and TypeScript snapshot types;
4. advisory Live Assist UI;
5. local dismissal-feedback ledger and rule-review count;
6. ignored private evaluation, five offline live trials, and explicit activation decision.

No implementation begins from this draft. Schema details, UI copy, and the activation gate
must be approved before code changes.

## 11. Resolved review decisions

1. Dismissal is exchange-local and never suppresses a future warning; repeated dismissals are
   counted locally as rule-review evidence.
2. Evidence excerpts are loaded only on demand for post-hoc review; the live indicator stays
   compact.
3. Synthetic and private controls are followed by five offline live trials on unseen answers
   before any advisory warning appears at runtime.
