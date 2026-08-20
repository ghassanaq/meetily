# Project Context retrieval spike results

Date: 2026-08-20

Status: provider-free representative-corpus result; real user corpus validation remains outstanding

## Configuration

- One Person Identity bundle.
- One Role Context bundle.
- Two Project Context bundles: Atlas and Beacon.
- Seventeen fixture files, including 12 Markdown sources.
- Roughly 381 authored corpus words by the review inventory; 369 passage-body words are actually indexed by the parser.
- Ten representative retrieval questions.
- Current, expired, and explicitly conflicting sources.
- Weighted lexical scoring over bundle name, source, heading, tags, and passage body.
- Expiry filtering before scoring.
- Semantic conflict-key matching and fail-closed conflict blocking.
- Relative score floor: selected candidates must score at least half of the strongest match.
- Compared maximum result limits of 3, 5, and 8.
- No provider, credentials, network, or model generation.

## Results

All ten questions passed at all three maximum result limits.

| Measurement | Result |
| --- | --- |
| Fixture scale | 17 files; 369 indexed passage-body words |
| Expected passage rank | First or second for every question |
| Irrelevant passages selected | 0 across all 30 question/limit combinations |
| Passages selected per question | 1–2 |
| Selected passage words per question | 30–78 |
| Single-project topic bleed | 0 |
| Two-project question | Retrieved the expected passage from both projects |
| Expired passage | Excluded before scoring |
| Relevant current conflict | Blocked before selection |
| Unrelated current conflict | Did not block or enter results |
| Imperative role policy | Preserved as typed `role_policy` data |

The limits of 3, 5, and 8 produced identical selected sets. The 50%-of-best relative-score floor prevented the selector from padding a strong answer with weak matches. The floor therefore did the selection work while the maximum limit was nearly inert. The smallest limit, 3, remains the provisional production ceiling for this corpus, but the floor is the load-bearing parameter that must be revisited on real material.

## Findings produced by the spike

### Conflict relevance cannot use any body-word overlap

The first run treated a single generic overlap such as “under” or “ceiling” as evidence that an unrelated explicit conflict applied. This caused unrelated vehicle-delegation records to block other questions.

The corrected rule establishes conflict relevance from the semantic terms in the explicit conflict key. A relevant vehicle-leasing question still fails closed, while unrelated questions proceed.

### A result limit is a ceiling, not a quota

The second run correctly ranked the procurement authority passage first but filled the remaining slots with weaker vehicle passages. This met recall but failed precision.

The corrected selector discards candidates scoring below half of the strongest match before applying the maximum result limit. This removed all irrelevant filler in the representative corpus.

The current 50% floor is provisional retrieval policy, not a validated universal threshold. At this fixture scale, vocabulary is sparse and largely disjoint across bundles, so strong and weak candidates separate easily. A real 20,000–50,000-word corpus is expected to reuse terms such as approval, escalation, delivery, stakeholder, and risk across Person, Role, and Project sources. That narrower score distribution may cause the same floor either to admit noise or exclude necessary evidence.

## Interpretation

Weighted lexical retrieval is sufficient for this small, deliberately structured representative corpus. With only 369 indexed passage-body words—less than a typical page—the result demonstrates parser, provenance, expiry, conflict, scoring, floor, and selection mechanics. It does not establish that lexical scoring can discriminate across the user's real CV, TOR, guides, authority records, or project files.

The next evidence required is the same regression suite populated with user-authored canonical Markdown outside Git. The ignored `private_corpus_retrieval_measurements` test reads `PROJECT_CONTEXT_PATH` and a private `retrieval-eval.json` containing 10–20 real questions, expected/relevant passage IDs, allowed project bundles, evaluation time, and the floor being measured. It emits identifiers and metrics without printing question or passage content. The anonymized fixtures remain the permanent CI mechanics test. If precision or recall then fails, the corpus and provenance contract remain valid; only the retrieval mechanism or floor changes.
