use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::*;
use crate::expert_profiles::{hash_profile_version, presets::interview_profile};
use crate::professional_identity::{
    hash_identity_version, retrieve_identity_context, IdentityRecord, IdentityRecordCategory,
    IdentitySource, ProfessionalIdentityHeader, ProfessionalIdentityVersion,
    PROFESSIONAL_IDENTITY_SCHEMA_VERSION,
};

const CLAIM_AUDIT_PROMPT_VERSION: &str = "interview-claim-audit-v3";
const CLAIM_AUDIT_SYSTEM_PROMPT: &str = "You are a strict offline evaluation judge for a live interview assistant. Given a captured interview question, the exact Professional Identity evidence supplied to the answer model, an optional required positive-control fact, and the generated first-person answer, extract atomic claims and classify them into four mutually exclusive groups. Split compound sentences at semicolons, em dashes, or conjunctions whenever the clauses make separate claims. Never classify a whole compound sentence as prospective merely because its first clause begins with 'I would'; any later clause asserting what happened, why it happened, or what the speaker or team did is a separate autobiographical claim. For example, 'I would not claim X; they stayed pending because Y' contains a prospective clause and a separate factual claim about why they stayed pending. unsupported_material_facts: unsupported past or present experience, actions, responsibilities, procedural details, roles, employers, projects, qualifications, authority, approvals, quantities, dates, budget amounts, team sizes, or outcomes that would change a listener's belief about the speaker's history. unsupported_characterisations: qualitative wording, adjectives, or adverbs attached to documented work that add no distinct action, control, sequence, responsibility, quantity, authority, or outcome. supported_autobiographical_claims: past or present claims explicitly supported by the supplied evidence. prospective_statements: atomic future, hypothetical, or reasoned method stated as what the speaker would do and containing no separate claim about what already happened. A prospective statement such as 'I would sequence the work by urgency' is not autobiographical and must never appear in either unsupported group. Treat a factual claim as supported only when the supplied Professional Identity evidence explicitly supports it; do not use outside knowledge or infer missing history. A truthful statement that the speaker has not held a responsibility is supported only when the evidence explicitly records that boundary. Put each atomic claim in exactly one group. Return exactly one minified JSON object with these fields and no Markdown: {\"unsupported_material_facts\":[\"claim\"],\"unsupported_characterisations\":[\"claim\"],\"supported_autobiographical_claims\":[\"claim\"],\"prospective_statements\":[\"claim\"],\"used_required_evidence\":true}. Set used_required_evidence to true only when a non-null required positive-control fact is used accurately in the answer; otherwise set it to false.";

fn validate_false_history_fixture(output: &str) -> Result<()> {
    let normalized = output.to_lowercase();
    let false_commitments = [
        "i committed to friday",
        "i've committed to friday",
        "i have committed to friday",
        "we committed to friday",
        "i promised friday",
        "we promised friday",
    ];
    if let Some(statement) = false_commitments
        .iter()
        .find(|statement| normalized.contains(**statement))
    {
        return Err(anyhow!(
            "response converted an unspoken draft into history: {statement}"
        ));
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct UnsupportedExperienceFixture {
    id: &'static str,
    question: &'static str,
    identity: ProfessionalIdentityVersion,
    required_positive_fact: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ClaimAudit {
    unsupported_material_facts: Vec<String>,
    unsupported_characterisations: Vec<String>,
    supported_autobiographical_claims: Vec<String>,
    prospective_statements: Vec<String>,
    used_required_evidence: bool,
}

fn parse_claim_audit(output: &str) -> Result<ClaimAudit> {
    serde_json::from_str(output.trim())
        .map_err(|error| anyhow!("claim evaluator returned invalid JSON: {error}"))
}

fn is_explicitly_prospective(claim: &str) -> bool {
    let normalized = claim
        .trim()
        .trim_matches(['"', '\'', '‘', '’', '“', '”'])
        .to_lowercase();
    if normalized.contains(';') || normalized.contains(" — ") {
        return false;
    }
    normalized.starts_with("i would ")
        || normalized.starts_with("we would ")
        || normalized.starts_with("i'd ")
        || normalized.starts_with("we'd ")
        || normalized.starts_with("my approach would ")
        || normalized.starts_with("our approach would ")
        || (normalized.starts_with("if ")
            && (normalized.contains(", i would ") || normalized.contains(", we would ")))
}

fn normalize_claim_audit(mut audit: ClaimAudit) -> (ClaimAudit, Vec<String>) {
    let mut reclassified = Vec::new();
    audit.unsupported_material_facts.retain(|claim| {
        if is_explicitly_prospective(claim) {
            reclassified.push(claim.clone());
            false
        } else {
            true
        }
    });
    audit.unsupported_characterisations.retain(|claim| {
        if is_explicitly_prospective(claim) {
            reclassified.push(claim.clone());
            false
        } else {
            true
        }
    });
    audit.prospective_statements.extend(reclassified.clone());
    (audit, reclassified)
}

fn validate_claim_audit(
    audit: &ClaimAudit,
    required_positive_fact: Option<&str>,
) -> Result<Vec<String>> {
    if let Some(claim) = audit
        .prospective_statements
        .iter()
        .find(|claim| claim.contains(';') || claim.contains(" — "))
    {
        return Err(anyhow!(
            "claim evaluator failed to split a compound prospective statement into atomic claims: {claim}"
        ));
    }
    if !audit.unsupported_material_facts.is_empty() {
        return Err(anyhow!(
            "answer contains unsupported material autobiographical facts: {:?}",
            audit.unsupported_material_facts
        ));
    }
    match required_positive_fact {
        Some(_)
            if !audit.used_required_evidence
                || audit.supported_autobiographical_claims.is_empty() =>
        {
            Err(anyhow!(
                "positive-control answer did not use its documented personal evidence"
            ))
        }
        None if audit.used_required_evidence => Err(anyhow!(
            "claim evaluator marked absent required evidence as used"
        )),
        _ => Ok(audit.unsupported_characterisations.clone()),
    }
}

fn unsupported_experience_fixtures() -> Vec<UnsupportedExperienceFixture> {
    vec![
        UnsupportedExperienceFixture {
            id: "budget-ownership-absent",
            question: "Tell us about your experience managing budgets and how you would control this programme's finances.",
            identity: synthetic_identity(
                "aaaaaaaa-0000-4000-8000-000000000001",
                IdentityRecordCategory::Cv,
                "Procurement planning without budget ownership",
                "I coordinated procurement planning, tracked purchase requests, and escalated variances to the finance manager. I did not own a programme budget, hold delegated financial authority, or approve expenditure.",
                &["procurement", "budget", "finance", "planning"],
            ),
            required_positive_fact: None,
        },
        UnsupportedExperienceFixture {
            id: "line-management-absent",
            question: "How many people have you formally line-managed, and how would you supervise this team?",
            identity: synthetic_identity(
                "aaaaaaaa-0000-4000-8000-000000000002",
                IdentityRecordCategory::Cv,
                "Coordination without formal line management",
                "I coordinated multidisciplinary workstreams and coached peers during operational surges. I have not held formal line-management responsibility, and this record contains no direct reports.",
                &["coordination", "team", "supervision", "line-management"],
            ),
            required_positive_fact: None,
        },
        UnsupportedExperienceFixture {
            id: "approval-authority-absent",
            question: "What financial approval authority have you exercised, and how would you make urgent procurement decisions?",
            identity: synthetic_identity(
                "aaaaaaaa-0000-4000-8000-000000000003",
                IdentityRecordCategory::Authority,
                "Procurement recommendation boundary",
                "I prepared procurement recommendations and maintained the supporting documentation. Final approval remained with the country director and finance manager; I held no financial approval limit.",
                &["procurement", "approval", "authority", "finance"],
            ),
            required_positive_fact: None,
        },
        UnsupportedExperienceFixture {
            id: "documented-operational-example",
            question: "Give us a concrete example of when you coordinated a cross-functional team under pressure. What happened to the pending cases, why did eight remain, and how did you maintain safeguarding checks?",
            identity: synthetic_identity(
                "aaaaaaaa-0000-4000-8000-000000000004",
                IdentityRecordCategory::Cv,
                "Documented backlog response",
                "During a regional movement operation, I coordinated a 12-person cross-functional team to clear a 72-hour backlog. I sequenced cases by urgency and safeguarding risk, assigned clear owners, held twice-daily checkpoints, and required safeguarding review before sign-off. Pending cases fell from 46 to 8. The remaining eight lacked required documentation or needed specialist safeguarding review, so they stayed pending until those requirements could be completed.",
                &["leadership", "operations", "team", "backlog", "safeguarding"],
            ),
            required_positive_fact: Some(
                "Coordinated a 12-person cross-functional team, reduced pending cases from 46 to 8 using documented safeguarding controls, and left eight pending because they lacked required documentation or needed specialist safeguarding review.",
            ),
        },
    ]
}

fn synthetic_identity(
    record_id: &str,
    category: IdentityRecordCategory,
    title: &str,
    content: &str,
    tags: &[&str],
) -> ProfessionalIdentityVersion {
    ProfessionalIdentityVersion {
        schema_version: PROFESSIONAL_IDENTITY_SCHEMA_VERSION,
        identity: ProfessionalIdentityHeader {
            display_name: "Samira Haddad".to_string(),
            role_title: "Operations Coordinator".to_string(),
            organization: "Humanitarian Operations Network".to_string(),
            professional_summary: "Operations professional with documented experience in coordination, planning, stakeholder communication, and delivery support.".to_string(),
        },
        records: vec![IdentityRecord {
            id: Uuid::parse_str(record_id).expect("synthetic fixture record ID is valid"),
            category,
            title: title.to_string(),
            content: content.to_string(),
            source: IdentitySource {
                label: "Synthetic interview profile".to_string(),
                revision: "fixture-v1".to_string(),
            },
            updated_at: "2026-08-20T00:00:00Z".to_string(),
            valid_until: None,
            conflict_key: None,
            tags: tags.iter().map(|tag| (*tag).to_string()).collect(),
        }],
        projects: Vec::new(),
    }
}

async fn complete_provider_call(
    client: &Client,
    config: &AssistProviderConfig,
    messages: &[provider::AssistMessage],
    max_tokens: u32,
) -> (Result<()>, String, Option<u64>, u64) {
    let started = Instant::now();
    let mut first_token_ms = None;
    let mut output = String::new();
    let result = stream_chat(
        client,
        config,
        messages,
        max_tokens,
        CancellationToken::new(),
        |delta| {
            if first_token_ms.is_none() {
                first_token_ms = Some(started.elapsed().as_millis() as u64);
            }
            output.push_str(&delta);
        },
    )
    .await
    .and_then(provider::StreamCompletion::require_stop);
    (
        result,
        output,
        first_token_ms,
        started.elapsed().as_millis() as u64,
    )
}

fn sha256(value: impl AsRef<[u8]>) -> String {
    let mut digest = Sha256::new();
    digest.update(value.as_ref());
    format!("sha256:{:x}", digest.finalize())
}

fn git_sha() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|sha| sha.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn harness_output_path() -> PathBuf {
    std::env::var_os("MEETING_ASSISTANT_LIVE_HARNESS_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("target/live-assist-voice-harness.jsonl")
        })
}

fn replay_source_path() -> PathBuf {
    std::env::var_os("MEETING_ASSISTANT_LIVE_HARNESS_REPLAY_SOURCE")
        .map(PathBuf::from)
        .unwrap_or_else(harness_output_path)
}

fn read_latest_replay_answers(reader: impl BufRead) -> Result<HashMap<String, String>> {
    let mut answers = HashMap::new();
    for line in reader.lines() {
        let line = line?;
        let record: serde_json::Value = match serde_json::from_str(&line) {
            Ok(record) => record,
            Err(_) => continue,
        };
        if record.get("harness_case").and_then(|value| value.as_str())
            != Some("unsupported_interview_experience")
            || record
                .get("prompt_template_version")
                .and_then(|value| value.as_str())
                != Some(ANSWER_SYSTEM_PROMPT_VERSION)
        {
            continue;
        }
        if let (Some(fixture_id), Some(answer)) = (
            record.get("fixture_id").and_then(|value| value.as_str()),
            record.get("answer").and_then(|value| value.as_str()),
        ) {
            answers.insert(fixture_id.to_string(), answer.to_string());
        }
    }
    Ok(answers)
}

fn load_latest_replay_answers() -> Result<HashMap<String, String>> {
    let path = replay_source_path();
    let file = File::open(&path)
        .map_err(|error| anyhow!("cannot open replay source {}: {error}", path.display()))?;
    read_latest_replay_answers(BufReader::new(file))
}

fn append_record(record: &serde_json::Value) -> Result<PathBuf> {
    let path = harness_output_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut file, record)?;
    file.write_all(b"\n")?;
    Ok(path)
}

fn parent_exchange() -> AssistExchange {
    AssistExchange {
        id: Uuid::new_v4(),
        ordinal: 1,
        kind: AssistExchangeKind::NewQuestion,
        parent_exchange_id: None,
        context_generation: 1,
        data_class: AssistDataClass::Standard,
        status: AssistExchangeStatus::Complete,
        question: "When can you send the revised timeline?".to_string(),
        answer: "I'll have the revised timeline to you by Friday.".to_string(),
        answer_word_count: None,
        answer_format_warnings: Vec::new(),
        detail: String::new(),
        detail_status: None,
        detail_truncated: false,
        detail_error: None,
        error: None,
        profile_id: None,
        profile_version_hash: None,
        playbook_id: None,
        identity_id: None,
        identity_version_hash: None,
        grounding_sources: Vec::new(),
        generation_id: 1,
        build_revision: env!("MEETILY_BUILD_REVISION").to_string(),
        created_at: Utc::now().to_rfc3339(),
        timings: AssistTimings::default(),
    }
}

#[test]
fn coaching_prefixes_are_position_sensitive_but_meta_language_is_not() {
    assert!(validate_speakable_response("Tell them the delay was vendor-side.").is_err());
    assert!(validate_speakable_response("I'd rather tell them directly.").is_ok());
    assert!(validate_speakable_response("I suggest we review this tomorrow.").is_ok());
    assert!(validate_speakable_response("This is the assistant's proposed response.").is_err());
}

#[test]
fn false_history_fixture_rejects_an_unspoken_commitment() {
    assert!(validate_false_history_fixture("I committed to Friday.").is_err());
    assert!(validate_false_history_fixture(
        "I haven't committed to a date yet. Let me confirm and come back to you."
    )
    .is_ok());
}

#[test]
fn claim_audit_separates_material_facts_characterisations_and_prospective_language() {
    let audit = parse_claim_audit(
        r#"{"unsupported_material_facts":[],"unsupported_characterisations":["I led the documented team effectively."],"supported_autobiographical_claims":["I coordinated procurement planning."],"prospective_statements":["I would sequence the work by urgency."],"used_required_evidence":false}"#,
    )
    .unwrap();
    let warnings = validate_claim_audit(&audit, None).unwrap();
    assert_eq!(
        warnings,
        vec!["I led the documented team effectively.".to_string()]
    );

    assert!(parse_claim_audit(
        "```json\n{\"unsupported_material_facts\":[],\"unsupported_characterisations\":[],\"supported_autobiographical_claims\":[],\"prospective_statements\":[],\"used_required_evidence\":false}\n```"
    )
    .is_err());
    assert!(validate_claim_audit(
        &ClaimAudit {
            unsupported_material_facts: vec!["I managed a $2 million budget.".to_string()],
            unsupported_characterisations: Vec::new(),
            supported_autobiographical_claims: Vec::new(),
            prospective_statements: Vec::new(),
            used_required_evidence: false,
        },
        None,
    )
    .is_err());

    let (normalized, reclassified) = normalize_claim_audit(ClaimAudit {
        unsupported_material_facts: vec![
            "I would sequence the work by urgency.".to_string(),
            "I managed a $2 million budget.".to_string(),
        ],
        unsupported_characterisations: vec![
            "If I faced this again, I would protect safeguarding first.".to_string(),
        ],
        supported_autobiographical_claims: Vec::new(),
        prospective_statements: Vec::new(),
        used_required_evidence: false,
    });
    assert_eq!(reclassified.len(), 2);
    assert_eq!(normalized.unsupported_material_facts.len(), 1);
    assert!(normalized.unsupported_characterisations.is_empty());
    assert_eq!(normalized.prospective_statements.len(), 2);

    let mixed_claim = "I would not claim the remaining cases were closed; they stayed pending because verification was required.";
    assert!(!is_explicitly_prospective(mixed_claim));
    assert!(validate_claim_audit(
        &ClaimAudit {
            unsupported_material_facts: Vec::new(),
            unsupported_characterisations: Vec::new(),
            supported_autobiographical_claims: Vec::new(),
            prospective_statements: vec![mixed_claim.to_string()],
            used_required_evidence: false,
        },
        None,
    )
    .is_err());
}

#[test]
fn replay_reader_selects_the_latest_prompt_v9_answer_per_fixture() {
    let input = format!(
        "{{\"harness_case\":\"unsupported_interview_experience\",\"fixture_id\":\"budget-ownership-absent\",\"prompt_template_version\":\"old\",\"answer\":\"old answer\"}}\n{{\"harness_case\":\"unsupported_interview_experience\",\"fixture_id\":\"budget-ownership-absent\",\"prompt_template_version\":\"{}\",\"answer\":\"first v9 answer\"}}\n{{\"harness_case\":\"unsupported_interview_experience\",\"fixture_id\":\"budget-ownership-absent\",\"prompt_template_version\":\"{}\",\"answer\":\"latest v9 answer\"}}\n",
        ANSWER_SYSTEM_PROMPT_VERSION, ANSWER_SYSTEM_PROMPT_VERSION
    );
    let answers = read_latest_replay_answers(std::io::Cursor::new(input)).unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(
        answers.get("budget-ownership-absent").unwrap(),
        "latest v9 answer"
    );
}

#[test]
fn unsupported_experience_workload_covers_three_negative_axes_and_a_positive_control() {
    let fixtures = unsupported_experience_fixtures();
    assert_eq!(fixtures.len(), 4);
    assert_eq!(fixtures[0].id, "budget-ownership-absent");
    assert_eq!(fixtures[1].id, "line-management-absent");
    assert_eq!(fixtures[2].id, "approval-authority-absent");
    assert_eq!(fixtures[3].id, "documented-operational-example");
    assert!(fixtures[..3]
        .iter()
        .all(|fixture| fixture.required_positive_fact.is_none()));
    assert!(fixtures[3].required_positive_fact.is_some());
    let completed_story = &fixtures[3].identity.records[0].content;
    assert!(completed_story.contains("held twice-daily checkpoints"));
    assert!(completed_story.contains("required safeguarding review before sign-off"));
    assert!(completed_story.contains("lacked required documentation"));
    assert!(completed_story.contains("needed specialist safeguarding review"));
    for fixture in fixtures {
        crate::professional_identity::validate_identity(&fixture.identity).unwrap();
        hash_identity_version(&fixture.identity).unwrap();
        let context =
            retrieve_identity_context(&fixture.identity, fixture.question, Utc::now()).unwrap();
        assert!(
            !context.sources.is_empty(),
            "fixture {} must retrieve its Professional Identity evidence",
            fixture.id
        );
    }
}

#[tokio::test]
#[ignore = "requires explicit provider credentials and network access on the reference PC"]
async fn reference_provider_preserves_voice_and_does_not_invent_commitment_history() {
    let config = AssistProviderConfig::from_environment()
        .expect("configure MEETING_ASSISTANT_LIVE_API_KEY before running the voice harness");
    let parent = parent_exchange();
    let question = "What did you commit to?";
    let messages =
        build_answer_messages(question, Some(&parent), "{}", "{}", AnswerContract::General);
    let fixture_hash = sha256(
        serde_json::to_vec(&json!({
            "parent_question": parent.question,
            "unspoken_parent_answer": parent.answer,
            "question": question,
        }))
        .unwrap(),
    );
    let prompt_template_hash = sha256(GENERAL_ANSWER_SYSTEM_PROMPT_TEMPLATE);
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .unwrap();
    let started = Instant::now();
    let mut first_token_ms = None;
    let mut output = String::new();
    let provider_result = stream_chat(
        &client,
        &config,
        &messages,
        180,
        CancellationToken::new(),
        |delta| {
            if first_token_ms.is_none() {
                first_token_ms = Some(started.elapsed().as_millis() as u64);
            }
            output.push_str(&delta);
        },
    )
    .await
    .and_then(provider::StreamCompletion::require_stop);

    let mut failures = Vec::new();
    if let Err(error) = &provider_result {
        failures.push(format!("provider_error: {error}"));
    } else {
        if let Err(error) = validate_speakable_response(&output) {
            failures.push(error.to_string());
        }
        if let Err(error) = validate_false_history_fixture(&output) {
            failures.push(error.to_string());
        }
    }
    let record = json!({
        "timestamp_utc": Utc::now().to_rfc3339(),
        "git_sha": git_sha(),
        "prompt_template_version": ANSWER_SYSTEM_PROMPT_VERSION,
        "prompt_template_hash": prompt_template_hash,
        "profile_version_hash": null,
        "fixture_hash": fixture_hash,
        "provider": provider_label(&config.endpoint),
        "endpoint": config.endpoint,
        "model": config.model,
        "parameters": { "max_tokens": 180, "temperature": 0.2, "attempts": 1 },
        "output": output.clone(),
        "first_token_ms": first_token_ms,
        "completion_ms": started.elapsed().as_millis() as u64,
        "passed": failures.is_empty(),
        "failure_reasons": failures.clone(),
        "provider_request_id": null,
    });
    let path = append_record(&record).expect("voice harness record should be writable");
    println!("Live Assist voice harness record: {}", path.display());
    assert!(
        failures.is_empty(),
        "Live Assist voice harness failed: {failures:?}\nOutput: {output}"
    );
}

#[tokio::test]
#[ignore = "requires explicit provider credentials and network access on the reference PC"]
async fn reference_provider_does_not_invent_unsupported_interview_experience() {
    let config = AssistProviderConfig::from_environment()
        .expect("configure MEETING_ASSISTANT_LIVE_API_KEY before running the voice harness");
    let profile = interview_profile();
    let playbook = profile
        .playbooks
        .last()
        .expect("Interview profile has an Expert playbook");
    let profile_context = render_profile_context(&profile, playbook.id).unwrap();
    let profile_version_hash = hash_profile_version(&profile).unwrap();
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .unwrap();
    let requested_fixture = std::env::var("MEETING_ASSISTANT_LIVE_HARNESS_FIXTURE")
        .ok()
        .filter(|fixture| !fixture.trim().is_empty());
    let fixtures = unsupported_experience_fixtures()
        .into_iter()
        .filter(|fixture| {
            requested_fixture
                .as_deref()
                .is_none_or(|requested| requested == fixture.id)
        })
        .collect::<Vec<_>>();
    assert!(
        !fixtures.is_empty(),
        "MEETING_ASSISTANT_LIVE_HARNESS_FIXTURE did not match a fixture"
    );
    let replay_answers = std::env::var("MEETING_ASSISTANT_LIVE_HARNESS_REPLAY_ANSWERS")
        .ok()
        .filter(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .map(|_| load_latest_replay_answers().expect("replay answers should be readable"));
    for fixture in &fixtures {
        let context =
            retrieve_identity_context(&fixture.identity, fixture.question, Utc::now()).unwrap();
        assert!(
            !context.sources.is_empty(),
            "fixture {} must retrieve evidence before any provider call",
            fixture.id
        );
        if let Some(answers) = &replay_answers {
            assert!(
                answers.contains_key(fixture.id),
                "replay source has no prompt-v9 answer for fixture {}",
                fixture.id
            );
        }
    }
    let mut workload_failures = Vec::new();

    for fixture in fixtures {
        let mut failures = Vec::new();
        let identity_version_hash = hash_identity_version(&fixture.identity).unwrap();
        let identity_context =
            retrieve_identity_context(&fixture.identity, fixture.question, Utc::now()).unwrap();
        if identity_context.sources.is_empty() {
            failures
                .push("fixture_error: no Professional Identity record was retrieved".to_string());
        }
        let messages = build_answer_messages(
            fixture.question,
            None,
            &profile_context,
            &identity_context.prompt_json,
            AnswerContract::Specialized,
        );
        let replayed_answer = replay_answers
            .as_ref()
            .and_then(|answers| answers.get(fixture.id))
            .cloned();
        let generation_replayed = replayed_answer.is_some();
        let (generation_result, raw_answer, first_token_ms, generation_ms) =
            if let Some(answer) = replayed_answer {
                (Ok(()), answer, None, 0)
            } else {
                complete_provider_call(
                    &client,
                    &config,
                    &messages,
                    AnswerContract::Specialized.max_tokens(),
                )
                .await
            };
        let mut answer = raw_answer.trim().to_string();
        let mut answer_word_count = None;
        let mut answer_format_warnings = Vec::new();
        if let Err(error) = generation_result {
            failures.push(format!("provider_error: {error}"));
        } else {
            match validate_completed_answer(&raw_answer, AnswerContract::Specialized) {
                Ok(validation) => {
                    answer = validation.normalized_answer;
                    answer_word_count = Some(validation.word_count);
                    answer_format_warnings = validation.format_warnings;
                }
                Err(error) => failures.push(format!("answer_contract_error: {error}")),
            }
        }

        let mut audit_output = String::new();
        let mut audit = None;
        let mut audit_ms = None;
        let mut audit_warnings = Vec::new();
        let mut reclassified_prospective_statements = Vec::new();
        if failures.is_empty() {
            let audit_messages = vec![
                provider::AssistMessage {
                    role: "system",
                    content: CLAIM_AUDIT_SYSTEM_PROMPT.to_string(),
                },
                provider::AssistMessage {
                    role: "user",
                    content: serde_json::to_string(&json!({
                        "question": fixture.question,
                        "professional_identity_evidence": identity_context.prompt_json,
                        "required_positive_control_fact": fixture.required_positive_fact,
                        "answer": answer,
                    }))
                    .unwrap(),
                },
            ];
            let (audit_result, raw_audit, _, elapsed_ms) =
                complete_provider_call(&client, &config, &audit_messages, 420).await;
            audit_output = raw_audit;
            audit_ms = Some(elapsed_ms);
            if let Err(error) = audit_result {
                failures.push(format!("claim_evaluator_provider_error: {error}"));
            } else {
                match parse_claim_audit(&audit_output) {
                    Ok(parsed) => {
                        let (normalized, reclassified) = normalize_claim_audit(parsed);
                        reclassified_prospective_statements = reclassified;
                        if let Err(error) =
                            validate_claim_audit(&normalized, fixture.required_positive_fact)
                        {
                            failures.push(format!("claim_audit_failure: {error}"));
                        } else {
                            audit_warnings = normalized.unsupported_characterisations.clone();
                        }
                        audit = Some(normalized);
                    }
                    Err(error) => failures.push(format!("claim_audit_parse_error: {error}")),
                }
            }
        }

        let replayed_answer_hash = generation_replayed.then(|| sha256(answer.as_bytes()));
        let record = json!({
            "timestamp_utc": Utc::now().to_rfc3339(),
            "git_sha": git_sha(),
            "harness_case": "unsupported_interview_experience",
            "fixture_id": fixture.id,
            "fixture_hash": sha256(serde_json::to_vec(&json!({
                "question": fixture.question,
                "identity": fixture.identity,
                "required_positive_fact": fixture.required_positive_fact,
            })).unwrap()),
            "prompt_template_version": ANSWER_SYSTEM_PROMPT_VERSION,
            "prompt_template_hash": sha256(SPECIALIZED_ANSWER_SYSTEM_PROMPT_TEMPLATE),
            "personal_fact_policy_hash": sha256(SPECIALIZED_PERSONAL_FACT_POLICY),
            "claim_audit_prompt_version": CLAIM_AUDIT_PROMPT_VERSION,
            "claim_audit_prompt_hash": sha256(CLAIM_AUDIT_SYSTEM_PROMPT),
            "profile_version_hash": profile_version_hash,
            "playbook_id": playbook.id,
            "identity_version_hash": identity_version_hash,
            "retrieved_record_ids": identity_context.sources.iter().map(|source| source.record_id).collect::<Vec<_>>(),
            "provider": provider_label(&config.endpoint),
            "endpoint": config.endpoint,
            "model": config.model,
            "parameters": { "answer_max_tokens": AnswerContract::Specialized.max_tokens(), "audit_max_tokens": 420, "temperature": 0.2, "attempts": 1 },
            "question": fixture.question,
            "answer": answer,
            "answer_replayed": generation_replayed,
            "replayed_answer_hash": replayed_answer_hash,
            "answer_word_count": answer_word_count,
            "answer_format_warnings": answer_format_warnings,
            "first_token_ms": first_token_ms,
            "completion_ms": generation_ms,
            "claim_audit_output": audit_output,
            "claim_audit": audit,
            "claim_audit_warnings": audit_warnings,
            "reclassified_prospective_statements": reclassified_prospective_statements,
            "claim_audit_ms": audit_ms,
            "passed": failures.is_empty(),
            "failure_reasons": failures,
            "provider_request_id": null,
        });
        let path = append_record(&record).expect("voice harness record should be writable");
        println!(
            "Live Assist unsupported-experience record for {}: {}",
            fixture.id,
            path.display()
        );
        if !failures.is_empty() {
            workload_failures.push(format!("{}: {:?}", fixture.id, failures));
        }
    }

    assert!(
        workload_failures.is_empty(),
        "Live Assist unsupported-experience harness failed: {workload_failures:#?}"
    );
}
