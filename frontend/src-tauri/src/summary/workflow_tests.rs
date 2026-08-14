use super::{service::SummaryService, CustomOpenAIConfig};
use crate::api::TranscriptSegment;
use crate::database::repositories::{
    setting::SettingsRepository, summary::SummaryProcessesRepository,
    transcript::TranscriptsRepository,
};
use crate::database::test_support::TestDatabase;
use serde_json::{json, Value};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn custom_openai_summary_orchestration_persists_the_completed_result() {
    let database = TestDatabase::new().await;
    let transcript_text = concat!(
        "The planning group approved a local-first meeting workspace. ",
        "Mina will document the follow-up actions, and Sam will review the draft on Friday."
    );
    let meeting_id = TranscriptsRepository::save_transcript(
        database.pool(),
        "Planning session",
        &[TranscriptSegment {
            id: "summary-segment-1".to_string(),
            text: transcript_text.to_string(),
            timestamp: "00:00:01".to_string(),
            audio_start_time: Some(1.0),
            audio_end_time: Some(8.0),
            duration: Some(7.0),
        }],
        Some("workspace/planning".to_string()),
    )
    .await
    .expect("failed to seed transcript for summary test");

    let mock_server = MockServer::start().await;
    let model_name = "local-summary-model";
    SettingsRepository::save_custom_openai_config(
        database.pool(),
        &CustomOpenAIConfig {
            endpoint: format!("{}/v1", mock_server.uri()),
            api_key: Some("local-test-key".to_string()),
            model: model_name.to_string(),
            max_tokens: Some(512),
            temperature: Some(0.2),
            top_p: Some(0.9),
        },
    )
    .await
    .expect("failed to persist custom OpenAI configuration");
    SummaryProcessesRepository::create_or_reset_process(database.pool(), &meeting_id)
        .await
        .expect("failed to create summary process");

    let summary_markdown = concat!(
        "# Planning session\n\n",
        "## Summary\nThe group approved the local-first workspace.\n\n",
        "## Action Items\n- Mina documents the actions.\n- Sam reviews the draft Friday."
    );
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer local-test-key"))
        .and(body_partial_json(json!({
            "model": model_name,
            "max_tokens": 512
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{"message": {"content": summary_markdown}}]
        })))
        .expect(1)
        .mount(&mock_server)
        .await;

    SummaryService::process_transcript_with_app_data_dir(
        None,
        database.pool().clone(),
        meeting_id.clone(),
        transcript_text.to_string(),
        "custom-openai".to_string(),
        model_name.to_string(),
        "Prioritize decisions and owners.".to_string(),
        "standard_meeting".to_string(),
        Some("en".to_string()),
    )
    .await;

    mock_server.verify().await;
    let requests = mock_server
        .received_requests()
        .await
        .expect("mock server did not retain requests");
    let request_body: Value =
        serde_json::from_slice(&requests[0].body).expect("request body was not valid JSON");
    let messages = request_body["messages"]
        .as_array()
        .expect("request did not contain chat messages");
    let temperature = request_body["temperature"]
        .as_f64()
        .expect("request did not contain a numeric temperature");
    let top_p = request_body["top_p"]
        .as_f64()
        .expect("request did not contain a numeric top_p");
    assert!((temperature - 0.2).abs() < 0.000_001);
    assert!((top_p - 0.9).abs() < 0.000_001);
    assert!(messages.iter().any(|message| {
        message["content"]
            .as_str()
            .is_some_and(|content| content.contains(transcript_text))
    }));
    assert!(messages.iter().any(|message| {
        message["content"]
            .as_str()
            .is_some_and(|content| content.contains("Prioritize decisions and owners."))
    }));

    let process = SummaryProcessesRepository::get_summary_data(database.pool(), &meeting_id)
        .await
        .expect("failed to load summary process")
        .expect("summary process was not persisted");
    assert_eq!(process.status, "completed");
    assert_eq!(process.chunk_count, 1);
    assert!(process.error.is_none());

    let result: Value = serde_json::from_str(
        process
            .result
            .as_deref()
            .expect("completed summary did not contain a result"),
    )
    .expect("summary result was not valid JSON");
    assert_eq!(
        result["markdown"],
        "## Summary\nThe group approved the local-first workspace.\n\n\
         ## Action Items\n- Mina documents the actions.\n- Sam reviews the draft Friday."
    );
    assert_eq!(result["english_cache"]["markdown"], summary_markdown);
    assert_eq!(
        result["english_cache"]["source"]["model_provider"],
        "custom-openai"
    );
}
