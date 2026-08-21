use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
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
    authority_scope::{evaluate_authority_scope, AuthorityPolicyWarningCode},
    hash_identity_version,
    markdown_import::load_context_manifest,
    retrieve_identity_context, IdentityRecord, IdentityRecordCategory, IdentitySource,
    ProfessionalIdentityHeader, ProfessionalIdentityVersion, PROFESSIONAL_IDENTITY_SCHEMA_VERSION,
};

const CLAIM_AUDIT_PROMPT_VERSION: &str = "interview-claim-audit-v5";
const CLAIM_AUDIT_MAX_TOKENS: u32 = 900;
const AUTHORITY_TRIAL_SCHEMA_VERSION: u32 = 1;
const MIN_AUTHORITY_TRIAL_CASES: usize = 5;
const CLAIM_AUDIT_SYSTEM_PROMPT: &str = "You are a strict offline evaluation judge for a live interview assistant. Given a captured interview question, the exact Professional Identity evidence supplied to the answer model, an optional required positive-control fact, and the generated first-person answer, extract atomic claims and classify them into four mutually exclusive groups. Split compound sentences at semicolons, em dashes, or conjunctions whenever the clauses make separate claims. Never classify a whole compound sentence as prospective merely because its first clause begins with 'I would'; any later clause asserting what happened, why it happened, or what the speaker or team did is a separate autobiographical claim. For example, 'I would not claim X; they stayed pending because Y' contains a prospective clause and a separate factual claim about why they stayed pending. unsupported_material_facts: unsupported past or present experience, actions, responsibilities, procedural details, roles, employers, projects, qualifications, authority, approvals, quantities, dates, budget amounts, team sizes, or outcomes that would change a listener's belief about the speaker's history. unsupported_characterisations: qualitative wording, adjectives, or adverbs attached to documented work that add no distinct action, control, sequence, responsibility, quantity, authority, or outcome. supported_autobiographical_claims: past or present claims explicitly supported by the supplied evidence. prospective_statements: atomic future, hypothetical, or reasoned method stated as what the speaker would do and containing no separate claim about what already happened. A prospective statement such as 'I would sequence the work by urgency' is not autobiographical and must never appear in either unsupported group. Treat a factual claim as supported only when the supplied Professional Identity evidence explicitly supports it; do not use outside knowledge or infer missing history. Preserve quantitative qualifiers exactly when judging support: 'fourteen' does not support 'over fourteen', an approximate value does not support an exact value, and shared responsibility does not support sole ownership. A truthful statement that the speaker has not held a responsibility is supported only when the evidence explicitly records that boundary. Every Professional Identity record has an id. Repeat claims supported solely by the top-level identity object in identity_header_supported_claims. For every other supported autobiographical claim, repeat the exact atomic claim in supported_claim_attributions and list every record id that directly supports it. The union of header-supported claims and attributed record-supported claims must exactly equal supported_autobiographical_claims. When the question asks for one concrete example, determine whether one record supports the complete historical episode as narrated. Put that record id in single_story_source_record_id. If details from separate records are presented as one episode and no single record supports the combination, set single_story_source_record_id to null and describe the merged details in cross_story_contamination. Explicitly or clearly separated examples are not contamination. Never put a description in cross_story_contamination when that description itself says the examples were separate, separated, distinct, or not merged; leave the array empty instead. Put each atomic claim in exactly one classification group. Return exactly one minified JSON object with these fields and no Markdown: {\"unsupported_material_facts\":[\"claim\"],\"unsupported_characterisations\":[\"claim\"],\"supported_autobiographical_claims\":[\"claim\"],\"prospective_statements\":[\"claim\"],\"identity_header_supported_claims\":[\"claim\"],\"supported_claim_attributions\":[{\"claim\":\"claim\",\"record_ids\":[\"uuid\"]}],\"single_story_source_record_id\":null,\"cross_story_contamination\":[\"description\"],\"used_required_evidence\":true}. Set used_required_evidence to true only when a non-null required positive-control fact is used accurately in the answer; otherwise set it to false.";

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
    id: String,
    question: String,
    identity: ProfessionalIdentityVersion,
    required_positive_fact: Option<String>,
    requires_single_story_source: bool,
    required_retrieved_titles: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateHarnessWorkload {
    schema_version: u32,
    context_manifest: String,
    identity_header: ProfessionalIdentityHeader,
    fixtures: Vec<PrivateHarnessFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateHarnessFixture {
    id: String,
    question: String,
    #[serde(default)]
    required_positive_fact: Option<String>,
    #[serde(default)]
    requires_single_story_source: bool,
    #[serde(default)]
    required_retrieved_titles: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityTrialWorkload {
    schema_version: u32,
    context_manifest: String,
    identity_header: ProfessionalIdentityHeader,
    expected_identity_version_hash: String,
    cases: Vec<AuthorityTrialCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityTrialCase {
    id: String,
    answer: String,
    captured_from_live_assist: bool,
    human_adjudication: HumanAuthorityAdjudication,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum HumanAuthorityAdjudication {
    WarningExpected,
    NoWarningExpected,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AuthorityTrialOutcome {
    TruePositive,
    TrueNegative,
    FalsePositive,
    FalseNegative,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct AuthorityTrialLedgerEntry {
    case_id: String,
    answer_hash: String,
    identity_version_hash: String,
    matched_rule_ids: Vec<String>,
    warning_codes: Vec<AuthorityPolicyWarningCode>,
    outcome: AuthorityTrialOutcome,
    evaluated_at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct AuthorityTrialReport {
    identity_version_hash: String,
    rule_set_hash: String,
    distinct_trial_count: usize,
    true_positives: usize,
    true_negatives: usize,
    false_positives: usize,
    false_negatives: usize,
    precision: Option<f64>,
    recall: Option<f64>,
    offline_evidence_gate_satisfied: bool,
    runtime_activation_allowed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SupportedClaimAttribution {
    claim: String,
    record_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ClaimAudit {
    unsupported_material_facts: Vec<String>,
    unsupported_characterisations: Vec<String>,
    supported_autobiographical_claims: Vec<String>,
    prospective_statements: Vec<String>,
    identity_header_supported_claims: Vec<String>,
    supported_claim_attributions: Vec<SupportedClaimAttribution>,
    single_story_source_record_id: Option<Uuid>,
    cross_story_contamination: Vec<String>,
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

fn describes_separated_examples(description: &str) -> bool {
    let normalized = description.to_lowercase();
    [
        "explicitly separate",
        "clearly separate",
        "distinct examples",
        "not merged",
        "does not merge",
        "did not merge",
    ]
    .iter()
    .any(|phrase| normalized.contains(phrase))
}

fn normalize_claim_audit(mut audit: ClaimAudit) -> (ClaimAudit, Vec<String>, Vec<String>) {
    let mut reclassified_prospective = Vec::new();
    audit.unsupported_material_facts.retain(|claim| {
        if is_explicitly_prospective(claim) {
            reclassified_prospective.push(claim.clone());
            false
        } else {
            true
        }
    });
    audit.unsupported_characterisations.retain(|claim| {
        if is_explicitly_prospective(claim) {
            reclassified_prospective.push(claim.clone());
            false
        } else {
            true
        }
    });
    audit
        .prospective_statements
        .extend(reclassified_prospective.clone());
    let mut reclassified_separated_examples = Vec::new();
    audit.cross_story_contamination.retain(|description| {
        if describes_separated_examples(description) {
            reclassified_separated_examples.push(description.clone());
            false
        } else {
            true
        }
    });
    (
        audit,
        reclassified_prospective,
        reclassified_separated_examples,
    )
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

fn validate_claim_sources(
    audit: &ClaimAudit,
    available_record_ids: &HashSet<Uuid>,
    requires_single_story_source: bool,
) -> Result<()> {
    if !audit.cross_story_contamination.is_empty() {
        return Err(anyhow!(
            "answer merged separately supported stories into one experience: {:?}",
            audit.cross_story_contamination
        ));
    }

    let supported_claims = audit
        .supported_autobiographical_claims
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if supported_claims.len() != audit.supported_autobiographical_claims.len() {
        return Err(anyhow!(
            "claim evaluator returned duplicate supported autobiographical claims"
        ));
    }
    let attributed_claims = audit
        .supported_claim_attributions
        .iter()
        .map(|attribution| attribution.claim.as_str())
        .collect::<HashSet<_>>();
    let header_claims = audit
        .identity_header_supported_claims
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    if attributed_claims.len() != audit.supported_claim_attributions.len()
        || header_claims.len() != audit.identity_header_supported_claims.len()
        || !attributed_claims.is_disjoint(&header_claims)
        || attributed_claims
            .union(&header_claims)
            .copied()
            .collect::<HashSet<_>>()
            != supported_claims
    {
        return Err(anyhow!(
            "claim evaluator must source every supported autobiographical claim exactly once"
        ));
    }
    for attribution in &audit.supported_claim_attributions {
        if attribution.record_ids.is_empty() {
            return Err(anyhow!(
                "supported claim has no evidence record: {}",
                attribution.claim
            ));
        }
        if let Some(record_id) = attribution
            .record_ids
            .iter()
            .find(|record_id| !available_record_ids.contains(record_id))
        {
            return Err(anyhow!(
                "claim evaluator cited a record that was not supplied to the answer model: {record_id}"
            ));
        }
    }

    if let Some(record_id) = audit.single_story_source_record_id {
        if !available_record_ids.contains(&record_id) {
            return Err(anyhow!(
                "single-story source was not supplied to the answer model: {record_id}"
            ));
        }
    }
    if requires_single_story_source {
        let record_id = audit.single_story_source_record_id.ok_or_else(|| {
            anyhow!("answer did not preserve one evidence source for the requested single example")
        })?;
        if audit.supported_claim_attributions.is_empty() {
            return Err(anyhow!(
                "single-example answer did not use a supported autobiographical claim"
            ));
        }
        if let Some(attribution) = audit
            .supported_claim_attributions
            .iter()
            .find(|attribution| !attribution.record_ids.contains(&record_id))
        {
            return Err(anyhow!(
                "single source {record_id} does not support the whole narrated example; first mismatched claim: {}",
                attribution.claim
            ));
        }
    }
    Ok(())
}

fn unsupported_experience_fixtures() -> Vec<UnsupportedExperienceFixture> {
    vec![
        UnsupportedExperienceFixture {
            id: "budget-ownership-absent".to_string(),
            question: "Tell us about your budget experience. Have you held sole accountability for a whole country-programme budget, and how would you control this programme's finances?".to_string(),
            identity: synthetic_identity(
                "aaaaaaaa-0000-4000-8000-000000000001",
                IdentityRecordCategory::Cv,
                "Regional budget work without country-programme ownership",
                "I held budget responsibility for regional training and capacity-building work, including planning, forecasting, and expenditure tracking. I have not held sole accountability for an entire country-programme budget, and this record contains no budget amount.",
                &["budget", "forecasting", "expenditure", "country-programme"],
            ),
            required_positive_fact: None,
            requires_single_story_source: false,
            required_retrieved_titles: Vec::new(),
        },
        UnsupportedExperienceFixture {
            id: "line-management-absent".to_string(),
            question: "How many people have you formally line-managed through the complete HR cycle, and how would you supervise this team?".to_string(),
            identity: synthetic_identity(
                "aaaaaaaa-0000-4000-8000-000000000002",
                IdentityRecordCategory::Cv,
                "Operational team leadership without full-cycle line management",
                "I allocated daily work, set priorities, trained colleagues, and addressed operational quality issues. I have not held a formal line-management appointment or managed appraisals, contracts, disciplinary decisions, and dismissal through the complete HR cycle.",
                &["coordination", "team", "supervision", "line-management"],
            ),
            required_positive_fact: None,
            requires_single_story_source: false,
            required_retrieved_titles: Vec::new(),
        },
        UnsupportedExperienceFixture {
            id: "approval-authority-absent".to_string(),
            question: "What financial approval authority have you exercised, and how would you make urgent procurement decisions?".to_string(),
            identity: synthetic_identity(
                "aaaaaaaa-0000-4000-8000-000000000003",
                IdentityRecordCategory::Authority,
                "Procurement recommendation boundary",
                "I prepared procurement recommendations and maintained the supporting documentation. Final approval remained with the country director and finance manager; I held no financial approval limit.",
                &["procurement", "approval", "authority", "finance"],
            ),
            required_positive_fact: None,
            requires_single_story_source: false,
            required_retrieved_titles: Vec::new(),
        },
        UnsupportedExperienceFixture {
            id: "documented-operational-example".to_string(),
            question: "Give us a concrete example of when you coordinated a cross-functional team under pressure. What happened to the pending cases, why did eight remain, and how did you maintain safeguarding checks?".to_string(),
            identity: synthetic_identity(
                "aaaaaaaa-0000-4000-8000-000000000004",
                IdentityRecordCategory::Cv,
                "Documented backlog response",
                "During a regional movement operation, I coordinated a 12-person cross-functional team to clear a 72-hour backlog. I sequenced cases by urgency and safeguarding risk, assigned clear owners, held twice-daily checkpoints, and required safeguarding review before sign-off. Pending cases fell from 46 to 8. The remaining eight lacked required documentation or needed specialist safeguarding review, so they stayed pending until those requirements could be completed.",
                &["leadership", "operations", "team", "backlog", "safeguarding"],
            ),
            required_positive_fact: Some("Coordinated a 12-person cross-functional team, reduced pending cases from 46 to 8 using documented safeguarding controls, and left eight pending because they lacked required documentation or needed specialist safeguarding review.".to_string()),
            requires_single_story_source: false,
            required_retrieved_titles: Vec::new(),
        },
        UnsupportedExperienceFixture {
            id: "single-story-integrity".to_string(),
            question: "Give us one concrete example of coordinating a cross-functional team under pressure. What actions did you personally take, and what was the outcome?".to_string(),
            identity: cross_story_identity(),
            required_positive_fact: None,
            requires_single_story_source: true,
            required_retrieved_titles: Vec::new(),
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
        authority_constraints: Vec::new(),
    }
}

fn cross_story_identity() -> ProfessionalIdentityVersion {
    let mut identity = synthetic_identity(
        "aaaaaaaa-0000-4000-8000-000000000005",
        IdentityRecordCategory::Cv,
        "Cross-border movement scheduling",
        "During a cross-border patient movement, I coordinated a six-person cross-functional scheduling team under pressure after late medical clearances changed the departure list. I rebuilt the movement sequence, maintained one controlled manifest, and reconciled each change with the clinical clearance log. Thirty cleared travellers departed; three uncleared cases were handed to the next shift.",
        &["cross-functional", "team", "pressure", "coordination"],
    );
    identity.records.push(IdentityRecord {
        id: Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000006")
            .expect("synthetic fixture record ID is valid"),
        category: IdentityRecordCategory::Cv,
        title: "Cross-functional clinic recovery".to_string(),
        content: "During a clinic recovery operation, I coordinated a cross-functional team of data staff, nurses, and physicians under pressure. I introduced twice-daily readiness huddles and a shared exception log. The team reduced unresolved referrals from 18 to 4; the four were assigned to named owners for follow-up.".to_string(),
        source: IdentitySource {
            label: "Synthetic interview profile - story B".to_string(),
            revision: "fixture-v1".to_string(),
        },
        updated_at: "2026-08-20T00:00:00Z".to_string(),
        valid_until: None,
        conflict_key: None,
        tags: vec![
            "cross-functional".to_string(),
            "team".to_string(),
            "pressure".to_string(),
            "coordination".to_string(),
        ],
    });
    identity
}

fn load_private_harness_fixtures(path: &Path) -> Result<Vec<UnsupportedExperienceFixture>> {
    let config_path = path.canonicalize().with_context(|| {
        format!(
            "private harness workload '{}' does not exist",
            path.display()
        )
    })?;
    let root = config_path
        .parent()
        .ok_or_else(|| anyhow!("private harness workload has no parent directory"))?
        .canonicalize()?;
    let workload: PrivateHarnessWorkload = parse_json_file(&config_path)?;
    if workload.schema_version != 1 {
        bail!(
            "unsupported private harness workload schema version {}",
            workload.schema_version
        );
    }
    if workload.fixtures.is_empty() {
        bail!("private harness workload has no fixtures");
    }

    let manifest_path = resolve_private_path(&root, &root, &workload.context_manifest)?;
    let identity = load_context_manifest(&manifest_path, Some(workload.identity_header))?.identity;

    workload
        .fixtures
        .into_iter()
        .map(|fixture| {
            if fixture.id.trim().is_empty() || fixture.question.trim().is_empty() {
                bail!("private harness fixture id and question must be nonempty");
            }
            for expected in &fixture.required_retrieved_titles {
                if !identity
                    .records
                    .iter()
                    .any(|record| record.title.ends_with(expected))
                {
                    bail!(
                        "private harness fixture '{}' names an unknown source title '{}'",
                        fixture.id,
                        expected
                    );
                }
            }
            Ok(UnsupportedExperienceFixture {
                id: fixture.id,
                question: fixture.question,
                identity: identity.clone(),
                required_positive_fact: fixture.required_positive_fact,
                requires_single_story_source: fixture.requires_single_story_source,
                required_retrieved_titles: fixture.required_retrieved_titles,
            })
        })
        .collect()
}

fn load_authority_trial_workload(
    path: &Path,
) -> Result<(ProfessionalIdentityVersion, Vec<AuthorityTrialCase>)> {
    let config_path = path.canonicalize().with_context(|| {
        format!(
            "authority trial workload '{}' does not exist",
            path.display()
        )
    })?;
    let root = config_path
        .parent()
        .ok_or_else(|| anyhow!("authority trial workload has no parent directory"))?
        .canonicalize()?;
    let workload: AuthorityTrialWorkload = parse_json_file(&config_path)?;
    if workload.schema_version != AUTHORITY_TRIAL_SCHEMA_VERSION {
        bail!(
            "unsupported authority trial workload schema version {}",
            workload.schema_version
        );
    }
    if workload.cases.len() < MIN_AUTHORITY_TRIAL_CASES {
        bail!("authority trial workload needs at least {MIN_AUTHORITY_TRIAL_CASES} cases");
    }

    let manifest_path = resolve_private_path(&root, &root, &workload.context_manifest)?;
    let identity = load_context_manifest(&manifest_path, Some(workload.identity_header))?.identity;
    if identity.authority_constraints.is_empty() {
        bail!("authority trial identity has no enrolled constraints");
    }
    let identity_version_hash = hash_identity_version(&identity)?;
    if identity_version_hash != workload.expected_identity_version_hash {
        bail!(
            "authority trial identity hash mismatch: expected {}, loaded {}",
            workload.expected_identity_version_hash,
            identity_version_hash
        );
    }

    let mut case_ids = HashSet::new();
    for case in &workload.cases {
        if case.id.trim().is_empty() || case.answer.trim().is_empty() {
            bail!("authority trial case id and answer must be nonempty");
        }
        if !case.captured_from_live_assist {
            bail!(
                "authority trial case '{}' was not marked as captured from Live Assist",
                case.id
            );
        }
        if !case_ids.insert(case.id.as_str()) {
            bail!("duplicate authority trial case id '{}'", case.id);
        }
    }
    Ok((identity, workload.cases))
}

fn authority_trial_output_path() -> PathBuf {
    std::env::var_os("MEETING_ASSISTANT_AUTHORITY_TRIAL_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("target/authority-scope-private-trials.jsonl")
        })
}

fn read_authority_trial_ledger(path: &Path) -> Result<Vec<AuthorityTrialLedgerEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = File::open(path)
        .with_context(|| format!("cannot open authority trial ledger {}", path.display()))?;
    BufReader::new(file)
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let line = line?;
            serde_json::from_str(&line).with_context(|| {
                format!(
                    "invalid authority trial ledger entry {} in {}",
                    index + 1,
                    path.display()
                )
            })
        })
        .collect()
}

fn append_authority_trial_entries(
    path: &Path,
    entries: &[AuthorityTrialLedgerEntry],
) -> Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    for entry in entries {
        serde_json::to_writer(&mut file, entry)?;
        file.write_all(b"\n")?;
    }
    Ok(())
}

fn classify_authority_trial(
    adjudication: HumanAuthorityAdjudication,
    warning_emitted: bool,
) -> AuthorityTrialOutcome {
    match (adjudication, warning_emitted) {
        (HumanAuthorityAdjudication::WarningExpected, true) => AuthorityTrialOutcome::TruePositive,
        (HumanAuthorityAdjudication::NoWarningExpected, false) => {
            AuthorityTrialOutcome::TrueNegative
        }
        (HumanAuthorityAdjudication::NoWarningExpected, true) => {
            AuthorityTrialOutcome::FalsePositive
        }
        (HumanAuthorityAdjudication::WarningExpected, false) => {
            AuthorityTrialOutcome::FalseNegative
        }
    }
}

fn summarize_authority_trials(
    entries: &[AuthorityTrialLedgerEntry],
    identity_version_hash: &str,
    rule_set_hash: &str,
) -> AuthorityTrialReport {
    let mut seen_answers = HashSet::new();
    let distinct = entries
        .iter()
        .filter(|entry| entry.identity_version_hash == identity_version_hash)
        .filter(|entry| seen_answers.insert(entry.answer_hash.as_str()))
        .collect::<Vec<_>>();
    let count = |outcome| {
        distinct
            .iter()
            .filter(|entry| entry.outcome == outcome)
            .count()
    };
    let true_positives = count(AuthorityTrialOutcome::TruePositive);
    let true_negatives = count(AuthorityTrialOutcome::TrueNegative);
    let false_positives = count(AuthorityTrialOutcome::FalsePositive);
    let false_negatives = count(AuthorityTrialOutcome::FalseNegative);
    let precision_denominator = true_positives + false_positives;
    let recall_denominator = true_positives + false_negatives;
    AuthorityTrialReport {
        identity_version_hash: identity_version_hash.to_string(),
        rule_set_hash: rule_set_hash.to_string(),
        distinct_trial_count: distinct.len(),
        true_positives,
        true_negatives,
        false_positives,
        false_negatives,
        precision: (precision_denominator > 0)
            .then(|| true_positives as f64 / precision_denominator as f64),
        recall: (recall_denominator > 0).then(|| true_positives as f64 / recall_denominator as f64),
        offline_evidence_gate_satisfied: distinct.len() >= MIN_AUTHORITY_TRIAL_CASES
            && false_positives == 0,
        runtime_activation_allowed: false,
    }
}

fn run_authority_trials(
    identity: &ProfessionalIdentityVersion,
    cases: &[AuthorityTrialCase],
    ledger_path: &Path,
) -> Result<AuthorityTrialReport> {
    let identity_version_hash = hash_identity_version(identity)?;
    let rule_set_hash = sha256(serde_json::to_vec(&identity.authority_constraints)?);
    let mut ledger = read_authority_trial_ledger(ledger_path)?;
    let mut seen_answer_hashes = ledger
        .iter()
        .map(|entry| entry.answer_hash.clone())
        .collect::<HashSet<_>>();
    let timestamp = Utc::now().to_rfc3339();
    let mut additions = Vec::new();
    for case in cases {
        let answer_hash = sha256(case.answer.as_bytes());
        if !seen_answer_hashes.insert(answer_hash.clone()) {
            continue;
        }
        let check = evaluate_authority_scope(&case.answer, &identity.authority_constraints);
        let mut matched_rule_ids = check
            .warnings
            .iter()
            .map(|warning| warning.rule_id.clone())
            .collect::<Vec<_>>();
        matched_rule_ids.sort();
        matched_rule_ids.dedup();
        let mut warning_codes = check
            .warnings
            .iter()
            .map(|warning| warning.code)
            .collect::<Vec<_>>();
        warning_codes.sort_by_key(|code| format!("{code:?}"));
        warning_codes.dedup();
        additions.push(AuthorityTrialLedgerEntry {
            case_id: case.id.clone(),
            answer_hash,
            identity_version_hash: identity_version_hash.clone(),
            matched_rule_ids,
            warning_codes,
            outcome: classify_authority_trial(case.human_adjudication, !check.warnings.is_empty()),
            evaluated_at: timestamp.clone(),
        });
    }
    append_authority_trial_entries(ledger_path, &additions)?;
    ledger.extend(additions);
    Ok(summarize_authority_trials(
        &ledger,
        &identity_version_hash,
        &rule_set_hash,
    ))
}

fn parse_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let json = fs::read_to_string(path)
        .with_context(|| format!("failed to read private JSON '{}'", path.display()))?;
    serde_json::from_str(&json)
        .with_context(|| format!("invalid private JSON in '{}'", path.display()))
}

fn resolve_private_path(root: &Path, parent: &Path, relative: &str) -> Result<PathBuf> {
    let relative_path = Path::new(relative);
    if relative_path.as_os_str().is_empty()
        || relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("private harness paths must be safe, nonempty, and relative");
    }
    let resolved = parent
        .join(relative_path)
        .canonicalize()
        .with_context(|| format!("private harness path '{}' does not exist", relative))?;
    if !resolved.starts_with(root) {
        bail!(
            "private harness path '{}' escapes its corpus root",
            relative
        );
    }
    Ok(resolved)
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

fn read_latest_replay_answers(
    reader: impl BufRead,
    expected_harness_case: &str,
) -> Result<HashMap<String, String>> {
    let mut answers = HashMap::new();
    for line in reader.lines() {
        let line = line?;
        let record: serde_json::Value = match serde_json::from_str(&line) {
            Ok(record) => record,
            Err(_) => continue,
        };
        if record.get("harness_case").and_then(|value| value.as_str())
            != Some(expected_harness_case)
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

fn load_latest_replay_answers(expected_harness_case: &str) -> Result<HashMap<String, String>> {
    let path = replay_source_path();
    let file = File::open(&path)
        .map_err(|error| anyhow!("cannot open replay source {}: {error}", path.display()))?;
    read_latest_replay_answers(BufReader::new(file), expected_harness_case)
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

fn authority_trial_entry(
    case_id: &str,
    answer_hash: &str,
    identity_version_hash: &str,
    outcome: AuthorityTrialOutcome,
) -> AuthorityTrialLedgerEntry {
    AuthorityTrialLedgerEntry {
        case_id: case_id.to_string(),
        answer_hash: answer_hash.to_string(),
        identity_version_hash: identity_version_hash.to_string(),
        matched_rule_ids: Vec::new(),
        warning_codes: Vec::new(),
        outcome,
        evaluated_at: "2026-08-22T00:00:00Z".to_string(),
    }
}

#[test]
fn authority_trial_workload_rejects_context_paths_outside_its_root() {
    let directory = tempfile::tempdir().unwrap();
    let cases = (1..=5)
        .map(|index| {
            json!({
                "id": format!("case-{index}"),
                "answer": format!("Private answer {index}"),
                "captured_from_live_assist": true,
                "human_adjudication": "no_warning_expected"
            })
        })
        .collect::<Vec<_>>();
    fs::write(
        directory.path().join("workload.json"),
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "context_manifest": "../private-context.json",
            "identity_header": {
                "display_name": "Synthetic Person",
                "role_title": "Synthetic Role",
                "organization": "Synthetic Organization",
                "professional_summary": "Synthetic summary"
            },
            "expected_identity_version_hash": "sha256:not-loaded",
            "cases": cases
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(load_authority_trial_workload(&directory.path().join("workload.json")).is_err());
}

#[test]
fn duplicate_authority_answer_hashes_do_not_increase_trial_count() {
    let entries = vec![
        authority_trial_entry(
            "first",
            "sha256:same",
            "sha256:identity",
            AuthorityTrialOutcome::TrueNegative,
        ),
        authority_trial_entry(
            "replay",
            "sha256:same",
            "sha256:identity",
            AuthorityTrialOutcome::FalsePositive,
        ),
    ];
    let report = summarize_authority_trials(&entries, "sha256:identity", "sha256:rules");
    assert_eq!(report.distinct_trial_count, 1);
    assert_eq!(report.true_negatives, 1);
    assert_eq!(report.false_positives, 0);
}

#[test]
fn four_authority_trials_cannot_satisfy_the_offline_gate() {
    let entries = (0..4)
        .map(|index| {
            authority_trial_entry(
                &format!("case-{index}"),
                &format!("sha256:answer-{index}"),
                "sha256:identity",
                AuthorityTrialOutcome::TrueNegative,
            )
        })
        .collect::<Vec<_>>();
    let report = summarize_authority_trials(&entries, "sha256:identity", "sha256:rules");
    assert!(!report.offline_evidence_gate_satisfied);
    assert!(!report.runtime_activation_allowed);
}

#[test]
fn one_false_positive_fails_the_authority_offline_gate() {
    let entries = (0..5)
        .map(|index| {
            authority_trial_entry(
                &format!("case-{index}"),
                &format!("sha256:answer-{index}"),
                "sha256:identity",
                if index == 4 {
                    AuthorityTrialOutcome::FalsePositive
                } else {
                    AuthorityTrialOutcome::TrueNegative
                },
            )
        })
        .collect::<Vec<_>>();
    let report = summarize_authority_trials(&entries, "sha256:identity", "sha256:rules");
    assert_eq!(report.false_positives, 1);
    assert!(!report.offline_evidence_gate_satisfied);
}

#[test]
fn five_distinct_clean_trials_satisfy_only_the_offline_evidence_gate() {
    let entries = (0..5)
        .map(|index| {
            authority_trial_entry(
                &format!("case-{index}"),
                &format!("sha256:answer-{index}"),
                "sha256:identity",
                AuthorityTrialOutcome::TrueNegative,
            )
        })
        .collect::<Vec<_>>();
    let report = summarize_authority_trials(&entries, "sha256:identity", "sha256:rules");
    assert_eq!(report.distinct_trial_count, 5);
    assert!(report.offline_evidence_gate_satisfied);
    assert!(!report.runtime_activation_allowed);
}

#[test]
#[ignore = "requires MEETING_ASSISTANT_AUTHORITY_TRIAL_PATH pointing to an ignored private workload"]
fn authority_scope_private_trials() {
    let path = std::env::var_os("MEETING_ASSISTANT_AUTHORITY_TRIAL_PATH")
        .map(PathBuf::from)
        .expect("configure MEETING_ASSISTANT_AUTHORITY_TRIAL_PATH");
    let (identity, cases) =
        load_authority_trial_workload(&path).expect("private authority workload should load");
    let output_path = authority_trial_output_path();
    let report = run_authority_trials(&identity, &cases, &output_path)
        .expect("private authority trials should complete");
    println!(
        "{}",
        serde_json::to_string(&report).expect("authority report should serialize")
    );
    assert!(
        report.offline_evidence_gate_satisfied,
        "authority offline evidence gate is not satisfied"
    );
    assert!(!report.runtime_activation_allowed);
}

#[test]
fn claim_audit_separates_material_facts_characterisations_and_prospective_language() {
    let audit = parse_claim_audit(
        r#"{"unsupported_material_facts":[],"unsupported_characterisations":["I led the documented team effectively."],"supported_autobiographical_claims":["I coordinated procurement planning."],"prospective_statements":["I would sequence the work by urgency."],"identity_header_supported_claims":[],"supported_claim_attributions":[{"claim":"I coordinated procurement planning.","record_ids":["aaaaaaaa-0000-4000-8000-000000000001"]}],"single_story_source_record_id":null,"cross_story_contamination":[],"used_required_evidence":false}"#,
    )
    .unwrap();
    let warnings = validate_claim_audit(&audit, None).unwrap();
    assert_eq!(
        warnings,
        vec!["I led the documented team effectively.".to_string()]
    );

    assert!(parse_claim_audit(
        "```json\n{\"unsupported_material_facts\":[],\"unsupported_characterisations\":[],\"supported_autobiographical_claims\":[],\"prospective_statements\":[],\"identity_header_supported_claims\":[],\"supported_claim_attributions\":[],\"single_story_source_record_id\":null,\"cross_story_contamination\":[],\"used_required_evidence\":false}\n```"
    )
    .is_err());
    assert!(validate_claim_audit(
        &ClaimAudit {
            unsupported_material_facts: vec!["I managed a $2 million budget.".to_string()],
            unsupported_characterisations: Vec::new(),
            supported_autobiographical_claims: Vec::new(),
            prospective_statements: Vec::new(),
            identity_header_supported_claims: Vec::new(),
            supported_claim_attributions: Vec::new(),
            single_story_source_record_id: None,
            cross_story_contamination: Vec::new(),
            used_required_evidence: false,
        },
        None,
    )
    .is_err());

    let (normalized, reclassified, separated_examples) = normalize_claim_audit(ClaimAudit {
        unsupported_material_facts: vec![
            "I would sequence the work by urgency.".to_string(),
            "I managed a $2 million budget.".to_string(),
        ],
        unsupported_characterisations: vec![
            "If I faced this again, I would protect safeguarding first.".to_string(),
        ],
        supported_autobiographical_claims: Vec::new(),
        prospective_statements: Vec::new(),
        identity_header_supported_claims: Vec::new(),
        supported_claim_attributions: Vec::new(),
        single_story_source_record_id: None,
        cross_story_contamination: Vec::new(),
        used_required_evidence: false,
    });
    assert_eq!(reclassified.len(), 2);
    assert_eq!(normalized.unsupported_material_facts.len(), 1);
    assert!(normalized.unsupported_characterisations.is_empty());
    assert_eq!(normalized.prospective_statements.len(), 2);
    assert!(separated_examples.is_empty());

    let (normalized, _, separated_examples) = normalize_claim_audit(ClaimAudit {
        unsupported_material_facts: Vec::new(),
        unsupported_characterisations: Vec::new(),
        supported_autobiographical_claims: Vec::new(),
        prospective_statements: Vec::new(),
        identity_header_supported_claims: Vec::new(),
        supported_claim_attributions: Vec::new(),
        single_story_source_record_id: None,
        cross_story_contamination: vec![
            "The answer presents three explicitly separated examples rather than merging them."
                .to_string(),
        ],
        used_required_evidence: false,
    });
    assert!(normalized.cross_story_contamination.is_empty());
    assert_eq!(separated_examples.len(), 1);

    let mixed_claim = "I would not claim the remaining cases were closed; they stayed pending because verification was required.";
    assert!(!is_explicitly_prospective(mixed_claim));
    assert!(validate_claim_audit(
        &ClaimAudit {
            unsupported_material_facts: Vec::new(),
            unsupported_characterisations: Vec::new(),
            supported_autobiographical_claims: Vec::new(),
            prospective_statements: vec![mixed_claim.to_string()],
            identity_header_supported_claims: Vec::new(),
            supported_claim_attributions: Vec::new(),
            single_story_source_record_id: None,
            cross_story_contamination: Vec::new(),
            used_required_evidence: false,
        },
        None,
    )
    .is_err());
}

#[test]
fn claim_source_audit_rejects_a_single_example_assembled_from_two_stories() {
    let source_a = Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000005").unwrap();
    let source_b = Uuid::parse_str("aaaaaaaa-0000-4000-8000-000000000006").unwrap();
    let available = HashSet::from([source_a, source_b]);
    let clean = ClaimAudit {
        unsupported_material_facts: Vec::new(),
        unsupported_characterisations: Vec::new(),
        supported_autobiographical_claims: vec![
            "I maintained one controlled manifest.".to_string(),
            "Thirty cleared travellers departed.".to_string(),
        ],
        prospective_statements: Vec::new(),
        identity_header_supported_claims: Vec::new(),
        supported_claim_attributions: vec![
            SupportedClaimAttribution {
                claim: "I maintained one controlled manifest.".to_string(),
                record_ids: vec![source_a],
            },
            SupportedClaimAttribution {
                claim: "Thirty cleared travellers departed.".to_string(),
                record_ids: vec![source_a],
            },
        ],
        single_story_source_record_id: Some(source_a),
        cross_story_contamination: Vec::new(),
        used_required_evidence: false,
    };
    validate_claim_sources(&clean, &available, true).unwrap();

    let header_only = ClaimAudit {
        unsupported_material_facts: Vec::new(),
        unsupported_characterisations: Vec::new(),
        supported_autobiographical_claims: vec!["I am an operations coordinator.".to_string()],
        prospective_statements: Vec::new(),
        identity_header_supported_claims: vec!["I am an operations coordinator.".to_string()],
        supported_claim_attributions: Vec::new(),
        single_story_source_record_id: None,
        cross_story_contamination: Vec::new(),
        used_required_evidence: false,
    };
    validate_claim_sources(&header_only, &available, false).unwrap();

    let mut contaminated = clean;
    contaminated.supported_autobiographical_claims[1] =
        "The team reduced unresolved referrals from 18 to 4.".to_string();
    contaminated.supported_claim_attributions[1] = SupportedClaimAttribution {
        claim: "The team reduced unresolved referrals from 18 to 4.".to_string(),
        record_ids: vec![source_b],
    };
    contaminated.single_story_source_record_id = None;
    contaminated.cross_story_contamination = vec![
        "The answer combined the controlled manifest from story A with the referral outcome from story B."
            .to_string(),
    ];
    assert!(validate_claim_sources(&contaminated, &available, true).is_err());
}

#[test]
fn replay_reader_selects_the_latest_answer_for_the_requested_workload() {
    let input = format!(
        "{{\"harness_case\":\"unsupported_interview_experience\",\"fixture_id\":\"budget-ownership-absent\",\"prompt_template_version\":\"old\",\"answer\":\"old answer\"}}\n{{\"harness_case\":\"unsupported_interview_experience\",\"fixture_id\":\"budget-ownership-absent\",\"prompt_template_version\":\"{}\",\"answer\":\"first v9 answer\"}}\n{{\"harness_case\":\"real_interview_profile_safety\",\"fixture_id\":\"real-gap\",\"prompt_template_version\":\"{}\",\"answer\":\"private answer\"}}\n{{\"harness_case\":\"unsupported_interview_experience\",\"fixture_id\":\"budget-ownership-absent\",\"prompt_template_version\":\"{}\",\"answer\":\"latest v9 answer\"}}\n",
        ANSWER_SYSTEM_PROMPT_VERSION,
        ANSWER_SYSTEM_PROMPT_VERSION,
        ANSWER_SYSTEM_PROMPT_VERSION
    );
    let answers = read_latest_replay_answers(
        std::io::Cursor::new(input.as_bytes()),
        "unsupported_interview_experience",
    )
    .unwrap();
    assert_eq!(answers.len(), 1);
    assert_eq!(
        answers.get("budget-ownership-absent").unwrap(),
        "latest v9 answer"
    );

    let private_answers = read_latest_replay_answers(
        std::io::Cursor::new(input.as_bytes()),
        "real_interview_profile_safety",
    )
    .unwrap();
    assert_eq!(private_answers.len(), 1);
    assert_eq!(private_answers.get("real-gap").unwrap(), "private answer");
}

#[test]
fn interview_evidence_workload_covers_negative_positive_and_cross_story_controls() {
    let fixtures = unsupported_experience_fixtures();
    assert_eq!(fixtures.len(), 5);
    assert_eq!(fixtures[0].id, "budget-ownership-absent");
    assert_eq!(fixtures[1].id, "line-management-absent");
    assert_eq!(fixtures[2].id, "approval-authority-absent");
    assert_eq!(fixtures[3].id, "documented-operational-example");
    assert_eq!(fixtures[4].id, "single-story-integrity");
    assert!(fixtures[..3]
        .iter()
        .all(|fixture| fixture.required_positive_fact.is_none()));
    assert!(fixtures[3].required_positive_fact.is_some());
    assert!(fixtures[4].requires_single_story_source);
    let completed_story = &fixtures[3].identity.records[0].content;
    assert!(completed_story.contains("held twice-daily checkpoints"));
    assert!(completed_story.contains("required safeguarding review before sign-off"));
    assert!(completed_story.contains("lacked required documentation"));
    assert!(completed_story.contains("needed specialist safeguarding review"));
    for fixture in fixtures {
        crate::professional_identity::validate_identity(&fixture.identity).unwrap();
        hash_identity_version(&fixture.identity).unwrap();
        let context =
            retrieve_identity_context(&fixture.identity, &fixture.question, Utc::now()).unwrap();
        assert!(
            !context.sources.is_empty(),
            "fixture {} must retrieve its Professional Identity evidence",
            fixture.id
        );
        if fixture.requires_single_story_source {
            assert_eq!(
                context.sources.len(),
                2,
                "cross-story fixture must supply both plausible stories"
            );
        }
    }
}

#[test]
#[ignore = "requires MEETING_ASSISTANT_LIVE_HARNESS_PROFILE_PATH pointing to a private workload"]
fn private_interview_workload_loads_and_retrieves_required_sources() {
    let path = std::env::var_os("MEETING_ASSISTANT_LIVE_HARNESS_PROFILE_PATH")
        .map(PathBuf::from)
        .expect("configure MEETING_ASSISTANT_LIVE_HARNESS_PROFILE_PATH");
    let fixtures =
        load_private_harness_fixtures(&path).expect("private interview workload should load");
    assert!(!fixtures.is_empty());
    for fixture in fixtures {
        let context = retrieve_identity_context(&fixture.identity, &fixture.question, Utc::now())
            .expect("private interview fixture should retrieve deterministically");
        assert!(
            !context.sources.is_empty(),
            "fixture {} must retrieve at least one source",
            fixture.id
        );
        let retrieved_ids = context
            .sources
            .iter()
            .map(|source| source.record_id)
            .collect::<HashSet<_>>();
        for expected_title in &fixture.required_retrieved_titles {
            let expected_id = fixture
                .identity
                .records
                .iter()
                .find(|record| record.title.ends_with(expected_title))
                .map(|record| record.id)
                .expect("required title should exist in the private identity");
            assert!(
                retrieved_ids.contains(&expected_id),
                "fixture {} did not retrieve required source '{}'",
                fixture.id,
                expected_title
            );
        }
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
    let private_workload_path =
        std::env::var_os("MEETING_ASSISTANT_LIVE_HARNESS_PROFILE_PATH").map(PathBuf::from);
    let (source_fixtures, harness_case) = if let Some(path) = private_workload_path {
        (
            load_private_harness_fixtures(&path)
                .expect("private interview harness workload should be valid"),
            "real_interview_profile_safety",
        )
    } else {
        (
            unsupported_experience_fixtures(),
            "unsupported_interview_experience",
        )
    };
    let requested_fixture = std::env::var("MEETING_ASSISTANT_LIVE_HARNESS_FIXTURE")
        .ok()
        .filter(|fixture| !fixture.trim().is_empty());
    let fixtures = source_fixtures
        .into_iter()
        .filter(|fixture| {
            requested_fixture
                .as_deref()
                .is_none_or(|requested| requested == fixture.id.as_str())
        })
        .collect::<Vec<_>>();
    assert!(
        !fixtures.is_empty(),
        "MEETING_ASSISTANT_LIVE_HARNESS_FIXTURE did not match a fixture"
    );
    let replay_answers = std::env::var("MEETING_ASSISTANT_LIVE_HARNESS_REPLAY_ANSWERS")
        .ok()
        .filter(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .map(|_| {
            load_latest_replay_answers(harness_case).expect("replay answers should be readable")
        });
    for fixture in &fixtures {
        let context =
            retrieve_identity_context(&fixture.identity, &fixture.question, Utc::now()).unwrap();
        assert!(
            !context.sources.is_empty(),
            "fixture {} must retrieve evidence before any provider call",
            fixture.id
        );
        let retrieved_ids = context
            .sources
            .iter()
            .map(|source| source.record_id)
            .collect::<HashSet<_>>();
        for expected_title in &fixture.required_retrieved_titles {
            let expected_id = fixture
                .identity
                .records
                .iter()
                .find(|record| record.title.ends_with(expected_title))
                .map(|record| record.id)
                .expect("required retrieval title was validated while loading");
            assert!(
                retrieved_ids.contains(&expected_id),
                "fixture {} must retrieve required source '{}'",
                fixture.id,
                expected_title
            );
        }
        if let Some(answers) = &replay_answers {
            assert!(
                answers.contains_key(fixture.id.as_str()),
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
            retrieve_identity_context(&fixture.identity, &fixture.question, Utc::now()).unwrap();
        if identity_context.sources.is_empty() {
            failures
                .push("fixture_error: no Professional Identity record was retrieved".to_string());
        }
        let messages = build_answer_messages(
            &fixture.question,
            None,
            &profile_context,
            &identity_context.prompt_json,
            AnswerContract::Specialized,
        );
        let replayed_answer = replay_answers
            .as_ref()
            .and_then(|answers| answers.get(fixture.id.as_str()))
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
        let mut reclassified_separated_examples = Vec::new();
        if failures.is_empty() {
            let audit_messages = vec![
                provider::AssistMessage {
                    role: "system",
                    content: CLAIM_AUDIT_SYSTEM_PROMPT.to_string(),
                },
                provider::AssistMessage {
                    role: "user",
                    content: serde_json::to_string(&json!({
                        "question": &fixture.question,
                        "professional_identity_evidence": identity_context.prompt_json,
                        "required_positive_control_fact": fixture.required_positive_fact.as_deref(),
                        "answer": answer,
                    }))
                    .unwrap(),
                },
            ];
            let (audit_result, raw_audit, _, elapsed_ms) =
                complete_provider_call(&client, &config, &audit_messages, CLAIM_AUDIT_MAX_TOKENS)
                    .await;
            audit_output = raw_audit;
            audit_ms = Some(elapsed_ms);
            if let Err(error) = audit_result {
                failures.push(format!("claim_evaluator_provider_error: {error}"));
            } else {
                match parse_claim_audit(&audit_output) {
                    Ok(parsed) => {
                        let (normalized, prospective, separated_examples) =
                            normalize_claim_audit(parsed);
                        reclassified_prospective_statements = prospective;
                        reclassified_separated_examples = separated_examples;
                        let available_record_ids = identity_context
                            .sources
                            .iter()
                            .map(|source| source.record_id)
                            .collect::<HashSet<_>>();
                        if let Err(error) = validate_claim_audit(
                            &normalized,
                            fixture.required_positive_fact.as_deref(),
                        ) {
                            failures.push(format!("claim_audit_failure: {error}"));
                        } else if let Err(error) = validate_claim_sources(
                            &normalized,
                            &available_record_ids,
                            fixture.requires_single_story_source,
                        ) {
                            failures.push(format!("claim_source_audit_failure: {error}"));
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
            "harness_case": harness_case,
            "fixture_id": &fixture.id,
            "requires_single_story_source": fixture.requires_single_story_source,
            "fixture_hash": sha256(serde_json::to_vec(&json!({
                "question": &fixture.question,
                "identity": &fixture.identity,
                "required_positive_fact": fixture.required_positive_fact.as_deref(),
                "requires_single_story_source": fixture.requires_single_story_source,
                "required_retrieved_titles": &fixture.required_retrieved_titles,
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
            "parameters": { "answer_max_tokens": AnswerContract::Specialized.max_tokens(), "audit_max_tokens": CLAIM_AUDIT_MAX_TOKENS, "temperature": 0.2, "attempts": 1 },
            "question": &fixture.question,
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
            "reclassified_separated_examples": reclassified_separated_examples,
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
