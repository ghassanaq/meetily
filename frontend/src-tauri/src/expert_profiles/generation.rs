use reqwest::Client;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::summary::llm_client::LLMProvider;
use crate::summary::processor::generate_meeting_summary;

use super::models::ExpertProfileVersion;
use super::rendering::{build_profile_render_spec, ProfileRenderError};

pub struct ProfileGenerationRequest<'a> {
    pub client: &'a Client,
    pub provider: &'a LLMProvider,
    pub model_name: &'a str,
    pub api_key: &'a str,
    pub transcript: &'a str,
    pub additional_user_context: Option<&'a str>,
    pub profile: &'a ExpertProfileVersion,
    pub playbook_id: Uuid,
    pub token_threshold: usize,
    pub ollama_endpoint: Option<&'a str>,
    pub custom_openai_endpoint: Option<&'a str>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub app_data_dir: Option<&'a PathBuf>,
    pub cancellation_token: Option<&'a CancellationToken>,
    pub summary_language: Option<&'a str>,
    pub detected_transcript_language: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileGenerationResult {
    pub final_markdown: String,
    pub english_markdown: String,
    pub chunk_count: i64,
    pub playbook_id: Uuid,
}

#[derive(Debug, thiserror::Error)]
pub enum ProfileGenerationError {
    #[error(transparent)]
    Render(#[from] ProfileRenderError),
    #[error("profile summary generation failed: {0}")]
    Provider(String),
}

/// The single profile-aware production generation path.
///
/// Both ordinary profile summaries and evaluation call this function. The
/// caller chooses where the returned inert Markdown is persisted; this core
/// has no meeting-summary or eval-table write access.
pub async fn generate_profile_summary(
    request: ProfileGenerationRequest<'_>,
) -> Result<ProfileGenerationResult, ProfileGenerationError> {
    let render = build_profile_render_spec(request.profile, request.playbook_id)?;
    let user_context = match request.additional_user_context {
        Some(additional) if !additional.trim().is_empty() => format!(
            "{}\n\n<additional_user_context>\n{}\n</additional_user_context>",
            render.configuration_context,
            escape_delimiter_text(additional)
        ),
        _ => render.configuration_context,
    };

    let (final_markdown, english_markdown, chunk_count) = generate_meeting_summary(
        request.client,
        request.provider,
        request.model_name,
        request.api_key,
        request.transcript,
        &user_context,
        &render.template_id,
        &render.template,
        request.token_threshold,
        request.ollama_endpoint,
        request.custom_openai_endpoint,
        request.max_tokens,
        request.temperature,
        request.top_p,
        request.app_data_dir,
        request.cancellation_token,
        request.summary_language,
        request.detected_transcript_language,
        None,
    )
    .await
    .map_err(ProfileGenerationError::Provider)?;

    Ok(ProfileGenerationResult {
        final_markdown,
        english_markdown,
        chunk_count,
        playbook_id: render.playbook_id,
    })
}

fn escape_delimiter_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
