use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use thiserror::Error;
use uuid::Uuid;

pub const TRANSCRIPT_VERSION_SCHEMA: u32 = 1;
pub const MAX_CANONICAL_INTEGER: u64 = 9_007_199_254_740_992;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingArtifactKind {
    Captured,
    Imported,
}

impl RecordingArtifactKind {
    pub(crate) fn as_db_str(self) -> &'static str {
        match self {
            Self::Captured => "captured",
            Self::Imported => "imported",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordingVersionSpec {
    pub version_hash: String,
    pub byte_length: u64,
    pub media_type: String,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptVersionSegment {
    pub segment_id: Uuid,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub speaker: Option<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TranscriptVersionContent {
    pub schema_version: u32,
    pub recording_artifact_id: Uuid,
    pub recording_version_hash: String,
    pub language: Option<String>,
    pub engine: String,
    pub model: String,
    pub configuration_hash: Option<String>,
    pub segments: Vec<TranscriptVersionSegment>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EvidenceValidationError {
    #[error("unsupported transcript schema version {actual}")]
    UnsupportedSchemaVersion { actual: u32 },
    #[error("{field} must not be empty")]
    EmptyField { field: &'static str },
    #[error("{field} must be a sha256 digest")]
    InvalidDigest { field: &'static str },
    #[error("{field} exceeds the canonical safe-integer range")]
    UnsafeInteger { field: &'static str },
    #[error("recording duration must be greater than zero")]
    EmptyRecording,
    #[error("a transcript version must contain at least one segment")]
    EmptyTranscript,
    #[error("segment {index} has an empty text value")]
    EmptySegment { index: usize },
    #[error("segment {index} has invalid half-open bounds [{start_ms}, {end_ms})")]
    InvalidSegmentBounds {
        index: usize,
        start_ms: u64,
        end_ms: u64,
    },
    #[error("segment {index} is out of timeline order")]
    SegmentOrder { index: usize },
    #[error("segment id {0} appears more than once")]
    DuplicateSegment(Uuid),
}

impl RecordingVersionSpec {
    pub fn validate(&self) -> Result<(), EvidenceValidationError> {
        validate_digest("recording version hash", &self.version_hash)?;
        validate_nonempty("media type", &self.media_type)?;
        validate_safe_integer("recording byte length", self.byte_length)?;
        validate_safe_integer("recording duration", self.duration_ms)?;
        if self.duration_ms == 0 {
            return Err(EvidenceValidationError::EmptyRecording);
        }
        Ok(())
    }
}

impl TranscriptVersionContent {
    pub fn validate(&self) -> Result<(), EvidenceValidationError> {
        if self.schema_version != TRANSCRIPT_VERSION_SCHEMA {
            return Err(EvidenceValidationError::UnsupportedSchemaVersion {
                actual: self.schema_version,
            });
        }
        validate_digest("recording version hash", &self.recording_version_hash)?;
        if let Some(hash) = &self.configuration_hash {
            validate_digest("transcription configuration hash", hash)?;
        }
        if let Some(language) = &self.language {
            validate_nonempty("language", language)?;
        }
        validate_nonempty("transcription engine", &self.engine)?;
        validate_nonempty("transcription model", &self.model)?;
        if self.segments.is_empty() {
            return Err(EvidenceValidationError::EmptyTranscript);
        }

        let mut segment_ids = HashSet::with_capacity(self.segments.len());
        let mut previous_start = None;
        for (index, segment) in self.segments.iter().enumerate() {
            validate_safe_integer("segment start", segment.start_ms)?;
            validate_safe_integer("segment end", segment.end_ms)?;
            if segment.end_ms <= segment.start_ms {
                return Err(EvidenceValidationError::InvalidSegmentBounds {
                    index,
                    start_ms: segment.start_ms,
                    end_ms: segment.end_ms,
                });
            }
            if previous_start.is_some_and(|start| segment.start_ms < start) {
                return Err(EvidenceValidationError::SegmentOrder { index });
            }
            if segment.text.trim().is_empty() {
                return Err(EvidenceValidationError::EmptySegment { index });
            }
            if !segment_ids.insert(segment.segment_id) {
                return Err(EvidenceValidationError::DuplicateSegment(
                    segment.segment_id,
                ));
            }
            previous_start = Some(segment.start_ms);
        }

        Ok(())
    }
}

pub(crate) fn validate_digest(
    field: &'static str,
    digest: &str,
) -> Result<(), EvidenceValidationError> {
    let Some(hex) = digest.strip_prefix("sha256:") else {
        return Err(EvidenceValidationError::InvalidDigest { field });
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(EvidenceValidationError::InvalidDigest { field });
    }
    Ok(())
}

fn validate_nonempty(field: &'static str, value: &str) -> Result<(), EvidenceValidationError> {
    if value.trim().is_empty() {
        return Err(EvidenceValidationError::EmptyField { field });
    }
    Ok(())
}

fn validate_safe_integer(field: &'static str, value: u64) -> Result<(), EvidenceValidationError> {
    if value > MAX_CANONICAL_INTEGER {
        return Err(EvidenceValidationError::UnsafeInteger { field });
    }
    Ok(())
}
