# Project Context retrieval spike results

Date: 2026-08-20

Status: provider-free representative- and private-corpus evidence; IDF/diversity experiment complete, selector not production-ready

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

## IDF and bundle-diversity experiment

The follow-up scorer calculates smoothed inverse document frequency over the current, non-expired candidate passages in the pinned snapshot. It uses deterministic fixed-point integer arithmetic and remains fully local. Each query term contributes once at its strongest matching field; passage body is weighted above heading, tags, and provenance so a title repeated across fields cannot outscore answer-bearing text merely through duplication.

On the three diversified private cases, the IDF-weighted, field-saturated raw top three produced the following diversity before bundle shortlisting:

| Case | Unique sources in raw top three | Unique bundles in raw top three |
| --- | ---: | ---: |
| No Q&A coverage | 1 | 1 |
| Split role and professional evidence | 2 | 1 |
| Partial Q&A coverage plus role fact | 2 | 2 |

This is an improvement over the earlier one-source top three in two cases, but not a coverage guarantee. IDF changes relative scores; it cannot require a second relevant bundle to survive the final budget.

The experiment therefore adds a separate bundle-diverse shortlist. It first determines eligible bundles from the pinned snapshot and query, including deterministic routing for explicitly named projects, applies the relative floor within each eligible bundle, and then selects in rounds so one eligible bundle cannot consume every slot before another contributes its strongest passage. Interview and Q&A documents remain sources inside the project/application bundle; they do not create a fourth context scope.

The new selector keeps all ten synthetic cases and the stricter synthetic private-evaluation rules green. On the first unchanged private witness, it promoted the required non-Q&A directive answer to rank 1, but also selected one application-bundle passage and one person-bundle passage. Those two irrelevant passages exceed the limit-3 precision allowance, so the 15-case private witness remains red exactly as intended.

That failure localizes the remaining problem: bundle diversity now provides coverage among bundles judged eligible, but lexical bundle eligibility is not precise enough on the 39,695-word corpus. The next design decision is not another passage-score floor. Live Assist must pin an explicit active application/project selection and the retrieval policy must distinguish session-selected bundles from merely loaded contextual bundles before lexical eligibility is treated as production-ready.

## Pinned session eligibility

The retrieval contract now has a hard session boundary: `active_project_bundle_ids` is pinned with the snapshot before retrieval. Person and Role remain the single shared bundles; Project/application bundles are an explicit subset and may be empty. Unknown selected IDs fail closed.

Selection happens before document-frequency calculation, conflict detection, lexical bundle eligibility, and ranking. The regression test loads both Atlas and Beacon, pins only Atlas, and proves that physically removing Beacon produces identical passage IDs and scores. It also proves that an empty Project selection cannot return a Project passage. An unselected bundle therefore cannot alter IDF or leak into results.

This closes session-selected eligibility as an architectural boundary, not the lexical precision problem inside an active set. The unchanged private run still activates all Project bundles in its context manifest and remains red on its first precision witness. The next retrieval experiment must improve precision among intentionally active Person, Role, and Project bundles. Only after the private gate passes should preflight expose the active application/project selection and durable meeting sessions persist it; Live Assist answer generation remains blocked.

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

The IDF and bundle-diversity experiment closes the ranking architecture, and pinned session selection now closes the hard eligibility boundary. Lexical precision inside the active bundle set remains open. Live Assist answer generation remains blocked until a selector passes both the synthetic mechanics tests and the unchanged diversified private suite.
