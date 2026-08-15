use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Manager, Runtime};
use uuid::Uuid;

use crate::app_paths::AppPaths;
use crate::database::repositories::expert_profile::{
    ExpertProfileRepositoryError, ExpertProfileSummary, ExpertProfilesRepository, StoredEvalPlan,
    StoredEvalRun, StoredProfileActivation, StoredProfileVersion,
};
use crate::database::repositories::meeting::MeetingsRepository;
use crate::database::repositories::setting::SettingsRepository;
use crate::database::repositories::summary::SummaryProcessesRepository;
use crate::state::AppState;
use crate::summary::llm_client::LLMProvider;

use super::bundle::{
    export_bundle, import_bundle, parse_bundle_json, BundleError, ImportIdentityMode,
};
use super::evaluation::{
    adjudicate_evaluation_report, run_evaluation, EvaluationReport, EvaluationRequest,
    ProductionProfileEvaluationBackend, SemanticAdjudication,
};
use super::generation::{
    generate_profile_summary, ProfileGenerationRequest, ProfileGenerationResult,
};
use super::hashing::{hash_serializable, prompt_renderer_hash};
use super::models::{EvalPlan, ExpertProfileVersion, GenerationParameters, ModelGenerationBinding};
use super::validation::{parse_eval_plan_json, parse_profile_json};
use super::OUTPUT_PARSER_VERSION;

static MODEL_ARTIFACT_HASH_CACHE: Lazy<Mutex<HashMap<PathBuf, (u64, u128, String)>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone, Serialize)]
pub struct ProfileCommandError {
    pub code: String,
    pub path: Option<String>,
    pub message: String,
}

impl ProfileCommandError {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            path: None,
            message: message.into(),
        }
    }
}

impl From<ExpertProfileRepositoryError> for ProfileCommandError {
    fn from(error: ExpertProfileRepositoryError) -> Self {
        let code = match &error {
            ExpertProfileRepositoryError::Validation(_) => "SCHEMA_MISMATCH",
            ExpertProfileRepositoryError::Hash(_)
            | ExpertProfileRepositoryError::StoredContentIntegrity { .. } => "DIGEST_MISMATCH",
            ExpertProfileRepositoryError::ProfileNotFound(_)
            | ExpertProfileRepositoryError::VersionNotFound { .. }
            | ExpertProfileRepositoryError::EvalPlanNotFound { .. }
            | ExpertProfileRepositoryError::EvalRunNotFound(_) => "NOT_FOUND",
            ExpertProfileRepositoryError::EvalRunNotQualifying { outcome, .. }
                if outcome == "inconclusive" =>
            {
                "EVAL_INCONCLUSIVE"
            }
            ExpertProfileRepositoryError::EvalRunNotQualifying { .. } => "EVAL_FAILED",
            ExpertProfileRepositoryError::ActivationInputChanged(_) => "ACTIVATION_INPUT_CHANGED",
            ExpertProfileRepositoryError::BindingSuperseded(_) => "BINDING_SUPERSEDED",
            ExpertProfileRepositoryError::ProfileActive => "PROFILE_ACTIVE",
            ExpertProfileRepositoryError::Database(_) => "DATABASE_ERROR",
        };
        Self::new(code, error.to_string())
    }
}

#[derive(Debug, Serialize)]
pub struct ProfileCreateResponse {
    pub profile_id: Uuid,
    pub plan_id: Uuid,
    pub profile_version: StoredProfileVersion,
    pub eval_plan: StoredEvalPlan,
}

#[derive(Debug, Serialize)]
pub struct ProfileEvalResponse {
    pub run: StoredEvalRun,
    pub report: EvaluationReport,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileEvalArgs {
    pub profile_id: Uuid,
    pub profile_version_hash: String,
    pub plan_id: Uuid,
    pub plan_hash: String,
    pub qualifying: bool,
    #[serde(default)]
    pub confirmed_removed_playbooks: Vec<Uuid>,
    #[serde(default)]
    pub adjudications: Vec<SemanticAdjudication>,
    #[serde(default)]
    pub cloud_consent: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileAdjudicationArgs {
    pub profile_id: Uuid,
    pub source_eval_run_id: i64,
    pub plan_id: Uuid,
    pub plan_hash: String,
    pub adjudications: Vec<SemanticAdjudication>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileSummaryArgs {
    pub meeting_id: String,
    pub transcript_text: String,
    pub profile_id: Uuid,
    pub playbook_id: Uuid,
    pub additional_user_context: Option<String>,
    pub summary_language: Option<String>,
    pub detected_transcript_language: Option<String>,
    #[serde(default)]
    pub cloud_consent: bool,
}

#[derive(Debug, Serialize)]
pub struct ProfileSummaryResponse {
    pub meeting_id: String,
    pub markdown: String,
    pub english_markdown: String,
    pub chunk_count: i64,
    pub provenance: ProfileSummaryProvenance,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileSummaryProvenance {
    pub profile_id: Uuid,
    pub profile_version_hash: String,
    pub playbook_id: Uuid,
    pub capability_revision_hash: String,
    pub model_binding_hash: String,
    pub prompt_renderer_hash: String,
    pub output_parser_version: u32,
}

#[derive(Debug, Serialize)]
pub struct ProfileActivationView {
    pub activation: StoredProfileActivation,
    pub binding: ModelGenerationBinding,
}

struct ResolvedProvider {
    provider: LLMProvider,
    model: String,
    api_key: String,
    ollama_endpoint: Option<String>,
    custom_openai_endpoint: Option<String>,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    top_p: Option<f32>,
    token_threshold: usize,
    app_data_dir: PathBuf,
    binding: ModelGenerationBinding,
}

#[tauri::command]
pub async fn profile_create(
    state: tauri::State<'_, AppState>,
    profile_json: String,
    eval_plan_json: String,
) -> Result<ProfileCreateResponse, ProfileCommandError> {
    let profile = parse_profile_json(&profile_json).map_err(validation_error)?;
    let plan = parse_eval_plan_json(&eval_plan_json).map_err(validation_error)?;
    plan.validate_for_profile(&profile)
        .map_err(validation_error)?;
    let profile_id = Uuid::new_v4();
    let plan_id = Uuid::new_v4();
    let (profile_version, eval_plan) = ExpertProfilesRepository::create_profile_with_plan(
        state.db_manager.pool(),
        profile_id,
        plan_id,
        &profile,
        &plan,
    )
    .await?;
    Ok(ProfileCreateResponse {
        profile_id,
        plan_id,
        profile_version,
        eval_plan,
    })
}

#[tauri::command]
pub async fn profile_create_version(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
    profile_json: String,
) -> Result<StoredProfileVersion, ProfileCommandError> {
    let profile = parse_profile_json(&profile_json).map_err(validation_error)?;
    Ok(ExpertProfilesRepository::create_profile_version(
        state.db_manager.pool(),
        profile_id,
        &profile,
    )
    .await?)
}

#[tauri::command]
pub async fn profile_store_eval_plan(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
    profile_version_hash: String,
    plan_id: Uuid,
    eval_plan_json: String,
) -> Result<StoredEvalPlan, ProfileCommandError> {
    let profile = load_profile(state.db_manager.pool(), profile_id, &profile_version_hash).await?;
    let plan = parse_eval_plan_json(&eval_plan_json).map_err(validation_error)?;
    Ok(ExpertProfilesRepository::store_eval_plan(
        state.db_manager.pool(),
        profile_id,
        plan_id,
        &profile,
        &plan,
    )
    .await?)
}

#[tauri::command]
pub async fn profile_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ExpertProfileSummary>, ProfileCommandError> {
    Ok(ExpertProfilesRepository::list_profiles(state.db_manager.pool()).await?)
}

#[tauri::command]
pub async fn profile_list_versions(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
) -> Result<Vec<StoredProfileVersion>, ProfileCommandError> {
    Ok(
        ExpertProfilesRepository::list_profile_versions(state.db_manager.pool(), profile_id)
            .await?,
    )
}

#[tauri::command]
pub async fn profile_list_eval_plans(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
) -> Result<Vec<StoredEvalPlan>, ProfileCommandError> {
    Ok(ExpertProfilesRepository::list_eval_plans(state.db_manager.pool(), profile_id).await?)
}

#[tauri::command]
pub async fn profile_get(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
    version_hash: String,
) -> Result<ExpertProfileVersion, ProfileCommandError> {
    load_profile(state.db_manager.pool(), profile_id, &version_hash).await
}

#[tauri::command]
pub async fn profile_get_eval_plan(
    state: tauri::State<'_, AppState>,
    plan_id: Uuid,
    plan_hash: String,
) -> Result<EvalPlan, ProfileCommandError> {
    ExpertProfilesRepository::get_eval_plan(state.db_manager.pool(), plan_id, &plan_hash)
        .await?
        .ok_or_else(|| ProfileCommandError::new("NOT_FOUND", "evaluation plan not found"))
}

#[tauri::command]
pub async fn profile_get_activation(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
) -> Result<Option<ProfileActivationView>, ProfileCommandError> {
    let Some(activation) =
        ExpertProfilesRepository::get_profile_activation(state.db_manager.pool(), profile_id)
            .await?
    else {
        return Ok(None);
    };
    let binding =
        ExpertProfilesRepository::get_activation_binding(state.db_manager.pool(), profile_id)
            .await?
            .ok_or_else(|| {
                ProfileCommandError::new("DIGEST_MISMATCH", "active profile has no model binding")
            })?;
    Ok(Some(ProfileActivationView {
        activation,
        binding,
    }))
}

#[tauri::command]
pub async fn profile_export(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
    version_hash: String,
    plan_id: Uuid,
    plan_hash: String,
) -> Result<String, ProfileCommandError> {
    let bundle = export_bundle(
        state.db_manager.pool(),
        profile_id,
        &version_hash,
        plan_id,
        &plan_hash,
    )
    .await
    .map_err(bundle_error)?;
    serde_json::to_string_pretty(&bundle)
        .map_err(|error| ProfileCommandError::new("EXPORT_FAILED", error.to_string()))
}

#[tauri::command]
pub async fn profile_import(
    state: tauri::State<'_, AppState>,
    bundle_json: String,
    identity_mode: ImportIdentityMode,
) -> Result<super::bundle::ImportResult, ProfileCommandError> {
    let bundle = parse_bundle_json(&bundle_json).map_err(bundle_error)?;
    import_bundle(state.db_manager.pool(), bundle, identity_mode)
        .await
        .map_err(bundle_error)
}

#[tauri::command]
pub async fn profile_run_evals<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    args: ProfileEvalArgs,
) -> Result<ProfileEvalResponse, ProfileCommandError> {
    let pool = state.db_manager.pool();
    let candidate = load_profile(pool, args.profile_id, &args.profile_version_hash).await?;
    let plan = ExpertProfilesRepository::get_eval_plan(pool, args.plan_id, &args.plan_hash)
        .await?
        .ok_or_else(|| ProfileCommandError::new("NOT_FOUND", "evaluation plan not found"))?;
    let active = ExpertProfilesRepository::get_profile_activation(pool, args.profile_id).await?;
    let baseline_hash = active
        .as_ref()
        .map(|activation| activation.profile_version_hash.clone());
    let baseline = match baseline_hash.as_deref() {
        Some(hash) if hash != args.profile_version_hash => {
            Some(load_profile(pool, args.profile_id, hash).await?)
        }
        Some(_) => Some(candidate.clone()),
        None => None,
    };
    let resolved = resolve_provider(&app, pool, args.cloud_consent).await?;
    let client = reqwest::Client::new();
    let base_request = generation_request(
        &client,
        &resolved,
        "",
        None,
        &candidate,
        candidate.playbooks[0].id,
        Some("en"),
        Some("en"),
    );
    let backend = ProductionProfileEvaluationBackend { base_request };
    let report = run_evaluation(
        &backend,
        EvaluationRequest {
            profile_id: args.profile_id,
            candidate_profile_version_hash: &args.profile_version_hash,
            candidate: &candidate,
            baseline_profile_version_hash: baseline_hash.as_deref(),
            baseline: baseline.as_ref(),
            plan: &plan,
            model_binding: &resolved.binding,
            qualifying: args.qualifying,
            confirmed_removed_playbooks: &args.confirmed_removed_playbooks,
            adjudications: &args.adjudications,
        },
    )
    .await
    .map_err(|error| ProfileCommandError::new("EVAL_FAILED", error.to_string()))?;
    let run =
        ExpertProfilesRepository::persist_evaluation_report(pool, args.profile_id, &report).await?;
    Ok(ProfileEvalResponse { run, report })
}

#[tauri::command]
pub async fn profile_adjudicate_eval(
    state: tauri::State<'_, AppState>,
    args: ProfileAdjudicationArgs,
) -> Result<ProfileEvalResponse, ProfileCommandError> {
    let pool = state.db_manager.pool();
    let source = ExpertProfilesRepository::get_evaluation_report(
        pool,
        args.profile_id,
        args.source_eval_run_id,
    )
    .await?;
    let plan = ExpertProfilesRepository::get_eval_plan(pool, args.plan_id, &args.plan_hash)
        .await?
        .ok_or_else(|| ProfileCommandError::new("NOT_FOUND", "evaluation plan not found"))?;
    let report = adjudicate_evaluation_report(&source, &plan, &args.adjudications)
        .map_err(|error| ProfileCommandError::new("EVAL_INCONCLUSIVE", error.to_string()))?;
    let run =
        ExpertProfilesRepository::persist_evaluation_report(pool, args.profile_id, &report).await?;
    Ok(ProfileEvalResponse { run, report })
}

#[tauri::command]
pub async fn profile_activate<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
    eval_run_id: i64,
    expected_previous_capability_hash: Option<String>,
    cloud_consent: bool,
) -> Result<StoredProfileActivation, ProfileCommandError> {
    let resolved = resolve_provider(&app, state.db_manager.pool(), cloud_consent).await?;
    Ok(ExpertProfilesRepository::activate_profile(
        state.db_manager.pool(),
        profile_id,
        eval_run_id,
        &resolved.binding,
        expected_previous_capability_hash.as_deref(),
    )
    .await?)
}

#[tauri::command]
pub async fn profile_retire(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
) -> Result<(), ProfileCommandError> {
    Ok(ExpertProfilesRepository::retire_profile(state.db_manager.pool(), profile_id).await?)
}

#[tauri::command]
pub async fn profile_restore(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
) -> Result<(), ProfileCommandError> {
    Ok(ExpertProfilesRepository::restore_profile(state.db_manager.pool(), profile_id).await?)
}

#[tauri::command]
pub async fn profile_delete(
    state: tauri::State<'_, AppState>,
    profile_id: Uuid,
) -> Result<(), ProfileCommandError> {
    Ok(ExpertProfilesRepository::delete_profile(state.db_manager.pool(), profile_id).await?)
}

#[tauri::command]
pub async fn summary_generate_with_profile<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, AppState>,
    args: ProfileSummaryArgs,
) -> Result<ProfileSummaryResponse, ProfileCommandError> {
    let pool = state.db_manager.pool();
    let resolved = resolve_provider(&app, pool, args.cloud_consent).await?;
    let activation = match ExpertProfilesRepository::require_active_binding(
        pool,
        args.profile_id,
        &resolved.binding,
    )
    .await
    {
        Ok(activation) => activation,
        Err(ExpertProfileRepositoryError::BindingSuperseded(reason)) => {
            ExpertProfilesRepository::mark_activation_superseded(pool, args.profile_id, &reason)
                .await?;
            return Err(ProfileCommandError::new("BINDING_SUPERSEDED", reason));
        }
        Err(error) => return Err(error.into()),
    };
    let profile = load_profile(pool, args.profile_id, &activation.profile_version_hash).await?;
    if !profile
        .playbooks
        .iter()
        .any(|playbook| playbook.id == args.playbook_id)
    {
        return Err(ProfileCommandError::new(
            "INVALID_PLAYBOOK",
            "selected playbook is not part of the active profile version",
        ));
    }

    let client = reqwest::Client::new();
    let start = Instant::now();
    let generated = generate_profile_summary(generation_request(
        &client,
        &resolved,
        &args.transcript_text,
        args.additional_user_context.as_deref(),
        &profile,
        args.playbook_id,
        args.summary_language.as_deref(),
        args.detected_transcript_language.as_deref(),
    ))
    .await
    .map_err(|error| ProfileCommandError::new("PROVIDER_UNAVAILABLE", error.to_string()))?;

    let provenance = ProfileSummaryProvenance {
        profile_id: args.profile_id,
        profile_version_hash: activation.profile_version_hash,
        playbook_id: args.playbook_id,
        capability_revision_hash: activation.capability_revision_hash,
        model_binding_hash: super::hashing::hash_model_binding(&resolved.binding)
            .map_err(|error| ProfileCommandError::new("DIGEST_MISMATCH", error.to_string()))?,
        prompt_renderer_hash: resolved.binding.prompt_renderer_hash.clone(),
        output_parser_version: resolved.binding.output_parser_version,
    };
    persist_profile_summary(
        pool,
        &args.meeting_id,
        &generated,
        &provenance,
        start.elapsed().as_secs_f64(),
    )
    .await?;

    Ok(ProfileSummaryResponse {
        meeting_id: args.meeting_id,
        markdown: generated.final_markdown,
        english_markdown: generated.english_markdown,
        chunk_count: generated.chunk_count,
        provenance,
    })
}

async fn load_profile(
    pool: &sqlx::SqlitePool,
    profile_id: Uuid,
    version_hash: &str,
) -> Result<ExpertProfileVersion, ProfileCommandError> {
    ExpertProfilesRepository::get_profile_version(pool, profile_id, version_hash)
        .await?
        .ok_or_else(|| ProfileCommandError::new("NOT_FOUND", "expert profile version not found"))
}

fn validation_error(errors: super::validation::ValidationErrors) -> ProfileCommandError {
    let first = errors.0.first();
    ProfileCommandError {
        code: first
            .map(|error| validation_code(error.code).to_string())
            .unwrap_or_else(|| "SCHEMA_MISMATCH".to_string()),
        path: first.map(|error| error.path.clone()),
        message: first
            .map(|error| error.message.clone())
            .unwrap_or_else(|| errors.to_string()),
    }
}

fn validation_code(code: super::validation::ValidationErrorCode) -> &'static str {
    use super::validation::ValidationErrorCode;
    match code {
        ValidationErrorCode::EmptyEvalPlan => "EMPTY_EVAL_PLAN",
        ValidationErrorCode::UnknownField => "UNKNOWN_FIELD",
        ValidationErrorCode::InvalidPlaybook => "INVALID_PLAYBOOK",
        ValidationErrorCode::SchemaMismatch => "SCHEMA_MISMATCH",
        ValidationErrorCode::LimitExceeded => "LIMIT_EXCEEDED",
        ValidationErrorCode::InvalidReference => "INVALID_REFERENCE",
        ValidationErrorCode::DuplicateValue => "DUPLICATE_VALUE",
        ValidationErrorCode::DigestMismatch => "DIGEST_MISMATCH",
    }
}

fn bundle_error(error: BundleError) -> ProfileCommandError {
    let code = match &error {
        BundleError::LimitExceeded | BundleError::DepthExceeded => "LIMIT_EXCEEDED",
        BundleError::UnsupportedFormat => "UNSUPPORTED_FORMAT_VERSION",
        BundleError::Schema(_) | BundleError::Validation(_) => "SCHEMA_MISMATCH",
        BundleError::DigestMismatch(_) | BundleError::Hash(_) => "DIGEST_MISMATCH",
        BundleError::IdentityConflict => "IDENTITY_CONFLICT",
        BundleError::Repository(_) => "DATABASE_ERROR",
    };
    ProfileCommandError::new(code, error.to_string())
}

async fn resolve_provider<R: Runtime>(
    app: &AppHandle<R>,
    pool: &sqlx::SqlitePool,
    cloud_consent: bool,
) -> Result<ResolvedProvider, ProfileCommandError> {
    let config = SettingsRepository::get_model_config(pool)
        .await
        .map_err(ExpertProfileRepositoryError::Database)?
        .ok_or_else(|| {
            ProfileCommandError::new("PROVIDER_UNAVAILABLE", "model is not configured")
        })?;
    let provider = LLMProvider::from_str(&config.provider)
        .map_err(|error| ProfileCommandError::new("PROVIDER_UNAVAILABLE", error))?;
    let custom = if provider == LLMProvider::CustomOpenAI {
        Some(
            SettingsRepository::get_custom_openai_config(pool)
                .await
                .map_err(ExpertProfileRepositoryError::Database)?
                .ok_or_else(|| {
                    ProfileCommandError::new(
                        "PROVIDER_UNAVAILABLE",
                        "custom OpenAI endpoint is not configured",
                    )
                })?,
        )
    } else {
        None
    };
    let model = custom
        .as_ref()
        .map(|config| config.model.clone())
        .unwrap_or(config.model);
    let api_key = if let Some(custom) = &custom {
        custom.api_key.clone().unwrap_or_default()
    } else {
        SettingsRepository::get_api_key(pool, &config.provider)
            .await
            .map_err(ExpertProfileRepositoryError::Database)?
            .unwrap_or_default()
    };
    if !matches!(
        provider,
        LLMProvider::Ollama | LLMProvider::BuiltInAI | LLMProvider::CustomOpenAI
    ) && api_key.is_empty()
    {
        return Err(ProfileCommandError::new(
            "PROVIDER_UNAVAILABLE",
            "API key is not configured for the selected provider",
        ));
    }

    let ollama_endpoint = if provider == LLMProvider::Ollama {
        config
            .ollama_endpoint
            .or_else(|| Some("http://localhost:11434".to_string()))
    } else {
        None
    };
    let custom_openai_endpoint = custom.as_ref().map(|config| config.endpoint.clone());
    let endpoint = custom_openai_endpoint
        .as_deref()
        .or(ollama_endpoint.as_deref())
        .or_else(|| fixed_provider_endpoint(&provider));
    let requires_cloud_consent = match provider {
        LLMProvider::BuiltInAI => false,
        LLMProvider::Ollama | LLMProvider::CustomOpenAI => {
            endpoint.is_some_and(|endpoint| !endpoint_is_local(endpoint))
        }
        _ => true,
    };
    if requires_cloud_consent && !cloud_consent {
        return Err(ProfileCommandError::new(
            "CLOUD_CONSENT_REQUIRED",
            "evaluation or generation would send transcript content to a remote provider",
        ));
    }

    let max_tokens = custom
        .as_ref()
        .and_then(|config| config.max_tokens)
        .and_then(|value| u32::try_from(value).ok());
    let temperature = custom.as_ref().and_then(|config| config.temperature);
    let top_p = custom.as_ref().and_then(|config| config.top_p);
    let token_threshold = provider_token_threshold(&provider, &model);
    let endpoint_fingerprint = endpoint
        .map(|endpoint| hash_serializable(b"meetily-provider-endpoint-v1\0", &endpoint))
        .transpose()
        .map_err(|error| ProfileCommandError::new("DIGEST_MISMATCH", error.to_string()))?;
    let app_data_dir = app.state::<AppPaths>().root().to_path_buf();
    let model_artifact_hash = if provider == LLMProvider::BuiltInAI {
        let path = crate::summary::summary_engine::models::get_model_path(&app_data_dir, &model)
            .map_err(|error| ProfileCommandError::new("PROVIDER_UNAVAILABLE", error.to_string()))?;
        Some(hash_model_artifact(path).await?)
    } else {
        None
    };
    let binding = ModelGenerationBinding {
        provider: config.provider,
        model: model.clone(),
        model_artifact_hash,
        endpoint_fingerprint,
        generation_parameters: GenerationParameters {
            temperature: f64::from(temperature.unwrap_or(0.0)),
            max_tokens: max_tokens.unwrap_or(2048),
        },
        prompt_renderer_hash: prompt_renderer_hash(),
        output_parser_version: OUTPUT_PARSER_VERSION,
    };
    Ok(ResolvedProvider {
        provider,
        model,
        api_key,
        ollama_endpoint,
        custom_openai_endpoint,
        max_tokens,
        temperature,
        top_p,
        token_threshold,
        app_data_dir,
        binding,
    })
}

async fn hash_model_artifact(path: PathBuf) -> Result<String, ProfileCommandError> {
    tokio::task::spawn_blocking(move || {
        let metadata = std::fs::metadata(&path)
            .map_err(|error| ProfileCommandError::new("PROVIDER_UNAVAILABLE", error.to_string()))?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let length = metadata.len();
        if let Some((_, _, digest)) = MODEL_ARTIFACT_HASH_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&path)
            .filter(|(cached_length, cached_modified, _)| {
                *cached_length == length && *cached_modified == modified
            })
            .cloned()
        {
            return Ok(digest);
        }

        let mut file = File::open(&path)
            .map_err(|error| ProfileCommandError::new("PROVIDER_UNAVAILABLE", error.to_string()))?;
        let mut hasher = Sha256::new();
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(|error| {
                ProfileCommandError::new("PROVIDER_UNAVAILABLE", error.to_string())
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        let digest = format!("sha256:{:x}", hasher.finalize());
        MODEL_ARTIFACT_HASH_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(path, (length, modified, digest.clone()));
        Ok(digest)
    })
    .await
    .map_err(|error| ProfileCommandError::new("PROVIDER_UNAVAILABLE", error.to_string()))?
}

#[allow(clippy::too_many_arguments)]
fn generation_request<'a>(
    client: &'a reqwest::Client,
    provider: &'a ResolvedProvider,
    transcript: &'a str,
    additional_user_context: Option<&'a str>,
    profile: &'a ExpertProfileVersion,
    playbook_id: Uuid,
    summary_language: Option<&'a str>,
    detected_transcript_language: Option<&'a str>,
) -> ProfileGenerationRequest<'a> {
    ProfileGenerationRequest {
        client,
        provider: &provider.provider,
        model_name: &provider.model,
        api_key: &provider.api_key,
        transcript,
        additional_user_context,
        profile,
        playbook_id,
        token_threshold: provider.token_threshold,
        ollama_endpoint: provider.ollama_endpoint.as_deref(),
        custom_openai_endpoint: provider.custom_openai_endpoint.as_deref(),
        max_tokens: provider.max_tokens,
        temperature: provider.temperature,
        top_p: provider.top_p,
        app_data_dir: Some(&provider.app_data_dir),
        cancellation_token: None,
        summary_language,
        detected_transcript_language,
    }
}

async fn persist_profile_summary(
    pool: &sqlx::SqlitePool,
    meeting_id: &str,
    generated: &ProfileGenerationResult,
    provenance: &ProfileSummaryProvenance,
    duration: f64,
) -> Result<(), ProfileCommandError> {
    SummaryProcessesRepository::create_or_reset_process(pool, meeting_id)
        .await
        .map_err(ExpertProfileRepositoryError::Database)?;
    if let Some(title) =
        crate::summary::processor::extract_meeting_name_from_markdown(&generated.final_markdown)
    {
        MeetingsRepository::update_meeting_name(pool, meeting_id, &title)
            .await
            .map_err(ExpertProfileRepositoryError::Database)?;
    }
    let payload = serde_json::json!({
        "markdown": strip_title(&generated.final_markdown),
        "english_markdown": generated.english_markdown,
        "profile_provenance": provenance,
    });
    SummaryProcessesRepository::update_process_completed(
        pool,
        meeting_id,
        payload,
        generated.chunk_count,
        duration,
    )
    .await
    .map_err(ExpertProfileRepositoryError::Database)?;
    Ok(())
}

fn strip_title(markdown: &str) -> String {
    let mut lines = markdown.lines();
    if lines
        .next()
        .is_some_and(|line| line.trim_start().starts_with("# "))
    {
        lines
            .skip_while(|line| line.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        markdown.to_string()
    }
}

fn endpoint_is_local(endpoint: &str) -> bool {
    let lowercase = endpoint.to_ascii_lowercase();
    lowercase.contains("://localhost")
        || lowercase.contains("://127.0.0.1")
        || lowercase.contains("://[::1]")
}

fn fixed_provider_endpoint(provider: &LLMProvider) -> Option<&'static str> {
    match provider {
        LLMProvider::OpenAI => Some("https://api.openai.com/v1/chat/completions"),
        LLMProvider::Claude => Some("https://api.anthropic.com/v1/messages"),
        LLMProvider::Groq => Some("https://api.groq.com/openai/v1/chat/completions"),
        LLMProvider::OpenRouter => Some("https://openrouter.ai/api/v1/chat/completions"),
        _ => None,
    }
}

fn provider_token_threshold(provider: &LLMProvider, model: &str) -> usize {
    match provider {
        LLMProvider::BuiltInAI => crate::summary::summary_engine::models::get_model_by_name(model)
            .map(|model| model.context_size.saturating_sub(300) as usize)
            .unwrap_or(1748),
        LLMProvider::Ollama => 4000,
        _ => 100_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_custom_endpoints_bypass_explicit_cloud_consent() {
        assert!(endpoint_is_local("http://localhost:8000/v1"));
        assert!(endpoint_is_local("http://127.0.0.1:11434"));
        assert!(!endpoint_is_local("https://models.example.com/v1"));
        assert!(!endpoint_is_local("http://192.168.1.20:8000/v1"));
    }

    #[test]
    fn stored_profile_summary_body_omits_only_the_generated_title() {
        assert_eq!(
            strip_title("# Meeting title\n\n**Summary**\n\nBody"),
            "**Summary**\n\nBody"
        );
        assert_eq!(strip_title("**Summary**\n\nBody"), "**Summary**\n\nBody");
    }

    #[tokio::test]
    async fn local_model_binding_hash_tracks_artifact_content() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("model.gguf");
        std::fs::write(&path, b"model-a").unwrap();
        let first = hash_model_artifact(path.clone()).await.unwrap();
        std::fs::write(&path, b"model-b-with-different-size").unwrap();
        let second = hash_model_artifact(path).await.unwrap();

        assert_ne!(first, second);
        assert!(first.starts_with("sha256:"));
    }
}
