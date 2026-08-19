use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use chrono::Utc;
use reqwest::Client;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::*;

fn validate_false_history_fixture(output: &str) -> Result<()> {
    let normalized = output.to_lowercase();
    let false_commitments = [
        "i committed to friday",
        "i've committed to friday",
        "i have committed to friday",
        "we committed to friday",
        "i promised friday",
        "we promised friday",
    ];
    if let Some(statement) = false_commitments
        .iter()
        .find(|statement| normalized.contains(**statement))
    {
        return Err(anyhow!(
            "response converted an unspoken draft into history: {statement}"
        ));
    }
    Ok(())
}

fn sha256(value: impl AsRef<[u8]>) -> String {
    let mut digest = Sha256::new();
    digest.update(value.as_ref());
    format!("sha256:{:x}", digest.finalize())
}

fn git_sha() -> String {
    Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|sha| sha.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn append_record(record: &serde_json::Value) -> Result<PathBuf> {
    let path = std::env::var_os("MEETING_ASSISTANT_LIVE_HARNESS_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("target/live-assist-voice-harness.jsonl")
        });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    serde_json::to_writer(&mut file, record)?;
    file.write_all(b"\n")?;
    Ok(path)
}

fn parent_exchange() -> AssistExchange {
    AssistExchange {
        id: Uuid::new_v4(),
        ordinal: 1,
        kind: AssistExchangeKind::NewQuestion,
        parent_exchange_id: None,
        context_generation: 1,
        data_class: AssistDataClass::Standard,
        status: AssistExchangeStatus::Complete,
        question: "When can you send the revised timeline?".to_string(),
        answer: "I'll have the revised timeline to you by Friday.".to_string(),
        answer_word_count: None,
        answer_format_warnings: Vec::new(),
        detail: String::new(),
        detail_status: None,
        detail_truncated: false,
        detail_error: None,
        error: None,
        profile_id: None,
        profile_version_hash: None,
        playbook_id: None,
        identity_id: None,
        identity_version_hash: None,
        grounding_sources: Vec::new(),
        generation_id: 1,
        build_revision: env!("MEETILY_BUILD_REVISION").to_string(),
        created_at: Utc::now().to_rfc3339(),
        timings: AssistTimings::default(),
    }
}

#[test]
fn coaching_prefixes_are_position_sensitive_but_meta_language_is_not() {
    assert!(validate_speakable_response("Tell them the delay was vendor-side.").is_err());
    assert!(validate_speakable_response("I'd rather tell them directly.").is_ok());
    assert!(validate_speakable_response("I suggest we review this tomorrow.").is_ok());
    assert!(validate_speakable_response("This is the assistant's proposed response.").is_err());
}

#[test]
fn false_history_fixture_rejects_an_unspoken_commitment() {
    assert!(validate_false_history_fixture("I committed to Friday.").is_err());
    assert!(validate_false_history_fixture(
        "I haven't committed to a date yet. Let me confirm and come back to you."
    )
    .is_ok());
}

#[tokio::test]
#[ignore = "requires explicit provider credentials and network access on the reference PC"]
async fn reference_provider_preserves_voice_and_does_not_invent_commitment_history() {
    let config = AssistProviderConfig::from_environment()
        .expect("configure MEETING_ASSISTANT_LIVE_API_KEY before running the voice harness");
    let parent = parent_exchange();
    let question = "What did you commit to?";
    let messages =
        build_answer_messages(question, Some(&parent), "{}", "{}", AnswerContract::General);
    let fixture_hash = sha256(
        serde_json::to_vec(&json!({
            "parent_question": parent.question,
            "unspoken_parent_answer": parent.answer,
            "question": question,
        }))
        .unwrap(),
    );
    let prompt_template_hash = sha256(GENERAL_ANSWER_SYSTEM_PROMPT_TEMPLATE);
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(90))
        .build()
        .unwrap();
    let started = Instant::now();
    let mut first_token_ms = None;
    let mut output = String::new();
    let provider_result = stream_chat(
        &client,
        &config,
        &messages,
        180,
        CancellationToken::new(),
        |delta| {
            if first_token_ms.is_none() {
                first_token_ms = Some(started.elapsed().as_millis() as u64);
            }
            output.push_str(&delta);
        },
    )
    .await
    .and_then(provider::StreamCompletion::require_stop);

    let mut failures = Vec::new();
    if let Err(error) = &provider_result {
        failures.push(format!("provider_error: {error}"));
    } else {
        if let Err(error) = validate_speakable_response(&output) {
            failures.push(error.to_string());
        }
        if let Err(error) = validate_false_history_fixture(&output) {
            failures.push(error.to_string());
        }
    }
    let record = json!({
        "timestamp_utc": Utc::now().to_rfc3339(),
        "git_sha": git_sha(),
        "prompt_template_version": ANSWER_SYSTEM_PROMPT_VERSION,
        "prompt_template_hash": prompt_template_hash,
        "profile_version_hash": null,
        "fixture_hash": fixture_hash,
        "provider": provider_label(&config.endpoint),
        "endpoint": config.endpoint,
        "model": config.model,
        "parameters": { "max_tokens": 180, "temperature": 0.2, "attempts": 1 },
        "output": output.clone(),
        "first_token_ms": first_token_ms,
        "completion_ms": started.elapsed().as_millis() as u64,
        "passed": failures.is_empty(),
        "failure_reasons": failures.clone(),
        "provider_request_id": null,
    });
    let path = append_record(&record).expect("voice harness record should be writable");
    println!("Live Assist voice harness record: {}", path.display());
    assert!(
        failures.is_empty(),
        "Live Assist voice harness failed: {failures:?}\nOutput: {output}"
    );
}
