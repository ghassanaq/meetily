# Project Context retrieval spike results

Date: 2026-08-20

Status: provider-free representative- and private-corpus evidence; global selector is not production-ready

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
- Synthetic-fixture relative score floor: selected candidates must score at least half of the strongest match.
- Compared maximum result limits of 3, 5, and 8.
- No provider, credentials, network, or model generation.

## Synthetic-fixture results

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

The limits of 3, 5, and 8 produced identical selected sets. The 50%-of-best relative-score floor prevented the selector from padding a strong answer with weak matches. The floor therefore did the selection work while the maximum limit was nearly inert. The smallest limit, 3, is sufficient for this fixture, but neither the limit nor the floor is a production recommendation.

## Private-corpus results

The private corpus contains 15 indexed Markdown sources and 39,695 passage-body words. The first evaluation used 12 questions whose expected passages all came from the primary Q&A document. All 36 question/limit combinations passed at an 85% floor, but that result was structurally biased: question-shaped Q&A text was the only expected document type, so the suite could not measure cross-source recall.

Three deliberately diversified questions invalidated that conclusion across a 50–85% floor sweep:

| Case | 50–70% | 75–85% |
| --- | --- | --- |
| No Q&A coverage; directive answer required | Answer ranked second, but same-document noise exceeded the precision allowance | Title/preamble survived; answer-bearing passage was excluded |
| Split role duty and professional evidence | All top-three slots went to role-document chunks; professional evidence was absent | Same failure |
| Partial Q&A coverage plus exact role staffing | All top-three slots went to role-document chunks; Q&A evidence was absent | Same failure |

No global floor in the measured range passed the diversified suite. Global-floor tuning is therefore a closed question for this selector: a threshold can only filter an already-ranked list; it cannot change which source produced the top candidates. The failure sits upstream in scoring, ranking, and source diversity because adjacent chunks from one source can monopolize the global top-three budget. The private ignored test now retains these 15 cases, with the 85% configuration serving as a known failing recall witness rather than a recommendation.

## Findings produced by the spike

### Conflict relevance cannot use any body-word overlap

The first run treated a single generic overlap such as “under” or “ceiling” as evidence that an unrelated explicit conflict applied. This caused unrelated vehicle-delegation records to block other questions.

The corrected rule establishes conflict relevance from the semantic terms in the explicit conflict key. A relevant vehicle-leasing question still fails closed, while unrelated questions proceed.

### A result limit is a ceiling, not a quota

The second run correctly ranked the procurement authority passage first but filled the remaining slots with weaker vehicle passages. This met recall but failed precision.

The corrected selector discards candidates scoring below half of the strongest match before applying the maximum result limit. This removed all irrelevant filler in the representative corpus.

The 50% floor is a synthetic-fixture parameter, not a validated universal threshold. Its committed constant is named `SYNTHETIC_FIXTURE_RELATIVE_SCORE_FLOOR_PERCENT` so a future implementation cannot mistake fixture tuning for production evidence.

## Interpretation

Weighted lexical retrieval is sufficient for the small, deliberately structured representative corpus. With only 369 indexed passage-body words—less than a typical page—that result demonstrates parser, provenance, expiry, conflict, scoring, floor, and selection mechanics.

The private regression suite supplied the missing scale and source-diversity evidence. It shows that a single global ranking and floor cannot yet retrieve split evidence reliably from the user's CV, role documents, Q&A material, and professional evidence. The anonymized fixtures remain the permanent CI mechanics test; the sensitive corpus remains outside Git behind `PROJECT_CONTEXT_PATH`.

The next retrieval experiment should build a source- or bundle-diverse shortlist before the final global budget, then rerun all 15 private questions across limits 3, 5, and 8. Live Assist integration remains blocked until a selector passes both the synthetic mechanics tests and the diversified private suite.
