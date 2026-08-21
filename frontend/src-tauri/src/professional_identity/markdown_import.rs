//! Safe, deterministic import of a user-selected Markdown context corpus.
//!
//! The importer reads one declarative context manifest and the bundle/source
//! files it references. Every referenced path must be relative and must resolve
//! beneath the manifest directory. Imported content becomes an immutable
//! `ProfessionalIdentityVersion`; no filesystem path is retained at runtime.

use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{
    validate_identity, IdentityRecord, IdentityRecordCategory, IdentitySource,
    ProfessionalIdentityHeader, ProfessionalIdentityVersion, PROFESSIONAL_IDENTITY_SCHEMA_VERSION,
};

const IMPORT_SCHEMA_VERSION: u32 = 1;
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_MARKDOWN_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ImportedIdentityContext {
    pub context_id: String,
    pub name: String,
    pub identity: ProfessionalIdentityVersion,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextManifest {
    schema_version: u32,
    context_id: String,
    name: String,
    #[serde(default)]
    identity: Option<ProfessionalIdentityHeader>,
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
    document_id: String,
    source: String,
    revision: String,
    updated_at: String,
    valid_until: Option<String>,
    conflict_key: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum BundleScope {
    Person,
    Role,
    Project,
}

pub fn load_context_manifest(
    path: &Path,
    identity_override: Option<ProfessionalIdentityHeader>,
) -> Result<ImportedIdentityContext> {
    let manifest_path = path
        .canonicalize()
        .with_context(|| format!("context manifest '{}' does not exist", path.display()))?;
    if manifest_path.extension().and_then(|value| value.to_str()) != Some("json") {
        bail!("context manifest must be a JSON file");
    }
    let root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("context manifest has no parent directory"))?
        .canonicalize()?;
    let manifest: ContextManifest = parse_json_file(&manifest_path)?;
    if manifest.schema_version != IMPORT_SCHEMA_VERSION {
        bail!(
            "unsupported context manifest schema version {}",
            manifest.schema_version
        );
    }
    if manifest.context_id.trim().is_empty() || manifest.name.trim().is_empty() {
        bail!("context manifest must have a nonempty context_id and name");
    }
    let identity_header = identity_override
        .or(manifest.identity)
        .ok_or_else(|| anyhow!("context manifest is missing its identity header"))?;

    let mut records = Vec::new();
    records.extend(load_bundle(
        &root,
        &manifest.identity_bundle,
        BundleScope::Person,
    )?);
    records.extend(load_bundle(
        &root,
        &manifest.role_bundle,
        BundleScope::Role,
    )?);
    for bundle in &manifest.project_bundles {
        records.extend(load_bundle(&root, bundle, BundleScope::Project)?);
    }

    let identity = ProfessionalIdentityVersion {
        schema_version: PROFESSIONAL_IDENTITY_SCHEMA_VERSION,
        identity: identity_header,
        records,
        projects: Vec::new(),
    };
    validate_identity(&identity)?;

    Ok(ImportedIdentityContext {
        context_id: manifest.context_id,
        name: manifest.name,
        identity,
    })
}

pub fn stable_context_identity_id(context_id: &str) -> Uuid {
    stable_uuid(&format!("professional-identity-context\n{context_id}"))
}

fn load_bundle(
    root: &Path,
    relative_path: &str,
    scope: BundleScope,
) -> Result<Vec<IdentityRecord>> {
    let bundle_path = resolve_relative_path(root, root, relative_path)?;
    let bundle: BundleManifest = parse_json_file(&bundle_path)?;
    if bundle.schema_version != IMPORT_SCHEMA_VERSION {
        bail!(
            "unsupported bundle schema version {} in '{}'",
            bundle.schema_version,
            bundle_path.display()
        );
    }
    if bundle.bundle_id.trim().is_empty()
        || bundle.name.trim().is_empty()
        || bundle.sources.is_empty()
    {
        bail!("bundle must have an id, name, and at least one Markdown source");
    }
    let bundle_directory = bundle_path
        .parent()
        .ok_or_else(|| anyhow!("bundle path has no parent"))?;
    let mut records = Vec::new();
    for source in &bundle.sources {
        let source_path = resolve_relative_path(root, bundle_directory, source)?;
        if source_path.extension().and_then(|value| value.to_str()) != Some("md") {
            bail!("bundle source '{}' is not Markdown", source_path.display());
        }
        records.extend(parse_markdown(&source_path, scope)?);
    }
    Ok(records)
}

fn parse_markdown(path: &Path, scope: BundleScope) -> Result<Vec<IdentityRecord>> {
    ensure_file_size(path, MAX_MARKDOWN_BYTES)?;
    let normalized = fs::read_to_string(path)
        .with_context(|| format!("failed to read Markdown source '{}'", path.display()))?
        .replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or_else(|| anyhow!("Markdown source must start with YAML frontmatter"))?;
    let frontmatter_end = rest
        .find("\n---\n")
        .ok_or_else(|| anyhow!("Markdown source has unterminated YAML frontmatter"))?;
    let metadata: DocumentMetadata = serde_yaml_ng::from_str(&rest[..frontmatter_end])
        .context("invalid Markdown frontmatter")?;
    if metadata.schema_version != IMPORT_SCHEMA_VERSION
        || metadata.document_id.trim().is_empty()
        || metadata.source.trim().is_empty()
        || metadata.revision.trim().is_empty()
    {
        bail!("Markdown metadata is incomplete or unsupported");
    }

    let body = &rest[frontmatter_end + "\n---\n".len()..];
    let mut breadcrumb = Vec::<String>::new();
    let mut current_heading = metadata.source.clone();
    let mut current_lines = Vec::<String>::new();
    let mut sections = Vec::<(String, String)>::new();
    for line in body.lines() {
        if let Some((level, title)) = markdown_heading(line) {
            push_section(&mut sections, &current_heading, &mut current_lines);
            breadcrumb.truncate(level.saturating_sub(1));
            breadcrumb.push(title.to_string());
            current_heading = breadcrumb.join(" > ");
        } else {
            current_lines.push(line.to_string());
        }
    }
    push_section(&mut sections, &current_heading, &mut current_lines);
    if sections.is_empty() {
        bail!("Markdown source '{}' has no content", path.display());
    }

    sections
        .into_iter()
        .enumerate()
        .map(|(index, (title, content))| {
            Ok(IdentityRecord {
                id: stable_uuid(&format!("{}\n{}\n{}", metadata.document_id, title, index)),
                category: record_category(scope, path),
                title,
                content,
                source: IdentitySource {
                    label: metadata.source.clone(),
                    revision: metadata.revision.clone(),
                },
                updated_at: metadata.updated_at.clone(),
                valid_until: metadata.valid_until.clone(),
                conflict_key: metadata.conflict_key.clone(),
                tags: metadata.tags.clone(),
            })
        })
        .collect()
}

fn push_section(sections: &mut Vec<(String, String)>, heading: &str, lines: &mut Vec<String>) {
    let content = lines.join("\n").trim().to_string();
    lines.clear();
    if !content.is_empty() {
        sections.push((heading.to_string(), content));
    }
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

fn record_category(scope: BundleScope, path: &Path) -> IdentityRecordCategory {
    match scope {
        BundleScope::Role => IdentityRecordCategory::TermsOfReference,
        BundleScope::Project => IdentityRecordCategory::Other,
        BundleScope::Person => match path.file_name().and_then(|value| value.to_str()) {
            Some(name) if name.contains("capability") || name.contains("authority") => {
                IdentityRecordCategory::Authority
            }
            Some(name) if name.contains("cv") => IdentityRecordCategory::Cv,
            _ => IdentityRecordCategory::Other,
        },
    }
}

fn stable_uuid(seed: &str) -> Uuid {
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn parse_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    ensure_file_size(path, MAX_MANIFEST_BYTES)?;
    let json = fs::read_to_string(path)
        .with_context(|| format!("failed to read JSON manifest '{}'", path.display()))?;
    serde_json::from_str(&json)
        .with_context(|| format!("invalid JSON manifest in '{}'", path.display()))
}

fn ensure_file_size(path: &Path, maximum: u64) -> Result<()> {
    let size = fs::metadata(path)
        .with_context(|| format!("failed to inspect '{}'", path.display()))?
        .len();
    if size > maximum {
        bail!("file '{}' exceeds the import size limit", path.display());
    }
    Ok(())
}

fn resolve_relative_path(root: &Path, parent: &Path, relative: &str) -> Result<PathBuf> {
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
        bail!("context paths must be safe, nonempty, and relative");
    }
    let resolved = parent
        .join(relative_path)
        .canonicalize()
        .with_context(|| format!("context path '{}' does not exist", relative))?;
    if !resolved.starts_with(root) {
        bail!("context path '{}' escapes its corpus root", relative);
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_context_id_is_deterministic_and_context_specific() {
        let first = stable_context_identity_id("role-a");
        assert_eq!(first, stable_context_identity_id("role-a"));
        assert_ne!(first, stable_context_identity_id("role-b"));
        assert_eq!(first.get_version_num(), 5);
    }

    #[test]
    fn headings_are_limited_to_three_markdown_levels() {
        assert_eq!(markdown_heading("# One"), Some((1, "One")));
        assert_eq!(markdown_heading("### Three"), Some((3, "Three")));
        assert_eq!(markdown_heading("#### Four"), None);
        assert_eq!(markdown_heading("#Missing space"), None);
    }

    #[test]
    fn imports_a_manifest_and_rejects_path_escape() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        fs::create_dir(root.join("identity")).unwrap();
        fs::write(
            root.join("identity/source.md"),
            "---\nschema_version: 1\ndocument_id: cv\nsource: CV\nrevision: current\nupdated_at: 2026-08-20T00:00:00Z\nvalid_until: null\nconflict_key: null\ntags: [experience]\n---\n# Work\nCoordinated humanitarian operations.",
        )
        .unwrap();
        fs::write(
            root.join("identity/role.md"),
            "---\nschema_version: 1\ndocument_id: role\nsource: TOR\nrevision: current\nupdated_at: 2026-08-20T00:00:00Z\nvalid_until: null\nconflict_key: null\ntags: [role]\n---\n# Responsibilities\nCoordinate the mission.",
        )
        .unwrap();
        fs::write(
            root.join("identity/bundle.json"),
            r#"{"schema_version":1,"bundle_id":"person","name":"Person","sources":["source.md"]}"#,
        )
        .unwrap();
        fs::write(
            root.join("identity/role-bundle.json"),
            r#"{"schema_version":1,"bundle_id":"role","name":"Role","sources":["role.md"]}"#,
        )
        .unwrap();
        fs::write(
            root.join("context.json"),
            r#"{"schema_version":1,"context_id":"role","name":"Role","identity":{"display_name":"Person","role_title":"Role","organization":"Org","professional_summary":"Summary"},"identity_bundle":"identity/bundle.json","role_bundle":"identity/role-bundle.json","project_bundles":[]}"#,
        )
        .unwrap();

        let imported = load_context_manifest(&root.join("context.json"), None).unwrap();
        assert_eq!(imported.identity.records.len(), 2);
        assert_eq!(imported.context_id, "role");
        assert!(resolve_relative_path(root, root, "../outside.json").is_err());
    }
}
