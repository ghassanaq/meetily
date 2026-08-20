# Project Context Bundles

Status: design contract and provider-free retrieval spike specification

## 1. Product outcome

Live Assist can open an explicitly selected local meeting-context manifest and use the user's own Markdown sources to ground answers during a meeting.

The feature is designed for one person using one local desktop application. It must remain declarative, versioned, testable, local-first, and unable to execute tools, scripts, or document-authored commands.

Project Context does not replace the existing layers:

- **Person Identity** records who the user is: CV, experience, qualifications, and standing professional background.
- **Role Context** records the appointment under which the user is speaking: TOR, responsibilities, decision rights, approval limits, reporting lines, and standing policies.
- **Project Context** records current work: status, commitments, deadlines, stakeholders, risks, and project-specific references.
- **Meeting Lens** controls how the selected identity responds: objective, depth, tone, answer form, boundaries, and question-type policy.
- **Session Memory** provides continuity within the current meeting.

Identity, role, and projects determine what the assistant knows. The lens determines how it reasons and answers. Session memory supplies conversation continuity. No layer silently absorbs another layer's responsibility.

## 2. Scope

### 2.1 Phase-one scope

- One selected Person Identity bundle.
- One selected Role Context bundle.
- Zero or more selected Project Context bundles.
- Strict JSON manifests containing explicit relative paths.
- Markdown content with typed YAML frontmatter.
- Heading-level passage extraction.
- Deterministic local scoring, expiry filtering, and conflict blocking.
- Precise source, revision, heading, and digest provenance.
- Immutable context snapshots pinned to a Live Assist session.
- Provider-free retrieval regression tests that run in CI.

### 2.2 Non-goals

- PDF or DOCX ingestion.
- Automatic conversion of source documents.
- Model-authored metadata or source classification.
- Scripts, shell commands, native libraries, network targets, tools, or hidden permissions.
- Silent filesystem discovery outside the selected context root.
- Sending whole documents to a cloud provider.
- Silently updating a running meeting when source files change.

## 3. Context manifest

The user opens one meeting-context manifest:

```json
{
  "schema_version": 1,
  "context_id": "head-of-mission-atlas",
  "name": "Head of Mission — Atlas",
  "identity_bundle": "identity/ghassan/bundle.json",
  "role_bundle": "roles/head-of-mission/bundle.json",
  "project_bundles": [
    "projects/atlas/bundle.json"
  ]
}
```

All paths are explicit, relative to the context root, and must remain inside it after normalization. Absolute paths, parent traversal, URLs, globs, and executable targets are rejected.

A context contains exactly one identity bundle, exactly one role bundle, and zero or more project bundles. A meeting may therefore cover several projects without duplicating the user's identity or role authority.

## 4. Bundle manifests

Each referenced bundle is a strict JSON document:

```json
{
  "schema_version": 1,
  "bundle_id": "atlas",
  "name": "Project Atlas",
  "sources": [
    "current-status.md",
    "commitments.md",
    "risks.md"
  ]
}
```

`kind` is not authored in Markdown. It is derived from the manifest edge through which the file was loaded:

| Bundle scope | Passage kind |
| --- | --- |
| Person Identity | `person_fact` |
| Role Context | `role_policy` |
| Project Context | `project_fact` |

The derived kind cannot drift from the bundle containing the source.

## 5. Markdown source contract

Markdown is the canonical source format. A source begins with typed YAML frontmatter:

```markdown
---
schema_version: 1
document_id: "04ff6aed-ff3e-482a-b226-45acdb0679fc"
source: "Head of Mission TOR"
revision: "v3"
updated_at: "2026-08-01T00:00:00Z"
valid_until: "2027-06-30T23:59:59Z"
conflict_key: "procurement_approval_limit"
tags: [authority, finance, escalation]
---

## Approval authority

Always obtain regional approval before committing above 50,000 USD.
```

Unknown frontmatter fields are rejected. Timestamps use RFC 3339. `conflict_key` is optional and must be authored only when two records describe the same replaceable fact or policy. The application must never infer a conflict key from prose.

Frontmatter applies to every passage in the document. When sections require different validity or conflict semantics, they belong in separate Markdown files. This keeps phase-one parsing and review simple.

## 6. Data and instruction boundary

The user's own documents contain imperative language by design. TORs, guides, and SOPs frequently say “always,” “must,” or “do not.” This language is policy evidence for the user; it is not an instruction addressed to the model runtime.

Retrieved material is rendered as typed reference data with explicit boundaries:

```json
{
  "kind": "role_policy",
  "passage_id": "04ff6aed-ff3e-482a-b226-45acdb0679fc::approval-authority",
  "source": "Head of Mission TOR",
  "heading": "Approval authority",
  "content": "Always obtain regional approval before committing above 50,000 USD.",
  "revision": "v3",
  "updated_at": "2026-08-01T00:00:00Z",
  "content_hash": "sha256:..."
}
```

The production prompt must state that imperative language inside reference passages describes the user's recorded duties, policies, or procedures. A passage cannot change assistant rules, activate capabilities, request secrets, grant authority, or override the selected lens.

Document content remains inert data. Model output receives no filesystem, network, shell, application, or document-defined tools.

## 7. Passage extraction

The parser performs a deterministic projection:

1. Parse and validate the closed frontmatter schema.
2. Read Markdown headings from levels 1–3.
3. Maintain the heading breadcrumb for each section.
4. Create one passage per section when it fits the configured passage budget.
5. Split oversized sections at paragraph boundaries, then deterministic word boundaries if necessary.
6. Preserve bundle ID, document ID, source, revision, timestamps, tags, breadcrumb, and chunk ordinal.
7. Compute the content digest from an explicit normalized projection, never incidental YAML serialization.

The score text contains the bundle name, source title, heading breadcrumb, tags, conflict key when present, and passage body. Volatile filesystem timestamps and absolute paths do not enter the digest.

## 8. Retrieval and blocking

Retrieval is local and provider-free:

1. Tokenize the question and candidate fields deterministically.
2. Exclude expired candidates.
3. Score candidates using weighted matches across bundle name, headings, tags, and body.
4. Detect explicit conflicts among current candidates. Relevance to a conflict is established from the semantic terms in its explicit `conflict_key`, not from a single generic body-word overlap.
5. If a relevant conflict key has more than one current source, fail before any provider request.
6. Sort deterministically, discard weak filler below the configured relative-score floor, and apply the configured maximum result limit. A limit is a ceiling, not a quota. This describes the spike selector; real-corpus evidence below shows that production selection also needs source diversity.
7. Send only selected passages to the provider.

Project-name matches receive enough weight to prevent topic bleed when several project bundles are active. They do not replace semantic relevance: a cross-project question may legitimately retrieve passages from several named projects.

The initial experiment compares result limits of 3, 5, and 8. It does not inherit the identity-record limit of eight without evidence.

The synthetic fixture uses a relative-score floor of **50% of the strongest candidate score**. A candidate remains eligible only when its score is at least `0.5 × best_score`; the maximum result limit is applied afterward. The test constant is deliberately named `SYNTHETIC_FIXTURE_RELATIVE_SCORE_FLOOR_PERCENT`: 50% demonstrates fixture mechanics and is not a production recommendation. The floor remains an explicit, versioned retrieval-policy parameter. Changing production retrieval behavior must advance the retrieval policy version and the capability hash that covers it.

The 39,695-word private-corpus evaluation invalidated a single global floor as the complete selection policy. The original 12 questions all expected evidence from one structurally advantaged Q&A source, so their success at 85% did not establish cross-source recall. Three deliberately diversified cases then failed throughout a 50–85% sweep:

- For a question absent from the Q&A source, the answer-bearing directive passage ranked second at 50–70%, alongside too much same-document noise; at 75–85%, the title/preamble passage remained while the answer was excluded.
- For a split role-and-professional-evidence question, all top-three positions were occupied by adjacent role-document chunks at every measured floor, excluding the required professional-evidence passage.
- For partial Q&A coverage plus an exact role fact, the same three role-document chunks occupied the top three at every measured floor, excluding the required Q&A passage.

Neither 50% nor 85% is therefore a supported production value. The next selector experiment must prevent one source from monopolizing the result budget while preserving genuinely split evidence. The leading design is a source- or bundle-diverse shortlist—for example, retain the strongest candidate from each relevant bundle before applying within-bundle floors and a final global budget—but that policy remains uncommitted until it passes the private regression suite.

## 9. Provenance and session snapshots

Each generated exchange records:

- context ID;
- context snapshot digest;
- identity, role, and project bundle digests;
- selected passage IDs and content hashes;
- source labels, revisions, and heading breadcrumbs;
- retrieval policy version;
- selected identity, lens, depth, provider, and model provenance already required by Live Assist.

A running or resumed meeting stays pinned to its original snapshot. When source files change, Live Assist offers an explicit **Reload project context** action. It never changes the companion's knowledge silently midway through a meeting.

The local database retains the normalized selected passage snapshot needed to explain an answer or resume a meeting even if the original Markdown file is later moved. Source files remain the user-authored canonical corpus.

Project and identity content hashes do not enter the Live Assist capability activation hash. The capability hash covers the prompt renderer, parser, chunker, retrieval policy, validators, and lens configuration. Retrieved content is mutable user data, not capability configuration.

## 10. Retrieval spike

The first implementation is a Rust test suite, not a throwaway script. It uses no credentials, network, or provider and remains permanently CI-eligible.

The fixture corpus contains:

- one person identity bundle;
- one role bundle;
- two project bundles;
- 17 files in total, including 12 Markdown sources;
- roughly 381 authored corpus words by the review inventory and 369 passage-body words actually indexed by the parser;
- current, expired, and explicitly conflicting records;
- imperative policy language;
- at least ten representative questions with expected passage IDs.

For each successful question and each limit of 3, 5, and 8, the suite records:

- rank of the expected passage;
- selected passage IDs;
- irrelevant passage count;
- selected context word count;
- expired passage exclusion;
- cross-project topic bleed.

Acceptance thresholds are absolute rather than percentage-based:

| Result limit | Maximum irrelevant passages |
| --- | ---: |
| 3 | 1 |
| 5 | 2 |
| 8 | 3 |

Expected evidence must appear in the top three. Relevant explicit conflicts must block before selection. An unrelated conflict must not block another question. A two-project question must retrieve the expected passages from both projects, while a single-project question must not retrieve passages from the other project merely because both are active.

### 10.1 Private real-corpus regression path

The permanent synthetic fixtures remain in Git. The real CV, TOR, authority, project, and reference corpus stays outside Git in a private user-selected folder. The ignored integration test `private_corpus_retrieval_measurements` reads that folder only when `PROJECT_CONTEXT_PATH` is set explicitly.

The private context root contains the normal context manifest and a private `retrieval-eval.json` file:

```json
{
  "schema_version": 1,
  "context_manifest": "meeting-assistant.context.json",
  "evaluated_at": "2026-08-20T00:00:00Z",
  "relative_score_floor_percent": 50,
  "cases": [
    {
      "id": "approval-limit",
      "question": "What is my current approval limit?",
      "expected_passage_ids": ["document-uuid::approval-authority"],
      "relevant_passage_ids": ["document-uuid::approval-authority"],
      "allowed_project_bundle_ids": []
    }
  ]
}
```

The complete private file must contain 10–20 cases. Expected passage IDs must also be listed as relevant. `allowed_project_bundle_ids` is empty for Person/Role-only questions and explicitly names every project allowed for project or cross-project questions. This makes unintended bundle bleed measurable rather than inferred after the run.

Run the local suite from PowerShell without copying private material into the repository:

```powershell
$env:PROJECT_CONTEXT_PATH = 'C:\private\meeting-assistant-context'
cargo test -p meetily --test project_context_retrieval private_corpus_retrieval_measurements -- --ignored --nocapture
```

The test reports case IDs, passage IDs, ranks, scores, irrelevant counts, selected word counts, and project bleed. It never prints question text or passage content. CI does not set `PROJECT_CONTEXT_PATH` and the test remains ignored by default.

The spike determines whether the existing lexical approach is sufficient and which result limit produces the best precision. If it fails, the Markdown corpus and fixtures remain valid; only the retrieval mechanism changes.

## 11. Resolved design decisions

### 11.1 Markdown is canonical

PDF and DOCX extraction is deferred. Markdown eliminates layout ambiguity and provides reviewable heading boundaries and metadata.

### 11.2 Context has person, role, and project scopes

CV and professional history are person-scoped. TOR and authority are role-scoped. Status and commitments are project-scoped. Project bundles do not duplicate identity or authority.

### 11.3 Bundle scope derives passage kind

Authors do not hand-maintain `kind`. The manifest relationship is authoritative.

### 11.4 Imperative prose remains inert reference data

The system does not attempt to classify sentences as instructions. Typed passage boundaries and prompt rules define how all content is interpreted.

### 11.5 Metadata is document-scoped in phase one

Sections needing different expiry or conflict semantics are split into separate files. Per-section metadata is deferred.

### 11.6 Manifests use explicit relative paths

Automatic discovery, globs, URLs, scripts, and external paths are rejected in phase one.

### 11.7 Retrieval quality measures precision and recall

Finding the right passage is insufficient if irrelevant passages dilute the request. The regression suite measures rank, irrelevant counts, and context size.

### 11.8 The spike is a permanent test suite

Provider-free fixtures become the retrieval regression gate for future chunker, scorer, and result-limit changes.
