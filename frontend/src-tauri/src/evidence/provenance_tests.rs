use chrono::Utc;
use uuid::Uuid;

use crate::database::test_support::TestDatabase;

use super::{
    build_audio_citation_from_segments, hash_citation_envelope, hash_derived_artifact_payload,
    CitationDependency, CitationRole, DerivedArtifactKind, DerivedArtifactSpec, EvidenceRepository,
    LegacyTranscriptProjection, ProvenanceRepository, ProvenanceRepositoryError,
    RecordingArtifactKind, RecordingSourceState, RecordingVersionSpec, TranscriptVersionContent,
    TranscriptVersionSegment, TRANSCRIPT_VERSION_SCHEMA,
};

const RECORDING_HASH: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

async fn fixture() -> (TestDatabase, String, Uuid, TranscriptVersionContent) {
    let database = TestDatabase::new().await;
    let meeting_id = format!("meeting-{}", Uuid::new_v4());
    let now = Utc::now();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(&meeting_id)
        .bind("Provenance transaction test")
        .bind(now)
        .bind(now)
        .execute(database.pool())
        .await
        .unwrap();

    let artifact_id = Uuid::new_v4();
    EvidenceRepository::create_recording_with_version(
        database.pool(),
        artifact_id,
        &meeting_id,
        RecordingArtifactKind::Captured,
        &RecordingVersionSpec {
            version_hash: RECORDING_HASH.to_owned(),
            byte_length: 100_000,
            media_type: "audio/mp4".to_owned(),
            duration_ms: 10_000,
        },
        Some("C:/meetings/provenance/audio.mp4"),
    )
    .await
    .unwrap();

    let transcript = TranscriptVersionContent {
        schema_version: TRANSCRIPT_VERSION_SCHEMA,
        recording_artifact_id: artifact_id,
        recording_version_hash: RECORDING_HASH.to_owned(),
        language: Some("en".to_owned()),
        engine: "whisper".to_owned(),
        model: "model-v1".to_owned(),
        configuration_hash: None,
        segments: vec![
            segment(1_000, 3_000, "Budget is approved."),
            segment(4_000, 6_000, "Mina will send the proposal Friday."),
        ],
    };
    EvidenceRepository::install_transcript_version(database.pool(), Uuid::new_v4(), &transcript)
        .await
        .unwrap();
    (database, meeting_id, artifact_id, transcript)
}

fn segment(start_ms: u64, end_ms: u64, text: &str) -> TranscriptVersionSegment {
    TranscriptVersionSegment {
        segment_id: Uuid::new_v4(),
        start_ms,
        end_ms,
        text: text.to_owned(),
        speaker: None,
        source: Some("captured".to_owned()),
    }
}

fn available() -> RecordingSourceState {
    RecordingSourceState::Available {
        actual_version_hash: RECORDING_HASH.to_owned(),
    }
}

async fn persist_citation(
    database: &TestDatabase,
    transcript: &TranscriptVersionContent,
    segment_index: usize,
) -> super::StoredEvidenceCitation {
    let citation = build_audio_citation_from_segments(
        Uuid::new_v4(),
        transcript,
        10_000,
        &[transcript.segments[segment_index].segment_id],
    )
    .unwrap();
    ProvenanceRepository::persist_verified_citation(
        database.pool(),
        &citation,
        10_000,
        &available(),
        transcript,
        transcript,
    )
    .await
    .unwrap()
}

async fn register_artifact(
    database: &TestDatabase,
    meeting_id: &str,
    citation_id: Uuid,
) -> DerivedArtifactSpec {
    let payload = format!("derived output for {citation_id}").into_bytes();
    let artifact = DerivedArtifactSpec {
        id: Uuid::new_v4(),
        version_hash: hash_derived_artifact_payload(&payload),
        meeting_id: meeting_id.to_owned(),
        kind: DerivedArtifactKind::Intelligence,
        content_payload: payload,
    };
    ProvenanceRepository::register_derived_artifact(
        database.pool(),
        &artifact,
        &[CitationDependency {
            citation_id,
            role: CitationRole::Supporting,
        }],
    )
    .await
    .unwrap();
    artifact
}

#[tokio::test]
async fn citation_persistence_requires_current_verified_evidence() {
    let (database, _, _, transcript) = fixture().await;
    let citation = build_audio_citation_from_segments(
        Uuid::new_v4(),
        &transcript,
        10_000,
        &[transcript.segments[0].segment_id],
    )
    .unwrap();
    let mut superseding = transcript.clone();
    superseding.model = "model-v2".to_owned();

    let error = ProvenanceRepository::persist_verified_citation(
        database.pool(),
        &citation,
        10_000,
        &available(),
        &transcript,
        &superseding,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        error,
        ProvenanceRepositoryError::CitationNotVerified
    ));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM evidence_citations")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn changed_span_invalidates_only_its_dependent_artifact() {
    let (database, meeting_id, _, transcript) = fixture().await;
    let first_citation = persist_citation(&database, &transcript, 0).await;
    let second_citation = persist_citation(&database, &transcript, 1).await;
    let first_artifact = register_artifact(
        &database,
        &meeting_id,
        Uuid::parse_str(&first_citation.id).unwrap(),
    )
    .await;
    let second_artifact = register_artifact(
        &database,
        &meeting_id,
        Uuid::parse_str(&second_citation.id).unwrap(),
    )
    .await;

    let mut replacement = transcript.clone();
    replacement.model = "model-v2".to_owned();
    replacement.segments[0].segment_id = Uuid::new_v4();
    replacement.segments[0].text = "Budget approval is still pending.".to_owned();
    replacement.segments[1].segment_id = Uuid::new_v4();
    let outcome = ProvenanceRepository::install_transcript_version_and_invalidate(
        database.pool(),
        Uuid::new_v4(),
        &replacement,
    )
    .await
    .unwrap();

    assert_eq!(outcome.invalidations_created, 1);
    let invalidated_ids: Vec<String> =
        sqlx::query_scalar("SELECT derived_artifact_id FROM derived_artifact_invalidations")
            .fetch_all(database.pool())
            .await
            .unwrap();
    assert_eq!(invalidated_ids, vec![first_artifact.id.to_string()]);
    assert_ne!(invalidated_ids[0], second_artifact.id.to_string());
}

#[tokio::test]
async fn replacement_segment_ids_with_unchanged_evidence_do_not_invalidate() {
    let (database, meeting_id, _, transcript) = fixture().await;
    let citation = persist_citation(&database, &transcript, 0).await;
    register_artifact(
        &database,
        &meeting_id,
        Uuid::parse_str(&citation.id).unwrap(),
    )
    .await;

    let mut replacement = transcript.clone();
    replacement.model = "model-v2".to_owned();
    for segment in &mut replacement.segments {
        segment.segment_id = Uuid::new_v4();
    }
    let outcome = ProvenanceRepository::install_transcript_version_and_invalidate(
        database.pool(),
        Uuid::new_v4(),
        &replacement,
    )
    .await
    .unwrap();

    assert_eq!(outcome.invalidations_created, 0);
}

#[tokio::test]
async fn retranscription_updates_immutable_head_and_legacy_projection_atomically() {
    let (database, meeting_id, artifact_id, transcript) = fixture().await;
    sqlx::query(
        r#"
        INSERT INTO transcripts
            (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration)
        VALUES ('old-row', ?, 'old text', ?, 0.0, 1.0, 1.0)
        "#,
    )
    .bind(&meeting_id)
    .bind(Utc::now())
    .execute(database.pool())
    .await
    .unwrap();

    let mut replacement = transcript.clone();
    replacement.model = "model-v2".to_owned();
    replacement.segments[0].text = "New immutable interpretation.".to_owned();
    let projection = vec![LegacyTranscriptProjection {
        id: "new-row".to_owned(),
        text: "New immutable interpretation.".to_owned(),
        timestamp: Utc::now().to_rfc3339(),
        audio_start_time: Some(1.0),
        audio_end_time: Some(3.0),
        duration: Some(2.0),
    }];
    ProvenanceRepository::install_retranscription_and_invalidate(
        database.pool(),
        Uuid::new_v4(),
        &replacement,
        &meeting_id,
        &projection,
    )
    .await
    .unwrap();

    let active = EvidenceRepository::get_active_transcript_version(database.pool(), artifact_id)
        .await
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(
        active.version_hash,
        super::hash_transcript_version(&replacement).unwrap()
    );
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, transcript FROM transcripts WHERE meeting_id = ?")
            .bind(&meeting_id)
            .fetch_all(database.pool())
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![(
            "new-row".to_owned(),
            "New immutable interpretation.".to_owned()
        )]
    );
}

#[tokio::test]
async fn invalidation_is_idempotent_for_the_same_new_transcript_version() {
    let (database, meeting_id, _, transcript) = fixture().await;
    let citation = persist_citation(&database, &transcript, 0).await;
    register_artifact(
        &database,
        &meeting_id,
        Uuid::parse_str(&citation.id).unwrap(),
    )
    .await;
    let mut replacement = transcript.clone();
    replacement.model = "model-v2".to_owned();
    replacement.segments[0].text = "The budget was rejected.".to_owned();

    let first = ProvenanceRepository::install_transcript_version_and_invalidate(
        database.pool(),
        Uuid::new_v4(),
        &replacement,
    )
    .await
    .unwrap();
    let second = ProvenanceRepository::install_transcript_version_and_invalidate(
        database.pool(),
        Uuid::new_v4(),
        &replacement,
    )
    .await
    .unwrap();
    assert_eq!(first.invalidations_created, 1);
    assert_eq!(second.invalidations_created, 0);
}

#[tokio::test]
async fn invalidation_failure_rolls_back_the_active_transcript_head() {
    let (database, meeting_id, artifact_id, transcript) = fixture().await;
    sqlx::query(
        r#"
        INSERT INTO transcripts
            (id, meeting_id, transcript, timestamp, audio_start_time, audio_end_time, duration)
        VALUES ('old-row', ?, 'old text', ?, 0.0, 1.0, 1.0)
        "#,
    )
    .bind(&meeting_id)
    .bind(Utc::now())
    .execute(database.pool())
    .await
    .unwrap();
    let active_before =
        EvidenceRepository::get_active_transcript_version(database.pool(), artifact_id)
            .await
            .unwrap()
            .unwrap()
            .0;

    let malformed_id = Uuid::new_v4();
    let malformed_digest =
        "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    sqlx::query(
        r#"
        INSERT INTO evidence_citations
            (id, citation_digest, recording_artifact_id, recording_version_hash,
             transcript_version_hash, locator_type, envelope_payload, created_at)
        VALUES (?, ?, ?, ?, ?, 'audio_timeline', ?, ?)
        "#,
    )
    .bind(malformed_id.to_string())
    .bind(malformed_digest)
    .bind(artifact_id.to_string())
    .bind(RECORDING_HASH)
    .bind(super::hash_transcript_version(&transcript).unwrap())
    .bind(b"{}".as_slice())
    .bind(Utc::now())
    .execute(database.pool())
    .await
    .unwrap();
    register_artifact(&database, &meeting_id, malformed_id).await;

    let mut replacement = transcript.clone();
    replacement.model = "model-v2".to_owned();
    replacement.segments[0].text = "Changed after malformed provenance.".to_owned();
    let replacement_projection = [LegacyTranscriptProjection {
        id: "replacement-row".to_owned(),
        text: "Changed after malformed provenance.".to_owned(),
        timestamp: Utc::now().to_rfc3339(),
        audio_start_time: Some(1.0),
        audio_end_time: Some(3.0),
        duration: Some(2.0),
    }];
    let result = ProvenanceRepository::install_retranscription_and_invalidate(
        database.pool(),
        Uuid::new_v4(),
        &replacement,
        &meeting_id,
        &replacement_projection,
    )
    .await;
    assert!(result.is_err());

    let active_after =
        EvidenceRepository::get_active_transcript_version(database.pool(), artifact_id)
            .await
            .unwrap()
            .unwrap()
            .0;
    assert_eq!(active_after.version_hash, active_before.version_hash);
    let replacement_hash = super::hash_transcript_version(&replacement).unwrap();
    let replacement_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transcript_versions WHERE version_hash = ?")
            .bind(replacement_hash)
            .fetch_one(database.pool())
            .await
            .unwrap();
    assert_eq!(replacement_count, 0);
    let legacy_rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, transcript FROM transcripts WHERE meeting_id = ?")
            .bind(&meeting_id)
            .fetch_all(database.pool())
            .await
            .unwrap();
    assert_eq!(
        legacy_rows,
        vec![("old-row".to_owned(), "old text".to_owned())]
    );
}

#[tokio::test]
async fn citation_and_derived_versions_are_immutable_in_sqlite() {
    let (database, meeting_id, _, transcript) = fixture().await;
    let citation = persist_citation(&database, &transcript, 0).await;
    let artifact = register_artifact(
        &database,
        &meeting_id,
        Uuid::parse_str(&citation.id).unwrap(),
    )
    .await;

    assert!(
        sqlx::query("UPDATE evidence_citations SET locator_type = 'document_passage'")
            .execute(database.pool())
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE derived_artifacts SET kind = 'summary' WHERE id = ?")
            .bind(artifact.id.to_string())
            .execute(database.pool())
            .await
            .is_err()
    );
}

#[test]
fn citation_digest_is_not_a_model_authored_marker() {
    let transcript = TranscriptVersionContent {
        schema_version: TRANSCRIPT_VERSION_SCHEMA,
        recording_artifact_id: Uuid::new_v4(),
        recording_version_hash: RECORDING_HASH.to_owned(),
        language: None,
        engine: "fixture".to_owned(),
        model: "fixture".to_owned(),
        configuration_hash: None,
        segments: vec![segment(0, 1_000, "Decision text")],
    };
    let citation = build_audio_citation_from_segments(
        Uuid::new_v4(),
        &transcript,
        1_000,
        &[transcript.segments[0].segment_id],
    )
    .unwrap();
    let digest = hash_citation_envelope(&citation).unwrap();
    assert_ne!(digest, "E000001");
}
