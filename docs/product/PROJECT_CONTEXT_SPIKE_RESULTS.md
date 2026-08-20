# Project Context retrieval spike results

Date: 2026-08-20

Status: provider-free representative-corpus result; real user corpus validation remains outstanding

## Configuration

- One Person Identity bundle.
- One Role Context bundle.
- Two Project Context bundles: Atlas and Beacon.
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

The limits of 3, 5, and 8 produced identical selected sets. The relative-score floor prevented the selector from padding a strong answer with weak matches. The smallest limit, 3, is therefore the provisional production ceiling for this corpus.

## Findings produced by the spike

### Conflict relevance cannot use any body-word overlap

The first run treated a single generic overlap such as “under” or “ceiling” as evidence that an unrelated explicit conflict applied. This caused unrelated vehicle-delegation records to block other questions.

The corrected rule establishes conflict relevance from the semantic terms in the explicit conflict key. A relevant vehicle-leasing question still fails closed, while unrelated questions proceed.

### A result limit is a ceiling, not a quota

The second run correctly ranked the procurement authority passage first but filled the remaining slots with weaker vehicle passages. This met recall but failed precision.

The corrected selector discards candidates scoring below half of the strongest match before applying the maximum result limit. This removed all irrelevant filler in the representative corpus.

## Interpretation

Weighted lexical retrieval is sufficient for this small, deliberately structured representative corpus. This result does not establish that it is sufficient for the user's real CV, TOR, guides, or project files.

The next evidence required is the same regression suite populated with user-authored canonical Markdown. The real-corpus run should retain these fixtures as stable mechanics tests and add questions that reflect actual meetings. If precision or recall then fails, the corpus and provenance contract remain valid; only the retrieval mechanism changes.
