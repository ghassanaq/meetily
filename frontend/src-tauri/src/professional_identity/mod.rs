//! Versioned, declarative professional identity data for Live Assist.
//!
//! Identity content is inert local data. The schema intentionally exposes no
//! executable hooks, tools, filesystem paths, provider endpoints, or permission
//! grants. Retrieval returns source metadata alongside prompt context so the UI
//! never relies on model-authored provenance.

pub(crate) mod authority_scope;
pub mod authority_scope_repository;
pub mod commands;
mod composition;
pub mod markdown_import;
pub mod repository;

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::expert_profiles::hashing::canonical_json;

pub const MIN_PROFESSIONAL_IDENTITY_SCHEMA_VERSION: u32 = 1;
pub const PROFESSIONAL_IDENTITY_SCHEMA_VERSION: u32 = 2;
const IDENTITY_HASH_DOMAIN: &[u8] = b"meetily-professional-identity-v1\0";
const MAX_JSON_BYTES: usize = 1024 * 1024;
const MAX_RECORDS: usize = 256;
const MAX_PROJECTS: usize = 64;
const MAX_PROJECT_FACTS: usize = 128;
const MAX_TAGS: usize = 32;
const MAX_STRING_BYTES: usize = 32 * 1024;
const MAX_RETRIEVED_SOURCES: usize = 8;
const MAX_AUTHORITY_CONSTRAINTS: usize = 64;
const MAX_AUTHORITY_ALIASES: usize = 16;
const MAX_AUTHORITY_ALIAS_BYTES: usize = 128;
const MAX_AUTHORITY_LABEL_BYTES: usize = 256;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProfessionalIdentityVersion {
    pub schema_version: u32,
    pub identity: ProfessionalIdentityHeader,
    pub records: Vec<IdentityRecord>,
    pub projects: Vec<IdentityProject>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authority_constraints: Vec<AuthorityConstraint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AuthorityConstraint {
    pub id: String,
    pub label: String,
    pub contexts: Vec<String>,
    pub action_families: Vec<AuthorityActionFamily>,
    pub permitted_objects: Vec<String>,
    pub excluded_objects: Vec<String>,
    pub evidence_record_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityActionFamily {
    Manage,
    Lead,
    Own,
    Oversee,
    ResponsibleFor,
    Approve,
    Decide,
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
    pub authority_constraints: Vec<AuthorityConstraint>,
}

#[derive(Serialize)]
struct PromptIdentityContext<'a> {
    context_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    compose_profile: Option<&'static str>,
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
    composition_priority: u8,
    document_order: usize,
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
    if !(MIN_PROFESSIONAL_IDENTITY_SCHEMA_VERSION..=PROFESSIONAL_IDENTITY_SCHEMA_VERSION)
        .contains(&profile.schema_version)
    {
        return Err(anyhow!(
            "expected professional identity schema version {MIN_PROFESSIONAL_IDENTITY_SCHEMA_VERSION}..={PROFESSIONAL_IDENTITY_SCHEMA_VERSION}, got {}",
            profile.schema_version
        ));
    }
    if profile.schema_version == MIN_PROFESSIONAL_IDENTITY_SCHEMA_VERSION
        && !profile.authority_constraints.is_empty()
    {
        return Err(anyhow!(
            "professional identity schema version 1 cannot contain authority constraints"
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
    let mut record_ids = HashSet::new();
    for (index, record) in profile.records.iter().enumerate() {
        if !ids.insert(record.id) {
            return Err(anyhow!(
                "duplicate identity record UUID at records[{index}]"
            ));
        }
        record_ids.insert(record.id);
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
    validate_authority_constraints(&profile.authority_constraints, &record_ids)?;
    Ok(())
}

fn validate_authority_constraints(
    constraints: &[AuthorityConstraint],
    record_ids: &HashSet<Uuid>,
) -> Result<()> {
    if constraints.len() > MAX_AUTHORITY_CONSTRAINTS {
        return Err(anyhow!(
            "authority_constraints exceeds the {MAX_AUTHORITY_CONSTRAINTS} item limit"
        ));
    }
    let mut rule_ids = HashSet::new();
    for (index, rule) in constraints.iter().enumerate() {
        let path = format!("authority_constraints[{index}]");
        if rule.id.is_empty()
            || rule.id.len() > MAX_AUTHORITY_ALIAS_BYTES
            || !rule.id.bytes().all(|byte| {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            })
        {
            return Err(anyhow!(
                "{path}.id must use lowercase ASCII letters, digits, '_' or '-'"
            ));
        }
        if !rule_ids.insert(rule.id.clone()) {
            return Err(anyhow!("duplicate authority constraint id '{}'", rule.id));
        }
        validate_bounded_text(
            &format!("{path}.label"),
            &rule.label,
            MAX_AUTHORITY_LABEL_BYTES,
        )?;
        validate_authority_aliases(&format!("{path}.contexts"), &rule.contexts, false)?;
        let permitted = validate_authority_aliases(
            &format!("{path}.permitted_objects"),
            &rule.permitted_objects,
            true,
        )?;
        let excluded = validate_authority_aliases(
            &format!("{path}.excluded_objects"),
            &rule.excluded_objects,
            true,
        )?;
        if let Some(overlap) = permitted.intersection(&excluded).next() {
            return Err(anyhow!(
                "{path} contains object alias '{overlap}' in both permitted and excluded sets"
            ));
        }
        if rule.action_families.is_empty() || rule.action_families.len() > MAX_AUTHORITY_ALIASES {
            return Err(anyhow!(
                "{path}.action_families must contain 1..={MAX_AUTHORITY_ALIASES} items"
            ));
        }
        let unique_actions = rule.action_families.iter().copied().collect::<HashSet<_>>();
        if unique_actions.len() != rule.action_families.len() {
            return Err(anyhow!("{path}.action_families contains duplicates"));
        }
        if rule.evidence_record_ids.is_empty()
            || rule.evidence_record_ids.len() > MAX_AUTHORITY_ALIASES
        {
            return Err(anyhow!(
                "{path}.evidence_record_ids must contain 1..={MAX_AUTHORITY_ALIASES} items"
            ));
        }
        let unique_evidence = rule
            .evidence_record_ids
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        if unique_evidence.len() != rule.evidence_record_ids.len() {
            return Err(anyhow!("{path}.evidence_record_ids contains duplicates"));
        }
        if let Some(unknown) = unique_evidence.difference(record_ids).next() {
            return Err(anyhow!(
                "{path}.evidence_record_ids references unknown record {unknown}"
            ));
        }
    }
    Ok(())
}

fn validate_authority_aliases(
    path: &str,
    aliases: &[String],
    required: bool,
) -> Result<HashSet<String>> {
    if (required && aliases.is_empty()) || aliases.len() > MAX_AUTHORITY_ALIASES {
        let minimum = usize::from(required);
        return Err(anyhow!(
            "{path} must contain {minimum}..={MAX_AUTHORITY_ALIASES} items"
        ));
    }
    let mut normalized = HashSet::new();
    for (index, alias) in aliases.iter().enumerate() {
        validate_bounded_text(
            &format!("{path}[{index}]"),
            alias,
            MAX_AUTHORITY_ALIAS_BYTES,
        )?;
        let value = normalize_authority_alias(alias);
        if !normalized.insert(value) {
            return Err(anyhow!("{path} contains duplicate alias '{alias}'"));
        }
    }
    Ok(normalized)
}

fn validate_bounded_text(path: &str, value: &str, max_bytes: usize) -> Result<()> {
    if value.trim().is_empty() {
        return Err(anyhow!("{path} must not be empty"));
    }
    if value.len() > max_bytes {
        return Err(anyhow!("{path} exceeds the {max_bytes} byte limit"));
    }
    Ok(())
}

fn normalize_authority_alias(value: &str) -> String {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
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
    let mut document_order = 0usize;

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
                "{} {} {} {}",
                record.title,
                record.content,
                record.tags.join(" "),
                record.conflict_key.as_deref().unwrap_or_default()
            ),
            composition_priority: composition::evidence_priority(
                record.category,
                &record.title,
                &record.tags,
            ),
            document_order,
        });
        document_order += 1;
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
            composition_priority: 3,
            document_order,
        });
        document_order += 1;
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
                composition_priority: 3,
                document_order,
            });
            document_order += 1;
        }
    }

    let compose_professional_introduction = composition::is_professional_introduction(question);
    let conflicting_key_counts = candidates
        .iter()
        .filter(|candidate| !is_expired(candidate.valid_until, now))
        .filter_map(|candidate| candidate.conflict_key)
        .fold(std::collections::HashMap::new(), |mut counts, key| {
            *counts.entry(key.to_string()).or_insert(0usize) += 1;
            counts
        });
    let mut ranked: Vec<(usize, Candidate<'_>)> = if compose_professional_introduction {
        candidates
            .into_iter()
            .filter(|candidate| !is_expired(candidate.valid_until, now))
            .filter(|candidate| {
                candidate.conflict_key.map_or(true, |key| {
                    conflicting_key_counts
                        .get(key)
                        .map_or(true, |count| *count <= 1)
                })
            })
            .map(|candidate| (0, candidate))
            .collect()
    } else {
        candidates
            .into_iter()
            .filter(|candidate| !is_expired(candidate.valid_until, now))
            .map(|candidate| {
                (
                    lexical_score(&query_terms, &candidate.score_text),
                    candidate,
                )
            })
            .filter(|(score, _)| *score > 0)
            .collect()
    };
    let mut relevant_conflicting_keys = ranked
        .iter()
        .filter_map(|(_, candidate)| candidate.conflict_key)
        .filter(|key| {
            conflicting_key_counts
                .get(*key)
                .is_some_and(|count| *count > 1)
        })
        .map(str::to_string)
        .collect::<Vec<_>>();
    relevant_conflicting_keys.sort();
    relevant_conflicting_keys.dedup();
    if !relevant_conflicting_keys.is_empty() {
        return Err(anyhow!(
            "professional identity has conflicting current sources for: {}",
            relevant_conflicting_keys.join(", ")
        ));
    }
    if compose_professional_introduction {
        ranked.sort_by(|(_, left), (_, right)| {
            left.composition_priority
                .cmp(&right.composition_priority)
                .then_with(|| left.document_order.cmp(&right.document_order))
                .then_with(|| left.id.cmp(&right.id))
        });
        apply_composition_budget(&mut ranked);
    } else {
        ranked.sort_by(|(left_score, left), (right_score, right)| {
            right_score
                .cmp(left_score)
                .then_with(|| left.id.cmp(&right.id))
        });
        ranked.truncate(MAX_RETRIEVED_SOURCES);
    }

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
        context_type: if compose_professional_introduction {
            "professional_identity_composition"
        } else {
            "professional_identity"
        },
        compose_profile: compose_professional_introduction
            .then_some(composition::PROFESSIONAL_INTRODUCTION_PROFILE),
        identity: &profile.identity,
        records,
    })?;

    Ok(RetrievedIdentityContext {
        prompt_json,
        sources,
        authority_constraints: profile.authority_constraints.clone(),
    })
}

fn apply_composition_budget(ranked: &mut Vec<(usize, Candidate<'_>)>) {
    let mut remaining = composition::TOTAL_EVIDENCE_CHAR_BUDGET;
    let mut selected = 0usize;
    ranked.retain_mut(|(_, candidate)| {
        if selected >= MAX_RETRIEVED_SOURCES || remaining == 0 {
            return false;
        }
        let cap = remaining.min(composition::PER_RECORD_CHAR_BUDGET);
        let Some(excerpt) = composition::evidence_excerpt(&candidate.content, cap) else {
            return false;
        };
        remaining = remaining.saturating_sub(excerpt.chars().count());
        candidate.content = excerpt;
        selected += 1;
        true
    });
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
            authority_constraints: vec![],
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
    fn version_one_hash_remains_byte_compatible() {
        let mut identity = sample_identity();
        identity.schema_version = 1;
        assert_eq!(
            hash_identity_version(&identity).unwrap(),
            "sha256:046e189db04ff360e8c11764255fd590e9f29199fa14f3124008e57c98d7492a"
        );
    }

    #[test]
    fn authority_constraints_are_versioned_hashed_and_evidence_bound() {
        let mut identity = sample_identity();
        let mut second_record = identity.records[0].clone();
        second_record.id = Uuid::from_u128(2);
        identity.records.push(second_record);
        identity.authority_constraints.push(AuthorityConstraint {
            id: "workstream-boundary".to_string(),
            label: "Workstream boundary".to_string(),
            contexts: vec![],
            action_families: vec![AuthorityActionFamily::Manage],
            permitted_objects: vec!["processing workstream".to_string()],
            excluded_objects: vec!["whole operation".to_string()],
            evidence_record_ids: vec![Uuid::from_u128(1)],
        });
        let first = hash_identity_version(&identity).unwrap();
        let json = serde_json::to_string(&identity).unwrap();
        assert_eq!(parse_identity_json(&json).unwrap(), identity);

        identity.authority_constraints[0].excluded_objects[0] = "entire mission".to_string();
        assert_ne!(first, hash_identity_version(&identity).unwrap());
        identity.authority_constraints[0].excluded_objects[0] = "whole operation".to_string();
        identity.authority_constraints[0].action_families[0] = AuthorityActionFamily::Lead;
        assert_ne!(first, hash_identity_version(&identity).unwrap());
        identity.authority_constraints[0].action_families[0] = AuthorityActionFamily::Manage;
        identity.authority_constraints[0].evidence_record_ids[0] = Uuid::from_u128(2);
        assert_ne!(first, hash_identity_version(&identity).unwrap());

        identity.schema_version = 1;
        assert!(validate_identity(&identity).is_err());
        identity.schema_version = 2;
        identity.authority_constraints[0].evidence_record_ids[0] = Uuid::from_u128(99);
        assert!(validate_identity(&identity).is_err());
    }

    #[test]
    fn authority_constraints_reject_duplicates_overlap_and_unknown_fields() {
        let mut identity = sample_identity();
        let rule = AuthorityConstraint {
            id: "workstream-boundary".to_string(),
            label: "Workstream boundary".to_string(),
            contexts: vec!["Tripoli".to_string()],
            action_families: vec![AuthorityActionFamily::Manage],
            permitted_objects: vec!["processing workstream".to_string()],
            excluded_objects: vec!["whole operation".to_string()],
            evidence_record_ids: vec![Uuid::from_u128(1)],
        };
        identity.authority_constraints = vec![rule.clone(), rule.clone()];
        assert!(validate_identity(&identity).is_err());

        identity.authority_constraints = vec![rule];
        identity.authority_constraints[0].excluded_objects =
            vec!["Processing, workstream!".to_string()];
        assert!(validate_identity(&identity).is_err());

        let mut json = serde_json::to_value(sample_identity()).unwrap();
        json.as_object_mut()
            .unwrap()
            .insert("unexpected".to_string(), serde_json::json!(true));
        assert!(parse_identity_json(&serde_json::to_string(&json).unwrap()).is_err());
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
        assert!(result
            .prompt_json
            .contains(r#""context_type":"professional_identity""#));
        assert!(!result.prompt_json.contains("compose_profile"));
    }

    #[test]
    fn professional_introduction_composes_a_career_brief_in_document_order() {
        let mut identity = sample_identity();
        identity.records.extend([
            IdentityRecord {
                id: Uuid::from_u128(2),
                category: IdentityRecordCategory::Cv,
                title: "Earlier programme delivery".to_string(),
                content: "I coordinated regional programme delivery and partner reporting."
                    .to_string(),
                source: IdentitySource {
                    label: "Professional CV".to_string(),
                    revision: "2026-08".to_string(),
                },
                updated_at: "2026-08-12T00:00:00Z".to_string(),
                valid_until: None,
                conflict_key: None,
                tags: vec!["experience".to_string(), "programme".to_string()],
            },
            IdentityRecord {
                id: Uuid::from_u128(3),
                category: IdentityRecordCategory::Cv,
                title: "Current operations leadership".to_string(),
                content: "I lead daily operations, train colleagues, and resolve complex cases."
                    .to_string(),
                source: IdentitySource {
                    label: "Professional CV".to_string(),
                    revision: "2026-08".to_string(),
                },
                updated_at: "2026-08-12T00:00:00Z".to_string(),
                valid_until: None,
                conflict_key: None,
                tags: vec!["leadership".to_string(), "operations".to_string()],
            },
        ]);

        let result = retrieve_identity_context(
            &identity,
            "Tell us about yourself.",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        let prompt: serde_json::Value = serde_json::from_str(&result.prompt_json).unwrap();
        assert_eq!(prompt["context_type"], "professional_identity_composition");
        assert_eq!(
            prompt["compose_profile"],
            composition::PROFESSIONAL_INTRODUCTION_PROFILE
        );
        assert_eq!(result.sources.len(), 3);
        assert_eq!(result.sources[0].record_id, Uuid::from_u128(2));
        assert_eq!(result.sources[1].record_id, Uuid::from_u128(3));
        assert!(result.prompt_json.contains("regional programme delivery"));
        assert!(result.prompt_json.contains("lead daily operations"));
    }

    #[test]
    fn imported_context_routes_a_broad_introduction_to_professional_background() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/project_context/meeting-assistant.context.json");
        let imported =
            markdown_import::load_context_manifest(&manifest, Some(sample_identity().identity))
                .unwrap();
        let result = retrieve_identity_context(
            &imported.identity,
            "Tell us about yourself.",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        let prompt: serde_json::Value = serde_json::from_str(&result.prompt_json).unwrap();
        assert_eq!(prompt["context_type"], "professional_identity_composition");
        assert!(result
            .prompt_json
            .contains("twelve years of leadership experience"));
        assert_eq!(result.sources.first().unwrap().label, "Professional CV");
        assert!(!result.prompt_json.contains("20,000 USD"));
        assert!(!result.prompt_json.contains("30,000 USD"));
    }

    #[test]
    fn composition_caps_each_record_and_the_total_evidence_budget() {
        let mut identity = sample_identity();
        identity.records.clear();
        let sentence = "I coordinated a documented operational workstream with partners. ";
        for id in 1..=8 {
            identity.records.push(IdentityRecord {
                id: Uuid::from_u128(id),
                category: IdentityRecordCategory::Cv,
                title: format!("Career evidence {id}"),
                content: sentence.repeat(40),
                source: IdentitySource {
                    label: format!("CV section {id}"),
                    revision: "current".to_string(),
                },
                updated_at: "2026-08-12T00:00:00Z".to_string(),
                valid_until: None,
                conflict_key: None,
                tags: vec!["experience".to_string()],
            });
        }

        let result = retrieve_identity_context(
            &identity,
            "Could you walk me through your background?",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        let prompt: serde_json::Value = serde_json::from_str(&result.prompt_json).unwrap();
        let records = prompt["records"].as_array().unwrap();
        let total = records
            .iter()
            .map(|record| record["content"].as_str().unwrap().chars().count())
            .sum::<usize>();
        assert!(records.len() <= MAX_RETRIEVED_SOURCES);
        assert!(records.iter().all(|record| {
            record["content"].as_str().unwrap().chars().count()
                <= composition::PER_RECORD_CHAR_BUDGET
        }));
        assert!(total <= composition::TOTAL_EVIDENCE_CHAR_BUDGET);
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
    fn relevant_current_records_with_the_same_explicit_key_fail_closed() {
        let mut identity = sample_identity();
        identity.records[0].conflict_key = Some("cadence-rule".to_string());
        let mut conflicting = identity.records[0].clone();
        conflicting.id = Uuid::from_u128(2);
        conflicting.title = "Alternative schedule".to_string();
        conflicting.content = "I use a monthly rhythm.".to_string();
        conflicting.tags = vec!["frequency".to_string()];
        identity.records.push(conflicting);
        let error = retrieve_identity_context(
            &identity,
            "How often do I run staff check-ins?",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap_err();
        assert!(error
            .to_string()
            .contains("conflicting current sources for: cadence-rule"));
    }

    #[test]
    fn unrelated_conflicts_do_not_block_or_enter_the_prompt() {
        let mut identity = sample_identity();
        for (id, amount) in [(2, "USD 25,000"), (3, "USD 50,000")] {
            identity.records.push(IdentityRecord {
                id: Uuid::from_u128(id),
                category: IdentityRecordCategory::Authority,
                title: "Procurement approval limit".to_string(),
                content: format!("I may approve procurement up to {amount}."),
                source: IdentitySource {
                    label: format!("Delegation schedule {id}"),
                    revision: "current".to_string(),
                },
                updated_at: "2026-08-12T00:00:00Z".to_string(),
                valid_until: None,
                conflict_key: Some("procurement-approval-limit".to_string()),
                tags: vec!["procurement".to_string(), "finance".to_string()],
            });
        }
        let result = retrieve_identity_context(
            &identity,
            "How do I maintain staff duty of care and safety?",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].record_id, Uuid::from_u128(1));
        assert!(!result.prompt_json.contains("procurement"));
        assert!(!result.prompt_json.contains("25,000"));
        assert!(!result.prompt_json.contains("50,000"));
    }

    #[test]
    fn retrieval_excludes_non_matching_facts() {
        let mut identity = sample_identity();
        identity.records.push(IdentityRecord {
            id: Uuid::from_u128(2),
            category: IdentityRecordCategory::Commitment,
            title: "Vehicle replacement".to_string(),
            content: "I committed to replace the field vehicle in November.".to_string(),
            source: IdentitySource {
                label: "Fleet plan".to_string(),
                revision: "2026-Q3".to_string(),
            },
            updated_at: "2026-08-12T00:00:00Z".to_string(),
            valid_until: None,
            conflict_key: None,
            tags: vec!["fleet".to_string()],
        });
        let result = retrieve_identity_context(
            &identity,
            "How do I maintain staff duty of care and safety?",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        assert_eq!(result.sources.len(), 1);
        assert!(!result.prompt_json.contains("field vehicle"));
        assert!(!result.prompt_json.contains("Fleet plan"));
    }

    #[test]
    fn retrieval_cap_is_deterministic_for_large_profiles() {
        let mut identity = sample_identity();
        for id in 2..=12 {
            let mut record = identity.records[0].clone();
            record.id = Uuid::from_u128(id);
            record.source.label = format!("Safety source {id}");
            identity.records.push(record);
        }
        let result = retrieve_identity_context(
            &identity,
            "How do I maintain staff safety?",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        assert_eq!(result.sources.len(), MAX_RETRIEVED_SOURCES);
        assert_eq!(result.sources[0].record_id, Uuid::from_u128(1));
        assert_eq!(result.sources[7].record_id, Uuid::from_u128(8));
    }

    #[test]
    fn expired_project_excludes_the_project_and_its_facts() {
        let mut identity = sample_identity();
        identity.projects.push(IdentityProject {
            id: Uuid::from_u128(2),
            name: "Project Atlas".to_string(),
            role: "Sponsor".to_string(),
            status: "Delivery due in September".to_string(),
            source: IdentitySource {
                label: "Atlas plan".to_string(),
                revision: "2026-07".to_string(),
            },
            updated_at: "2026-07-01T00:00:00Z".to_string(),
            valid_until: Some("2026-08-17T00:00:00Z".to_string()),
            tags: vec!["delivery".to_string()],
            facts: vec![IdentityProjectFact {
                id: Uuid::from_u128(3),
                content: "The Atlas delivery deadline is September 15.".to_string(),
                source: IdentitySource {
                    label: "Atlas milestone table".to_string(),
                    revision: "2026-07".to_string(),
                },
                conflict_key: None,
                tags: vec!["deadline".to_string()],
            }],
        });
        let result = retrieve_identity_context(
            &identity,
            "What is the Atlas delivery deadline?",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        assert!(result.sources.is_empty());
        assert!(!result.prompt_json.contains("Atlas"));
        assert!(!result.prompt_json.contains("September 15"));
    }

    #[test]
    fn grounding_metadata_matches_the_records_in_the_prompt() {
        let mut identity = sample_identity();
        let mut second = identity.records[0].clone();
        second.id = Uuid::from_u128(2);
        second.title = "Staff escalation route".to_string();
        second.content =
            "I use the security focal point for urgent staff safety escalation.".to_string();
        second.source = IdentitySource {
            label: "Security protocol".to_string(),
            revision: "2026-08".to_string(),
        };
        identity.records.push(second);
        let result = retrieve_identity_context(
            &identity,
            "How do I handle staff safety escalation?",
            DateTime::parse_from_rfc3339("2026-08-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        let prompt: serde_json::Value = serde_json::from_str(&result.prompt_json).unwrap();
        let prompt_records = prompt["records"].as_array().unwrap();
        assert_eq!(prompt_records.len(), result.sources.len());
        for (record, source) in prompt_records.iter().zip(&result.sources) {
            assert_eq!(record["id"].as_str().unwrap(), source.record_id.to_string());
            assert_eq!(record["source_label"].as_str().unwrap(), source.label);
            assert_eq!(record["source_revision"].as_str().unwrap(), source.revision);
            assert_eq!(record["updated_at"].as_str().unwrap(), source.updated_at);
        }
    }

    #[test]
    fn unknown_or_executable_shaped_fields_are_rejected_by_the_closed_schema() {
        let mut value = serde_json::to_value(sample_identity()).unwrap();
        value["script"] = serde_json::json!("powershell.exe");
        assert!(parse_identity_json(&value.to_string()).is_err());
    }
}
