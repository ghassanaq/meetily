use uuid::Uuid;

use super::{
    build_audio_citation_for_interval, build_audio_citation_from_segments, hash_citation_envelope,
    hash_citation_text, hash_transcript_version, normalize_citation_text, resolve_current_citation,
    resolve_historical_citation, seconds_to_millis, CitationResolutionStatus, EvidenceArtifactKind,
    EvidenceArtifactReference, EvidenceLocator, EvidenceResolverError, RecordingSourceState,
    TranscriptVersionContent, TranscriptVersionSegment, CITATION_TEXT_NORMALIZATION,
    TRANSCRIPT_VERSION_SCHEMA,
};

const RECORDING_HASH: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn transcript() -> TranscriptVersionContent {
    TranscriptVersionContent {
        schema_version: TRANSCRIPT_VERSION_SCHEMA,
        recording_artifact_id: Uuid::new_v4(),
        recording_version_hash: RECORDING_HASH.to_owned(),
        language: Some("en".to_owned()),
        engine: "fixture-engine".to_owned(),
        model: "fixture-model-v1".to_owned(),
        configuration_hash: None,
        segments: vec![
            segment(0, 1_000, "  Cafe\u{301}\r\nconfirmed.  "),
            segment(1_000, 2_000, "Ship on Friday."),
            segment(2_500, 3_000, "Follow up next week."),
        ],
    }
}

fn segment(start_ms: u64, end_ms: u64, text: &str) -> TranscriptVersionSegment {
    TranscriptVersionSegment {
        segment_id: Uuid::new_v4(),
        start_ms,
        end_ms,
        text: text.to_owned(),
        speaker: None,
        source: None,
    }
}

fn available() -> RecordingSourceState {
    RecordingSourceState::Available {
        actual_version_hash: RECORDING_HASH.to_owned(),
    }
}

#[test]
fn snapshot_normalization_is_nfc_line_ending_stable_and_outer_trimmed() {
    let decomposed = " \tCafe\u{301}\r\nconfirmed.\r ";
    let normalized = normalize_citation_text(decomposed);

    assert_eq!(normalized, "Café\nconfirmed.");
    assert_eq!(
        hash_citation_text(decomposed),
        hash_citation_text("Café\nconfirmed.")
    );
}

#[test]
fn interval_selection_uses_half_open_overlap_and_snaps_to_whole_segments() {
    let transcript = transcript();
    let citation =
        build_audio_citation_for_interval(Uuid::new_v4(), &transcript, 3_000, 1_000, 1_500)
            .expect("the interval should select only the second segment");

    assert_eq!(
        citation.locator,
        EvidenceLocator::AudioTimeline {
            start_ms: 1_000,
            end_ms: 2_000
        }
    );
    assert_eq!(
        citation.resolution.segment_ids,
        vec![transcript.segments[1].segment_id]
    );
    assert_eq!(citation.snapshot.text, "Ship on Friday.");
    assert_eq!(citation.snapshot.normalization, CITATION_TEXT_NORMALIZATION);
}

#[test]
fn overlapping_segments_are_kept_and_gaps_do_not_invent_evidence() {
    let mut transcript = transcript();
    transcript.segments[0].end_ms = 1_200;
    let overlapping =
        build_audio_citation_for_interval(Uuid::new_v4(), &transcript, 3_000, 1_100, 1_150)
            .expect("both overlapping segments should be retained");

    assert_eq!(overlapping.resolution.segment_ids.len(), 2);
    assert_eq!(
        overlapping.locator,
        EvidenceLocator::AudioTimeline {
            start_ms: 0,
            end_ms: 2_000
        }
    );

    let gap = build_audio_citation_for_interval(Uuid::new_v4(), &transcript, 3_000, 2_100, 2_400);
    assert!(matches!(
        gap,
        Err(EvidenceResolverError::NoOverlappingSegments)
    ));
}

#[test]
fn explicit_segment_selection_rejects_noncontiguous_or_reordered_ids() {
    let transcript = transcript();
    let noncontiguous = build_audio_citation_from_segments(
        Uuid::new_v4(),
        &transcript,
        3_000,
        &[
            transcript.segments[0].segment_id,
            transcript.segments[2].segment_id,
        ],
    );
    assert!(matches!(
        noncontiguous,
        Err(EvidenceResolverError::NoncontiguousSegments)
    ));

    let reordered = build_audio_citation_from_segments(
        Uuid::new_v4(),
        &transcript,
        3_000,
        &[
            transcript.segments[1].segment_id,
            transcript.segments[0].segment_id,
        ],
    );
    assert!(matches!(
        reordered,
        Err(EvidenceResolverError::NoncontiguousSegments)
    ));
}

#[test]
fn seconds_conversion_is_rounded_and_rejects_invalid_or_unsafe_values() {
    assert_eq!(seconds_to_millis(12.5004).unwrap(), 12_500);
    assert_eq!(seconds_to_millis(12.5006).unwrap(), 12_501);
    assert!(seconds_to_millis(f64::NAN).is_err());
    assert!(seconds_to_millis(f64::INFINITY).is_err());
    assert!(seconds_to_millis(-0.001).is_err());
}

#[test]
fn resolver_distinguishes_verified_superseded_and_changed_evidence() {
    let pinned = transcript();
    let citation =
        build_audio_citation_for_interval(Uuid::new_v4(), &pinned, 3_000, 1_200, 1_500).unwrap();

    assert_eq!(
        resolve_current_citation(&citation, 3_000, &available(), Some(&pinned), Some(&pinned),),
        CitationResolutionStatus::Verified
    );

    let mut superseding = pinned.clone();
    superseding.model = "fixture-model-v2".to_owned();
    assert_ne!(
        hash_transcript_version(&pinned).unwrap(),
        hash_transcript_version(&superseding).unwrap()
    );
    assert_eq!(
        resolve_current_citation(
            &citation,
            3_000,
            &available(),
            Some(&pinned),
            Some(&superseding)
        ),
        CitationResolutionStatus::Superseded
    );

    superseding.segments[1].text = "Ship on Monday.".to_owned();
    assert_eq!(
        resolve_current_citation(
            &citation,
            3_000,
            &available(),
            Some(&pinned),
            Some(&superseding)
        ),
        CitationResolutionStatus::EvidenceChanged
    );
}

#[test]
fn missing_replaced_and_missing_versions_have_distinct_closed_states() {
    let transcript = transcript();
    let citation =
        build_audio_citation_for_interval(Uuid::new_v4(), &transcript, 3_000, 1_200, 1_500)
            .unwrap();

    assert_eq!(
        resolve_current_citation(
            &citation,
            3_000,
            &RecordingSourceState::Missing,
            Some(&transcript),
            Some(&transcript)
        ),
        CitationResolutionStatus::SourceMissing
    );
    assert_eq!(
        resolve_current_citation(
            &citation,
            3_000,
            &RecordingSourceState::Available {
                actual_version_hash:
                    "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                        .to_owned(),
            },
            Some(&transcript),
            Some(&transcript)
        ),
        CitationResolutionStatus::ArtifactMismatch
    );
    assert_eq!(
        resolve_current_citation(&citation, 3_000, &available(), None, Some(&transcript)),
        CitationResolutionStatus::VersionMissing
    );
    assert_eq!(
        resolve_current_citation(&citation, 3_000, &available(), Some(&transcript), None),
        CitationResolutionStatus::VersionMissing
    );
    assert_eq!(
        resolve_historical_citation(&citation, 3_000, &available(), Some(&transcript)),
        CitationResolutionStatus::Verified
    );
}

#[test]
fn one_millisecond_recording_rounding_tolerance_is_explicit_and_bounded() {
    let mut transcript = transcript();
    transcript.segments[2].end_ms = 3_001;
    let tolerated = build_audio_citation_from_segments(
        Uuid::new_v4(),
        &transcript,
        3_000,
        &[transcript.segments[2].segment_id],
    );
    assert!(tolerated.is_ok());

    transcript.segments[2].end_ms = 3_002;
    let rejected = build_audio_citation_from_segments(
        Uuid::new_v4(),
        &transcript,
        3_000,
        &[transcript.segments[2].segment_id],
    );
    assert!(matches!(
        rejected,
        Err(EvidenceResolverError::BeyondRecording)
    ));
}

#[test]
fn envelope_digest_excludes_random_citation_identity_but_covers_evidence() {
    let transcript = transcript();
    let citation =
        build_audio_citation_for_interval(Uuid::new_v4(), &transcript, 3_000, 1_200, 1_500)
            .unwrap();
    let mut another_identity = citation.clone();
    another_identity.citation_id = Uuid::new_v4();
    assert_eq!(
        hash_citation_envelope(&citation).unwrap(),
        hash_citation_envelope(&another_identity).unwrap()
    );

    another_identity.snapshot.text = "different".to_owned();
    assert_ne!(
        hash_citation_envelope(&citation).unwrap(),
        hash_citation_envelope(&another_identity).unwrap()
    );
}

#[test]
fn document_locator_is_a_closed_round_trippable_reserved_variant() {
    let locator = EvidenceLocator::DocumentPassage {
        page_index: 0,
        section_path: vec!["Decisions".to_owned(), "Rollout".to_owned()],
        start_byte: 140,
        end_byte: 267,
    };
    let json = serde_json::to_string(&locator).unwrap();
    assert_eq!(
        serde_json::from_str::<EvidenceLocator>(&json).unwrap(),
        locator
    );
    assert!(serde_json::from_str::<EvidenceLocator>(
        r#"{"type":"document_passage","page_index":0,"section_path":[],"start_byte":0,"end_byte":1,"script":"no"}"#
    )
    .is_err());

    let artifact = EvidenceArtifactReference {
        id: Uuid::new_v4(),
        kind: EvidenceArtifactKind::Document,
        version_hash: RECORDING_HASH.to_owned(),
    };
    assert_eq!(artifact.kind, EvidenceArtifactKind::Document);
}
