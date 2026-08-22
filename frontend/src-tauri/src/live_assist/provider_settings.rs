use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use tauri::{AppHandle, Manager, Runtime, State};
use uuid::Uuid;

use crate::state::AppState;

use super::provider::{test_connection, AssistProviderConfig};
use super::LiveAssistState;

const MAX_DISPLAY_NAME_BYTES: usize = 120;
const MAX_ENDPOINT_BYTES: usize = 2_048;
const MAX_MODEL_BYTES: usize = 240;
const MAX_API_KEY_BYTES: usize = 16 * 1_024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    Deepseek,
    Kimi,
    Openai,
    Custom,
}

impl ProviderKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Deepseek => "deepseek",
            Self::Kimi => "kimi",
            Self::Openai => "openai",
            Self::Custom => "custom",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "deepseek" => Ok(Self::Deepseek),
            "kimi" => Ok(Self::Kimi),
            "openai" => Ok(Self::Openai),
            "custom" => Ok(Self::Custom),
            _ => Err(anyhow!("unsupported provider kind")),
        }
    }
}

#[derive(Debug, Clone, FromRow)]
struct ProviderRecord {
    id: String,
    display_name: String,
    provider_kind: String,
    endpoint: String,
    model: String,
    credential_revision: i64,
    last_tested_config_hash: Option<String>,
    last_tested_at: Option<String>,
    is_active: i64,
    created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    id: String,
    display_name: String,
    provider_kind: ProviderKind,
    endpoint: String,
    model: String,
    is_active: bool,
    key_configured: bool,
    last_tested_at: Option<String>,
    test_current: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveProviderRequest {
    id: Option<String>,
    display_name: String,
    provider_kind: ProviderKind,
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActiveProviderDescriptor {
    pub id: String,
    pub display_name: String,
    pub endpoint: String,
    pub model: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ManagedProviderConfig {
    pub id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
    pub credential_revision: i64,
    pub configuration_hash: String,
    pub last_tested_at: String,
}

impl ActiveProviderDescriptor {
    fn from_record(record: &ProviderRecord) -> Self {
        Self {
            id: record.id.clone(),
            display_name: record.display_name.clone(),
            endpoint: record.endpoint.clone(),
            model: record.model.clone(),
        }
    }
}

pub(crate) async fn hydrate_runtime_provider(
    pool: &SqlitePool,
    state: &LiveAssistState,
) -> Result<()> {
    let provider_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM live_assist_providers")
        .fetch_one(pool)
        .await?;
    let active = get_active_record(pool)
        .await?
        .map(|record| ActiveProviderDescriptor::from_record(&record));
    state.set_managed_provider_state(provider_count > 0, active);
    Ok(())
}

pub(crate) async fn load_active_config(
    pool: &SqlitePool,
    descriptor: &ActiveProviderDescriptor,
) -> Result<AssistProviderConfig> {
    let record = get_record(pool, &descriptor.id)
        .await?
        .ok_or_else(|| anyhow!("the active Live Assist provider no longer exists"))?;
    if record.is_active != 1 {
        return Err(anyhow!("the selected Live Assist provider is not active"));
    }
    let api_key = credential_store::read(&record.id)?
        .ok_or_else(|| anyhow!("the active Live Assist provider has no saved API key"))?;
    Ok(AssistProviderConfig {
        endpoint: record.endpoint,
        api_key,
        model: record.model,
    })
}

pub(crate) async fn load_tested_config(
    pool: &SqlitePool,
    provider_id: &str,
) -> Result<ManagedProviderConfig> {
    let id = validate_id(provider_id)?;
    let record = get_record(pool, &id)
        .await?
        .ok_or_else(|| anyhow!("the selected evaluation provider no longer exists"))?;
    let api_key = credential_store::read(&id)?
        .ok_or_else(|| anyhow!("the selected evaluation provider has no saved API key"))?;
    let config_hash = configuration_hash(&record);
    if record.last_tested_config_hash.as_deref() != Some(&config_hash) {
        return Err(anyhow!(
            "Test Connection must succeed for the selected endpoint, model, and key before evaluation"
        ));
    }
    let last_tested_at = record.last_tested_at.clone().ok_or_else(|| {
        anyhow!("the selected evaluation provider has no successful connection test")
    })?;
    Ok(ManagedProviderConfig {
        id: record.id,
        display_name: record.display_name,
        provider_kind: record.provider_kind,
        endpoint: record.endpoint,
        model: record.model,
        api_key,
        credential_revision: record.credential_revision,
        configuration_hash: config_hash,
        last_tested_at,
    })
}

#[tauri::command]
pub async fn live_assist_provider_list(
    state: State<'_, AppState>,
) -> std::result::Result<Vec<ProviderSummary>, String> {
    summaries(state.db_manager.pool()).await.map_err(user_error)
}

#[tauri::command]
pub async fn live_assist_provider_save<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    request: SaveProviderRequest,
) -> std::result::Result<ProviderSummary, String> {
    let pool = state.db_manager.pool();
    let id = request
        .id
        .as_deref()
        .map(validate_id)
        .transpose()
        .map_err(user_error)?
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let display_name = bounded(
        "Provider name",
        &request.display_name,
        MAX_DISPLAY_NAME_BYTES,
    )
    .map_err(user_error)?;
    let endpoint = validate_endpoint(&request.endpoint).map_err(user_error)?;
    let model = bounded("Model", &request.model, MAX_MODEL_BYTES).map_err(user_error)?;
    let api_key = request
        .api_key
        .map(|value| bounded("API key", &value, MAX_API_KEY_BYTES))
        .transpose()
        .map_err(user_error)?;

    let existing = get_record(pool, &id).await.map_err(user_error)?;
    let key_changed = api_key.is_some();
    let configuration_changed = existing.as_ref().map_or(true, |record| {
        record.provider_kind != request.provider_kind.as_str()
            || record.endpoint != endpoint
            || record.model != model
    }) || key_changed;
    let now = Utc::now().to_rfc3339();
    let created_at = existing
        .as_ref()
        .map(|record| record.created_at.clone())
        .unwrap_or_else(|| now.clone());
    let credential_revision = existing
        .as_ref()
        .map(|record| record.credential_revision)
        .unwrap_or(0)
        + if key_changed { 1 } else { 0 };
    let old_secret = if key_changed {
        credential_store::read(&id).map_err(user_error)?
    } else {
        None
    };
    if let Some(secret) = api_key.as_deref() {
        credential_store::write(&id, secret).map_err(user_error)?;
    }

    let result = sqlx::query(
        r#"
        INSERT INTO live_assist_providers (
            id, display_name, provider_kind, endpoint, model, credential_revision,
            last_tested_config_hash, last_tested_at, is_active, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, 0, ?7, ?8)
        ON CONFLICT(id) DO UPDATE SET
            display_name = excluded.display_name,
            provider_kind = excluded.provider_kind,
            endpoint = excluded.endpoint,
            model = excluded.model,
            credential_revision = excluded.credential_revision,
            last_tested_config_hash = CASE WHEN ?9 THEN NULL ELSE live_assist_providers.last_tested_config_hash END,
            last_tested_at = CASE WHEN ?9 THEN NULL ELSE live_assist_providers.last_tested_at END,
            is_active = CASE WHEN ?9 THEN 0 ELSE live_assist_providers.is_active END,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(&id)
    .bind(&display_name)
    .bind(request.provider_kind.as_str())
    .bind(&endpoint)
    .bind(&model)
    .bind(credential_revision)
    .bind(created_at)
    .bind(&now)
    .bind(configuration_changed)
    .execute(pool)
    .await;

    if let Err(error) = result {
        if key_changed {
            match old_secret {
                Some(secret) => {
                    let _ = credential_store::write(&id, &secret);
                }
                None => {
                    let _ = credential_store::delete(&id);
                }
            }
        }
        return Err(user_error(error));
    }

    hydrate_runtime_provider(pool, &app.state::<LiveAssistState>())
        .await
        .map_err(user_error)?;
    summary_for_id(pool, &id).await.map_err(user_error)
}

#[tauri::command]
pub async fn live_assist_provider_test(
    state: State<'_, AppState>,
    provider_id: String,
) -> std::result::Result<ProviderSummary, String> {
    let id = validate_id(&provider_id).map_err(user_error)?;
    let pool = state.db_manager.pool();
    let record = get_record(pool, &id)
        .await
        .map_err(user_error)?
        .ok_or_else(|| "Live Assist provider was not found".to_string())?;
    let api_key = credential_store::read(&id)
        .map_err(user_error)?
        .ok_or_else(|| "Save an API key before testing this provider".to_string())?;
    let config = AssistProviderConfig {
        endpoint: record.endpoint.clone(),
        api_key,
        model: record.model.clone(),
    };
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(8))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(user_error)?;
    if let Err(error) = test_connection(&client, &config).await {
        sqlx::query(
            "UPDATE live_assist_providers SET last_tested_config_hash = NULL, last_tested_at = NULL WHERE id = ?1",
        )
        .bind(&id)
        .execute(pool)
        .await
        .map_err(user_error)?;
        return Err(user_error(error));
    }
    let now = Utc::now().to_rfc3339();
    let config_hash = configuration_hash(&record);
    sqlx::query(
        "UPDATE live_assist_providers SET last_tested_config_hash = ?1, last_tested_at = ?2, updated_at = ?2 WHERE id = ?3",
    )
    .bind(config_hash)
    .bind(now)
    .bind(&id)
    .execute(pool)
    .await
    .map_err(user_error)?;
    summary_for_id(pool, &id).await.map_err(user_error)
}

#[tauri::command]
pub async fn live_assist_provider_activate<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    provider_id: String,
) -> std::result::Result<ProviderSummary, String> {
    let id = validate_id(&provider_id).map_err(user_error)?;
    let pool = state.db_manager.pool();
    let record = get_record(pool, &id)
        .await
        .map_err(user_error)?
        .ok_or_else(|| "Live Assist provider was not found".to_string())?;
    if credential_store::read(&id).map_err(user_error)?.is_none() {
        return Err("Save an API key before activating this provider".to_string());
    }
    if record.last_tested_config_hash.as_deref() != Some(&configuration_hash(&record)) {
        return Err("Test Connection must succeed for the current endpoint, model, and key before activation".to_string());
    }
    let mut transaction = pool.begin().await.map_err(user_error)?;
    sqlx::query("UPDATE live_assist_providers SET is_active = 0 WHERE is_active = 1")
        .execute(&mut *transaction)
        .await
        .map_err(user_error)?;
    sqlx::query("UPDATE live_assist_providers SET is_active = 1, updated_at = ?1 WHERE id = ?2")
        .bind(Utc::now().to_rfc3339())
        .bind(&id)
        .execute(&mut *transaction)
        .await
        .map_err(user_error)?;
    transaction.commit().await.map_err(user_error)?;
    hydrate_runtime_provider(pool, &app.state::<LiveAssistState>())
        .await
        .map_err(user_error)?;
    summary_for_id(pool, &id).await.map_err(user_error)
}

#[tauri::command]
pub async fn live_assist_provider_delete<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    provider_id: String,
) -> std::result::Result<(), String> {
    let id = validate_id(&provider_id).map_err(user_error)?;
    let pool = state.db_manager.pool();
    let record = get_record(pool, &id)
        .await
        .map_err(user_error)?
        .ok_or_else(|| "Live Assist provider was not found".to_string())?;
    if record.is_active == 1 {
        return Err(
            "Activate another provider before removing the active Live Assist provider".to_string(),
        );
    }
    sqlx::query("DELETE FROM live_assist_providers WHERE id = ?1")
        .bind(&id)
        .execute(pool)
        .await
        .map_err(user_error)?;
    credential_store::delete(&id).map_err(user_error)?;
    hydrate_runtime_provider(pool, &app.state::<LiveAssistState>())
        .await
        .map_err(user_error)
}

async fn summaries(pool: &SqlitePool) -> Result<Vec<ProviderSummary>> {
    let records = sqlx::query_as::<_, ProviderRecord>(
        "SELECT * FROM live_assist_providers ORDER BY is_active DESC, display_name COLLATE NOCASE, id",
    )
    .fetch_all(pool)
    .await?;
    records.into_iter().map(summary).collect()
}

async fn summary_for_id(pool: &SqlitePool, id: &str) -> Result<ProviderSummary> {
    let record = get_record(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Live Assist provider was not found"))?;
    summary(record)
}

fn summary(record: ProviderRecord) -> Result<ProviderSummary> {
    let key_configured = credential_store::read(&record.id)?.is_some();
    let test_current = key_configured
        && record.last_tested_config_hash.as_deref() == Some(&configuration_hash(&record));
    Ok(ProviderSummary {
        id: record.id,
        display_name: record.display_name,
        provider_kind: ProviderKind::parse(&record.provider_kind)?,
        endpoint: record.endpoint,
        model: record.model,
        is_active: record.is_active == 1,
        key_configured,
        last_tested_at: record.last_tested_at,
        test_current,
    })
}

async fn get_record(pool: &SqlitePool, id: &str) -> Result<Option<ProviderRecord>> {
    sqlx::query_as::<_, ProviderRecord>("SELECT * FROM live_assist_providers WHERE id = ?1")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
}

async fn get_active_record(pool: &SqlitePool) -> Result<Option<ProviderRecord>> {
    sqlx::query_as::<_, ProviderRecord>(
        "SELECT * FROM live_assist_providers WHERE is_active = 1 LIMIT 1",
    )
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

fn configuration_hash(record: &ProviderRecord) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"meeting-assistant-live-provider-v1\0");
    hasher.update(record.provider_kind.as_bytes());
    hasher.update([0]);
    hasher.update(record.endpoint.as_bytes());
    hasher.update([0]);
    hasher.update(record.model.as_bytes());
    hasher.update([0]);
    hasher.update(record.credential_revision.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

fn validate_id(value: &str) -> Result<String> {
    Ok(Uuid::parse_str(value)
        .context("invalid provider id")?
        .to_string())
}

fn bounded(label: &str, value: &str, max_bytes: usize) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(anyhow!("{label} is required"));
    }
    if value.len() > max_bytes {
        return Err(anyhow!("{label} is too long"));
    }
    Ok(value.to_string())
}

fn validate_endpoint(value: &str) -> Result<String> {
    let endpoint = bounded("API endpoint", value, MAX_ENDPOINT_BYTES)?;
    let parsed = url::Url::parse(&endpoint).context("API endpoint is not a valid URL")?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow!("API endpoint must include a host"))?;
    let local_http = parsed.scheme() == "http"
        && matches!(
            host.to_ascii_lowercase().as_str(),
            "localhost" | "127.0.0.1" | "::1"
        );
    if parsed.scheme() != "https" && !local_http {
        return Err(anyhow!(
            "API endpoint must use HTTPS, except for a local loopback provider"
        ));
    }
    Ok(endpoint)
}

fn user_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

#[cfg(target_os = "windows")]
mod credential_store {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    use anyhow::{anyhow, Context, Result};
    use windows::core::{HRESULT, PCWSTR, PWSTR};
    use windows::Win32::Foundation::ERROR_NOT_FOUND;
    use windows::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    const TARGET_PREFIX: &str = "MeetingAssistant/LiveAssist/";

    pub fn write(provider_id: &str, secret: &str) -> Result<()> {
        let mut target = wide(&target(provider_id));
        let mut username = wide("Live Assist provider");
        let mut secret_bytes = secret.as_bytes().to_vec();
        let blob_size = u32::try_from(secret_bytes.len()).context("API key is too large")?;
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target.as_mut_ptr()),
            CredentialBlobSize: blob_size,
            CredentialBlob: secret_bytes.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR(username.as_mut_ptr()),
            ..Default::default()
        };
        unsafe { CredWriteW(&credential, 0) }
            .context("Windows Credential Manager could not save the API key")?;
        secret_bytes.fill(0);
        Ok(())
    }

    pub fn read(provider_id: &str) -> Result<Option<String>> {
        let target = wide(&target(provider_id));
        let mut raw: *mut CREDENTIALW = ptr::null_mut();
        let result =
            unsafe { CredReadW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None, &mut raw) };
        if let Err(error) = result {
            if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) {
                return Ok(None);
            }
            return Err(anyhow!(error))
                .context("Windows Credential Manager could not read the API key");
        }
        if raw.is_null() {
            return Err(anyhow!(
                "Windows Credential Manager returned an empty credential"
            ));
        }
        let credential = unsafe { &*raw };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
        };
        let value = String::from_utf8(bytes.to_vec()).context("saved API key is not valid UTF-8");
        unsafe { CredFree(raw.cast()) };
        value.map(Some)
    }

    pub fn delete(provider_id: &str) -> Result<()> {
        let target = wide(&target(provider_id));
        match unsafe { CredDeleteW(PCWSTR(target.as_ptr()), CRED_TYPE_GENERIC, None) } {
            Ok(()) => Ok(()),
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()),
            Err(error) => Err(anyhow!(error))
                .context("Windows Credential Manager could not delete the API key"),
        }
    }

    fn target(provider_id: &str) -> String {
        format!("{TARGET_PREFIX}{provider_id}")
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain(Some(0)).collect()
    }
}

#[cfg(not(target_os = "windows"))]
mod credential_store {
    use anyhow::{bail, Result};

    pub fn write(_provider_id: &str, _secret: &str) -> Result<()> {
        bail!("Secure Live Assist provider storage is currently available on Windows only")
    }

    pub fn read(_provider_id: &str) -> Result<Option<String>> {
        Ok(None)
    }

    pub fn delete(_provider_id: &str) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_plain_http_endpoints_are_rejected() {
        assert!(validate_endpoint("http://api.example.com/v1/chat/completions").is_err());
        assert!(validate_endpoint("http://localhost:11434/v1/chat/completions").is_ok());
        assert!(validate_endpoint("https://api.example.com/v1/chat/completions").is_ok());
    }

    #[test]
    fn configuration_hash_changes_without_hashing_secret_material() {
        let record = ProviderRecord {
            id: Uuid::nil().to_string(),
            display_name: "Example".to_string(),
            provider_kind: "custom".to_string(),
            endpoint: "https://api.example.com/v1/chat/completions".to_string(),
            model: "model-a".to_string(),
            credential_revision: 1,
            last_tested_config_hash: None,
            last_tested_at: None,
            is_active: 0,
            created_at: "2026-08-21T00:00:00Z".to_string(),
        };
        let first = configuration_hash(&record);
        let mut changed = record.clone();
        changed.credential_revision += 1;
        assert_ne!(first, configuration_hash(&changed));
    }
}
