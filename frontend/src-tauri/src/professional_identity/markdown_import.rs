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
    validate_identity, AuthorityActionFamily, AuthorityConstraint, IdentityRecord,
    IdentityRecordCategory, IdentitySource, ProfessionalIdentityHeader,
    ProfessionalIdentityVersion,
};

const MIN_IMPORT_SCHEMA_VERSION: u32 = 1;
const IMPORT_SCHEMA_VERSION: u32 = 2;
const BUNDLE_SCHEMA_VERSION: u32 = 1;
const DOCUMENT_SCHEMA_VERSION: u32 = 1;
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
    #[serde(default)]
    authority_constraints: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityConstraintSidecar {
    schema_version: u32,
    rules: Vec<AuthoredAuthorityConstraint>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredAuthorityConstraint {
    id: String,
    label: String,
    #[serde(default)]
    contexts: Vec<String>,
    action_families: Vec<AuthorityActionFamily>,
    permitted_objects: Vec<String>,
    excluded_objects: Vec<String>,
    evidence: Vec<AuthorityEvidenceSelector>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthorityEvidenceSelector {
    source_label: String,
    title: String,
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
    if !(MIN_IMPORT_SCHEMA_VERSION..=IMPORT_SCHEMA_VERSION).contains(&manifest.schema_version) {
        bail!(
            "unsupported context manifest schema version {}",
            manifest.schema_version
        );
    }
    if manifest.schema_version == MIN_IMPORT_SCHEMA_VERSION
        && manifest.authority_constraints.is_some()
    {
        bail!("context manifest schema version 1 cannot declare authority constraints");
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
    let authority_constraints = manifest
        .authority_constraints
        .as_deref()
        .map(|relative| load_authority_constraints(&root, relative, &records))
        .transpose()?
        .unwrap_or_default();

    let identity = ProfessionalIdentityVersion {
        schema_version: manifest.schema_version,
        identity: identity_header,
        records,
        projects: Vec::new(),
        authority_constraints,
    };
    validate_identity(&identity)?;

    Ok(ImportedIdentityContext {
        context_id: manifest.context_id,
        name: manifest.name,
        identity,
    })
}

fn load_authority_constraints(
    root: &Path,
    relative_path: &str,
    records: &[IdentityRecord],
) -> Result<Vec<AuthorityConstraint>> {
    let path = resolve_relative_path(root, root, relative_path)?;
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
        bail!("authority constraint sidecar must be a JSON file");
    }
    let sidecar: AuthorityConstraintSidecar = parse_json_file(&path)?;
    if sidecar.schema_version != 1 {
        bail!(
            "unsupported authority constraint sidecar schema version {}",
            sidecar.schema_version
        );
    }
    if sidecar.rules.is_empty() {
        bail!("declared authority constraint sidecar must contain at least one rule");
    }

    sidecar
        .rules
        .into_iter()
        .enumerate()
        .map(|(rule_index, rule)| {
            if rule.evidence.is_empty() {
                bail!(
                    "authority constraint rule '{}' must reference evidence",
                    rule.id
                );
            }
            let mut evidence_record_ids = Vec::new();
            for (selector_index, selector) in rule.evidence.iter().enumerate() {
                let matches = records
                    .iter()
                    .filter(|record| {
                        record.source.label == selector.source_label
                            && record.title == selector.title
                    })
                    .collect::<Vec<_>>();
                if matches.len() != 1 {
                    bail!(
                        "authority constraint rules[{rule_index}].evidence[{selector_index}] must resolve to exactly one record, found {}",
                        matches.len()
                    );
                }
                evidence_record_ids.push(matches[0].id);
            }
            Ok(AuthorityConstraint {
                id: rule.id,
                label: rule.label,
                contexts: rule.contexts,
                action_families: rule.action_families,
                permitted_objects: rule.permitted_objects,
                excluded_objects: rule.excluded_objects,
                evidence_record_ids,
            })
        })
        .collect()
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
    if bundle.schema_version != BUNDLE_SCHEMA_VERSION {
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
    if metadata.schema_version != DOCUMENT_SCHEMA_VERSION
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
        assert_eq!(imported.identity.schema_version, 1);
        assert!(imported.identity.authority_constraints.is_empty());
        assert_eq!(imported.context_id, "role");
        assert!(resolve_relative_path(root, root, "../outside.json").is_err());

        fs::write(
            root.join("authority-rules.json"),
            r#"{"schema_version":1,"rules":[{"id":"work-boundary","label":"Work boundary","contexts":[],"action_families":["manage"],"permitted_objects":["workstream"],"excluded_objects":["whole operation"],"evidence":[{"source_label":"CV","title":"Work"}]}]}"#,
        )
        .unwrap();
        fs::write(
            root.join("context.json"),
            r#"{"schema_version":2,"context_id":"role","name":"Role","identity":{"display_name":"Person","role_title":"Role","organization":"Org","professional_summary":"Summary"},"identity_bundle":"identity/bundle.json","role_bundle":"identity/role-bundle.json","project_bundles":[],"authority_constraints":"authority-rules.json"}"#,
        )
        .unwrap();
        let imported_v2 = load_context_manifest(&root.join("context.json"), None).unwrap();
        assert_eq!(imported_v2.identity.schema_version, 2);
        assert_eq!(imported_v2.identity.authority_constraints.len(), 1);
        assert_eq!(
            imported_v2.identity.authority_constraints[0].evidence_record_ids,
            vec![stable_uuid("cv\nWork\n0")]
        );
        let serialized = serde_json::to_string(&imported_v2.identity).unwrap();
        assert!(!serialized.contains("authority-rules.json"));
        assert!(!serialized.contains("source.md"));

        fs::write(
            root.join("authority-rules.json"),
            r#"{"schema_version":1,"rules":[{"id":"work-boundary","label":"Work boundary","contexts":[],"action_families":["manage"],"permitted_objects":["workstream"],"excluded_objects":["whole operation"],"evidence":[{"source_label":"CV","title":"Missing"}]}]}"#,
        )
        .unwrap();
        assert!(load_context_manifest(&root.join("context.json"), None).is_err());

        fs::write(
            root.join("identity/duplicate.md"),
            "---\nschema_version: 1\ndocument_id: duplicate\nsource: CV\nrevision: current\nupdated_at: 2026-08-20T00:00:00Z\nvalid_until: null\nconflict_key: null\ntags: [experience]\n---\n# Work\nA second record with the same selector.",
        )
        .unwrap();
        fs::write(
            root.join("identity/bundle.json"),
            r#"{"schema_version":1,"bundle_id":"person","name":"Person","sources":["source.md","duplicate.md"]}"#,
        )
        .unwrap();
        fs::write(
            root.join("authority-rules.json"),
            r#"{"schema_version":1,"rules":[{"id":"work-boundary","label":"Work boundary","contexts":[],"action_families":["manage"],"permitted_objects":["workstream"],"excluded_objects":["whole operation"],"evidence":[{"source_label":"CV","title":"Work"}]}]}"#,
        )
        .unwrap();
        assert!(load_context_manifest(&root.join("context.json"), None).is_err());

        fs::write(
            root.join("context.json"),
            r#"{"schema_version":2,"context_id":"role","name":"Role","identity":{"display_name":"Person","role_title":"Role","organization":"Org","professional_summary":"Summary"},"identity_bundle":"identity/bundle.json","role_bundle":"identity/role-bundle.json","project_bundles":[],"authority_constraints":"../authority-rules.json"}"#,
        )
        .unwrap();
        assert!(load_context_manifest(&root.join("context.json"), None).is_err());

        fs::write(
            root.join("context.json"),
            r#"{"schema_version":2,"context_id":"role","name":"Role","identity":{"display_name":"Person","role_title":"Role","organization":"Org","professional_summary":"Summary"},"identity_bundle":"identity/bundle.json","role_bundle":"identity/role-bundle.json","project_bundles":[],"authority_constraints":"authority-rules.txt"}"#,
        )
        .unwrap();
        fs::write(
            root.join("authority-rules.txt"),
            r#"{"schema_version":1,"rules":[]}"#,
        )
        .unwrap();
        assert!(load_context_manifest(&root.join("context.json"), None).is_err());

        fs::write(
            root.join("context.json"),
            r#"{"schema_version":1,"context_id":"role","name":"Role","identity":{"display_name":"Person","role_title":"Role","organization":"Org","professional_summary":"Summary"},"identity_bundle":"identity/bundle.json","role_bundle":"identity/role-bundle.json","project_bundles":[],"authority_constraints":"authority-rules.json"}"#,
        )
        .unwrap();
        assert!(load_context_manifest(&root.join("context.json"), None).is_err());
    }
}
