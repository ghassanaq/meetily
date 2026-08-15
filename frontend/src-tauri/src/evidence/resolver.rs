use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::expert_profiles::hashing::{hash_serializable, HashError};

use super::citation::{
    CitationEnvelope, CitationResolutionProvenance, CitationResolutionStatus, CitationSnapshot,
    EvidenceArtifactKind, EvidenceArtifactReference, EvidenceLocator, RecordingSourceState,
    CITATION_SCHEMA_VERSION, CITATION_TEXT_NORMALIZATION,
};
use super::hashing::hash_transcript_version;
use super::models::{
    validate_digest, EvidenceValidationError, TranscriptVersionContent, TranscriptVersionSegment,
    MAX_CANONICAL_INTEGER,
};

const CITATION_TEXT_HASH_DOMAIN: &[u8] = b"meeting-assistant-citation-text-v1\0";
const CITATION_ENVELOPE_HASH_DOMAIN: &[u8] = b"meeting-assistant-citation-envelope-v1\0";

#[derive(Debug, Error)]
pub enum EvidenceResolverError {
    #[error(transparent)]
    Validation(#[from] EvidenceValidationError),
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error("audio bounds [{start_ms}, {end_ms}) are invalid")]
    InvalidAudioBounds { start_ms: u64, end_ms: u64 },
    #[error("seconds value must be finite and non-negative")]
    InvalidSeconds,
    #[error("audio bounds exceed the recording duration")]
    BeyondRecording,
    #[error("no transcript segments overlap the requested audio interval")]
    NoOverlappingSegments,
    #[error("selected transcript segments must be unique, ordered, and contiguous")]
    NoncontiguousSegments,
    #[error("selected transcript segments do not form a closed audio interval")]
    AmbiguousSegmentSelection,
    #[error("the selected segment id {0} does not exist")]
    SegmentNotFound(Uuid),
    #[error("document passage resolution is reserved but not implemented")]
    DocumentResolutionUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedAudioSpan {
    start_ms: u64,
    end_ms: u64,
    text: String,
    segment_ids: Vec<Uuid>,
}

#[derive(Serialize)]
struct CitationIdentityProjection<'a> {
    schema_version: u32,
    artifact: &'a EvidenceArtifactReference,
    locator: &'a EvidenceLocator,
    snapshot: &'a CitationSnapshot,
    resolution: &'a CitationResolutionProvenance,
}

pub fn normalize_citation_text(text: &str) -> String {
    let nfc: String = text.nfc().collect();
    nfc.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned()
}

pub fn hash_citation_text(text: &str) -> String {
    let normalized = normalize_citation_text(text);
    let mut hasher = Sha256::new();
    hasher.update(CITATION_TEXT_HASH_DOMAIN);
    hasher.update(normalized.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

pub fn hash_citation_envelope(citation: &CitationEnvelope) -> Result<String, HashError> {
    let identity = CitationIdentityProjection {
        schema_version: citation.schema_version,
        artifact: &citation.artifact,
        locator: &citation.locator,
        snapshot: &citation.snapshot,
        resolution: &citation.resolution,
    };
    hash_serializable(CITATION_ENVELOPE_HASH_DOMAIN, &identity)
}

pub fn seconds_to_millis(seconds: f64) -> Result<u64, EvidenceResolverError> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(EvidenceResolverError::InvalidSeconds);
    }
    let milliseconds = (seconds * 1000.0).round();
    if milliseconds > MAX_CANONICAL_INTEGER as f64 {
        return Err(EvidenceResolverError::InvalidSeconds);
    }
    Ok(milliseconds as u64)
}

pub fn build_audio_citation_for_interval(
    citation_id: Uuid,
    transcript: &TranscriptVersionContent,
    recording_duration_ms: u64,
    requested_start_ms: u64,
    requested_end_ms: u64,
) -> Result<CitationEnvelope, EvidenceResolverError> {
    transcript.validate()?;
    validate_audio_bounds(requested_start_ms, requested_end_ms, recording_duration_ms)?;
    let mut closed_start = requested_start_ms;
    let mut closed_end = requested_end_ms;
    let segment_ids = loop {
        let selected = transcript
            .segments
            .iter()
            .filter(|segment| segment.start_ms < closed_end && segment.end_ms > closed_start)
            .collect::<Vec<_>>();
        if selected.is_empty() {
            return Err(EvidenceResolverError::NoOverlappingSegments);
        }
        let next_start = selected[0].start_ms;
        let next_end = selected
            .iter()
            .map(|segment| segment.end_ms)
            .max()
            .expect("selected segments are non-empty");
        if next_start == closed_start && next_end == closed_end {
            break selected
                .iter()
                .map(|segment| segment.segment_id)
                .collect::<Vec<_>>();
        }
        closed_start = next_start;
        closed_end = next_end;
    };
    build_audio_citation_from_segments(citation_id, transcript, recording_duration_ms, &segment_ids)
}

pub fn build_audio_citation_from_segments(
    citation_id: Uuid,
    transcript: &TranscriptVersionContent,
    recording_duration_ms: u64,
    selected_segment_ids: &[Uuid],
) -> Result<CitationEnvelope, EvidenceResolverError> {
    transcript.validate()?;
    if selected_segment_ids.is_empty() {
        return Err(EvidenceResolverError::NoOverlappingSegments);
    }

    let mut indices = Vec::with_capacity(selected_segment_ids.len());
    for selected_id in selected_segment_ids {
        let index = transcript
            .segments
            .iter()
            .position(|segment| segment.segment_id == *selected_id)
            .ok_or(EvidenceResolverError::SegmentNotFound(*selected_id))?;
        indices.push(index);
    }
    if indices
        .windows(2)
        .any(|pair| pair[1] != pair[0].saturating_add(1))
    {
        return Err(EvidenceResolverError::NoncontiguousSegments);
    }

    let selected = &transcript.segments[indices[0]..=indices[indices.len() - 1]];
    let span = resolved_span(selected)?;
    validate_audio_bounds(span.start_ms, span.end_ms, recording_duration_ms)?;
    let all_overlapping_ids = transcript
        .segments
        .iter()
        .filter(|segment| segment.start_ms < span.end_ms && segment.end_ms > span.start_ms)
        .map(|segment| segment.segment_id)
        .collect::<Vec<_>>();
    if all_overlapping_ids != selected_segment_ids {
        return Err(EvidenceResolverError::AmbiguousSegmentSelection);
    }
    let normalized_text = normalize_citation_text(&span.text);
    let transcript_version_hash = hash_transcript_version(transcript)?;

    Ok(CitationEnvelope {
        schema_version: CITATION_SCHEMA_VERSION,
        citation_id,
        artifact: EvidenceArtifactReference {
            id: transcript.recording_artifact_id,
            kind: EvidenceArtifactKind::Recording,
            version_hash: transcript.recording_version_hash.clone(),
        },
        locator: EvidenceLocator::AudioTimeline {
            start_ms: span.start_ms,
            end_ms: span.end_ms,
        },
        snapshot: CitationSnapshot {
            content_hash: hash_citation_text(&normalized_text),
            text: normalized_text,
            normalization: CITATION_TEXT_NORMALIZATION.to_owned(),
        },
        resolution: CitationResolutionProvenance {
            transcript_version_hash,
            segment_ids: span.segment_ids,
        },
    })
}

pub fn resolve_historical_citation(
    citation: &CitationEnvelope,
    recording_duration_ms: u64,
    source_state: &RecordingSourceState,
    pinned_transcript: Option<&TranscriptVersionContent>,
) -> CitationResolutionStatus {
    if citation.schema_version != CITATION_SCHEMA_VERSION
        || citation.snapshot.normalization != CITATION_TEXT_NORMALIZATION
        || validate_digest("artifact version hash", &citation.artifact.version_hash).is_err()
        || validate_digest("snapshot content hash", &citation.snapshot.content_hash).is_err()
        || validate_digest(
            "transcript version hash",
            &citation.resolution.transcript_version_hash,
        )
        .is_err()
        || hash_citation_text(&citation.snapshot.text) != citation.snapshot.content_hash
    {
        return CitationResolutionStatus::Unresolvable;
    }
    if citation.artifact.kind != EvidenceArtifactKind::Recording {
        return CitationResolutionStatus::Unresolvable;
    }
    match source_state {
        RecordingSourceState::Missing => return CitationResolutionStatus::SourceMissing,
        RecordingSourceState::VersionMissing => return CitationResolutionStatus::VersionMissing,
        RecordingSourceState::Available {
            actual_version_hash,
        } if actual_version_hash != &citation.artifact.version_hash => {
            return CitationResolutionStatus::ArtifactMismatch
        }
        RecordingSourceState::Available { .. } => {}
    }

    let Some(pinned) = pinned_transcript else {
        return CitationResolutionStatus::VersionMissing;
    };
    let Ok(pinned_hash) = hash_transcript_version(pinned) else {
        return CitationResolutionStatus::Unresolvable;
    };
    if pinned.recording_artifact_id != citation.artifact.id
        || pinned.recording_version_hash != citation.artifact.version_hash
        || pinned_hash != citation.resolution.transcript_version_hash
    {
        return CitationResolutionStatus::VersionMissing;
    }
    let Ok(pinned_span) = resolve_audio_locator(pinned, &citation.locator, recording_duration_ms)
    else {
        return CitationResolutionStatus::Unresolvable;
    };
    if pinned_span.segment_ids != citation.resolution.segment_ids
        || hash_citation_text(&pinned_span.text) != citation.snapshot.content_hash
    {
        return CitationResolutionStatus::Unresolvable;
    }

    CitationResolutionStatus::Verified
}

pub fn resolve_current_citation(
    citation: &CitationEnvelope,
    recording_duration_ms: u64,
    source_state: &RecordingSourceState,
    pinned_transcript: Option<&TranscriptVersionContent>,
    active_transcript: Option<&TranscriptVersionContent>,
) -> CitationResolutionStatus {
    let historical = resolve_historical_citation(
        citation,
        recording_duration_ms,
        source_state,
        pinned_transcript,
    );
    if historical != CitationResolutionStatus::Verified {
        return historical;
    }
    let Some(pinned) = pinned_transcript else {
        return CitationResolutionStatus::VersionMissing;
    };
    let Some(active) = active_transcript else {
        return CitationResolutionStatus::VersionMissing;
    };
    let Ok(pinned_hash) = hash_transcript_version(pinned) else {
        return CitationResolutionStatus::Unresolvable;
    };
    let Ok(active_hash) = hash_transcript_version(active) else {
        return CitationResolutionStatus::Unresolvable;
    };
    if active_hash == pinned_hash {
        return CitationResolutionStatus::Verified;
    }
    if active.recording_artifact_id != citation.artifact.id
        || active.recording_version_hash != citation.artifact.version_hash
    {
        return CitationResolutionStatus::VersionMissing;
    }
    let Ok(active_span) = resolve_audio_locator(active, &citation.locator, recording_duration_ms)
    else {
        return CitationResolutionStatus::Unresolvable;
    };
    if hash_citation_text(&active_span.text) == citation.snapshot.content_hash {
        CitationResolutionStatus::Superseded
    } else {
        CitationResolutionStatus::EvidenceChanged
    }
}

fn resolve_audio_locator(
    transcript: &TranscriptVersionContent,
    locator: &EvidenceLocator,
    recording_duration_ms: u64,
) -> Result<ResolvedAudioSpan, EvidenceResolverError> {
    transcript.validate()?;
    let EvidenceLocator::AudioTimeline { start_ms, end_ms } = locator else {
        return Err(EvidenceResolverError::DocumentResolutionUnavailable);
    };
    validate_audio_bounds(*start_ms, *end_ms, recording_duration_ms)?;
    let selected = transcript
        .segments
        .iter()
        .filter(|segment| segment.start_ms < *end_ms && segment.end_ms > *start_ms)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err(EvidenceResolverError::NoOverlappingSegments);
    }
    resolved_span_from_refs(&selected)
}

fn validate_audio_bounds(
    start_ms: u64,
    end_ms: u64,
    recording_duration_ms: u64,
) -> Result<(), EvidenceResolverError> {
    if end_ms <= start_ms || end_ms > MAX_CANONICAL_INTEGER {
        return Err(EvidenceResolverError::InvalidAudioBounds { start_ms, end_ms });
    }
    if end_ms > recording_duration_ms.saturating_add(1) {
        return Err(EvidenceResolverError::BeyondRecording);
    }
    Ok(())
}

fn resolved_span(
    segments: &[TranscriptVersionSegment],
) -> Result<ResolvedAudioSpan, EvidenceResolverError> {
    let refs = segments.iter().collect::<Vec<_>>();
    resolved_span_from_refs(&refs)
}

fn resolved_span_from_refs(
    segments: &[&TranscriptVersionSegment],
) -> Result<ResolvedAudioSpan, EvidenceResolverError> {
    let Some(first) = segments.first() else {
        return Err(EvidenceResolverError::NoOverlappingSegments);
    };
    Ok(ResolvedAudioSpan {
        start_ms: first.start_ms,
        end_ms: segments
            .iter()
            .map(|segment| segment.end_ms)
            .max()
            .expect("resolved segments are non-empty"),
        text: segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        segment_ids: segments.iter().map(|segment| segment.segment_id).collect(),
    })
}
