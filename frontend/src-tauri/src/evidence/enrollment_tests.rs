use chrono::Utc;
use tempfile::tempdir;
use uuid::Uuid;

use crate::database::test_support::TestDatabase;

use super::{enroll_recording_file, RecordingArtifactKind};

#[tokio::test]
async fn file_enrollment_hashes_bytes_and_is_idempotent_without_modifying_source() {
    let database = TestDatabase::new().await;
    let meeting_id = format!("meeting-{}", Uuid::new_v4());
    let now = Utc::now();
    sqlx::query("INSERT INTO meetings (id, title, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(&meeting_id)
        .bind("Enrollment test")
        .bind(now)
        .bind(now)
        .execute(database.pool())
        .await
        .unwrap();
    let directory = tempdir().unwrap();
    let recording_path = directory.path().join("recording.wav");
    std::fs::write(&recording_path, b"abc").unwrap();

    let first = enroll_recording_file(
        database.pool(),
        &meeting_id,
        RecordingArtifactKind::Imported,
        &recording_path,
        1_000,
    )
    .await
    .unwrap();
    let second = enroll_recording_file(
        database.pool(),
        &meeting_id,
        RecordingArtifactKind::Captured,
        &recording_path,
        1_000,
    )
    .await
    .unwrap();

    assert_eq!(first.artifact.id, second.artifact.id);
    assert_eq!(second.artifact.kind, "imported");
    assert_eq!(
        first.version.version_hash,
        "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(std::fs::read(&recording_path).unwrap(), b"abc");
    let version_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM recording_artifact_versions")
        .fetch_one(database.pool())
        .await
        .unwrap();
    assert_eq!(version_count, 1);
}
