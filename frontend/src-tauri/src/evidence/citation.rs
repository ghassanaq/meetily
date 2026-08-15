use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CITATION_SCHEMA_VERSION: u32 = 1;
pub const CITATION_TEXT_NORMALIZATION: &str = "citation-text-v1";
pub const EVIDENCE_RESOLVER_CAPABILITY: &str =
    "evidence-resolver-v1/citation-text-v1/audio-segment-boundaries-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceArtifactKind {
    Recording,
    Document,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceArtifactReference {
    pub id: Uuid,
    pub kind: EvidenceArtifactKind,
    pub version_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceLocator {
    AudioTimeline {
        start_ms: u64,
        end_ms: u64,
    },
    DocumentPassage {
        page_index: u64,
        section_path: Vec<String>,
        start_byte: u64,
        end_byte: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CitationSnapshot {
    pub text: String,
    pub normalization: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CitationResolutionProvenance {
    pub transcript_version_hash: String,
    pub segment_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CitationEnvelope {
    pub schema_version: u32,
    pub citation_id: Uuid,
    pub artifact: EvidenceArtifactReference,
    pub locator: EvidenceLocator,
    pub snapshot: CitationSnapshot,
    pub resolution: CitationResolutionProvenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CitationResolutionStatus {
    Verified,
    Superseded,
    EvidenceChanged,
    SourceMissing,
    ArtifactMismatch,
    VersionMissing,
    Unresolvable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordingSourceState {
    Available { actual_version_hash: String },
    Missing,
    VersionMissing,
}
