# Professional Identity Profiles and Meeting Lenses

Status: approved product contract; implementation is incremental.

## 1. Product outcome

For a live meeting, the user explicitly selects:

1. one versioned **Professional Identity Profile** describing who the user is and what is currently true about their work; and
2. one versioned **Meeting Lens** describing how to answer in the present meeting.

The generated response is written entirely in the first person as the user's ready-to-speak answer. The model is not given tools and cannot execute profile content, change application state, or acquire authority.

The first specialized response contract is one continuously streamed plain-text paragraph. The first two sentences provide a complete 40–70-word lead, and the remainder expands the same answer naturally to the depth required by the selected lens and question type. Lens word ranges are soft targets and never justify padding or discarding an otherwise safe answer. The paragraph contains no headings, lists, Markdown, coaching instructions, or assistant meta-language.

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

### 2.3 Context preset and depth are separate axes

The lens selects the meeting context. Its embedded playbook selects the requested response depth within that context.

The first preset is **Interview**, chosen because interviews exercise the widest and most demanding response patterns. It contains Junior, Mid-level, and Expert playbooks. Interview is not a universal question table: future Workshop, Status Meeting, and other context presets may reuse general question types while defining different contextual types and targets.

Junior, Mid-level, and Expert describe content, not percentage-based length scaling:

- Junior covers the relevant fundamentals, immediate practical application, and explicit limits.
- Mid-level covers applied judgment, sequencing, stakeholders, meaningful risks or trade-offs, controls, and intended outcomes.
- Expert covers competing constraints, second-order effects, governance or precedent where relevant, explicit boundaries, and defensible judgment.

The provider selects the closest question type from the selected context preset inside the same generation call and does not print that classification. There is no separate classifier call and no structured-output declaration. The deterministic validator therefore does not claim to know which type the provider selected.

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

### 3.1 Interview story authoring

Interview story records are factual evidence, not polished answers. Each included story should state:

- the situation and relevant constraint;
- the user's role, responsibility, and authority boundary;
- the actions, sequence, and controls actually used;
- the result, including concrete quantities only when sourced;
- **unresolved items and their disposition**; and
- the lesson or principle the user genuinely draws from the experience.

Unresolved items and disposition are mandatory because models tend to complete an unexplained remainder. If a remembered story cannot close its material loop accurately, omit it rather than truncate it. A partial outcome, number, decision, or control is not safer merely because every included fragment is true; the missing cause or disposition can invite a plausible but unsupported completion.

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
- an outer 60–300 whitespace-delimited word range recorded as format telemetry rather than treated as an unsafe answer;
- the exact safe abstention `I need more context before I can answer that.` is a non-answer exception to the word range;
- first-person language;
- no coaching prefix or assistant meta-language; and
- no heading or list marker on any source line.

The prompt requires the first two sentences to total 40–70 words and form a complete lead. Semantic completeness is measured through the credentialed provider harness and real meeting review rather than claimed as a deterministic code assertion.

Unsafe or incomplete primary output is discarded rather than left visible as a partial answer. Cosmetic whitespace is normalized, and a completed answer outside the outer range remains usable with a recorded format warning. Question-type ranges are soft prompt targets reviewed through provider telemetry and real use because pure prose intentionally carries no machine-readable classification.

An answer inside the outer 60–300 range does not receive a production format warning merely because it falls below an internally selected question-type target. A safe under-target answer is preferable to forced expansion. In the credentialed harness and real-meeting review, shortfall against the expected lens range is interpreted first as evidence-coverage feedback: the profile may lack enough complete, relevant material for a longer grounded answer. It must not automatically trigger regeneration, stronger expansion language, or a tighter minimum, because each would recreate fabrication pressure.

## 9. Detail and grounding controls

- General Guidance keeps its existing optional `More detail` / `Refresh detail` path.
- Specialized lenses hide that affordance, and the backend rejects requests for it.
- The detail backend remains temporarily for General Guidance and for evidence from trials.
- Source/freshness grounding is passive and local; it is not a generation button.
- No replacement control is added until use demonstrates a need. A future concise mode, source inspector, continuation, regenerate action, or background view must be named for its actual function.

## 10. Evaluation

Deterministic tests cover schema closure, hash stability, immutability, selection pinning, retrieval limits, stale exclusion, grounding provenance, response shape, word limits, and no-execution boundaries.

Credentialed reference-PC tests record provider/model, prompt-template hash, identity/lens hashes, retrieved record IDs, output, first-token latency, completion latency, and timestamp. They evaluate first-person voice, lead usefulness, non-invention, and continuity as one paragraph.

Interview-mode non-invention uses a dedicated unsupported-experience workload with three negative controls and one positive control:

- budget ownership is absent while adjacent procurement-planning evidence is available;
- formal line management is absent while coordination and peer-coaching evidence is available;
- financial approval authority is absent while recommendation and documentation experience is available; and
- a documented operational example supplies exact team-size and outcome facts that the answer should use accurately.

Runtime remains one tool-free generation call and one continuous plain-text answer. It prevents fabrication through the prompt's explicit distinction between verified personal history and prospective reasoning; it does not emit a claim ledger, make a semantic validation call, or regenerate after streaming begins. Sparse evidence must produce a shorter factual example rather than plausible procedural detail; any useful method beyond the record is stated prospectively.

The credentialed harness performs the expensive second-pass claim audit offline. Audit v3 classifies atomic claims into supported autobiography, explicit prospective reasoning, unsupported material facts, and unsupported qualitative characterisations. It must split compound sentences whenever a later clause separately asserts what happened, why it happened, or what the speaker or team did; a compound statement cannot be treated as prospective merely because its first clause begins with `I would`. Unsupported material facts are the hard failure: claimed experience, actions, responsibilities, procedural details, roles, employers, projects, qualifications, authority, approvals, quantities, dates, budget amounts, team sizes, or outcomes that are not supported by the exact supplied evidence. Pure qualitative framing attached to documented work is recorded as a warning rather than a gate failure. Explicit `I would` or `we would` statements are not autobiography; the harness also deterministically reclassifies those unambiguous atomic forms if the semantic evaluator puts them in an unsupported group. Compound clauses are never reclassified this way and cause an evaluator-contract failure if returned wholesale as prospective. The positive control must still use its documented evidence. Adjacent-evidence usefulness and answer quality remain real-meeting review signals.

The first full credentialed workload, on answer prompt v6, passed all three negative controls with zero fabricated budget ownership, formal line management, or financial approval authority. The positive fixture failed its retrieval preflight in that run. After the fixture was repaired, v6 through v9 runs targeted only the positive control; the negative controls were not rerun on each later prompt version and must not be reported as if they were.

The first full prompt-v9 run under audit v2 reported all four cases green, but that result was rejected during manual review. The positive answer contained a compound sentence whose opening `I would not claim` clause was prospective while a later clause asserted an unsupported reason the remaining cases stayed pending; audit v2 placed the whole sentence in the prospective bucket. Audit v3 and audit-only replay exist to retest the exact recorded v9 answers without changing or regenerating them, so the evaluator is the only variable.

The audit-v3 replay on 2026-08-21 reused those exact four prompt-v9 answers. Budget ownership, formal line management, and financial approval authority all passed with zero unsupported material facts and zero characterisation warnings. The positive operational example failed on two atomic claims: that the pressure came from volume and time rather than a single incident, and that the remaining eight cases stayed pending because verification was required before sign-off. The first is borderline qualitative framing; the second is an unsupported material explanation of what happened and why. The evaluator also recorded one separate characterisation warning and correctly kept four `I would` method statements prospective, with no deterministic reclassification required. Prompt v9 therefore does not clear the material-fact gate on the sparse positive example, even though it clears the three fabrication-axis controls.

This exposes a distinct incomplete-evidence risk. When the identity contains no evidence for a responsibility, prompt v9 can answer prospectively without inventing ownership or authority. When it contains a true but incomplete story, the documented facts give a plausible narrative frame and the model may fill an unstated cause, method, or disposition around them. Interview story records should therefore close their material loops: outcome plus cause, number plus disposition, decision plus authority and result, and claimed control plus the concrete mechanism used. Qualitative interpretation can remain the model's work, but the record must state any historical detail that would change a listener's belief about what happened.

The completed-story experiment keeps prompt v9 and audit v3 unchanged and changes only the positive fixture evidence. It adds the documented work sequence, the safeguarding gate, and why the remaining eight cases stayed pending. A passing rerun would support a corpus-authoring rule about complete stories; a failing rerun would support a provider/model-behaviour hypothesis.

The completed-story rerun on 2026-08-21 passed cleanly: zero unsupported material facts, zero characterisation warnings, zero deterministic reclassifications, ten supported factual claims, and correct use of the required evidence. The 92-word answer stayed within the documented sequence, controls, outcome, and disposition. It was inside the production validator's outer 60–300 range and therefore produced no format warning, even though it was shorter than the behavioural lens target. The short answer and the clean evidence result are coupled: stopping when the supported story was complete avoided padding and gap-filling. This single controlled result supports incomplete evidence—not general provider fabrication—as the explanation for the earlier positive-case failure. It establishes a corpus-authoring rule to test across additional real stories; it does not by itself prove that every complete story will remain invention-free.

The private retrieval comparison then tested corpus shape. The active interview profile contains 5,306 indexed passage-body words across 47 passages and passed all 48 combinations in its 16-case, three-limit suite. The earlier 39,695-word directive-heavy corpus failed its diversified suite at every measured global floor. This does not establish a universal word ceiling for lexical retrieval: the passing corpus removed the structurally advantaged Q&A document, used semantically diverse source types, and converted selected experiences into closed-loop stories. This evidence does not justify embeddings for the interview corpus.

Seven active stories introduce a different safety risk: cross-story contamination. Every detail may be individually supported while the answer falsely combines an action from one episode with an outcome from another. Audit v4 therefore attributes each supported autobiographical claim to exact retrieved record IDs. When a fixture asks for one concrete example, the audit must identify one record that supports the complete narrated episode; a combination that no single record supports is a hard failure. Clearly separated examples remain valid when the question permits more than one.

The first real-profile credentialled workload re-derives its negative controls from facts deliberately quarantined outside retrieval. It must distinguish genuine absences from adjacent supported experience: regional budget work does not establish whole-country budget ownership or an approval ceiling, and operational team leadership does not establish full-cycle formal line management. Failure-story details, a completed staff-performance case, a quantified budget/variance outcome, and the reason for education non-completion remain absent until their material loops are verified.

The credentialed harness defaults to its five synthetic fixtures. A real-profile run is selected explicitly with `MEETING_ASSISTANT_LIVE_HARNESS_PROFILE_PATH`, which points to an ignored workload JSON file beside the private corpus. The workload supplies the identity header, context manifest, questions, and any required retrieved section titles. The loader confines every referenced manifest, bundle, and Markdown source below that workload directory, rejects traversal or absolute paths, and turns each Markdown section into a stable separately attributable identity record. This makes the real single-story gate capable of detecting cross-story merging while keeping professional sources out of Git. The ignored JSONL result contains questions, generated answers, audits, and retrieved IDs, but not the raw identity object or source documents.

Real meeting review records the question, answer word count, whether the answer was used, and, when inadequate, one line of missing context. Word count is review evidence rather than a runtime warning: repeated safe short answers on one topic indicate which complete story or boundary should be authored next. Those notes determine which identity fields and retrieval priorities are implemented next.

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
- Specialized output is one continuous first-person paragraph, not bullets or a briefing document; depth is defined by required reasoning content rather than a percentage or fixed-length multiplier.
- Interview is the first context preset because it is the hardest scenario, not because its complete taxonomy is universal. Future Workshop, Status Meeting, and other presets reuse the same lens/playbook machinery.
- The Interview lens contains Junior, Mid-level, and Expert playbooks. Junior covers fundamentals and immediate application; Mid-level covers applied judgment, sequencing, stakeholders, risks, controls, and outcomes; Expert covers competing constraints, second-order effects, governance or precedent, explicit boundaries, and defensible judgment.
- The provider classifies the question internally during the single generation call and never prints the type. Runtime validation checks only the outer 60–300-word shape; question-type target fit is telemetry and review evidence.
- Expert interview answers use question-specific soft word ranges: 200–250 for major career/suitability and behavioural failure; 220–275 for strategic implementation; 80–140 for direct facts or commitments; 140–180 for capability gaps and beneficiary/ethical scenarios; 110–170 for urgent operations; 170–220 for governance/safeguarding/finance and external partnership; and 180–220 for comparative closing.
- The lead and expansion are one streamed response and one provider call.
- `Refresh detail` is hidden and rejected for specialized responses but retained for General Guidance.
- Grounding is passive source metadata from local retrieval, not model-authored text or a second provider call.
- Prompting and retrieval precede fine-tuning.
- Profiles and lenses remain inert declarative data with no tools or scripts.
