use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistDataClass {
    Private,
    Standard,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistExchangeKind {
    NewQuestion,
    FollowUp,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AssistExchangeStatus {
    Capturing,
    Transcribing,
    TranscriptOnly,
    Requesting,
    Streaming,
    Complete,
    Interrupted,
    Failed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AssistTimings {
    pub capture_ms: Option<u64>,
    pub transcription_ms: Option<u64>,
    pub request_to_first_token_ms: Option<u64>,
    pub request_to_complete_ms: Option<u64>,
    pub stop_to_first_delta_ms: Option<u64>,
    pub first_delta_at_unix_ms: Option<u64>,
    pub first_delta_to_paint_ms: Option<u64>,
    pub stop_to_visible_text_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssistExchange {
    pub id: Uuid,
    pub ordinal: u32,
    pub kind: AssistExchangeKind,
    pub parent_exchange_id: Option<Uuid>,
    pub context_generation: u64,
    pub data_class: AssistDataClass,
    pub status: AssistExchangeStatus,
    pub question: String,
    pub answer: String,
    pub detail: String,
    pub detail_status: Option<AssistExchangeStatus>,
    pub detail_truncated: bool,
    pub detail_error: Option<String>,
    pub error: Option<String>,
    pub profile_id: Option<Uuid>,
    pub profile_version_hash: Option<String>,
    pub playbook_id: Option<Uuid>,
    pub generation_id: u64,
    pub build_revision: String,
    pub created_at: String,
    pub timings: AssistTimings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AssistProfileChoice {
    pub profile_id: Uuid,
    pub profile_version_hash: String,
    pub profile_name: String,
    pub playbooks: Vec<AssistPlaybookChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AssistPlaybookChoice {
    pub id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AssistSnapshot {
    pub armed: bool,
    pub receiving: bool,
    pub stalled: bool,
    pub level_rms: f32,
    pub cloud_enabled: bool,
    pub provider_configured: bool,
    pub provider_name: Option<String>,
    pub model_name: Option<String>,
    pub stream_error: Option<String>,
    pub selected_profile_id: Option<Uuid>,
    pub selected_profile_version_hash: Option<String>,
    pub selected_playbook_id: Option<Uuid>,
    pub current_exchange_id: Option<Uuid>,
    pub capturing: bool,
    pub context_generation: u64,
    pub stall_count: u32,
    pub exchanges: Vec<AssistExchange>,
    pub capture_shortcut: String,
    pub follow_up_shortcut: String,
}
