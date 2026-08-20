use std::collections::{BTreeSet, HashMap, HashSet};
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
const SYNTHETIC_FIXTURE_RELATIVE_SCORE_FLOOR_PERCENT: usize = 50;
const IDF_SCALE: usize = 100;
const IDF_RATIO_SCALE: usize = 1024;
const SPIKE_MIN_PERSON_ROLE_BUNDLE_QUERY_TERMS: usize = 2;
const SPIKE_MIN_PROJECT_BUNDLE_QUERY_TERMS: usize = 3;
const PRIVATE_CONTEXT_PATH_ENV: &str = "PROJECT_CONTEXT_PATH";
const PRIVATE_EVALUATION_FILE: &str = "retrieval-eval.json";
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateEvaluationSuite {
    schema_version: u32,
    context_manifest: String,
    evaluated_at: String,
    relative_score_floor_percent: usize,
    cases: Vec<PrivateRetrievalCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PrivateRetrievalCase {
    id: String,
    question: String,
    expected_passage_ids: Vec<String>,
    relevant_passage_ids: Vec<String>,
    allowed_project_bundle_ids: Vec<String>,
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

#[derive(Clone, Copy, Debug)]
struct RankedPassage<'a> {
    passage: &'a Passage,
    score: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PinnedBundleSelection {
    project_bundle_ids: BTreeSet<String>,
}

impl PinnedBundleSelection {
    fn all_projects(passages: &[Passage]) -> Self {
        Self {
            project_bundle_ids: passages
                .iter()
                .filter(|passage| passage.scope == BundleScope::Project)
                .map(|passage| passage.bundle_id.clone())
                .collect(),
        }
    }

    fn projects<const N: usize>(bundle_ids: [&str; N]) -> Self {
        Self {
            project_bundle_ids: bundle_ids.into_iter().map(str::to_string).collect(),
        }
    }
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
    retrieve_with_floor(
        passages,
        question,
        now,
        limit,
        SYNTHETIC_FIXTURE_RELATIVE_SCORE_FLOOR_PERCENT,
    )
}

fn retrieve_with_floor<'a>(
    passages: &'a [Passage],
    question: &str,
    now: DateTime<Utc>,
    limit: usize,
    relative_score_floor_percent: usize,
) -> Result<Vec<RankedPassage<'a>>> {
    let selection = PinnedBundleSelection::all_projects(passages);
    retrieve_with_selection_and_floor(
        passages,
        &selection,
        question,
        now,
        limit,
        relative_score_floor_percent,
    )
}

fn retrieve_with_selection_and_floor<'a>(
    passages: &'a [Passage],
    selection: &PinnedBundleSelection,
    question: &str,
    now: DateTime<Utc>,
    limit: usize,
    relative_score_floor_percent: usize,
) -> Result<Vec<RankedPassage<'a>>> {
    if relative_score_floor_percent > 100 {
        bail!("relative score floor must be between 0 and 100 percent");
    }
    validate_pinned_bundle_selection(passages, selection)?;
    let query_terms = tokenize(question);
    let current = passages
        .iter()
        .filter(|passage| passage.valid_until.map_or(true, |until| until >= now))
        .filter(|passage| {
            passage.scope != BundleScope::Project
                || selection.project_bundle_ids.contains(&passage.bundle_id)
        })
        .collect::<Vec<_>>();
    let inverse_document_frequencies = inverse_document_frequencies(&query_terms, &current);
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
            let score = passage_score(&query_terms, &inverse_document_frequencies, passage);
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
    Ok(select_bundle_diverse(
        ranked,
        &query_terms,
        limit,
        relative_score_floor_percent,
    ))
}

fn validate_pinned_bundle_selection(
    passages: &[Passage],
    selection: &PinnedBundleSelection,
) -> Result<()> {
    let available_projects = passages
        .iter()
        .filter(|passage| passage.scope == BundleScope::Project)
        .map(|passage| passage.bundle_id.as_str())
        .collect::<HashSet<_>>();
    let mut unknown = selection
        .project_bundle_ids
        .iter()
        .filter(|bundle_id| !available_projects.contains(bundle_id.as_str()))
        .map(String::as_str)
        .collect::<Vec<_>>();
    unknown.sort_unstable();
    if !unknown.is_empty() {
        bail!(
            "pinned snapshot selects unknown project bundles: {}",
            unknown.join(", ")
        );
    }
    Ok(())
}

fn select_bundle_diverse<'a>(
    ranked: Vec<RankedPassage<'a>>,
    query_terms: &HashSet<String>,
    limit: usize,
    relative_score_floor_percent: usize,
) -> Vec<RankedPassage<'a>> {
    let eligible_bundles = eligible_bundle_ids(query_terms, &ranked);
    let mut bundle_order = Vec::<&str>::new();
    let mut by_bundle = HashMap::<&str, Vec<RankedPassage<'a>>>::new();
    for item in ranked
        .into_iter()
        .filter(|item| eligible_bundles.contains(item.passage.bundle_id.as_str()))
    {
        let bundle_id = item.passage.bundle_id.as_str();
        if !by_bundle.contains_key(bundle_id) {
            bundle_order.push(bundle_id);
        }
        by_bundle.entry(bundle_id).or_default().push(item);
    }
    for candidates in by_bundle.values_mut() {
        if let Some(best_score) = candidates.first().map(|item| item.score) {
            candidates.retain(|item| {
                item.score.saturating_mul(100)
                    >= best_score.saturating_mul(relative_score_floor_percent)
            });
        }
    }

    let mut selected = Vec::new();
    let mut round = 0;
    while selected.len() < limit {
        let mut round_candidates = bundle_order
            .iter()
            .filter_map(|bundle_id| by_bundle.get(bundle_id).and_then(|items| items.get(round)))
            .copied()
            .collect::<Vec<_>>();
        if round_candidates.is_empty() {
            break;
        }
        round_candidates.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.passage.passage_id.cmp(&right.passage.passage_id))
        });
        selected.extend(
            round_candidates
                .into_iter()
                .take(limit.saturating_sub(selected.len())),
        );
        round += 1;
    }
    selected
}

fn eligible_bundle_ids<'a>(
    query_terms: &HashSet<String>,
    ranked: &[RankedPassage<'a>],
) -> HashSet<&'a str> {
    let explicitly_named_projects = ranked
        .iter()
        .filter(|item| item.passage.scope == BundleScope::Project)
        .filter(|item| {
            label_is_explicitly_named(query_terms, &item.passage.bundle_id)
                || label_is_explicitly_named(query_terms, &item.passage.bundle_name)
        })
        .map(|item| item.passage.bundle_id.as_str())
        .collect::<HashSet<_>>();

    if !explicitly_named_projects.is_empty() {
        return ranked
            .iter()
            .filter(|item| {
                if item.passage.scope == BundleScope::Project {
                    return explicitly_named_projects.contains(item.passage.bundle_id.as_str());
                }
                query_terms
                    .intersection(&passage_score_terms(item.passage))
                    .count()
                    >= SPIKE_MIN_PERSON_ROLE_BUNDLE_QUERY_TERMS
            })
            .map(|item| item.passage.bundle_id.as_str())
            .collect();
    }

    ranked
        .iter()
        .filter(|item| {
            let matched_terms = query_terms
                .intersection(&passage_score_terms(item.passage))
                .count();
            match item.passage.scope {
                BundleScope::Person | BundleScope::Role => {
                    matched_terms >= SPIKE_MIN_PERSON_ROLE_BUNDLE_QUERY_TERMS
                }
                BundleScope::Project => matched_terms >= SPIKE_MIN_PROJECT_BUNDLE_QUERY_TERMS,
            }
        })
        .map(|item| item.passage.bundle_id.as_str())
        .collect()
}

fn label_is_explicitly_named(query_terms: &HashSet<String>, label: &str) -> bool {
    let label_terms = tokenize(label);
    let required_matches = label_terms.len().min(2);
    required_matches > 0 && query_terms.intersection(&label_terms).count() >= required_matches
}

fn conflict_key_matches(query_terms: &HashSet<String>, conflict_key: &str) -> bool {
    let key_terms = tokenize(conflict_key);
    let required_matches = key_terms.len().min(2);
    required_matches > 0 && query_terms.intersection(&key_terms).count() >= required_matches
}

fn inverse_document_frequencies(
    query_terms: &HashSet<String>,
    passages: &[&Passage],
) -> HashMap<String, usize> {
    query_terms
        .iter()
        .map(|term| {
            let document_frequency = passages
                .iter()
                .filter(|passage| passage_score_terms(passage).contains(term))
                .count();
            (
                term.clone(),
                smoothed_inverse_document_frequency(passages.len(), document_frequency),
            )
        })
        .collect()
}

fn smoothed_inverse_document_frequency(document_count: usize, document_frequency: usize) -> usize {
    let ratio = document_count
        .saturating_add(1)
        .saturating_mul(IDF_RATIO_SCALE)
        / document_frequency.saturating_add(1);
    let integer_log = ratio.ilog2().saturating_sub(IDF_RATIO_SCALE.ilog2()) as usize;
    let integer_base = IDF_RATIO_SCALE << integer_log;
    let fractional_log =
        ratio.saturating_sub(integer_base).saturating_mul(IDF_SCALE) / integer_base;
    IDF_SCALE
        .saturating_add(integer_log.saturating_mul(IDF_SCALE))
        .saturating_add(fractional_log)
}

fn passage_score_terms(passage: &Passage) -> HashSet<String> {
    tokenize(&format!(
        "{} {} {} {} {}",
        passage.bundle_name,
        passage.source,
        passage.heading,
        passage.tags.join(" "),
        passage.content
    ))
}

fn passage_score(
    query_terms: &HashSet<String>,
    inverse_document_frequencies: &HashMap<String, usize>,
    passage: &Passage,
) -> usize {
    let heading_terms = tokenize(&passage.heading);
    let tag_terms = tokenize(&passage.tags.join(" "));
    let provenance_terms = tokenize(&format!("{} {}", passage.bundle_name, passage.source));
    let content_terms = tokenize(&passage.content);

    query_terms
        .iter()
        .map(|term| {
            let field_weight = [
                (content_terms.contains(term), 4usize),
                (heading_terms.contains(term), 3usize),
                (tag_terms.contains(term), 2usize),
                (provenance_terms.contains(term), 1usize),
            ]
            .into_iter()
            .filter_map(|(matches, weight)| matches.then_some(weight))
            .max()
            .unwrap_or_default();
            field_weight.saturating_mul(
                inverse_document_frequencies
                    .get(term)
                    .copied()
                    .unwrap_or(IDF_SCALE),
            )
        })
        .sum()
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

fn max_irrelevant_for_limit(limit: usize) -> usize {
    match limit {
        3 => 1,
        5 => 2,
        8 => 3,
        _ => unreachable!(),
    }
}

fn count_regular_files(root: &Path) -> Result<usize> {
    let mut count = 0usize;
    for entry in fs::read_dir(root)
        .with_context(|| format!("failed to read fixture directory '{}'", root.display()))?
    {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            count += count_regular_files(&entry.path())?;
        } else if file_type.is_file() {
            count += 1;
        }
    }
    Ok(count)
}

#[test]
fn fixture_scale_is_explicit_and_parser_consistent() {
    let root = fixture_root();
    let context = load_context(&root, "meeting-assistant.context.json").unwrap();
    let indexed_words = context
        .passages
        .iter()
        .map(|passage| passage.content.split_whitespace().count())
        .sum::<usize>();

    assert_eq!(count_regular_files(&root).unwrap(), 17);
    assert_eq!(indexed_words, 369);
}

#[test]
fn idf_weight_is_deterministic_and_downweights_common_passage_terms() {
    assert_eq!(smoothed_inverse_document_frequency(10, 10), IDF_SCALE);
    assert_eq!(smoothed_inverse_document_frequency(10, 1), 337);
    assert!(
        smoothed_inverse_document_frequency(10, 1) > smoothed_inverse_document_frequency(10, 9)
    );
}

#[test]
fn idf_document_frequency_uses_only_current_snapshot_candidates() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    let query_terms = tokenize("legacy travel delegation");
    let current = context
        .passages
        .iter()
        .filter(|passage| {
            passage
                .valid_until
                .map_or(true, |until| until >= fixed_now())
        })
        .collect::<Vec<_>>();
    let all = context.passages.iter().collect::<Vec<_>>();

    let current_idf = inverse_document_frequencies(&query_terms, &current);
    let all_idf = inverse_document_frequencies(&query_terms, &all);

    assert!(current_idf["legacy"] > all_idf["legacy"]);
}

#[test]
fn pinned_project_selection_filters_candidates_before_idf_and_ranking() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    let selection = PinnedBundleSelection::projects(["atlas"]);
    let question = "How do the Atlas rollout and Beacon partner onboarding affect each other?";
    let selected_from_full_context = retrieve_with_selection_and_floor(
        &context.passages,
        &selection,
        question,
        fixed_now(),
        8,
        SYNTHETIC_FIXTURE_RELATIVE_SCORE_FLOOR_PERCENT,
    )
    .unwrap();

    let without_beacon = context
        .passages
        .iter()
        .filter(|passage| passage.bundle_id != "beacon")
        .cloned()
        .collect::<Vec<_>>();
    let selected_without_beacon = retrieve_with_selection_and_floor(
        &without_beacon,
        &selection,
        question,
        fixed_now(),
        8,
        SYNTHETIC_FIXTURE_RELATIVE_SCORE_FLOOR_PERCENT,
    )
    .unwrap();

    let result_projection = |results: &[RankedPassage<'_>]| {
        results
            .iter()
            .map(|item| (item.passage.passage_id.clone(), item.score))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        result_projection(&selected_from_full_context),
        result_projection(&selected_without_beacon)
    );
    assert!(selected_from_full_context.iter().all(|item| {
        item.passage.scope != BundleScope::Project || item.passage.bundle_id == "atlas"
    }));
}

#[test]
fn pinned_project_selection_rejects_unknown_bundles_and_allows_none() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    let unknown = retrieve_with_selection_and_floor(
        &context.passages,
        &PinnedBundleSelection::projects(["unknown-project"]),
        "What is the project status?",
        fixed_now(),
        3,
        SYNTHETIC_FIXTURE_RELATIVE_SCORE_FLOOR_PERCENT,
    )
    .unwrap_err();
    assert!(unknown
        .to_string()
        .contains("pinned snapshot selects unknown project bundles: unknown-project"));

    let no_projects = retrieve_with_selection_and_floor(
        &context.passages,
        &PinnedBundleSelection::projects([]),
        "What is the Atlas rollout status?",
        fixed_now(),
        8,
        SYNTHETIC_FIXTURE_RELATIVE_SCORE_FLOOR_PERCENT,
    )
    .unwrap();
    assert!(no_projects
        .iter()
        .all(|item| item.passage.scope != BundleScope::Project));
}

#[test]
fn passage_terms_are_saturated_at_the_strongest_matching_field() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    let passage = context
        .passages
        .iter()
        .find(|passage| passage.passage_id == PERSON_BACKGROUND)
        .unwrap();
    let query_terms = tokenize("leadership");
    let idf = HashMap::from([("leadership".to_string(), IDF_SCALE)]);

    assert_eq!(passage_score(&query_terms, &idf, passage), 4 * IDF_SCALE);
}

#[test]
fn bundle_diverse_shortlist_prevents_one_bundle_from_taking_every_slot() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    let passage = |passage_id: &str| {
        context
            .passages
            .iter()
            .find(|candidate| candidate.passage_id == passage_id)
            .unwrap()
    };
    let ranked = vec![
        RankedPassage {
            passage: passage(ATLAS_STATUS),
            score: 100,
        },
        RankedPassage {
            passage: passage(ATLAS_COMMITMENT),
            score: 95,
        },
        RankedPassage {
            passage: passage(BEACON_STATUS),
            score: 90,
        },
    ];

    let selected = select_bundle_diverse(
        ranked,
        &tokenize("How do the Atlas and Beacon projects affect each other?"),
        2,
        50,
    );
    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].passage.passage_id, ATLAS_STATUS);
    assert_eq!(selected[1].passage.passage_id, BEACON_STATUS);
}

#[test]
fn explicit_project_routing_requires_a_distinctive_name_match() {
    assert!(label_is_explicitly_named(
        &tokenize("What is the Atlas rollout status?"),
        "atlas"
    ));
    assert!(label_is_explicitly_named(
        &tokenize("Why Children Not Numbers?"),
        "children-not-numbers-interview"
    ));
    assert!(!label_is_explicitly_named(
        &tokenize("Have you managed programmes for Palestinian children?"),
        "children-not-numbers-interview"
    ));
}

#[test]
#[ignore = "requires PROJECT_CONTEXT_PATH pointing to a private corpus root"]
fn private_corpus_retrieval_measurements() {
    run_private_corpus_retrieval_measurements().unwrap();
}

#[test]
fn private_corpus_measurement_rules_are_ci_covered_on_synthetic_data() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    let private_cases = cases()
        .into_iter()
        .map(|case| {
            let allowed_project_bundle_ids = match case.exclusive_project {
                Some(project) => vec![project.to_string()],
                None if case.name == "cross-project dependencies" => {
                    vec!["atlas".to_string(), "beacon".to_string()]
                }
                None => Vec::new(),
            };
            PrivateRetrievalCase {
                id: case.name.replace(' ', "-"),
                question: case.question.to_string(),
                expected_passage_ids: case
                    .expected
                    .iter()
                    .map(|passage_id| (*passage_id).to_string())
                    .collect(),
                relevant_passage_ids: case
                    .relevant
                    .iter()
                    .map(|passage_id| (*passage_id).to_string())
                    .collect(),
                allowed_project_bundle_ids,
            }
        })
        .collect();
    let suite = PrivateEvaluationSuite {
        schema_version: CONTEXT_SCHEMA_VERSION,
        context_manifest: "meeting-assistant.context.json".to_string(),
        evaluated_at: "2026-08-20T00:00:00Z".to_string(),
        relative_score_floor_percent: SYNTHETIC_FIXTURE_RELATIVE_SCORE_FLOOR_PERCENT,
        cases: private_cases,
    };

    evaluate_private_suite(&context, &suite).unwrap();
}

fn run_private_corpus_retrieval_measurements() -> Result<()> {
    let root = std::env::var_os(PRIVATE_CONTEXT_PATH_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("{PRIVATE_CONTEXT_PATH_ENV} is not set"))?
        .canonicalize()
        .context("PROJECT_CONTEXT_PATH does not identify an existing context root")?;
    let suite_path = resolve_existing_relative_path(&root, PRIVATE_EVALUATION_FILE)?;
    let suite: PrivateEvaluationSuite = parse_json_file(&suite_path)?;
    let context = load_context(&root, &suite.context_manifest)?;
    evaluate_private_suite(&context, &suite)
}

fn evaluate_private_suite(context: &LoadedContext, suite: &PrivateEvaluationSuite) -> Result<()> {
    if suite.schema_version != CONTEXT_SCHEMA_VERSION {
        bail!(
            "unsupported private evaluation schema version {}",
            suite.schema_version
        );
    }
    if !(10..=20).contains(&suite.cases.len()) {
        bail!("private evaluation suite must contain between 10 and 20 cases");
    }
    if suite.relative_score_floor_percent > 100 {
        bail!("relative_score_floor_percent must be between 0 and 100");
    }

    let evaluated_at = parse_timestamp("evaluated_at", &suite.evaluated_at)?;
    let mut case_ids = HashSet::new();

    for case in &suite.cases {
        validate_nonempty("private evaluation case id", &case.id)?;
        validate_nonempty("private evaluation question", &case.question)?;
        if !case_ids.insert(case.id.as_str()) {
            bail!("duplicate private evaluation case id '{}'", case.id);
        }
        if case.expected_passage_ids.is_empty() || case.relevant_passage_ids.is_empty() {
            bail!(
                "private evaluation case '{}' must declare expected and relevant passage IDs",
                case.id
            );
        }

        let expected = case
            .expected_passage_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let relevant = case
            .relevant_passage_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        let allowed_projects = case
            .allowed_project_bundle_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        for project_id in &case.allowed_project_bundle_ids {
            validate_nonempty("allowed project bundle id", project_id)?;
        }
        if expected.len() != case.expected_passage_ids.len()
            || relevant.len() != case.relevant_passage_ids.len()
            || allowed_projects.len() != case.allowed_project_bundle_ids.len()
        {
            bail!(
                "private evaluation case '{}' contains duplicate IDs",
                case.id
            );
        }
        if !expected.is_subset(&relevant) {
            bail!(
                "private evaluation case '{}' expected IDs must also be relevant IDs",
                case.id
            );
        }

        for limit in [3usize, 5, 8] {
            let results = retrieve_with_floor(
                &context.passages,
                &case.question,
                evaluated_at,
                limit,
                suite.relative_score_floor_percent,
            )?;
            let selected_ids = results
                .iter()
                .map(|item| item.passage.passage_id.as_str())
                .collect::<Vec<_>>();
            let expected_ranks = case
                .expected_passage_ids
                .iter()
                .map(|passage_id| {
                    selected_ids
                        .iter()
                        .position(|selected| *selected == passage_id.as_str())
                        .map(|index| index + 1)
                        .unwrap_or(usize::MAX)
                })
                .collect::<Vec<_>>();
            if expected_ranks.iter().any(|rank| *rank > 3) {
                bail!(
                    "private evaluation case '{}' at limit {} placed expected evidence outside the top three; ranks={:?}, selected={:?}",
                    case.id,
                    limit,
                    expected_ranks,
                    selected_ids
                );
            }

            let irrelevant = results
                .iter()
                .filter(|item| !relevant.contains(item.passage.passage_id.as_str()))
                .count();
            if irrelevant > max_irrelevant_for_limit(limit) {
                bail!(
                    "private evaluation case '{}' at limit {} returned {} irrelevant passages; selected={:?}",
                    case.id,
                    limit,
                    irrelevant,
                    selected_ids
                );
            }

            let project_bleed = results.iter().any(|item| {
                item.passage.scope == BundleScope::Project
                    && !allowed_projects.contains(item.passage.bundle_id.as_str())
            });
            if project_bleed {
                bail!(
                    "private evaluation case '{}' at limit {} selected a project outside allowed_project_bundle_ids; selected={:?}",
                    case.id,
                    limit,
                    selected_ids
                );
            }

            let words = results
                .iter()
                .map(|item| item.passage.content.split_whitespace().count())
                .sum::<usize>();
            let selected_with_scores = results
                .iter()
                .map(|item| format!("{}@{}", item.passage.passage_id, item.score))
                .collect::<Vec<_>>();
            println!(
                "private_eval case={} limit={} floor_percent={} ranks={:?} selected={} irrelevant={} words={} passages={:?}",
                case.id,
                limit,
                suite.relative_score_floor_percent,
                expected_ranks,
                results.len(),
                irrelevant,
                words,
                selected_with_scores
            );
        }
    }

    Ok(())
}

#[test]
fn ten_question_fixture_meets_rank_precision_and_project_isolation_targets() {
    let context = load_context(&fixture_root(), "meeting-assistant.context.json").unwrap();
    assert_eq!(context.context_id, "head-of-mission-multi-project");
    assert_eq!(context.name, "Head of Mission — Atlas and Beacon");

    for limit in [3usize, 5, 8] {
        let max_irrelevant = max_irrelevant_for_limit(limit);
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
