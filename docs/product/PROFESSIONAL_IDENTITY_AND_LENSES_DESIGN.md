# Professional Identity Profiles and Meeting Lenses

Status: approved product contract; implementation is incremental.

## 1. Product outcome

For a live meeting, the user explicitly selects:

1. one versioned **Professional Identity Profile** describing who the user is and what is currently true about their work; and
2. one versioned **Meeting Lens** describing how to answer in the present meeting.

The generated response is written entirely in the first person as the user's ready-to-speak answer. The model is not given tools and cannot execute profile content, change application state, or acquire authority.

The first specialized response contract is one continuously streamed plain-text paragraph of 200–300 words. The first two sentences provide a complete 40–70-word lead, and the remainder expands the same answer naturally. The paragraph contains no headings, lists, Markdown, coaching instructions, or assistant meta-language.

General Guidance remains a separate short mode. Its existing optional detail request remains available. Specialized responses hide and reject that legacy detail request.

## 2. Separation of responsibilities

### 2.1 Professional Identity Profile: who is answering

Identity content may include:

- CV and professional background;
- current role and organization;
- TORs, responsibilities, and accountabilities;
- decision rights, approval limits, and explicit non-authorities;
- reporting lines and stakeholders;
- active projects, workstreams, status, owners, deadlines, and risks;
- confirmed commitments and negotiation boundaries;
- recurring operating practices, such as staff check-ins or escalation routes; and
- dated source records and freshness metadata.

Identity content is factual context. It does not contain response formatting, meeting tactics, executable instructions, or provider configuration.

### 2.2 Meeting Lens: how to answer

A lens may define:

- meeting purpose and desired outcome;
- perspective and priorities;
- tone and level of formality;
- response length and form;
- relevant boundaries and abstention behavior;
- meeting-specific playbook guidance; and
- which identity source categories may be retrieved.

The existing phase-one Expert Profile and embedded Meeting Playbook implementation is the current lens layer. It must not be relabeled as professional identity. A later migration may rename it without changing its content identity or provenance.

## 3. Declarative identity schema

The first identity schema is closed and versioned. Unknown fields are rejected. No field can name a script, command, library, tool, permission, filesystem path, provider endpoint, or executable hook.

```json
{
  "schema_version": 1,
  "identity": {
    "display_name": "Ghassan Aqrabawi",
    "role_title": "Head of Mission",
    "organization": "Example Mission",
    "professional_summary": "..."
  },
  "records": [
    {
      "id": "<uuid>",
      "category": "terms_of_reference",
      "title": "Duty of care responsibility",
      "content": "...",
      "source": {
        "label": "Head of Mission TOR",
        "revision": "2026-08"
      },
      "updated_at": "2026-08-12T00:00:00Z",
      "valid_until": null,
      "conflict_key": null,
      "tags": ["staff", "duty-of-care"]
    }
  ],
  "projects": [
    {
      "id": "<uuid>",
      "name": "Project Atlas",
      "role": "Executive sponsor",
      "status": "On track",
      "source": {
        "label": "Atlas portfolio record",
        "revision": "2026-W33"
      },
      "updated_at": "2026-08-12T00:00:00Z",
      "valid_until": null,
      "tags": ["delivery", "atlas"],
      "facts": [
        {
          "id": "<uuid>",
          "content": "...",
          "source": {
            "label": "Atlas weekly status",
            "revision": "2026-W33"
          },
          "conflict_key": null,
          "tags": ["delivery-date"]
        }
      ]
    }
  ]
}
```

The concrete Rust model may normalize repeated source fields into a shared type, but its serialized form remains closed, deterministic, and reviewable.

## 4. Versioning and session pinning

- Each identity has a stable UUID.
- Each immutable version is identified by RFC 8785 canonical JSON and a domain-separated SHA-256 content hash.
- Editing creates a new version; stored versions cannot be updated in place.
- A Live Assist exchange records the identity UUID, identity version hash, lens/profile UUID, lens version hash, and playbook UUID used when capture began.
- Navigation or later selection changes cannot retarget an in-flight exchange.
- Changing the selected identity version, lens version, or playbook starts a fresh follow-up context; exchanges created under the previous selection cannot silently seed the new one.
- Existing answers retain their original provenance and are never relabeled.

## 5. Retrieval and grounding

Sending an entire CV, TOR, and project portfolio on every provider request is not the target architecture. Each request includes:

1. the compact identity header;
2. lens-authorized identity records selected deterministically for the captured question and current follow-up context; and
3. the selected lens/playbook context.

The first retrieval implementation may use deterministic lexical matching over the small local corpus. Embeddings are not required to make the first version useful. Retrieval behavior is versioned and testable.

Each retrieved record keeps its identity record ID, source label, source revision, and freshness fields. The overlay passively shows what grounded the answer, for example:

`Grounded in: Head of Mission TOR · Atlas weekly status (updated Aug 12)`

This line is generated from local retrieval metadata, never from model-authored text and never from another provider call.

If no identity record was retrieved, the UI says `No profile source used`; it does not imply grounding merely because an identity or lens was selected.

## 6. Freshness and conflicts

- Records past `valid_until` are excluded from generation.
- Project data always carries `updated_at`; the UI surfaces its age.
- Records or project facts may share an explicit `conflict_key`. When more than one current item has the same key, any retrieved member is marked `conflicting_current_sources: true`, and the answer must abstain rather than silently choose one. The app does not guess semantic conflicts.
- Exact dates, deadlines, monetary amounts, and approval commitments may be asserted only when supported by a retrieved current record.
- Missing information produces natural first-person uncertainty rather than invention.

## 7. Privacy boundary

Professional identity context is more sensitive than a captured question. Cloud transmission remains explicit and visible. The request record identifies which local source records were sent, without logging their prose.

Private exchanges never enter later cloud context. Changing cloud state starts a new context generation. Identity data contains no provider credentials.

## 8. Specialized response validation

For the first specialized lens contract, deterministic production validation requires:

- provider completion with the normal stop reason;
- one non-empty paragraph after harmless whitespace is collapsed;
- a target of 200–300 whitespace-delimited words, with the observed count and any drift recorded as telemetry rather than treated as an unsafe answer;
- the exact safe abstention `I need more context before I can answer that.` is a non-answer exception to the word range;
- first-person language;
- no coaching prefix or assistant meta-language; and
- no heading or list marker on any source line.

The prompt requires the first two sentences to total 40–70 words and form a complete lead. Semantic completeness is measured through the credentialed provider harness and real meeting review rather than claimed as a deterministic code assertion.

Unsafe or incomplete primary output is discarded rather than left visible as a partial answer. Cosmetic whitespace is normalized, and a completed answer outside the target word range remains usable with a recorded format warning.

## 9. Detail and grounding controls

- General Guidance keeps its existing optional `More detail` / `Refresh detail` path.
- Specialized lenses hide that affordance, and the backend rejects requests for it.
- The detail backend remains temporarily for General Guidance and for evidence from trials.
- Source/freshness grounding is passive and local; it is not a generation button.
- No replacement control is added until use demonstrates a need. A future concise mode, source inspector, continuation, regenerate action, or background view must be named for its actual function.

## 10. Evaluation

Deterministic tests cover schema closure, hash stability, immutability, selection pinning, retrieval limits, stale exclusion, grounding provenance, response shape, word limits, and no-execution boundaries.

Credentialed reference-PC tests record provider/model, prompt-template hash, identity/lens hashes, retrieved record IDs, output, first-token latency, completion latency, and timestamp. They evaluate first-person voice, lead usefulness, non-invention, and continuity as one paragraph.

Real meeting review records whether the answer was used and, when inadequate, one line of missing context. Those notes determine which identity fields and retrieval priorities are implemented next.

## 11. Implementation sequence

1. Keep PR #11 as the validated General Guidance prototype.
2. Add the specialized lens response contract and hide/reject detail for selected specialized lenses.
3. Add versioned Professional Identity Profile storage and a local editor/import path.
4. Add explicit identity selection, pinning, deterministic retrieval, and passive grounding metadata.
5. Add the first real Head of Mission identity content from user-supplied CV, TORs, and current project records.
6. Run focused provider fixtures and real meeting trials before generalizing formats or adding controls.

## 12. Resolved decisions

- Professional identity and meeting lens are separate; neither is inferred from the other.
- Existing Expert Profiles are the lens layer, not the user's biography or authority.
- Specialized output is one continuous first-person 200–300-word paragraph, not bullets or a briefing document.
- The lead and expansion are one streamed response and one provider call.
- `Refresh detail` is hidden and rejected for specialized responses but retained for General Guidance.
- Grounding is passive source metadata from local retrieval, not model-authored text or a second provider call.
- Prompting and retrieval precede fine-tuning.
- Profiles and lenses remain inert declarative data with no tools or scripts.
