use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub const DEFAULT_ENDPOINT: &str = "https://api.deepseek.com/chat/completions";
pub const DEFAULT_MODEL: &str = "deepseek-v4-pro";
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const KIMI_K3_REASONING_TOKEN_ALLOWANCE: u32 = 1_024;

#[derive(Debug, Clone)]
pub struct AssistProviderConfig {
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
}

impl AssistProviderConfig {
    pub fn from_environment() -> Result<Self> {
        let api_key = std::env::var("MEETING_ASSISTANT_LIVE_API_KEY")
            .map_err(|_| anyhow!("MEETING_ASSISTANT_LIVE_API_KEY is not configured"))?;
        let model = std::env::var("MEETING_ASSISTANT_LIVE_MODEL")
            .unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        let endpoint = std::env::var("MEETING_ASSISTANT_LIVE_ENDPOINT")
            .unwrap_or_else(|_| DEFAULT_ENDPOINT.to_string());
        let parsed = url::Url::parse(&endpoint).context("invalid Live Assist endpoint")?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(anyhow!("Live Assist endpoint must use http or https"));
        }
        if api_key.trim().is_empty() || model.trim().is_empty() {
            return Err(anyhow!("Live Assist API key and model must not be empty"));
        }
        Ok(Self {
            endpoint,
            api_key,
            model,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AssistMessage {
    pub role: &'static str,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamCompletion {
    Stop,
    Length,
}

impl StreamCompletion {
    pub fn require_stop(self) -> Result<()> {
        match self {
            Self::Stop => Ok(()),
            Self::Length => Err(anyhow!(
                "Live Assist provider stopped because the response reached its token limit"
            )),
        }
    }
}

pub async fn stream_chat(
    client: &Client,
    config: &AssistProviderConfig,
    messages: &[AssistMessage],
    max_tokens: u32,
    cancellation: CancellationToken,
    mut on_delta: impl FnMut(String) + Send,
) -> Result<StreamCompletion> {
    let body = request_body(config, messages, max_tokens);
    let request = client
        .post(&config.endpoint)
        .bearer_auth(&config.api_key)
        .json(&body)
        .send();
    let response = tokio::select! {
        _ = cancellation.cancelled() => {
            return Err(anyhow!("Live Assist generation was interrupted"));
        }
        response = request => response.context("failed to contact Live Assist provider")?,
    };
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow!(
            "Live Assist provider returned {status}: {}",
            sanitize_provider_error(&body, &config.api_key)
        ));
    }

    let mut stream = response.bytes_stream();
    let mut parser = SseParser::default();
    let mut finish_reason = None;
    loop {
        let next = tokio::select! {
            _ = cancellation.cancelled() => return Err(anyhow!("Live Assist generation was interrupted")),
            next = tokio::time::timeout(STREAM_IDLE_TIMEOUT, stream.next()) => {
                next.map_err(|_| anyhow!("Live Assist provider stream timed out"))?
            },
        };
        let Some(chunk) = next else {
            let pending = parser.finish()?;
            if pending.len() == 1 && pending[0] == "[DONE]" {
                return classify_completion(finish_reason.as_deref());
            }
            return Err(anyhow!("Live Assist provider stream ended without [DONE]"));
        };
        let chunk = chunk.context("Live Assist stream failed")?;
        for payload in parser.push(&chunk)? {
            if payload == "[DONE]" {
                return classify_completion(finish_reason.as_deref());
            }
            let value: Value = serde_json::from_str(&payload)
                .context("Live Assist provider emitted malformed JSON")?;
            if value.get("error").is_some() {
                return Err(anyhow!(
                    "Live Assist provider stream error: {}",
                    value["error"]
                ));
            }
            if value.pointer("/choices/0/delta/tool_calls").is_some()
                || value.pointer("/choices/0/message/tool_calls").is_some()
            {
                return Err(anyhow!("Live Assist does not permit provider tool calls"));
            }
            if let Some(delta) = value
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
            {
                if !delta.is_empty() {
                    on_delta(delta.to_string());
                }
            }
            if let Some(reason) = value
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
            {
                finish_reason = Some(reason.to_string());
            }
        }
    }
}

pub async fn test_connection(client: &Client, config: &AssistProviderConfig) -> Result<()> {
    let messages = [AssistMessage {
        role: "user",
        content: "Reply with OK.".to_string(),
    }];
    let mut body = request_body(config, &messages, 8);
    body["stream"] = json!(false);
    let response = client
        .post(&config.endpoint)
        .bearer_auth(&config.api_key)
        .json(&body)
        .send()
        .await
        .context("failed to contact Live Assist provider")?;
    let status = response.status();
    let response_body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow!(
            "Provider connection test returned {status}: {}",
            sanitize_provider_error(&response_body, &config.api_key)
        ));
    }
    let response_json: Value = serde_json::from_str(&response_body)
        .context("provider connection test returned invalid JSON")?;
    if response_json
        .pointer("/choices/0/message/tool_calls")
        .is_some()
    {
        return Err(anyhow!("provider connection test returned a tool call"));
    }
    if response_json
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .is_none()
    {
        return Err(anyhow!(
            "provider connection test did not return an OpenAI-compatible message"
        ));
    }
    Ok(())
}

fn request_body(
    config: &AssistProviderConfig,
    messages: &[AssistMessage],
    max_tokens: u32,
) -> Value {
    let mut body = json!({
        "model": config.model,
        "stream": true,
        "messages": messages.iter().map(|message| json!({
            "role": message.role,
            "content": message.content,
        })).collect::<Vec<_>>(),
    });
    if is_openai_endpoint(&config.endpoint) || is_kimi_endpoint(&config.endpoint) {
        let is_kimi_k3 =
            is_kimi_endpoint(&config.endpoint) && config.model.eq_ignore_ascii_case("kimi-k3");
        let completion_budget = if is_kimi_k3 {
            max_tokens.saturating_add(KIMI_K3_REASONING_TOKEN_ALLOWANCE)
        } else {
            max_tokens
        };
        body["max_completion_tokens"] = json!(completion_budget);
        if is_kimi_k3 {
            body["reasoning_effort"] = json!("low");
        }
    } else {
        body["temperature"] = json!(0.2);
        body["max_tokens"] = json!(max_tokens);
        if is_deepseek_endpoint(&config.endpoint) {
            body["thinking"] = json!({ "type": "disabled" });
        }
    }
    body
}

fn classify_completion(finish_reason: Option<&str>) -> Result<StreamCompletion> {
    match finish_reason {
        Some("stop") => Ok(StreamCompletion::Stop),
        Some("length") => Ok(StreamCompletion::Length),
        Some(reason) => Err(anyhow!(
            "Live Assist provider stopped with unexpected reason: {reason}"
        )),
        None => Err(anyhow!(
            "Live Assist provider completed without a finish reason"
        )),
    }
}

#[derive(Default)]
struct SseParser {
    pending: String,
}

impl SseParser {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        self.pending.push_str(
            std::str::from_utf8(bytes).context("Live Assist provider emitted non-UTF-8 data")?,
        );
        let mut events = Vec::new();
        while let Some(index) = self.pending.find('\n') {
            let mut line = self.pending.drain(..=index).collect::<String>();
            line.truncate(line.trim_end_matches(['\r', '\n']).len());
            if let Some(payload) = line.strip_prefix("data:") {
                events.push(payload.trim_start().to_string());
            }
        }
        Ok(events)
    }

    fn finish(mut self) -> Result<Vec<String>> {
        if self.pending.trim().is_empty() {
            return Ok(Vec::new());
        }
        self.pending.push('\n');
        self.push(&[])
    }
}

fn truncate(value: &str, characters: usize) -> String {
    value.chars().take(characters).collect()
}

fn sanitize_provider_error(value: &str, api_key: &str) -> String {
    let redacted = if api_key.is_empty() {
        value.to_string()
    } else {
        value.replace(api_key, "[REDACTED]")
    };
    truncate(&redacted, 240)
}

fn is_openai_endpoint(endpoint: &str) -> bool {
    url::Url::parse(endpoint)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.eq_ignore_ascii_case("api.openai.com"))
        })
        .unwrap_or(false)
}

fn is_deepseek_endpoint(endpoint: &str) -> bool {
    url::Url::parse(endpoint)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.eq_ignore_ascii_case("api.deepseek.com"))
        })
        .unwrap_or(false)
}

fn is_kimi_endpoint(endpoint: &str) -> bool {
    url::Url::parse(endpoint)
        .ok()
        .and_then(|url| {
            url.host_str()
                .map(|host| host.eq_ignore_ascii_case("api.moonshot.ai"))
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parser_handles_split_chunks_without_silently_dropping_data() {
        let mut parser = SseParser::default();
        assert!(parser.push(b"data: {\"cho").unwrap().is_empty());
        assert_eq!(
            parser.push(b"ices\":[]}\n\ndata: [DONE]\n").unwrap(),
            ["{\"choices\":[]}", "[DONE]"]
        );
    }

    #[test]
    fn incomplete_terminal_line_is_still_parsed() {
        let mut parser = SseParser::default();
        assert!(parser.push(b"data: [DONE]").unwrap().is_empty());
        assert_eq!(parser.finish().unwrap(), ["[DONE]"]);
    }

    #[test]
    fn openai_endpoint_uses_current_completion_limit_shape() {
        assert!(is_openai_endpoint(
            "https://api.openai.com/v1/chat/completions"
        ));
        assert!(!is_openai_endpoint(DEFAULT_ENDPOINT));
        assert!(!is_openai_endpoint(
            "http://localhost:11434/v1/chat/completions"
        ));
    }

    #[test]
    fn kimi_k3_uses_low_reasoning_without_unsupported_sampling_parameters() {
        let config = AssistProviderConfig {
            endpoint: "https://api.moonshot.ai/v1/chat/completions".to_string(),
            api_key: "test-key".to_string(),
            model: "kimi-k3".to_string(),
        };
        let messages = [AssistMessage {
            role: "user",
            content: "How do you approach budgeting?".to_string(),
        }];

        let body = request_body(&config, &messages, 520);

        assert_eq!(body["max_completion_tokens"], 1_544);
        assert_eq!(body["reasoning_effort"], "low");
        assert!(body.get("max_tokens").is_none());
        assert!(body.get("temperature").is_none());
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn deepseek_defaults_use_v4_pro_and_non_thinking_chat_completions() {
        assert_eq!(DEFAULT_MODEL, "deepseek-v4-pro");
        assert!(is_deepseek_endpoint(DEFAULT_ENDPOINT));
        assert!(!is_deepseek_endpoint(
            "https://api.openai.com/v1/chat/completions"
        ));
    }

    #[test]
    fn completion_reasons_are_closed_and_length_requires_explicit_handling() {
        let stopped = classify_completion(Some("stop")).unwrap();
        assert_eq!(stopped, StreamCompletion::Stop);
        assert!(stopped.require_stop().is_ok());

        let limited = classify_completion(Some("length")).unwrap();
        assert_eq!(limited, StreamCompletion::Length);
        assert!(limited.require_stop().is_err());

        assert!(classify_completion(Some("content_filter")).is_err());
        assert!(classify_completion(None).is_err());
    }

    #[test]
    fn provider_errors_never_echo_the_configured_key() {
        let key = "sensitive-test-credential";
        let sanitized = sanitize_provider_error(
            "Authentication failed for sensitive-test-credential",
            key,
        );
        assert!(!sanitized.contains(key));
        assert!(sanitized.contains("[REDACTED]"));
    }
}
