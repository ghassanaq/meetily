use super::repositories::{
    meeting::MeetingsRepository, summary::SummaryProcessesRepository,
    transcript::TranscriptsRepository,
};
use super::test_support::TestDatabase;
use crate::api::TranscriptSegment;
use serde_json::json;

fn transcript_segments() -> Vec<TranscriptSegment> {
    vec![
        TranscriptSegment {
            id: "segment-1".to_string(),
            text: "The team approved the local-first workspace plan.".to_string(),
            timestamp: "00:00:01".to_string(),
            audio_start_time: Some(1.0),
            audio_end_time: Some(3.5),
            duration: Some(2.5),
        },
        TranscriptSegment {
            id: "segment-2".to_string(),
            text: "Mina will document the follow-up actions.".to_string(),
            timestamp: "00:00:04".to_string(),
            audio_start_time: Some(4.0),
            audio_end_time: Some(6.0),
            duration: Some(2.0),
        },
    ]
}

async fn seed_meeting(database: &TestDatabase) -> String {
    TranscriptsRepository::save_transcript(
        database.pool(),
        "Baseline workflow",
        &transcript_segments(),
        Some("workspace/baseline".to_string()),
    )
    .await
    .expect("failed to persist meeting transcript")
}

#[tokio::test]
async fn migrated_database_persists_meeting_transcripts_and_summary() {
    let database = TestDatabase::new().await;
    let meeting_id = seed_meeting(&database).await;

    let metadata = MeetingsRepository::get_meeting_metadata(database.pool(), &meeting_id)
        .await
        .expect("failed to load meeting metadata")
        .expect("meeting metadata was not persisted");
    assert_eq!(metadata.title, "Baseline workflow");
    assert_eq!(metadata.folder_path.as_deref(), Some("workspace/baseline"));

    let meeting = MeetingsRepository::get_meeting(database.pool(), &meeting_id)
        .await
        .expect("failed to load meeting")
        .expect("meeting was not persisted");
    assert_eq!(meeting.transcripts.len(), 2);
    assert_eq!(meeting.transcripts[0].audio_start_time, Some(1.0));
    assert_eq!(meeting.transcripts[1].duration, Some(2.0));

    let (page, total) =
        MeetingsRepository::get_meeting_transcripts_paginated(database.pool(), &meeting_id, 1, 1)
            .await
            .expect("failed to load transcript page");
    assert_eq!(total, 2);
    assert_eq!(page.len(), 1);
    assert_eq!(
        page[0].transcript,
        "Mina will document the follow-up actions."
    );

    let search_results = TranscriptsRepository::search_transcripts(database.pool(), "local-first")
        .await
        .expect("failed to search transcripts");
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0].id, meeting_id);

    SummaryProcessesRepository::create_or_reset_process(database.pool(), &meeting_id)
        .await
        .expect("failed to create summary process");
    SummaryProcessesRepository::update_process_completed(
        database.pool(),
        &meeting_id,
        json!({"summary": "The workspace plan was approved."}),
        1,
        0.25,
    )
    .await
    .expect("failed to persist completed summary");

    let summary = SummaryProcessesRepository::get_summary_data(database.pool(), &meeting_id)
        .await
        .expect("failed to load summary process")
        .expect("summary process was not persisted");
    assert_eq!(summary.status, "completed");
    assert_eq!(summary.chunk_count, 1);
    assert_eq!(summary.processing_time, 0.25);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            summary
                .result
                .as_deref()
                .expect("summary result was missing")
        )
        .expect("summary result was not valid JSON"),
        json!({"summary": "The workspace plan was approved."})
    );
}

#[tokio::test]
async fn deleting_a_meeting_removes_its_persisted_workflow_state() {
    let database = TestDatabase::new().await;
    let meeting_id = seed_meeting(&database).await;

    SummaryProcessesRepository::create_or_reset_process(database.pool(), &meeting_id)
        .await
        .expect("failed to create summary process");
    assert!(MeetingsRepository::update_meeting_title(
        database.pool(),
        &meeting_id,
        "Renamed baseline workflow"
    )
    .await
    .expect("failed to update meeting title"));

    assert!(
        MeetingsRepository::delete_meeting(database.pool(), &meeting_id)
            .await
            .expect("failed to delete meeting")
    );
    assert!(MeetingsRepository::get_meetings(database.pool())
        .await
        .expect("failed to list meetings")
        .is_empty());
    assert!(
        SummaryProcessesRepository::get_summary_data(database.pool(), &meeting_id)
            .await
            .expect("failed to query summary after deletion")
            .is_none()
    );

    let transcript_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM transcripts WHERE meeting_id = ?")
            .bind(&meeting_id)
            .fetch_one(database.pool())
            .await
            .expect("failed to count transcripts after deletion");
    assert_eq!(transcript_count, 0);
}
