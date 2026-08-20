use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const CONTEXT_SCHEMA_VERSION: u32 = 1;
const DOCUMENT_SCHEMA_VERSION: u32 = 1;
const MAX_PASSAGE_WORDS: usize = 180;
const HASH_DOMAIN: &[u8] = b"meeting-assistant-project-passage-v1\0";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextManifest {
    schema_version: u32,
    context_id: String,
    name: String,
    identity_bundle: String,
    role_bundle: String,
    project_bundles: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    schema_version: u32,
    bundle_id: String,
    name: String,
    sources: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentMetadata {
    schema_version: u32,
    document_id: Uuid,
    source: String,
    revision: String,
    updated_at: String,
    valid_until: Option<String>,
    conflict_key: Option<String>,
    tags: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BundleScope {
    Person,
    Role,
    Project,
}

impl BundleScope {
    fn passage_kind(self) -> &'static str {
        match self {
            Self::Person => "person_fact",
            Self::Role => "role_policy",
            Self::Project => "project_fact",
        }
    }
}

#[derive(Debug)]
struct LoadedContext {
    context_id: String,
    name: String,
    passages: Vec<Passage>,
}

#[derive(Clone, Debug)]
struct Passage {
    passage_id: String,
    bundle_id: String,
    bundle_name: String,
    scope: BundleScope,
    kind: &'static str,
    source: String,
    revision: String,
    updated_at: DateTime<Utc>,
    valid_until: Option<DateTime<Utc>>,
    conflict_key: Option<String>,
    tags: Vec<String>,
    heading: String,
    content: String,
    content_hash: String,
}

#[derive(Debug)]
struct RankedPassage<'a> {
    passage: &'a Passage,
    score: usize,
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("project_context")
}

fn load_context(root: &Path, manifest_relative_path: &str) -> Result<LoadedContext> {
    let root = root
        .canonicalize()
        .with_context(|| format!("context root '{}' does not exist", root.display()))?;
    let manifest_path = resolve_existing_relative_path(&root, manifest_relative_path)?;
    let manifest: ContextManifest = parse_json_file(&manifest_path)?;
    if manifest.schema_version != CONTEXT_SCHEMA_VERSION {
        bail!(
            "unsupported context schema version {}",
            manifest.schema_version
        );
    }
    validate_nonempty("context_id", &manifest.context_id)?;
    validate_nonempty("name", &manifest.name)?;

    let mut passages = Vec::new();
    passages.extend(load_bundle(
        &root,
        &manifest.identity_bundle,
        BundleScope::Person,
    )?);
    passages.extend(load_bundle(
        &root,
        &manifest.role_bundle,
        BundleScope::Role,
    )?);
    for bundle in &manifest.project_bundles {
        passages.extend(load_bundle(&root, bundle, BundleScope::Project)?);
    }

    let mut passage_ids = HashSet::new();
    for passage in &passages {
        if !passage_ids.insert(passage.passage_id.clone()) {
            bail!("duplicate passage ID '{}'", passage.passage_id);
        }
    }

    Ok(LoadedContext {
        context_id: manifest.context_id,
        name: manifest.name,
        passages,
    })
}

fn load_bundle(
    root: &Path,
    bundle_relative_path: &str,
    scope: BundleScope,
) -> Result<Vec<Passage>> {
    let bundle_path = resolve_existing_relative_path(root, bundle_relative_path)?;
    let bundle: BundleManifest = parse_json_file(&bundle_path)?;
    if bundle.schema_version != CONTEXT_SCHEMA_VERSION {
        bail!(
            "unsupported bundle schema version {} in '{}'",
            bundle.schema_version,
            bundle_path.display()
        );
    }
    validate_nonempty("bundle_id", &bundle.bundle_id)?;
    validate_nonempty("bundle name", &bundle.name)?;
    if bundle.sources.is_empty() {
        bail!("bundle '{}' has no Markdown sources", bundle.bundle_id);
    }

    let bundle_directory = bundle_path
        .parent()
        .ok_or_else(|| anyhow!("bundle path has no parent"))?;
    let mut passages = Vec::new();
    for source in &bundle.sources {
        let source_path = resolve_existing_child_path(root, bundle_directory, source)?;
        if source_path.extension().and_then(|value| value.to_str()) != Some("md") {
            bail!("bundle source '{}' is not Markdown", source_path.display());
        }
        let markdown = fs::read_to_string(&source_path)
            .with_context(|| format!("failed to read '{}'", source_path.display()))?;
        passages.extend(parse_markdown(
            &markdown,
            &bundle.bundle_id,
            &bundle.name,
            scope,
        )?);
    }
    Ok(passages)
}

fn parse_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T> {
    let json =
        fs::read_to_string(path).with_context(|| format!("failed to read '{}'", path.display()))?;
    serde_json::from_str(&json).with_context(|| format!("invalid JSON in '{}'", path.display()))
}

fn resolve_existing_relative_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let relative_path = Path::new(relative);
    ensure_safe_relative_path(relative_path)?;
    let resolved = root
        .join(relative_path)
        .canonicalize()
        .with_context(|| format!("referenced path '{}' does not exist", relative))?;
    if !resolved.starts_with(root) {
        bail!("referenced path '{}' escapes the context root", relative);
    }
    Ok(resolved)
}

fn resolve_existing_child_path(root: &Path, parent: &Path, relative: &str) -> Result<PathBuf> {
    let relative_path = Path::new(relative);
    ensure_safe_relative_path(relative_path)?;
    let resolved = parent
        .join(relative_path)
        .canonicalize()
        .with_context(|| format!("referenced source '{}' does not exist", relative))?;
    if !resolved.starts_with(root) {
        bail!("referenced source '{}' escapes the context root", relative);
    }
    Ok(resolved)
}

fn ensure_safe_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        bail!("manifest paths must be nonempty and relative");
    }
    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("manifest paths may not traverse outside the context root");
    }
    Ok(())
}

fn parse_markdown(
    input: &str,
    bundle_id: &str,
    bundle_name: &str,
    scope: BundleScope,
) -> Result<Vec<Passage>> {
    let normalized = input.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| anyhow!("Markdown source must start with YAML frontmatter"))?;
    let end = rest
        .find("\n---\n")
        .ok_or_else(|| anyhow!("Markdown source has unterminated YAML frontmatter"))?;
    let metadata: DocumentMetadata = serde_yaml_ng::from_str(&rest[..end])
        .context("invalid or unsupported Markdown frontmatter")?;
    validate_metadata(&metadata)?;
    let body = &rest[end + "\n---\n".len()..];

    let mut passages = Vec::new();
    let mut breadcrumb = Vec::<String>::new();
    let mut current_heading = metadata.source.clone();
    let mut current_lines = Vec::<String>::new();

    let flush =
        |heading: &str, lines: &mut Vec<String>, passages: &mut Vec<Passage>| -> Result<()> {
            let body = lines.join("\n");
            lines.clear();
            for (chunk_index, content) in split_passage_body(&body, MAX_PASSAGE_WORDS)
                .into_iter()
                .enumerate()
            {
                if content.is_empty() {
                    continue;
                }
                let base = format!("{}::{}", metadata.document_id, slugify(heading));
                let passage_id = if chunk_index == 0 {
                    base
                } else {
                    format!("{base}-part-{}", chunk_index + 1)
                };
                let content_hash = hash_passage(
                    scope.passage_kind(),
                    bundle_id,
                    &metadata,
                    heading,
                    chunk_index,
                    &content,
                );
                passages.push(Passage {
                    passage_id,
                    bundle_id: bundle_id.to_string(),
                    bundle_name: bundle_name.to_string(),
                    scope,
                    kind: scope.passage_kind(),
                    source: metadata.source.clone(),
                    revision: metadata.revision.clone(),
                    updated_at: parse_timestamp("updated_at", &metadata.updated_at)?,
                    valid_until: metadata
                        .valid_until
                        .as_deref()
                        .map(|value| parse_timestamp("valid_until", value))
                        .transpose()?,
                    conflict_key: metadata.conflict_key.clone(),
                    tags: metadata.tags.clone(),
                    heading: heading.to_string(),
                    content,
                    content_hash,
                });
            }
            Ok(())
        };

    for line in body.lines() {
        if let Some((level, title)) = markdown_heading(line) {
            flush(&current_heading, &mut current_lines, &mut passages)?;
            breadcrumb.truncate(level.saturating_sub(1));
            breadcrumb.push(title.to_string());
            current_heading = breadcrumb.join(" > ");
        } else {
            current_lines.push(line.to_string());
        }
    }
    flush(&current_heading, &mut current_lines, &mut passages)?;
    if passages.is_empty() {
        bail!(
            "Markdown source '{}' has no passage content",
            metadata.source
        );
    }
    Ok(passages)
}

fn validate_metadata(metadata: &DocumentMetadata) -> Result<()> {
    if metadata.schema_version != DOCUMENT_SCHEMA_VERSION {
        bail!(
            "unsupported Markdown schema version {}",
            metadata.schema_version
        );
    }
    validate_nonempty("source", &metadata.source)?;
    validate_nonempty("revision", &metadata.revision)?;
    parse_timestamp("updated_at", &metadata.updated_at)?;
    if let Some(valid_until) = metadata.valid_until.as_deref() {
        parse_timestamp("valid_until", valid_until)?;
    }
    if let Some(conflict_key) = metadata.conflict_key.as_deref() {
        validate_nonempty("conflict_key", conflict_key)?;
    }
    let mut tags = HashSet::new();
    for tag in &metadata.tags {
        validate_nonempty("tag", tag)?;
        if !tags.insert(tag.to_lowercase()) {
            bail!("duplicate Markdown tag '{tag}'");
        }
    }
    Ok(())
}

fn validate_nonempty(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

fn parse_timestamp(field: &str, value: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|timestamp| timestamp.with_timezone(&Utc))
        .with_context(|| format!("{field} must be an RFC 3339 timestamp"))
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let title = line.get(hashes..)?.strip_prefix(' ')?.trim();
    (!title.is_empty()).then_some((hashes, title))
}

fn split_passage_body(body: &str, max_words: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = Vec::<String>::new();
    let mut current_words = 0usize;

    for paragraph in body.split("\n\n") {
        let normalized = paragraph.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.is_empty() {
            continue;
        }
        let paragraph_words = normalized.split_whitespace().count();
        if paragraph_words > max_words {
            if !current.is_empty() {
                chunks.push(current.join("\n\n"));
                current.clear();
                current_words = 0;
            }
            let words = normalized.split_whitespace().collect::<Vec<_>>();
            chunks.extend(words.chunks(max_words).map(|chunk| chunk.join(" ")));
            continue;
        }
        if current_words + paragraph_words > max_words && !current.is_empty() {
            chunks.push(current.join("\n\n"));
            current.clear();
            current_words = 0;
        }
        current.push(normalized);
        current_words += paragraph_words;
    }
    if !current.is_empty() {
        chunks.push(current.join("\n\n"));
    }
    chunks
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            pending_dash = false;
        } else {
            pending_dash = true;
        }
    }
    slug
}

fn hash_passage(
    kind: &str,
    bundle_id: &str,
    metadata: &DocumentMetadata,
    heading: &str,
    chunk_index: usize,
    content: &str,
) -> String {
    let mut digest = Sha256::new();
    digest.update(HASH_DOMAIN);
    for value in [
        kind,
        bundle_id,
        &metadata.document_id.to_string(),
        &metadata.source,
        &metadata.revision,
        &metadata.updated_at,
        metadata.valid_until.as_deref().unwrap_or_default(),
        metadata.conflict_key.as_deref().unwrap_or_default(),
        heading,
        &chunk_index.to_string(),
        content,
    ] {
        digest.update(value.as_bytes());
        digest.update([0]);
    }
    for tag in &metadata.tags {
        digest.update(tag.as_bytes());
        digest.update([0]);
    }
    format!("sha256:{:x}", digest.finalize())
}

fn retrieve<'a>(
    passages: &'a [Passage],
    question: &str,
    now: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<RankedPassage<'a>>> {
    let query_terms = tokenize(question);
    let current = passages
        .iter()
        .filter(|passage| passage.valid_until.map_or(true, |until| until >= now))
        .collect::<Vec<_>>();
    let conflict_counts = current
        .iter()
        .filter_map(|passage| passage.conflict_key.as_deref())
        .fold(HashMap::<&str, usize>::new(), |mut counts, key| {
            *counts.entry(key).or_default() += 1;
            counts
        });

    let mut ranked = current
        .into_iter()
        .filter_map(|passage| {
            let score = passage_score(&query_terms, passage);
            (score > 0).then_some(RankedPassage { passage, score })
        })
        .collect::<Vec<_>>();

    let mut relevant_conflicts = ranked
        .iter()
        .filter_map(|item| item.passage.conflict_key.as_deref())
        .filter(|key| conflict_key_matches(&query_terms, key))
        .filter(|key| conflict_counts.get(key).is_some_and(|count| *count > 1))
        .collect::<Vec<_>>();
    relevant_conflicts.sort_unstable();
    relevant_conflicts.dedup();
    if !relevant_conflicts.is_empty() {
        bail!(
            "project context has conflicting current sources for: {}",
            relevant_conflicts.join(", ")
        );
    }

    ranked.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.passage.passage_id.cmp(&right.passage.passage_id))
    });
    if let Some(best_score) = ranked.first().map(|item| item.score) {
        ranked.retain(|item| item.score.saturating_mul(2) >= best_score);
    }
    ranked.truncate(limit);
    Ok(ranked)
}

fn conflict_key_matches(query_terms: &HashSet<String>, conflict_key: &str) -> bool {
    let key_terms = tokenize(conflict_key);
    let required_matches = key_terms.len().min(2);
    required_matches > 0 && query_terms.intersection(&key_terms).count() >= required_matches
}

fn passage_score(query_terms: &HashSet<String>, passage: &Passage) -> usize {
    weighted_overlap(query_terms, &passage.heading, 5)
        + weighted_overlap(query_terms, &passage.tags.join(" "), 4)
        + weighted_overlap(
            query_terms,
            &format!("{} {}", passage.bundle_name, passage.source),
            3,
        )
        + weighted_overlap(query_terms, &passage.content, 1)
}

fn weighted_overlap(query_terms: &HashSet<String>, value: &str, weight: usize) -> usize {
    query_terms.intersection(&tokenize(value)).count() * weight
}

fn tokenize(value: &str) -> HashSet<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| word.chars().count() >= 3)
        .filter(|word| !STOP_WORDS.contains(&word.as_str()))
        .collect()
}

const STOP_WORDS: &[&str] = &[
    "and", "are", "but", "for", "from", "how", "into", "our", "that", "the", "their", "this",
    "under", "was", "what", "when", "where", "which", "while", "with", "you", "your",
];

struct RetrievalCase {
    name: &'static str,
    question: &'static str,
    expected: &'static [&'static str],
    relevant: &'static [&'static str],
    exclusive_project: Option<&'static str>,
}

const PERSON_BACKGROUND: &str = "11111111-1111-4111-8111-111111111111::professional-background";
const ROLE_DUTIES: &str = "22222222-2222-4222-8222-222222222222::core-leadership-duties";
const ROLE_AUTHORITY: &str = "33333333-3333-4333-8333-333333333333::procurement-approval-authority";
const ROLE_SAFETY: &str = "44444444-4444-4444-8444-444444444444::staff-safety-and-escalation";
const ATLAS_STATUS: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa::atlas-rollout-status";
const ATLAS_COMMITMENT: &str = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb::atlas-training-commitment";
const ATLAS_RISK: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc::atlas-data-migration-risk";
const BEACON_STATUS: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd::beacon-partner-onboarding";
const BEACON_PARTNERSHIP: &str = "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee::beacon-partnership-method";

fn cases() -> Vec<RetrievalCase> {
    vec![
        RetrievalCase {
            name: "person leadership experience",
            question: "What leadership experience do I bring to this role?",
            expected: &[PERSON_BACKGROUND],
            relevant: &[PERSON_BACKGROUND, ROLE_DUTIES],
            exclusive_project: None,
        },
        RetrievalCase {
            name: "role duty of care",
            question: "How do I maintain duty of care for staff under pressure?",
            expected: &[ROLE_SAFETY],
            relevant: &[ROLE_SAFETY, ROLE_DUTIES],
            exclusive_project: None,
        },
        RetrievalCase {
            name: "role approval authority",
            question: "What is my procurement approval authority and financial limit?",
            expected: &[ROLE_AUTHORITY],
            relevant: &[ROLE_AUTHORITY],
            exclusive_project: None,
        },
        RetrievalCase {
            name: "role security escalation",
            question: "Who do I contact for an urgent staff security escalation?",
            expected: &[ROLE_SAFETY],
            relevant: &[ROLE_SAFETY],
            exclusive_project: None,
        },
        RetrievalCase {
            name: "Atlas delivery status",
            question: "When is the Atlas field rollout due and what is its current status?",
            expected: &[ATLAS_STATUS],
            relevant: &[ATLAS_STATUS],
            exclusive_project: Some("atlas"),
        },
        RetrievalCase {
            name: "Atlas training commitment",
            question: "What commitment have I made about Atlas staff training?",
            expected: &[ATLAS_COMMITMENT],
            relevant: &[ATLAS_COMMITMENT, ATLAS_STATUS],
            exclusive_project: Some("atlas"),
        },
        RetrievalCase {
            name: "Atlas migration risk",
            question: "What is the main Atlas data migration risk and mitigation?",
            expected: &[ATLAS_RISK],
            relevant: &[ATLAS_RISK],
            exclusive_project: Some("atlas"),
        },
        RetrievalCase {
            name: "Beacon onboarding status",
            question: "What is the current Beacon partner onboarding status?",
            expected: &[BEACON_STATUS],
            relevant: &[BEACON_STATUS, BEACON_PARTNERSHIP],
            exclusive_project: Some("beacon"),
        },
        RetrievalCase {
            name: "Beacon partnership method",
            question: "How should I manage external partners in Beacon?",
            expected: &[BEACON_PARTNERSHIP],
            relevant: &[BEACON_PARTNERSHIP, BEACON_STATUS],
            exclusive_project: Some("beacon"),
        },
        RetrievalCase {
            name: "cross-project dependencies",
            question: "How do the Atlas rollout and Beacon partner onboarding affect each other?",
            expected: &[ATLAS_STATUS, BEACON_STATUS],
            relevant: &[ATLAS_STATUS, BEACON_STATUS, BEACON_PARTNERSHIP],
            exclusive_project: None,
        },
    ]
}

fn fixed_now() -> DateTime<Utc> {
    parse_timestamp("now", "2026-08-20T00:00:00Z").unwrap()
}

#[test]
fn ten_question_fixture_meets_rank_precision_and_project_isolation_targets() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    assert_eq!(context.context_id, "head-of-mission-multi-project");
    assert_eq!(context.name, "Head of Mission — Atlas and Beacon");

    for limit in [3usize, 5, 8] {
        let max_irrelevant = match limit {
            3 => 1,
            5 => 2,
            8 => 3,
            _ => unreachable!(),
        };
        for case in cases() {
            let results = retrieve(&context.passages, case.question, fixed_now(), limit).unwrap();
            for expected in case.expected {
                let rank = results
                    .iter()
                    .position(|item| item.passage.passage_id == *expected)
                    .map(|index| index + 1)
                    .unwrap_or(usize::MAX);
                assert!(
                    rank <= 3,
                    "{} at limit {} placed expected passage {} at rank {:?}; selected {:?}",
                    case.name,
                    limit,
                    expected,
                    rank,
                    results
                        .iter()
                        .map(|item| item.passage.passage_id.as_str())
                        .collect::<Vec<_>>()
                );
            }
            let irrelevant = results
                .iter()
                .filter(|item| !case.relevant.contains(&item.passage.passage_id.as_str()))
                .count();
            assert!(
                irrelevant <= max_irrelevant,
                "{} at limit {} returned {} irrelevant passages; selected {:?}",
                case.name,
                limit,
                irrelevant,
                results
                    .iter()
                    .map(|item| item.passage.passage_id.as_str())
                    .collect::<Vec<_>>()
            );
            if let Some(project) = case.exclusive_project {
                let bleed = results.iter().any(|item| {
                    item.passage.scope == BundleScope::Project && item.passage.bundle_id != project
                });
                assert!(
                    !bleed,
                    "{} at limit {} leaked another project into {:?}",
                    case.name,
                    limit,
                    results
                        .iter()
                        .map(|item| item.passage.bundle_id.as_str())
                        .collect::<Vec<_>>()
                );
            }
            let words = results
                .iter()
                .map(|item| item.passage.content.split_whitespace().count())
                .sum::<usize>();
            println!(
                "limit={limit} case={} selected={} irrelevant={} words={} top={:?}",
                case.name,
                results.len(),
                irrelevant,
                words,
                results
                    .iter()
                    .map(|item| format!("{}@{}", item.passage.passage_id, item.score))
                    .collect::<Vec<_>>()
            );
        }
    }
}

#[test]
fn bundle_scope_derives_passage_kind_and_imperative_policy_stays_typed_data() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    assert!(context
        .passages
        .iter()
        .filter(|passage| passage.scope == BundleScope::Person)
        .all(|passage| passage.kind == "person_fact"));
    assert!(context
        .passages
        .iter()
        .filter(|passage| passage.scope == BundleScope::Role)
        .all(|passage| passage.kind == "role_policy"));
    assert!(context
        .passages
        .iter()
        .filter(|passage| passage.scope == BundleScope::Project)
        .all(|passage| passage.kind == "project_fact"));

    let authority = context
        .passages
        .iter()
        .find(|passage| passage.passage_id == ROLE_AUTHORITY)
        .unwrap();
    assert!(authority.content.starts_with("Always obtain"));
    assert_eq!(authority.kind, "role_policy");
    assert_eq!(authority.source, "Head of Mission delegation schedule");
    assert_eq!(authority.revision, "2026-08");
    assert!(authority.updated_at <= fixed_now());
    assert!(authority.content_hash.starts_with("sha256:"));
}

#[test]
fn expired_passages_are_excluded_before_scoring() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    let results = retrieve(
        &context.passages,
        "What was the legacy Atlas pilot travel ceiling?",
        fixed_now(),
        8,
    )
    .unwrap();
    assert!(results
        .iter()
        .all(|item| !item.passage.passage_id.starts_with("55555555-")));
}

#[test]
fn relevant_current_conflicts_block_before_selection_but_unrelated_conflicts_do_not() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    let error = retrieve(
        &context.passages,
        "What is the vehicle leasing approval ceiling?",
        fixed_now(),
        3,
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("conflicting current sources for: vehicle_leasing_ceiling"));

    let unrelated = retrieve(
        &context.passages,
        "How do I maintain duty of care for staff?",
        fixed_now(),
        3,
    )
    .unwrap();
    assert!(unrelated
        .iter()
        .all(|item| item.passage.conflict_key.as_deref() != Some("vehicle_leasing_ceiling")));
}

#[test]
fn manifests_reject_parent_traversal_and_frontmatter_rejects_unknown_fields() {
    assert!(ensure_safe_relative_path(Path::new("../outside.md")).is_err());
    let invalid = r#"---
schema_version: 1
document_id: "99999999-9999-4999-8999-999999999999"
source: "Unsafe"
revision: "1"
updated_at: "2026-08-01T00:00:00Z"
valid_until: null
conflict_key: null
tags: []
script: "powershell.exe"
---

## Content

This must not parse.
"#;
    assert!(parse_markdown(invalid, "unsafe", "Unsafe", BundleScope::Project).is_err());
}

#[test]
fn oversized_sections_split_deterministically_and_keep_stable_digests() {
    let words = (0..390)
        .map(|index| format!("term{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let markdown = format!(
        "---\nschema_version: 1\ndocument_id: \"99999999-9999-4999-8999-999999999999\"\nsource: \"Long guide\"\nrevision: \"1\"\nupdated_at: \"2026-08-01T00:00:00Z\"\nvalid_until: null\nconflict_key: null\ntags: [guide]\n---\n\n## Long section\n\n{words}\n"
    );
    let first = parse_markdown(&markdown, "guide", "Guide", BundleScope::Project).unwrap();
    let second = parse_markdown(&markdown, "guide", "Guide", BundleScope::Project).unwrap();
    assert_eq!(first.len(), 3);
    assert!(first
        .iter()
        .all(|passage| passage.content.split_whitespace().count() <= MAX_PASSAGE_WORDS));
    assert_eq!(
        first
            .iter()
            .map(|passage| (&passage.passage_id, &passage.content_hash))
            .collect::<Vec<_>>(),
        second
            .iter()
            .map(|passage| (&passage.passage_id, &passage.content_hash))
            .collect::<Vec<_>>()
    );
}
