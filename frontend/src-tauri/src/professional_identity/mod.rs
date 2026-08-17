//! Versioned, declarative professional identity data for Live Assist.
//!
//! Identity content is inert local data. The schema intentionally exposes no
//! executable hooks, tools, filesystem paths, provider endpoints, or permission
//! grants. Retrieval returns source metadata alongside prompt context so the UI
//! never relies on model-authored provenance.

pub mod commands;
pub mod repository;

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::expert_profiles::hashing::canonical_json;

pub const PROFESSIONAL_IDENTITY_SCHEMA_VERSION: u32 = 1;
const IDENTITY_HASH_DOMAIN: &[u8] = b"meetily-professional-identity-v1\0";
const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_RECORDS: usize = 256;
const MAX_PROJECTS: usize = 64;
const MAX_PROJECT_FACTS: usize = 128;
const MAX_TAGS: usize = 32;
const MAX_STRING_BYTES: usize = 32 * 1024;
const MAX_RETRIEVED_SOURCES: usize = 8;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfessionalIdentityVersion {
    pub schema_version: u32,
    pub identity: ProfessionalIdentityHeader,
    pub records: Vec<IdentityRecord>,
    pub projects: Vec<IdentityProject>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfessionalIdentityHeader {
    pub display_name: String,
    pub role_title: String,
    pub organization: String,
    pub professional_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IdentityRecord {
    pub id: Uuid,
    pub category: IdentityRecordCategory,
    pub title: String,
    pub content: String,
    pub source: IdentitySource,
    pub updated_at: String,
    pub valid_until: Option<String>,
    pub conflict_key: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IdentityRecordCategory {
    Cv,
    TermsOfReference,
    Authority,
    Stakeholder,
    Commitment,
    OperatingPractice,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IdentitySource {
    pub label: String,
    pub revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IdentityProject {
    pub id: Uuid,
    pub name: String,
    pub role: String,
    pub status: String,
    pub source: IdentitySource,
    pub updated_at: String,
    pub valid_until: Option<String>,
    pub tags: Vec<String>,
    pub facts: Vec<IdentityProjectFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IdentityProjectFact {
    pub id: Uuid,
    pub content: String,
    pub source: IdentitySource,
    pub conflict_key: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GroundingSource {
    pub record_id: Uuid,
    pub label: String,
    pub revision: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievedIdentityContext {
    pub prompt_json: String,
    pub sources: Vec<GroundingSource>,
}

#[derive(Serialize)]
struct PromptIdentityContext<'a> {
    context_type: &'static str,
    identity: &'a ProfessionalIdentityHeader,
    records: Vec<PromptIdentityRecord<'a>>,
}

#[derive(Serialize)]
struct PromptIdentityRecord<'a> {
    id: Uuid,
    category: &'static str,
    title: &'a str,
    content: &'a str,
    source_label: &'a str,
    source_revision: &'a str,
    updated_at: &'a str,
    conflicting_current_sources: bool,
}

struct Candidate<'a> {
    id: Uuid,
    category: &'static str,
    title: &'a str,
    content: String,
    source: &'a IdentitySource,
    updated_at: &'a str,
    valid_until: Option<&'a str>,
    conflict_key: Option<&'a str>,
    score_text: String,
}

pub fn parse_identity_json(input: &str) -> Result<ProfessionalIdentityVersion> {
    if input.len() > MAX_JSON_BYTES {
        return Err(anyhow!("identity JSON exceeds the 1 MiB limit"));
    }
    let profile: ProfessionalIdentityVersion = serde_json::from_str(input)
        .map_err(|error| anyhow!("invalid professional identity JSON: {error}"))?;
    validate_identity(&profile)?;
    Ok(profile)
}

pub fn validate_identity(profile: &ProfessionalIdentityVersion) -> Result<()> {
    if profile.schema_version != PROFESSIONAL_IDENTITY_SCHEMA_VERSION {
        return Err(anyhow!(
            "expected professional identity schema version {PROFESSIONAL_IDENTITY_SCHEMA_VERSION}, got {}",
            profile.schema_version
        ));
    }
    validate_text("identity.display_name", &profile.identity.display_name)?;
    validate_text("identity.role_title", &profile.identity.role_title)?;
    validate_text("identity.organization", &profile.identity.organization)?;
    validate_text(
        "identity.professional_summary",
        &profile.identity.professional_summary,
    )?;
    if profile.records.len() > MAX_RECORDS {
        return Err(anyhow!(
            "identity records exceed the {MAX_RECORDS} item limit"
        ));
    }
    if profile.projects.len() > MAX_PROJECTS {
        return Err(anyhow!(
            "identity projects exceed the {MAX_PROJECTS} item limit"
        ));
    }

    let mut ids = HashSet::new();
    for (index, record) in profile.records.iter().enumerate() {
        if !ids.insert(record.id) {
            return Err(anyhow!(
                "duplicate identity record UUID at records[{index}]"
            ));
        }
        validate_text(&format!("records[{index}].title"), &record.title)?;
        validate_text(&format!("records[{index}].content"), &record.content)?;
        validate_source(&format!("records[{index}].source"), &record.source)?;
        validate_timestamp(&format!("records[{index}].updated_at"), &record.updated_at)?;
        validate_optional_timestamp(
            &format!("records[{index}].valid_until"),
            record.valid_until.as_deref(),
        )?;
        validate_optional_text(
            &format!("records[{index}].conflict_key"),
            record.conflict_key.as_deref(),
        )?;
        validate_tags(&format!("records[{index}].tags"), &record.tags)?;
    }

    for (project_index, project) in profile.projects.iter().enumerate() {
        if !ids.insert(project.id) {
            return Err(anyhow!(
                "duplicate identity UUID at projects[{project_index}]"
            ));
        }
        let path = format!("projects[{project_index}]");
        validate_text(&format!("{path}.name"), &project.name)?;
        validate_text(&format!("{path}.role"), &project.role)?;
        validate_text(&format!("{path}.status"), &project.status)?;
        validate_source(&format!("{path}.source"), &project.source)?;
        validate_timestamp(&format!("{path}.updated_at"), &project.updated_at)?;
        validate_optional_timestamp(
            &format!("{path}.valid_until"),
            project.valid_until.as_deref(),
        )?;
        validate_tags(&format!("{path}.tags"), &project.tags)?;
        if project.facts.len() > MAX_PROJECT_FACTS {
            return Err(anyhow!(
                "{path}.facts exceeds the {MAX_PROJECT_FACTS} item limit"
            ));
        }
        for (fact_index, fact) in project.facts.iter().enumerate() {
            if !ids.insert(fact.id) {
                return Err(anyhow!(
                    "duplicate identity UUID at {path}.facts[{fact_index}]"
                ));
            }
            let fact_path = format!("{path}.facts[{fact_index}]");
            validate_text(&format!("{fact_path}.content"), &fact.content)?;
            validate_source(&format!("{fact_path}.source"), &fact.source)?;
            validate_optional_text(
                &format!("{fact_path}.conflict_key"),
                fact.conflict_key.as_deref(),
            )?;
            validate_tags(&format!("{fact_path}.tags"), &fact.tags)?;
        }
    }
    Ok(())
}

pub fn hash_identity_version(profile: &ProfessionalIdentityVersion) -> Result<String> {
    validate_identity(profile)?;
    let canonical = canonical_json(profile)?;
    let mut digest = Sha256::new();
    digest.update(IDENTITY_HASH_DOMAIN);
    digest.update(canonical);
    Ok(format!("sha256:{:x}", digest.finalize()))
}

pub fn retrieve_identity_context(
    profile: &ProfessionalIdentityVersion,
    question: &str,
    now: DateTime<Utc>,
) -> Result<RetrievedIdentityContext> {
    validate_identity(profile)?;
    let query_terms = tokenize(question);
    let mut candidates = Vec::new();

    for record in &profile.records {
        candidates.push(Candidate {
            id: record.id,
            category: category_name(record.category),
            title: &record.title,
            content: record.content.clone(),
            source: &record.source,
            updated_at: &record.updated_at,
            valid_until: record.valid_until.as_deref(),
            conflict_key: record.conflict_key.as_deref(),
            score_text: format!(
                "{} {} {}",
                record.title,
                record.content,
                record.tags.join(" ")
            ),
        });
    }
    for project in &profile.projects {
        let project_title = project.name.as_str();
        let project_context = format!(
            "Project: {}. My role: {}. Current status: {}.",
            project.name, project.role, project.status
        );
        candidates.push(Candidate {
            id: project.id,
            category: "project",
            title: project_title,
            content: project_context.clone(),
            source: &project.source,
            updated_at: &project.updated_at,
            valid_until: project.valid_until.as_deref(),
            conflict_key: None,
            score_text: format!(
                "{} {} {} {}",
                project.name,
                project.role,
                project.status,
                project.tags.join(" ")
            ),
        });
        for fact in &project.facts {
            candidates.push(Candidate {
                id: fact.id,
                category: "project_fact",
                title: project_title,
                content: fact.content.clone(),
                source: &fact.source,
                updated_at: &project.updated_at,
                valid_until: project.valid_until.as_deref(),
                conflict_key: fact.conflict_key.as_deref(),
                score_text: format!("{} {} {}", project.name, fact.content, fact.tags.join(" ")),
            });
        }
    }

    let conflicting_keys = candidates
        .iter()
        .filter(|candidate| !is_expired(candidate.valid_until, now))
        .filter_map(|candidate| candidate.conflict_key)
        .fold(std::collections::HashMap::new(), |mut counts, key| {
            *counts.entry(key.to_string()).or_insert(0usize) += 1;
            counts
        });
    let mut ranked: Vec<(usize, Candidate<'_>)> = candidates
        .into_iter()
        .filter(|candidate| !is_expired(candidate.valid_until, now))
        .map(|candidate| {
            (
                lexical_score(&query_terms, &candidate.score_text),
                candidate,
            )
        })
        .filter(|(score, _)| *score > 0)
        .collect();
    ranked.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.id.cmp(&right.id))
    });
    ranked.truncate(MAX_RETRIEVED_SOURCES);

    let records = ranked
        .iter()
        .map(|(_, candidate)| PromptIdentityRecord {
            id: candidate.id,
            category: candidate.category,
            title: candidate.title,
            content: &candidate.content,
            source_label: &candidate.source.label,
            source_revision: &candidate.source.revision,
            updated_at: candidate.updated_at,
            conflicting_current_sources: candidate
                .conflict_key
                .and_then(|key| conflicting_keys.get(key))
                .is_some_and(|count| *count > 1),
        })
        .collect::<Vec<_>>();
    let sources = ranked
        .iter()
        .map(|(_, candidate)| GroundingSource {
            record_id: candidate.id,
            label: candidate.source.label.clone(),
            revision: candidate.source.revision.clone(),
            updated_at: candidate.updated_at.to_string(),
        })
        .collect();
    let prompt_json = serde_json::to_string(&PromptIdentityContext {
        context_type: "professional_identity",
        identity: &profile.identity,
        records,
    })?;

    Ok(RetrievedIdentityContext {
        prompt_json,
        sources,
    })
}

fn category_name(category: IdentityRecordCategory) -> &'static str {
    match category {
        IdentityRecordCategory::Cv => "cv",
        IdentityRecordCategory::TermsOfReference => "terms_of_reference",
        IdentityRecordCategory::Authority => "authority",
        IdentityRecordCategory::Stakeholder => "stakeholder",
        IdentityRecordCategory::Commitment => "commitment",
        IdentityRecordCategory::OperatingPractice => "operating_practice",
        IdentityRecordCategory::Other => "other",
    }
}

fn tokenize(value: &str) -> HashSet<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| word.chars().count() >= 3)
        .filter(|word| !STOP_WORDS.contains(&word.as_str()))
        .collect()
}

fn lexical_score(query_terms: &HashSet<String>, candidate: &str) -> usize {
    let candidate_terms = tokenize(candidate);
    query_terms.intersection(&candidate_terms).count()
}

fn is_expired(valid_until: Option<&str>, now: DateTime<Utc>) -> bool {
    valid_until
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .is_some_and(|value| value.with_timezone(&Utc) < now)
}

fn validate_text(path: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        return Err(anyhow!("{path} must not be empty"));
    }
    if value.len() > MAX_STRING_BYTES {
        return Err(anyhow!("{path} exceeds the {MAX_STRING_BYTES} byte limit"));
    }
    Ok(())
}

fn validate_source(path: &str, source: &IdentitySource) -> Result<()> {
    validate_text(&format!("{path}.label"), &source.label)?;
    validate_text(&format!("{path}.revision"), &source.revision)
}

fn validate_tags(path: &str, tags: &[String]) -> Result<()> {
    if tags.len() > MAX_TAGS {
        return Err(anyhow!("{path} exceeds the {MAX_TAGS} item limit"));
    }
    let mut unique = HashSet::new();
    for (index, tag) in tags.iter().enumerate() {
        validate_text(&format!("{path}[{index}]"), tag)?;
        if !unique.insert(tag.trim().to_lowercase()) {
            return Err(anyhow!("{path} contains duplicate tag '{tag}'"));
        }
    }
    Ok(())
}

fn validate_timestamp(path: &str, value: &str) -> Result<()> {
    DateTime::parse_from_rfc3339(value)
        .map(|_| ())
        .map_err(|_| anyhow!("{path} must be an RFC 3339 timestamp"))
}

fn validate_optional_timestamp(path: &str, value: Option<&str>) -> Result<()> {
    value.map_or(Ok(()), |value| validate_timestamp(path, value))
}

fn validate_optional_text(path: &str, value: Option<&str>) -> Result<()> {
    value.map_or(Ok(()), |value| validate_text(path, value))
}

const STOP_WORDS: &[&str] = &[
    "and", "are", "but", "for", "from", "how", "into", "our", "that", "the", "their", "this",
    "was", "what", "when", "where", "which", "while", "with", "you", "your",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_identity() -> ProfessionalIdentityVersion {
        ProfessionalIdentityVersion {
            schema_version: PROFESSIONAL_IDENTITY_SCHEMA_VERSION,
            identity: ProfessionalIdentityHeader {
                display_name: "Ghassan Aqrabawi".to_string(),
                role_title: "Head of Mission".to_string(),
                organization: "Example Mission".to_string(),
                professional_summary: "I lead a small mission team.".to_string(),
            },
            records: vec![IdentityRecord {
                id: Uuid::from_u128(1),
                category: IdentityRecordCategory::TermsOfReference,
                title: "Duty of care".to_string(),
                content:
                    "I maintain staff safety through weekly check-ins and clear escalation routes."
                        .to_string(),
                source: IdentitySource {
                    label: "Head of Mission TOR".to_string(),
                    revision: "2026-08".to_string(),
                },
                updated_at: "2026-08-12T00:00:00Z".to_string(),
                valid_until: None,
                conflict_key: None,
                tags: vec!["staff".to_string(), "safety".to_string()],
            }],
            projects: vec![],
        }
    }

    #[test]
    fn identity_hash_is_stable_and_domain_separated() {
        let identity = sample_identity();
        let first = hash_identity_version(&identity).unwrap();
        let second = hash_identity_version(&identity).unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with("sha256:"));
    }

    #[test]
    fn retrieval_returns_real_source_metadata_for_matching_current_records() {
        let identity = sample_identity();
        let result = retrieve_identity_context(
            &identity,
            "How do I maintain staff duty of care and safety?",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].label, "Head of Mission TOR");
        assert!(result.prompt_json.contains("weekly check-ins"));
    }

    #[test]
    fn expired_records_are_excluded_from_prompt_and_grounding() {
        let mut identity = sample_identity();
        identity.records[0].valid_until = Some("2026-08-17T00:00:00Z".to_string());
        let result = retrieve_identity_context(
            &identity,
            "How do I maintain staff duty of care and safety?",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        assert!(result.sources.is_empty());
        assert!(!result.prompt_json.contains("weekly check-ins"));
    }

    #[test]
    fn current_records_with_the_same_explicit_key_are_marked_as_conflicting() {
        let mut identity = sample_identity();
        identity.records[0].conflict_key = Some("staff-check-in-cadence".to_string());
        let mut conflicting = identity.records[0].clone();
        conflicting.id = Uuid::from_u128(2);
        conflicting.content = "I run monthly staff check-ins.".to_string();
        identity.records.push(conflicting);
        let result = retrieve_identity_context(
            &identity,
            "How often do I run staff check-ins?",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        assert_eq!(result.sources.len(), 2);
        assert!(result.prompt_json.contains("\"conflicting_current_sources\":true"));
    }

    #[test]
    fn unknown_or_executable_shaped_fields_are_rejected_by_the_closed_schema() {
        let mut value = serde_json::to_value(sample_identity()).unwrap();
        value["script"] = serde_json::json!("powershell.exe");
        assert!(parse_identity_json(&value.to_string()).is_err());
    }
}
