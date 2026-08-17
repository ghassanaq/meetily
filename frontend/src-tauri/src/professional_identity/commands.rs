use serde::Serialize;
use uuid::Uuid;

use crate::state::AppState;

use super::repository::{
    ProfessionalIdentityRepository, ProfessionalIdentityRepositoryError,
    ProfessionalIdentitySummary, StoredProfessionalIdentityVersion,
};
use super::{parse_identity_json, ProfessionalIdentityVersion};

#[derive(Debug, Serialize)]
pub struct IdentityCommandError {
    pub code: String,
    pub message: String,
}

impl From<ProfessionalIdentityRepositoryError> for IdentityCommandError {
    fn from(error: ProfessionalIdentityRepositoryError) -> Self {
        let code = match &error {
            ProfessionalIdentityRepositoryError::IdentityNotFound(_)
            | ProfessionalIdentityRepositoryError::VersionNotFound { .. } => "NOT_FOUND",
            ProfessionalIdentityRepositoryError::StoredContentIntegrity
            | ProfessionalIdentityRepositoryError::Hash(_) => "DIGEST_MISMATCH",
            ProfessionalIdentityRepositoryError::Validation(_) => "SCHEMA_MISMATCH",
            ProfessionalIdentityRepositoryError::Database(_) => "DATABASE_ERROR",
        };
        Self {
            code: code.to_string(),
            message: error.to_string(),
        }
    }
}

fn parse_error(error: anyhow::Error) -> IdentityCommandError {
    IdentityCommandError {
        code: "SCHEMA_MISMATCH".to_string(),
        message: error.to_string(),
    }
}

#[tauri::command]
pub async fn identity_create(
    state: tauri::State<'_, AppState>,
    identity_json: String,
) -> Result<StoredProfessionalIdentityVersion, IdentityCommandError> {
    let content = parse_identity_json(&identity_json).map_err(parse_error)?;
    Ok(
        ProfessionalIdentityRepository::create(state.db_manager.pool(), Uuid::new_v4(), &content)
            .await?,
    )
}

#[tauri::command]
pub async fn identity_create_version(
    state: tauri::State<'_, AppState>,
    identity_id: Uuid,
    identity_json: String,
) -> Result<StoredProfessionalIdentityVersion, IdentityCommandError> {
    let content = parse_identity_json(&identity_json).map_err(parse_error)?;
    Ok(ProfessionalIdentityRepository::create_version(
        state.db_manager.pool(),
        identity_id,
        &content,
    )
    .await?)
}

#[tauri::command]
pub async fn identity_list(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProfessionalIdentitySummary>, IdentityCommandError> {
    Ok(ProfessionalIdentityRepository::list(state.db_manager.pool()).await?)
}

#[tauri::command]
pub async fn identity_list_versions(
    state: tauri::State<'_, AppState>,
    identity_id: Uuid,
) -> Result<Vec<StoredProfessionalIdentityVersion>, IdentityCommandError> {
    Ok(ProfessionalIdentityRepository::list_versions(state.db_manager.pool(), identity_id).await?)
}

#[tauri::command]
pub async fn identity_get(
    state: tauri::State<'_, AppState>,
    identity_id: Uuid,
    version_hash: String,
) -> Result<ProfessionalIdentityVersion, IdentityCommandError> {
    ProfessionalIdentityRepository::get(state.db_manager.pool(), identity_id, &version_hash)
        .await?
        .ok_or_else(|| {
            ProfessionalIdentityRepositoryError::VersionNotFound {
                identity_id,
                version_hash,
            }
            .into()
        })
}

#[tauri::command]
pub async fn identity_retire(
    state: tauri::State<'_, AppState>,
    identity_id: Uuid,
) -> Result<(), IdentityCommandError> {
    Ok(ProfessionalIdentityRepository::retire(state.db_manager.pool(), identity_id).await?)
}

#[tauri::command]
pub async fn identity_restore(
    state: tauri::State<'_, AppState>,
    identity_id: Uuid,
) -> Result<(), IdentityCommandError> {
    Ok(ProfessionalIdentityRepository::restore(state.db_manager.pool(), identity_id).await?)
}

#[tauri::command]
pub async fn identity_delete(
    state: tauri::State<'_, AppState>,
    identity_id: Uuid,
) -> Result<(), IdentityCommandError> {
    Ok(ProfessionalIdentityRepository::delete(state.db_manager.pool(), identity_id).await?)
}
