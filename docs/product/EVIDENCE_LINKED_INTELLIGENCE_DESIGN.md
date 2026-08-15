# Evidence-Linked Meeting Intelligence

Status: proposed design contract; no implementation is included in this change.

This document defines the evidence and provenance layer for delivery item 5 in
`PRODUCT-HANDOFF.md`. It is deliberately forward-compatible with cited private
document retrieval (item 6), but it does not implement document ingestion or
retrieval.

The design is local-first, declarative, versioned, deterministic where the
application controls the result, and incapable of executing tools or scripts.
It extends the Expert Profiles contract without weakening its central rule:
evaluation and production must use the same generation, parsing, and evidence
resolution path.

## 1. Product outcome

Meeting Assistant may derive the following closed set of intelligence items:

- `decision`
- `action`
- `direct_question`
- `risk`
- `topic_change`
- `coaching_suggestion`

Each item must declare one epistemic status:

- `fact`
- `assumption`
- `analysis`
- `recommendation`
- `suggested_wording`

Every persisted intelligence item must carry at least one application-created
citation. The UI must expose the item's type, epistemic status, evidence, and
verification state. Recommendations and suggested wording must never be
presented as facts merely because they cite factual evidence.

Phase 1 is manual and inert: a user explicitly requests intelligence for one
completed meeting. This phase does not add live monitoring, automatic profile
activation, automatic actions, notifications, or external side effects.

### 1.1 Availability contract

Evidence-linked intelligence is deliberately stricter and less failure-tolerant
than ordinary summarization. It is available only when all of the following are
true:

- the meeting has retained audio enrolled as a verified recording artifact;
- an active immutable transcript version resolves against that exact recording
  version;
- the selected Expert Profile and model binding are active and evaluated for the
  current capability hash;
- required local or explicitly consented remote providers are available;
- every planned evidence-bearing map, reduction, and final-render input fits its
  model budget; and
- every planned evidence-bearing generation stage succeeds without cancellation
  or partial chunk loss.

Preflight failures disable the evidence-linked action with a specific reason.
Runtime provider, budget, cancellation, or chunk failures before the canonical
English artifact is validated produce no partially validated artifact. They
never fall back to profiled but uncited output. Optional presentation translation
follows section 5.1 and does not determine evidence availability. The user may
separately choose ordinary unprofiled summarization, whose result must not be
labelled evidence-linked or inherit the profile provenance.

The UI uses this section as the single availability source of truth. Detailed
sections below define the corresponding error states:

- missing retained audio: `EVIDENCE_SOURCE_REQUIRED`;
- superseded model/profile binding: `BINDING_SUPERSEDED`;
- unavailable provider: `PROVIDER_UNAVAILABLE`;
- prompt or evidence appendix overflow: `EVIDENCE_BUDGET_EXCEEDED`; and
- incomplete map/reduce execution: `EVIDENCE_CHUNK_INCOMPLETE`.

## 2. The source of truth is the recording

Transcript rows are not stable evidence identifiers. The shipped
retranscription path deletes all transcript rows for a meeting and inserts a new
set. Row IDs therefore identify one replaceable interpretation, not the source
artifact.

The recording is the durable source artifact. Transcript segments already use
recording-relative `audio_start_time` and `audio_end_time`, so an audio citation
is identified by:

```text
(recording_artifact_id, recording_version_hash, start_ms, end_ms)
```

The transcript version and segment IDs used to resolve that interval are
provenance, not citation identity. This lets a citation survive transcript row
replacement, model changes, and text corrections while still showing whether
the current interpretation of the cited audio has changed.

Meeting ID and recording ID are not interchangeable. Phase 1 may expose one
primary recording per meeting, but the storage contract permits more than one
recording or imported source later.

### 2.1 Stable artifact identity

`RecordingArtifact` has an application-minted UUID that remains stable for the
logical recording. Paths, filenames, meeting IDs, modification times, and
database row IDs must not be used as artifact identity.

For a new capture or import, the UUID is minted when the meeting workspace is
created, before the database meeting row necessarily exists. It is written to
workspace metadata and carried into the later database save. Retrying that save
must reuse the UUID. Disabling audio auto-save must not create a pretend
recording artifact merely because a workspace folder exists.

Each exact set of audio bytes is a `RecordingArtifactVersion`, identified by a
domain-separated SHA-256 digest and accompanied by its byte length, media type,
and duration in integer milliseconds. Replacing bytes at the same path creates
a different version; it must never silently change the meaning of an existing
citation.

The application never moves, rewrites, or deletes audio merely to enroll it as
an evidence source.

### 2.2 Meetings without durable audio

Meetily can currently retain a transcript while audio auto-save is disabled.
Such a meeting has no immutable recording artifact to verify.

The application may preserve snapshot-only provenance for display, with status
`source_missing`, but it must not label it verified. A hard
`evidence_linked` assertion fails for any intelligence item whose evidence is
only snapshot-backed. Availability and fallback behavior follow section 1.1.

Imported audio becomes an ordinary recording artifact after the application
copies it into the meeting workspace and hashes the copied bytes.

## 3. Immutable transcript versions

A transcript is a versioned interpretation of one exact recording version.
Corrections and retranscription create a new immutable `TranscriptVersion` and
atomically advance a separate active-version pointer. They never update a
version in place.

An immutable transcript version contains, at minimum:

- its UUID and canonical content hash;
- the recording artifact ID and recording version hash;
- ordered segments with integer millisecond bounds and text;
- speaker/source labels when known;
- language;
- transcription engine and model identity;
- relevant decoding/configuration identity; and
- creation time.

The canonical transcript hash covers all fields that can change resolution,
including ordered segment bounds, normalized text, speaker/source labels,
language, model/configuration identity, and recording version hash. It uses RFC
8785 canonical JSON, the same safe-integer restrictions used by Expert
Profiles, and a domain-separated SHA-256 digest.

Existing `transcripts` rows may remain as the current compatibility projection
during migration. Evidence resolution must use immutable transcript-version
segments, not that replaceable projection.

Versions referenced by a persisted derived artifact may not be pruned. An
unreferenced-version retention policy may be added later.

## 4. Shared citation envelope

Citation data is closed and declarative. All structures reject unknown fields.
There are no command, executable, script, plugin, tool, URL-fetch, or hidden
permission fields.

Conceptually, every citation has this shape:

```json
{
  "schema_version": 1,
  "citation_id": "uuid",
  "artifact": {
    "id": "uuid",
    "kind": "recording",
    "version_hash": "sha256:..."
  },
  "locator": {
    "type": "audio_timeline",
    "start_ms": 12500,
    "end_ms": 17800
  },
  "snapshot": {
    "text": "We will send the revised proposal on Friday.",
    "normalization": "citation-text-v1",
    "content_hash": "sha256:..."
  },
  "resolution": {
    "transcript_version_hash": "sha256:...",
    "segment_ids": ["uuid"]
  }
}
```

The persisted Rust and TypeScript types must use a tagged locator union. Phase
1 permits `audio_timeline`; the schema reserves the following addition for item
6:

```json
{
  "type": "document_passage",
  "page_index": 0,
  "section_path": ["Decisions", "Rollout"],
  "start_byte": 140,
  "end_byte": 267
}
```

For document citations, the envelope's artifact ID identifies the logical
document and its version hash identifies the exact canonical extracted-text
revision. Page indexes are zero-based. Byte offsets are UTF-8 offsets into the
canonical page text and must fall on Unicode scalar boundaries. Only the Rust
resolver creates offsets; UI code does not calculate them. `section_path` is
display metadata and is not a substitute for revision and range identity.

Adding a locator variant requires a schema version and capability revision.

### 4.1 Audio timeline rules

Audio bounds use integers and the half-open interval `[start_ms, end_ms)`.

- `start_ms` must be at least zero.
- `end_ms` must be greater than `start_ms`.
- Bounds must not exceed the recorded duration, apart from a documented
  one-millisecond legacy rounding tolerance.
- Existing finite, non-negative f64 seconds convert with
  `round(seconds * 1000)`; non-finite or negative values are rejected.
- Phase 1 anchors align to one or more complete transcript segments. A selected
  span expands to the first segment's start and last segment's end.

Phase 1 does not claim word-level timing. Arbitrarily clipping text inside a
segment would imply timestamp precision the current transcription data does not
contain.

### 4.2 Snapshot normalization and hashing

Every citation stores the cited text snapshot so historical output remains
readable when audio or a transcript version is unavailable. A snapshot is
tamper-evident, not proof that its source still exists.

`citation-text-v1` normalization is:

1. valid UTF-8;
2. Unicode NFC;
3. CRLF and CR converted to LF; and
4. leading and trailing Unicode whitespace removed.

Internal whitespace, line breaks, punctuation, and case are preserved. The
content hash is SHA-256 over a domain separator, a zero byte, and the normalized
UTF-8 bytes. Hashing rules are shared by production and evaluation.

The complete citation envelope also has a canonical RFC 8785 digest. UUIDs and
timestamps that do not affect evidence identity are excluded from its content
digest by an explicit projection, not by incidental serializer behavior.

## 5. Trusted evidence handles

The model must never author artifact IDs, hashes, transcript IDs, or time
coordinates. Accepting model-authored locators would allow fabricated citations
to acquire valid-looking syntax.

Before generation, the application builds one deterministic evidence-candidate
plan over the complete immutable transcript version. Each candidate receives a
generation-run-local opaque handle such as `E000001`. Handles are globally
unique within that generation and remain stable across every model request,
chunk, reduction, final-render, and translation pass in the run. The prompt
contains the applicable handle and rendered segment text. Structured model
output may refer only to those handles.

The production parser:

1. rejects unknown or duplicate handles where duplicates are not permitted;
2. maps accepted handles back to application-owned candidates;
3. resolves their bounds through the evidence resolver;
4. constructs and hashes citation envelopes itself; and
5. rejects the derived item if its required evidence is not verified.

Handles are not persisted as citation identity. They are generation-run parser
tokens only. Chunk-local numbering such as a different `E1` in each request is
forbidden.

This candidate-renderer and parser are part of the generation capability hash.
Changing candidate selection, rendering, output parsing, normalization, or
resolution supersedes previous profile validation for that target.

### 5.1 Long meetings and provenance-preserving map/reduce

The production summary path chunks long transcripts and may perform multiple
model calls before a final report is written. Evidence generation must support
that path; phase 1 is not limited to single-chunk meetings.

The evidence-aware chunk planner operates on ordered evidence candidates rather
than splitting an untracked text string. A candidate keeps the same global
handle if overlap places it in adjacent chunks. The planner records an immutable
generation manifest containing the transcript version, ordered candidate index,
chunk membership, overlap, token budget, and planner capability identity.

Every map call returns structured intermediate intelligence items with handle
references. Its output is parsed and its handles are validated before it may
enter a reduction. Free-form chunk summaries without provenance are not valid
evidence intermediates.

A reduction receives:

- the parsed intermediate items;
- the union of handles referenced by those items;
- a deduplicated evidence appendix mapping each handle to its exact
  application-owned candidate; and
- the generation manifest identity.

The reducer may combine or de-duplicate items and may cite several handles for a
cross-chunk claim. It may not introduce a handle outside the supplied union.
Recursive reductions preserve the same rule. The final report pass receives the
remaining structured items and evidence appendix, and the ordinary production
parser builds citations only after validating the final handle set.

Every intermediate or final item may reference at most eight distinct evidence
handles. This is a structural schema limit, and its value is included in the
capability hash. The limit bounds both review complexity and the evidence text a
single claim can pull into a later reduction; it does not permit dropping
additional evidence silently.

Before every reduction, the planner computes the complete prompt cost of the
structured items and deduplicated evidence appendix. If it exceeds the model
budget, the planner first repartitions items deterministically in temporal order
and performs recursive reductions. If any required partition or the eventual
final input still cannot include every referenced candidate in full, the run
aborts with `EVIDENCE_BUDGET_EXCEEDED`. Appendix text and handles are never
truncated, summarized without provenance, or omitted to make a prompt fit. A
reducer therefore cannot cite a candidate whose text was absent from its input.

All planned chunks must succeed. The existing best-effort behavior that can
continue after a failed chunk is not permitted for evidence-qualified
generation: an omitted chunk could hide a decision, risk, or question while the
result still appeared complete. A map or reduction failure aborts the evidence
run with `EVIDENCE_CHUNK_INCOMPLETE`; it produces no partially validated derived
artifact.

The existing cached Markdown summary is not an evidence-bearing intermediate and
must not be used to skip this process. A future evidence cache is valid only if
it stores the complete structured intermediate, generation manifest, candidate
mapping, citations, and hashes, keyed by the exact transcript, profile, model,
and capability identities.

Optional translation happens after the English structured result has been
parsed and its citations resolved. Translation may change display content fields
but cannot recreate, remove, or alter item IDs, handles, locators, snapshots, or
citation hashes. A translation failure leaves the verified English artifact
available rather than manufacturing translated provenance. The verified English
content remains the canonical intelligence item and stays inspectable; translated
text is labelled as a derived presentation and may not change claim kind,
epistemic status, or citation role.

## 6. Resolution and verification states

The resolver has two related modes.

Historical resolution verifies what a derived artifact saw: it resolves the
stored locator against the pinned transcript version and compares the result to
the stored snapshot hash.

Current resolution evaluates whether the same audio interval still resolves to
the same content under the active transcript version. It does not rewrite the
historical citation.

The closed status set is:

- `verified`: the pinned artifact and transcript versions are available, the
  locator resolves, and the snapshot hash matches.
- `superseded`: historical resolution remains verified, but a newer active
  transcript version exists and resolves to the same cited content.
- `evidence_changed`: the same interval resolves under the active transcript
  version, but its normalized content hash differs.
- `source_missing`: the snapshot remains displayable, but the exact source
  bytes are unavailable.
- `artifact_mismatch`: bytes exist at the expected storage location, but their
  version hash differs.
- `version_missing`: a referenced immutable transcript or artifact version is
  unavailable.
- `unresolvable`: bounds are invalid or no eligible transcript segments overlap
  the interval.

Only `verified` and historically verified `superseded` citations prove what a
generation saw. A current-generation hard evidence gate additionally requires
the active transcript version, so it cannot generate new validated output from
superseded evidence.

The UI must distinguish all states. It may show the snapshot for every state,
but must not visually present `source_missing`, `artifact_mismatch`,
`version_missing`, or `unresolvable` as verified.

## 7. Corrections, retranscription, and invalidation

Retranscription and manual correction use one transaction boundary:

1. build and validate the complete new transcript version;
2. insert its immutable version and segments;
3. advance the recording's active transcript pointer;
4. re-resolve citations used by dependent derived artifacts;
5. record an idempotent invalidation for every changed or unresolvable span;
   and
6. update the legacy current-transcript projection if it remains in use.

Any failure rolls back the pointer, projection, and invalidation changes. Audio
and old transcript versions remain untouched.

Invalidation never edits, deletes, or relabels historical derived output. The
output retains the profile version, model binding, transcript version, and
citations used to create it. Its current state becomes `stale` with a reason and
the old/new resolved content hashes. The UI offers explicit regeneration
against the corrected active transcript.

The invalidation operation is idempotent for the tuple of derived artifact,
prior citation digest, and new transcript version hash.

A full retranscription is expected to invalidate most or all citations for the
meeting. A different model commonly changes segment boundaries; because phase 1
anchors honestly snap to complete segments, the resolved text extent and hash
will often change even when the broad meaning is similar. The UI should present
retranscription as creating a new interpretation that normally requires
wholesale regeneration of derived intelligence, not as a harmless text refresh.

## 8. Derived intelligence contract

An intelligence item contains:

- immutable item ID and version;
- one `ClaimKind` from section 1;
- one `EpistemicStatus` from section 1;
- concise displayed content;
- zero or more separately labelled rationale fields;
- one or more citation IDs;
- generation provenance: profile/version, playbook UUID, target, provider/model
  binding, capability hash, transcript version hash, generation-manifest digest,
  successful/planned chunk counts, and generation time; and
- current provenance state (`current` or `stale`) plus invalidation details.

Citation roles are explicit, for example `primary_evidence`,
`context`, or `counterevidence`. A recommendation can cite the meeting facts
that motivate it without implying the recommendation itself was spoken.

Unknown claim kinds, epistemic statuses, or citation roles are rejected. Items
cannot contain actions to execute, tool calls, scripts, code, network requests,
or permissions.

## 9. Evaluation semantics

`evidence_linked` becomes a rule-based hard assertion backed by the real
resolver. Citation punctuation or marker syntax is irrelevant.

For every derived item, it verifies that:

- the structured output references at least one known candidate handle;
- the parser mapped each handle to an application-owned candidate;
- the recording artifact and exact version hash exist;
- every locator is structurally valid and resolves through the shared resolver;
- resolved normalized text exactly matches the snapshot content hash;
- the active transcript version is the evaluated version; and
- no citation is snapshot-only or in an unverifiable state.

Any resolver error, unavailable source, fabricated handle, hash mismatch, or
ambiguous outcome fails closed as `EVAL_INCONCLUSIVE` or the more specific
evidence error. It never defaults to pass.

Eval plans cover the availability errors in section 1.1. Provider unavailability
and budget exhaustion do not count as passing abstentions; they make the run
inconclusive or failed according to the declared hard assertion.

Mechanical integrity does not prove semantic support. A separate
`evidence_support` rubric evaluates whether the cited text actually supports the
claim. Phase 1 may require human review; a later LLM judge must be explicitly
model-bound, versioned, and fail inconclusive. It may not substitute for the
hard integrity assertion.

Eval fixtures create deterministic synthetic recording and transcript versions
and pass them through the same candidate renderer, production generation
function, parser, citation builder, and resolver as real meetings. A
purpose-built simulated evidence path is forbidden.

The capability hash includes stable identities for:

- production generation function;
- evidence candidate builder and renderer;
- evidence chunk planner and generation-manifest schema;
- map, reduction, final-render, and translation schemas;
- structured output parser;
- citation schema and canonicalizer;
- snapshot normalizer and hasher; and
- evidence resolver and assertion implementation.

Changing any identity invalidates prior eval results and requires re-evaluation
before profile activation for the target.

## 10. Storage model

Names below are descriptive; migrations may adapt them to repository naming
conventions without changing the invariants.

### 10.1 Source artifacts

`source_artifacts`

- stable UUID primary key;
- optional meeting ID;
- closed artifact kind;
- creation timestamp; and
- optional logical retirement state.

`source_artifact_versions`

- artifact UUID;
- content/version hash;
- media type and byte length;
- duration in milliseconds where applicable;
- local storage state and non-identity path hint;
- creation timestamp; and
- immutable composite primary key `(artifact_id, version_hash)`.

### 10.2 Transcript versions

`transcript_versions`

- immutable UUID and canonical hash;
- recording artifact UUID and version hash;
- language and transcription capability identity;
- creation timestamp; and
- immutable canonical payload or immutable child segments.

`recording_transcript_heads`

- recording artifact UUID primary key; and
- active transcript version UUID/hash.

The head is mutable only through the version-install transaction. Database
triggers reject in-place mutation of transcript-version content.

### 10.3 Citations and dependencies

`evidence_citations`

- immutable citation UUID and canonical digest;
- artifact UUID and version hash;
- tagged locator payload;
- snapshot text, normalization version, and content hash;
- resolution provenance payload; and
- creation timestamp.

`derived_artifact_citations`

- derived artifact UUID/version;
- citation UUID; and
- closed citation role.

`derived_artifact_invalidations`

- derived artifact UUID/version;
- prior citation digest;
- new transcript version hash;
- closed reason;
- old and new resolved span hashes where available; and
- creation timestamp with an idempotency uniqueness constraint.

All content remains local. The existing documented plaintext-storage deviation
also applies to snapshots, transcript versions, and derived intelligence. A
profile-enabled/evidence-enabled release remains blocked on the database-wide
encryption decision; profile-only or citation-only encryption is insufficient.
Citation snapshots contain verbatim meeting speech and are among the most
sensitive rows in the schema. Every additional plaintext version and snapshot
increases the eventual encryption migration surface, so database-wide encryption
should be scheduled before further evidence-bearing storage layers accumulate,
not treated as indefinite release cleanup.

## 11. Existing-data enrollment

SQL migrations add schema only. They must not hash multi-gigabyte audio files or
perform filesystem work inside a database migration or long transaction.

Existing recordings enroll lazily or through a journaled background operation:

1. locate the candidate audio through current meeting metadata and folder data;
2. record path, size, and modification metadata as non-authoritative hints;
3. hash the full audio bytes outside a database transaction;
4. build a transcript version from the current ordered transcript projection;
5. open a short transaction and recheck the hints;
6. insert the stable artifact, exact version, transcript version, and active
   pointer; and
7. retry without partial publication if the file changed during hashing.

An application-minted recording UUID is persisted in both authoritative
database state and workspace metadata where available. It is never derived from
a path. Enrollment is idempotent and never modifies the source audio.

If no audio exists, enrollment records that condition without inventing a
verified recording version.

## 12. Phase 1 vertical slice

The smallest practical implementation after approval is:

1. add artifact/version and immutable transcript-version storage;
2. enroll one existing or imported recording without changing its bytes;
3. implement integer timeline locators, snapshot hashing, and the shared
   resolver;
4. install new transcript versions transactionally during retranscription and
   record citation invalidations;
5. add deterministic evidence candidates and trusted handles to the existing
   production summary-generation path, including its provenance-preserving
   multi-chunk map/reduce flow;
6. support one manual Expert Profile playbook that generates the closed
   intelligence-item schema for one completed meeting;
7. persist verified citations and display citation chips that open the
   transcript and seek the recording;
8. make `evidence_linked` use the same resolver and fail closed; and
9. display stale evidence and offer explicit regeneration after retranscription
   or correction.

Document retrieval is not implemented in this slice. The locator union and
artifact/version envelope are included now so item 6 adds a resolver variant
rather than redesigning provenance.

## 13. Required tests

At minimum, focused tests must prove:

- retranscription retains recording identity, creates a transcript version, and
  does not mutate the old version;
- replacing all legacy transcript row IDs does not break a citation;
- changed spans invalidate only dependent derived artifacts;
- rerunning invalidation is idempotent;
- failed version installation rolls back the active pointer and projection;
- f64-second conversion, half-open boundaries, overlap, gaps, and the legacy
  rounding tolerance are deterministic;
- audio deletion yields `source_missing` while retaining a tamper-evident
  snapshot;
- replacement bytes at the same path yield `artifact_mismatch`;
- a meeting without audio cannot pass `evidence_linked`;
- fabricated, unknown, and malformed model handles are rejected;
- a long transcript uses one globally unique handle namespace across overlapping
  chunks, reductions, and the final report;
- a cross-chunk item preserves every cited candidate through the evidence
  appendix and resolves normally;
- one failed or omitted planned chunk aborts with
  `EVIDENCE_CHUNK_INCOMPLETE` and persists no partial artifact;
- an over-budget appendix is deterministically repartitioned or fails with
  `EVIDENCE_BUDGET_EXCEEDED`, without dropping a handle or its text;
- the eight-handle item limit is enforced structurally and covered by the
  capability hash;
- legacy cached Markdown cannot bypass evidence generation;
- translation cannot alter structured provenance or citation hashes;
- citation-looking text without a resolved citation fails the assertion;
- snapshot and citation hashing reject unsafe integers and remain stable across
  Rust/TypeScript fixtures;
- production and eval use the identical candidate, parser, builder, and resolver
  path, with those identities covered by the capability hash;
- the dormant `document_passage` variant round-trips without being executable;
  and
- pagination, search, and UI rendering do not alter resolver results.

Repository tests use temporary databases and synthetic local artifact files.
No test requires a cloud provider. The existing reference-laptop smoke-test gap
for real recording and transcription inference remains explicit.

## 14. Security and privacy properties

- Audio, transcript versions, snapshots, and derived intelligence remain local
  unless the user explicitly selects a configured remote generation provider.
- Prompts send only the selected transcript evidence required for that explicit
  generation, under the provider disclosure already required by Expert
  Profiles.
- Transcript text and retrieved documents are untrusted evidence, never
  instructions.
- The model selects opaque handles; it cannot create trusted locators or hashes.
- Schemas reject unknown fields and contain no executable capability.
- Citation resolution performs no network access and executes no content.
- Import/export, if added, verifies schema and digests before persistence and
  never carries secrets.
- The UI must disclose unverifiable and stale states instead of concealing them.

## 15. Resolved design decisions

The following alternatives are rejected:

1. **Transcript row IDs as anchors.** Retranscription deletes them, so they are
   resolution provenance only.
2. **Transcript text offsets as primary anchors.** Edits and retranscription
   move them; audio-time coordinates survive both.
3. **Paths or filenames as artifact identity.** They are mutable location hints,
   not content or logical identity.
4. **Meeting ID as recording identity.** A meeting may have no recording or more
   than one source over its lifetime.
5. **Model-authored coordinates, IDs, or hashes.** Models may select only
   application-created opaque handles.
6. **Citation markers as proof of grounding.** `evidence_linked` resolves and
   hash-checks real citations.
7. **Silently rewriting historical citations after correction.** Old provenance
   remains immutable and dependent output becomes stale.
8. **Treating a snapshot as verified when its source is absent.** It remains
   displayable and tamper-evident but explicitly unverifiable.
9. **A separate evaluation resolver.** Production, invalidation, UI inspection,
   and evals share one resolver implementation.
10. **Word-level claims from segment-level timing.** Phase 1 anchors expand to
    complete segments until genuine word timing exists.
11. **Hashing audio during SQL migration or startup transaction.** Enrollment is
    journaled, retryable filesystem work with short publication transactions.
12. **Automatic regeneration after evidence changes.** The user sees the stale
    state and explicitly regenerates, preserving historical output.
13. **Chunk-local handle namespaces or free-form reduction summaries.** Long
    meetings use one generation-global candidate index and every intermediate
    preserves validated handles.
14. **Silently accepting partial chunk success.** Evidence-qualified output
    fails closed because omitted transcript regions make completeness unknowable.
15. **Truncating an evidence appendix to fit model context.** The planner
    repartitions deterministically or fails with `EVIDENCE_BUDGET_EXCEEDED`;
    reducers always receive the full text for every handle they may cite.

## 16. Deferred decisions with recommended defaults

1. **Retention of unreferenced transcript versions.** Retain all referenced
   versions; introduce a conservative age/space policy only for unreferenced
   versions after usage is measured.
2. **Multiple recordings per meeting.** Permit them in schema; expose one primary
   recording in phase 1.
3. **Speaker identity.** Treat current speaker/source labels as versioned display
   data, never anchor identity. Defer person identity and diarization.
4. **Semantic evidence support.** Start with explicit human review for activation
   fixtures; add a pinned, evaluated judge only when its error rate is measured.
5. **Audio removal policy.** Never block or secretly reverse a user's deletion;
   preserve snapshots and report `source_missing`.
6. **Candidate selection size.** Begin with complete ordered segments and bounded
   deterministic windows. Retrieval/ranking requires its own evaluated change.
7. **Document extraction canon.** Define canonical PDF/text extraction and
   revision hashing before enabling `document_passage`; the locator shape alone
   does not authorize ingestion.
8. **Live intelligence.** Defer until recording, partial-transcript stability,
   latency, and interruption behavior have dedicated tests and product rules.

## 17. Non-goals

This contract does not add:

- word-level timestamps;
- speaker/person identification;
- live or autonomous coaching;
- automatic tool use or actions;
- document ingestion, embeddings, or retrieval;
- remote synchronization;
- filesystem retention enforcement;
- an updater or release mechanism; or
- a substitute for database-wide encryption.
