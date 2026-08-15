use chrono::Utc;
use uuid::Uuid;

use crate::database::test_support::TestDatabase;

use super::{
    hash_transcript_version, EvidenceRepository, EvidenceRepositoryError, EvidenceValidationError,
    RecordingArtifactKind, RecordingVersionSpec, TranscriptVersionContent,
    TranscriptVersionSegment, TRANSCRIPT_VERSION_SCHEMA,
};

const RECORDING_HASH: &str =
    "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

async fn database_with_meeting() -> (TestDatabase, String) {
    let database = TestDatabase::new().await;
    let meeting_id = format!("meeting-{}", Uuid::new_v4());
    let now = Utc::now();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(&meeting_id)
        .bind("Evidence foundation test")
        .bind(now)
        .bind(now)
        .execute(database.pool())
        .await
        .unwrap();
    (database, meeting_id)
}

fn recording_version() -> RecordingVersionSpec {
    RecordingVersionSpec {
        version_hash: RECORDING_HASH.to_string(),
        byte_length: 42_000,
        media_type: "audio/mp4".to_string(),
        duration_ms: 60_000,
    }
}

fn transcript_content(
    artifact_id: Uuid,
    first_text: &str,
    model: &str,
) -> TranscriptVersionContent {
    TranscriptVersionContent {
        schema_version: TRANSCRIPT_VERSION_SCHEMA,
        recording_artifact_id: artifact_id,
        recording_version_hash: RECORDING_HASH.to_string(),
        language: Some("en".to_string()),
        engine: "whisper".to_string(),
        model: model.to_string(),
        configuration_hash: Some(
            "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string(),
        ),
        segments: vec![
            TranscriptVersionSegment {
                segment_id: Uuid::new_v4(),
                start_ms: 1_000,
                end_ms: 4_000,
                text: first_text.to_string(),
                speaker: Some("microphone".to_string()),
                source: Some("captured".to_string()),
            },
            TranscriptVersionSegment {
                segment_id: Uuid::new_v4(),
                start_ms: 4_000,
                end_ms: 8_500,
                text: "Mina will send the revised proposal on Friday.".to_string(),
                speaker: Some("system".to_string()),
                source: Some("captured".to_string()),
            },
        ],
    }
}

async fn create_recording(database: &TestDatabase, meeting_id: &str, artifact_id: Uuid) {
    EvidenceRepository::create_recording_with_version(
        database.pool(),
        artifact_id,
        meeting_id,
        RecordingArtifactKind::Captured,
        &recording_version(),
        Some("C:/meetings/evidence-test/audio.mp4"),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn recording_and_active_transcript_round_trip_through_fresh_migration() {
    let (database, meeting_id) = database_with_meeting().await;
    let artifact_id = Uuid::new_v4();
    create_recording(&database, &meeting_id, artifact_id).await;
    let content = transcript_content(artifact_id, "The team approved the launch plan.", "base");

    let stored =
        EvidenceRepository::install_transcript_version(database.pool(), Uuid::new_v4(), &content)
            .await
            .unwrap();
    let (active, loaded) =
        EvidenceRepository::get_active_transcript_version(database.pool(), artifact_id)
            .await
            .unwrap()
            .expect("active transcript version should exist");

    assert_eq!(active, stored);
    assert_eq!(loaded, content);
    assert_eq!(
        stored.version_hash,
        hash_transcript_version(&loaded).unwrap()
    );
    assert_eq!(
        EvidenceRepository::get_recording_for_meeting(database.pool(), &meeting_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        artifact_id.to_string()
    );
    assert_eq!(
        EvidenceRepository::get_recording_location(database.pool(), artifact_id, RECORDING_HASH)
            .await
            .unwrap()
            .unwrap()
            .state,
        "available"
    );
    let relational_segment_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM transcript_version_segments WHERE transcript_version_id = ?",
    )
    .bind(&stored.id)
    .fetch_one(database.pool())
    .await
    .unwrap();
    assert_eq!(relational_segment_count, 2);
}

#[tokio::test]
async fn identical_transcript_content_deduplicates_and_reuses_the_original_identity() {
    let (database, meeting_id) = database_with_meeting().await;
    let artifact_id = Uuid::new_v4();
    create_recording(&database, &meeting_id, artifact_id).await;
    let content = transcript_content(artifact_id, "The team approved the launch plan.", "base");

    let first_id = Uuid::new_v4();
    let first = EvidenceRepository::install_transcript_version(database.pool(), first_id, &content)
        .await
        .unwrap();
    let duplicate =
        EvidenceRepository::install_transcript_version(database.pool(), Uuid::new_v4(), &content)
            .await
            .unwrap();

    assert_eq!(duplicate, first);
    assert_eq!(duplicate.id, first_id.to_string());
    assert_eq!(
        EvidenceRepository::list_transcript_versions(database.pool(), artifact_id)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn a_new_interpretation_advances_the_head_without_mutating_the_original() {
    let (database, meeting_id) = database_with_meeting().await;
    let artifact_id = Uuid::new_v4();
    create_recording(&database, &meeting_id, artifact_id).await;
    let original = transcript_content(artifact_id, "The launch plan was discussed.", "base");
    let corrected = transcript_content(artifact_id, "The launch plan was approved.", "large-v3");
    let original_id = Uuid::new_v4();
    let corrected_id = Uuid::new_v4();

    EvidenceRepository::install_transcript_version(database.pool(), original_id, &original)
        .await
        .unwrap();
    EvidenceRepository::install_transcript_version(database.pool(), corrected_id, &corrected)
        .await
        .unwrap();

    let (_, active) =
        EvidenceRepository::get_active_transcript_version(database.pool(), artifact_id)
            .await
            .unwrap()
            .unwrap();
    let (_, historical) = EvidenceRepository::get_transcript_version(database.pool(), original_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active, corrected);
    assert_eq!(historical, original);
    assert_eq!(
        EvidenceRepository::list_transcript_versions(database.pool(), artifact_id)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn database_triggers_reject_in_place_transcript_and_segment_mutation() {
    let (database, meeting_id) = database_with_meeting().await;
    let artifact_id = Uuid::new_v4();
    create_recording(&database, &meeting_id, artifact_id).await;
    let content = transcript_content(artifact_id, "The team approved the launch plan.", "base");
    let stored =
        EvidenceRepository::install_transcript_version(database.pool(), Uuid::new_v4(), &content)
            .await
            .unwrap();

    let version_error = sqlx::query("UPDATE transcript_versions SET model = ? WHERE id = ?")
        .bind("tampered")
        .bind(&stored.id)
        .execute(database.pool())
        .await
        .expect_err("immutable transcript version update must fail");
    let segment_error = sqlx::query(
        "UPDATE transcript_version_segments SET text = ? WHERE transcript_version_id = ?",
    )
    .bind("tampered")
    .bind(&stored.id)
    .execute(database.pool())
    .await
    .expect_err("immutable transcript segment update must fail");

    assert!(version_error.to_string().contains("immutable"));
    assert!(segment_error.to_string().contains("immutable"));
}

#[tokio::test]
async fn invalid_segment_order_has_zero_persistence_side_effects() {
    let (database, meeting_id) = database_with_meeting().await;
    let artifact_id = Uuid::new_v4();
    create_recording(&database, &meeting_id, artifact_id).await;
    let mut content = transcript_content(artifact_id, "The team approved the launch plan.", "base");
    content.segments[1].start_ms = 500;

    let error =
        EvidenceRepository::install_transcript_version(database.pool(), Uuid::new_v4(), &content)
            .await
            .unwrap_err();
    assert!(matches!(
        error,
        EvidenceRepositoryError::Validation(EvidenceValidationError::SegmentOrder { .. })
    ));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transcript_versions")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn segment_beyond_recording_rolls_back_the_new_version() {
    let (database, meeting_id) = database_with_meeting().await;
    let artifact_id = Uuid::new_v4();
    create_recording(&database, &meeting_id, artifact_id).await;
    let mut content = transcript_content(artifact_id, "The team approved the launch plan.", "base");
    content.segments[1].end_ms = 60_002;

    let error =
        EvidenceRepository::install_transcript_version(database.pool(), Uuid::new_v4(), &content)
            .await
            .unwrap_err();
    assert!(matches!(
        error,
        EvidenceRepositoryError::SegmentBeyondRecording { index: 1 }
    ));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transcript_versions")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn missing_meeting_cannot_leave_an_orphaned_recording() {
    let database = TestDatabase::new().await;
    let error = EvidenceRepository::create_recording_with_version(
        database.pool(),
        Uuid::new_v4(),
        "meeting-missing",
        RecordingArtifactKind::Imported,
        &recording_version(),
        None,
    )
    .await
    .unwrap_err();
    assert!(matches!(error, EvidenceRepositoryError::MeetingNotFound(_)));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM recording_artifacts")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn closed_transcript_schema_rejects_unknown_fields() {
    let artifact_id = Uuid::new_v4();
    let mut value = serde_json::to_value(transcript_content(
        artifact_id,
        "The team approved the launch plan.",
        "base",
    ))
    .unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("command".to_string(), serde_json::json!("run-me"));

    assert!(serde_json::from_value::<TranscriptVersionContent>(value).is_err());
}

#[test]
fn digests_are_canonical_lowercase_sha256_values() {
    let mut version = recording_version();
    version.version_hash =
        "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string();
    assert!(matches!(
        version.validate(),
        Err(EvidenceValidationError::InvalidDigest { .. })
    ));
}
