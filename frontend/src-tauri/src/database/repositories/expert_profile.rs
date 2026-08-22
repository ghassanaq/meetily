use chrono::Utc;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use thiserror::Error;
use uuid::Uuid;

use crate::expert_profiles::hashing::{
    canonical_json, hash_eval_plan, hash_model_binding, hash_profile_version, prompt_renderer_hash,
    HashError,
};
use crate::expert_profiles::{
    EvalPlan, EvaluationReport, ExpertProfileVersion, ModelGenerationBinding, Validate,
    ValidationErrors, OUTPUT_PARSER_VERSION,
};

pub struct ExpertProfilesRepository;

#[derive(Debug, Error)]
pub enum ExpertProfileRepositoryError {
    #[error(transparent)]
    Validation(#[from] ValidationErrors),
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("expert profile {0} was not found")]
    ProfileNotFound(Uuid),
    #[error("expert profile version {version_hash} was not found for {profile_id}")]
    VersionNotFound {
        profile_id: Uuid,
        version_hash: String,
    },
    #[error("expert evaluation plan {plan_id} with hash {content_hash} was not found")]
    EvalPlanNotFound { plan_id: Uuid, content_hash: String },
    #[error("stored {kind} content does not match its recorded digest")]
    StoredContentIntegrity { kind: &'static str },
    #[error("evaluation run {0} was not found")]
    EvalRunNotFound(i64),
    #[error("evaluation run {eval_run_id} has non-qualifying outcome {outcome}")]
    EvalRunNotQualifying { eval_run_id: i64, outcome: String },
    #[error("ACTIVATION_INPUT_CHANGED: {0}")]
    ActivationInputChanged(String),
    #[error("BINDING_SUPERSEDED: {0}")]
    BindingSuperseded(String),
    #[error("PROFILE_ACTIVE: deactivate or retire the profile before deletion")]
    ProfileActive,
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct ExpertProfileSummary {
    pub id: String,
    pub name: String,
    pub retired_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct StoredProfileVersion {
    pub profile_id: String,
    pub version_hash: String,
    pub seq: i64,
    pub schema_version: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct StoredEvalPlan {
    pub id: String,
    pub profile_id: String,
    pub content_hash: String,
    pub schema_version: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct StoredEvalRun {
    pub id: i64,
    pub profile_id: String,
    pub candidate_capability_hash: String,
    pub baseline_capability_hash: Option<String>,
    pub eval_plan_hash: String,
    pub safety_gate_version: String,
    pub model_binding_hash: String,
    pub outcome: String,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct StoredProfileActivation {
    pub profile_id: String,
    pub profile_version_hash: String,
    pub capability_revision_hash: String,
    pub eval_run_id: i64,
    pub status: String,
    pub superseded_reason: Option<String>,
    pub activated_at: String,
}

impl ExpertProfilesRepository {
    pub async fn create_profile_with_plan(
        pool: &SqlitePool,
        profile_id: Uuid,
        plan_id: Uuid,
        profile: &ExpertProfileVersion,
        plan: &EvalPlan,
    ) -> Result<(StoredProfileVersion, StoredEvalPlan), ExpertProfileRepositoryError> {
        profile.validate()?;
        plan.validate_for_profile(profile)?;

        let version_hash = hash_profile_version(profile)?;
        let plan_hash = hash_eval_plan(plan)?;
        let profile_payload = canonical_json(profile)?;
        let plan_payload = canonical_json(plan)?;
        let profile_id = profile_id.to_string();
        let plan_id = plan_id.to_string();
        let now = Utc::now().to_rfc3339();

        let mut transaction = pool.begin().await?;
        sqlx::query(
            r#"
            INSERT INTO expert_profiles (id, name, retired_at, created_at, updated_at)
            VALUES (?, ?, NULL, ?, ?)
            "#,
        )
        .bind(&profile_id)
        .bind(&profile.identity.name)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO expert_profile_versions
                (profile_id, version_hash, seq, content_payload, schema_version, created_at)
            VALUES (?, ?, 1, ?, ?, ?)
            "#,
        )
        .bind(&profile_id)
        .bind(&version_hash)
        .bind(profile_payload)
        .bind(i64::from(profile.schema_version))
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO expert_eval_plans
                (id, profile_id, content_hash, content_payload, schema_version, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&plan_id)
        .bind(&profile_id)
        .bind(&plan_hash)
        .bind(plan_payload)
        .bind(i64::from(plan.schema_version))
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        Ok((
            StoredProfileVersion {
                profile_id: profile_id.clone(),
                version_hash,
                seq: 1,
                schema_version: i64::from(profile.schema_version),
                created_at: now.clone(),
            },
            StoredEvalPlan {
                id: plan_id,
                profile_id,
                content_hash: plan_hash,
                schema_version: i64::from(plan.schema_version),
                created_at: now,
            },
        ))
    }

    pub async fn create_profile_version(
        pool: &SqlitePool,
        profile_id: Uuid,
        profile: &ExpertProfileVersion,
    ) -> Result<StoredProfileVersion, ExpertProfileRepositoryError> {
        profile.validate()?;

        let profile_id_text = profile_id.to_string();
        let version_hash = hash_profile_version(profile)?;
        let content_payload = canonical_json(profile)?;
        let mut transaction = pool.begin().await?;

        let profile_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM expert_profiles WHERE id = ?)")
                .bind(&profile_id_text)
                .fetch_one(&mut *transaction)
                .await?;
        if !profile_exists {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::ProfileNotFound(profile_id));
        }

        if let Some(existing) = sqlx::query_as::<_, StoredProfileVersion>(
            r#"
            SELECT profile_id, version_hash, seq, schema_version, created_at
            FROM expert_profile_versions
            WHERE profile_id = ? AND version_hash = ?
            "#,
        )
        .bind(&profile_id_text)
        .bind(&version_hash)
        .fetch_optional(&mut *transaction)
        .await?
        {
            transaction.rollback().await?;
            return Ok(existing);
        }

        let next_sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM expert_profile_versions WHERE profile_id = ?",
        )
        .bind(&profile_id_text)
        .fetch_one(&mut *transaction)
        .await?;
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            r#"
            INSERT INTO expert_profile_versions
                (profile_id, version_hash, seq, content_payload, schema_version, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&profile_id_text)
        .bind(&version_hash)
        .bind(next_sequence)
        .bind(content_payload)
        .bind(i64::from(profile.schema_version))
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        sqlx::query("UPDATE expert_profiles SET name = ?, updated_at = ? WHERE id = ?")
            .bind(&profile.identity.name)
            .bind(&now)
            .bind(&profile_id_text)
            .execute(&mut *transaction)
            .await?;

        transaction.commit().await?;

        Ok(StoredProfileVersion {
            profile_id: profile_id_text,
            version_hash,
            seq: next_sequence,
            schema_version: i64::from(profile.schema_version),
            created_at: now,
        })
    }

    pub async fn store_eval_plan(
        pool: &SqlitePool,
        profile_id: Uuid,
        plan_id: Uuid,
        profile: &ExpertProfileVersion,
        plan: &EvalPlan,
    ) -> Result<StoredEvalPlan, ExpertProfileRepositoryError> {
        profile.validate()?;
        plan.validate_for_profile(profile)?;

        let profile_id_text = profile_id.to_string();
        let plan_id_text = plan_id.to_string();
        let content_hash = hash_eval_plan(plan)?;
        let content_payload = canonical_json(plan)?;
        let now = Utc::now().to_rfc3339();

        let profile_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM expert_profiles WHERE id = ?)")
                .bind(&profile_id_text)
                .fetch_one(pool)
                .await?;
        if !profile_exists {
            return Err(ExpertProfileRepositoryError::ProfileNotFound(profile_id));
        }

        sqlx::query(
            r#"
            INSERT OR IGNORE INTO expert_eval_plans
                (id, profile_id, content_hash, content_payload, schema_version, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&plan_id_text)
        .bind(&profile_id_text)
        .bind(&content_hash)
        .bind(content_payload)
        .bind(i64::from(plan.schema_version))
        .bind(&now)
        .execute(pool)
        .await?;

        Self::get_eval_plan_record(pool, plan_id, &content_hash)
            .await?
            .ok_or_else(|| ExpertProfileRepositoryError::EvalPlanNotFound {
                plan_id,
                content_hash,
            })
    }

    pub async fn list_profiles(
        pool: &SqlitePool,
    ) -> Result<Vec<ExpertProfileSummary>, ExpertProfileRepositoryError> {
        Ok(sqlx::query_as::<_, ExpertProfileSummary>(
            r#"
            SELECT id, name, retired_at, created_at, updated_at
            FROM expert_profiles
            ORDER BY updated_at DESC
            "#,
        )
        .fetch_all(pool)
        .await?)
    }

    pub async fn list_profile_versions(
        pool: &SqlitePool,
        profile_id: Uuid,
    ) -> Result<Vec<StoredProfileVersion>, ExpertProfileRepositoryError> {
        Ok(sqlx::query_as::<_, StoredProfileVersion>(
            r#"
            SELECT profile_id, version_hash, seq, schema_version, created_at
            FROM expert_profile_versions
            WHERE profile_id = ?
            ORDER BY seq DESC
            "#,
        )
        .bind(profile_id.to_string())
        .fetch_all(pool)
        .await?)
    }

    pub async fn list_eval_plans(
        pool: &SqlitePool,
        profile_id: Uuid,
    ) -> Result<Vec<StoredEvalPlan>, ExpertProfileRepositoryError> {
        Ok(sqlx::query_as::<_, StoredEvalPlan>(
            r#"
            SELECT id, profile_id, content_hash, schema_version, created_at
            FROM expert_eval_plans
            WHERE profile_id = ?
            ORDER BY created_at DESC
            "#,
        )
        .bind(profile_id.to_string())
        .fetch_all(pool)
        .await?)
    }

    pub async fn get_profile_version(
        pool: &SqlitePool,
        profile_id: Uuid,
        version_hash: &str,
    ) -> Result<Option<ExpertProfileVersion>, ExpertProfileRepositoryError> {
        let payload: Option<Vec<u8>> = sqlx::query_scalar(
            r#"
            SELECT content_payload
            FROM expert_profile_versions
            WHERE profile_id = ? AND version_hash = ?
            "#,
        )
        .bind(profile_id.to_string())
        .bind(version_hash)
        .fetch_optional(pool)
        .await?;

        let Some(payload) = payload else {
            return Ok(None);
        };
        let profile: ExpertProfileVersion = serde_json::from_slice(&payload).map_err(|error| {
            ExpertProfileRepositoryError::Database(sqlx::Error::Protocol(format!(
                "invalid stored expert profile JSON: {error}"
            )))
        })?;
        profile.validate()?;
        if hash_profile_version(&profile)? != version_hash {
            return Err(ExpertProfileRepositoryError::StoredContentIntegrity {
                kind: "expert profile",
            });
        }

        Ok(Some(profile))
    }

    pub async fn get_eval_plan(
        pool: &SqlitePool,
        plan_id: Uuid,
        content_hash: &str,
    ) -> Result<Option<EvalPlan>, ExpertProfileRepositoryError> {
        let payload: Option<Vec<u8>> = sqlx::query_scalar(
            r#"
            SELECT content_payload
            FROM expert_eval_plans
            WHERE id = ? AND content_hash = ?
            "#,
        )
        .bind(plan_id.to_string())
        .bind(content_hash)
        .fetch_optional(pool)
        .await?;

        let Some(payload) = payload else {
            return Ok(None);
        };
        let plan: EvalPlan = serde_json::from_slice(&payload).map_err(|error| {
            ExpertProfileRepositoryError::Database(sqlx::Error::Protocol(format!(
                "invalid stored expert evaluation plan JSON: {error}"
            )))
        })?;
        plan.validate()?;
        if hash_eval_plan(&plan)? != content_hash {
            return Err(ExpertProfileRepositoryError::StoredContentIntegrity {
                kind: "expert evaluation plan",
            });
        }

        Ok(Some(plan))
    }

    pub async fn persist_evaluation_report(
        pool: &SqlitePool,
        profile_id: Uuid,
        report: &EvaluationReport,
    ) -> Result<StoredEvalRun, ExpertProfileRepositoryError> {
        let profile_id_text = profile_id.to_string();
        if report.candidate_capability_revision.profile_id != profile_id {
            return Err(ExpertProfileRepositoryError::ActivationInputChanged(
                "evaluation report profile identity does not match the target profile".to_string(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        let payload = canonical_json(report)?;
        let result = sqlx::query(
            r#"
            INSERT INTO expert_eval_runs
                (profile_id, candidate_capability_hash, baseline_capability_hash,
                 eval_plan_hash, safety_gate_version, model_binding_hash,
                 adjudicator_binding_hash, results_payload, outcome, created_at)
            VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?, ?)
            "#,
        )
        .bind(&profile_id_text)
        .bind(&report.candidate_capability_hash)
        .bind(&report.baseline_capability_hash)
        .bind(&report.eval_plan_hash)
        .bind(&report.safety_gate_version)
        .bind(&report.model_binding_hash)
        .bind(payload)
        .bind(report.outcome.as_db_str())
        .bind(&now)
        .execute(pool)
        .await?;

        Ok(StoredEvalRun {
            id: result.last_insert_rowid(),
            profile_id: profile_id_text,
            candidate_capability_hash: report.candidate_capability_hash.clone(),
            baseline_capability_hash: report.baseline_capability_hash.clone(),
            eval_plan_hash: report.eval_plan_hash.clone(),
            safety_gate_version: report.safety_gate_version.clone(),
            model_binding_hash: report.model_binding_hash.clone(),
            outcome: report.outcome.as_db_str().to_string(),
            created_at: now,
        })
    }

    pub async fn get_evaluation_report(
        pool: &SqlitePool,
        profile_id: Uuid,
        eval_run_id: i64,
    ) -> Result<EvaluationReport, ExpertProfileRepositoryError> {
        let payload: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT results_payload FROM expert_eval_runs WHERE id = ? AND profile_id = ?",
        )
        .bind(eval_run_id)
        .bind(profile_id.to_string())
        .fetch_optional(pool)
        .await?;
        let payload = payload.ok_or(ExpertProfileRepositoryError::EvalRunNotFound(eval_run_id))?;
        serde_json::from_slice(&payload).map_err(|error| {
            ExpertProfileRepositoryError::Database(sqlx::Error::Protocol(format!(
                "invalid stored expert evaluation report JSON: {error}"
            )))
        })
    }

    pub async fn activate_profile(
        pool: &SqlitePool,
        profile_id: Uuid,
        eval_run_id: i64,
        model_binding: &ModelGenerationBinding,
        expected_previous_capability_hash: Option<&str>,
    ) -> Result<StoredProfileActivation, ExpertProfileRepositoryError> {
        let profile_id_text = profile_id.to_string();
        let binding_hash = hash_model_binding(model_binding)?;
        let binding_payload = canonical_json(model_binding)?;
        let mut transaction = pool.begin().await?;

        let eval_row: Option<(String, String, String, String, String, Vec<u8>)> = sqlx::query_as(
            r#"
            SELECT candidate_capability_hash, eval_plan_hash, safety_gate_version,
                   model_binding_hash, outcome, results_payload
            FROM expert_eval_runs
            WHERE id = ? AND profile_id = ?
            "#,
        )
        .bind(eval_run_id)
        .bind(&profile_id_text)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some((
            candidate_capability_hash,
            eval_plan_hash,
            safety_gate_version,
            stored_binding_hash,
            outcome,
            results_payload,
        )) = eval_row
        else {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::EvalRunNotFound(eval_run_id));
        };

        if !matches!(outcome.as_str(), "pass" | "baseline_missing") {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::EvalRunNotQualifying {
                eval_run_id,
                outcome,
            });
        }
        let report: EvaluationReport =
            serde_json::from_slice(&results_payload).map_err(|error| {
                ExpertProfileRepositoryError::Database(sqlx::Error::Protocol(format!(
                    "invalid stored expert evaluation report JSON: {error}"
                )))
            })?;
        if report.candidate_capability_hash != candidate_capability_hash
            || report.eval_plan_hash != eval_plan_hash
            || report.model_binding_hash != stored_binding_hash
            || report.safety_gate_version != safety_gate_version
        {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::StoredContentIntegrity {
                kind: "expert evaluation report",
            });
        }
        if stored_binding_hash != binding_hash {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::ActivationInputChanged(
                "model binding changed after evaluation".to_string(),
            ));
        }
        if model_binding.prompt_renderer_hash != prompt_renderer_hash()
            || model_binding.output_parser_version != OUTPUT_PARSER_VERSION
            || safety_gate_version != crate::expert_profiles::safety_gate::SAFETY_GATE_VERSION
        {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::ActivationInputChanged(
                "renderer, parser, or application safety gate changed after evaluation".to_string(),
            ));
        }

        let version_exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
                SELECT 1 FROM expert_profile_versions
                WHERE profile_id = ? AND version_hash = ?
            )
            "#,
        )
        .bind(&profile_id_text)
        .bind(&report.candidate_profile_version_hash)
        .fetch_one(&mut *transaction)
        .await?;
        let plan_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM expert_eval_plans WHERE profile_id = ? AND content_hash = ?)",
        )
        .bind(&profile_id_text)
        .bind(&eval_plan_hash)
        .fetch_one(&mut *transaction)
        .await?;
        if !version_exists || !plan_exists {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::ActivationInputChanged(
                "evaluated profile version or evaluation plan is no longer available".to_string(),
            ));
        }

        let current: Option<StoredProfileActivation> = sqlx::query_as(
            r#"
            SELECT profile_id, profile_version_hash, capability_revision_hash,
                   eval_run_id, status, superseded_reason, activated_at
            FROM expert_profile_activations WHERE profile_id = ?
            "#,
        )
        .bind(&profile_id_text)
        .fetch_optional(&mut *transaction)
        .await?;
        if current
            .as_ref()
            .map(|item| item.capability_revision_hash.as_str())
            != expected_previous_capability_hash
        {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::ActivationInputChanged(
                "active capability changed while evaluation was running".to_string(),
            ));
        }

        let now = Utc::now().to_rfc3339();
        if let Some(previous) = &current {
            sqlx::query(
                r#"
                INSERT INTO expert_activation_journal
                    (profile_id, capability_revision_hash, previous_capability_hash,
                     eval_run_id, action, created_at)
                VALUES (?, ?, ?, ?, 'supersede', ?)
                "#,
            )
            .bind(&profile_id_text)
            .bind(&previous.capability_revision_hash)
            .bind(&previous.capability_revision_hash)
            .bind(previous.eval_run_id)
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
        }

        sqlx::query(
            r#"
            INSERT INTO expert_profile_activations
                (profile_id, profile_version_hash, capability_revision_hash,
                 model_binding_payload, eval_run_id, status, superseded_reason, activated_at)
            VALUES (?, ?, ?, ?, ?, 'active', NULL, ?)
            ON CONFLICT(profile_id) DO UPDATE SET
                profile_version_hash = excluded.profile_version_hash,
                capability_revision_hash = excluded.capability_revision_hash,
                model_binding_payload = excluded.model_binding_payload,
                eval_run_id = excluded.eval_run_id,
                status = 'active',
                superseded_reason = NULL,
                activated_at = excluded.activated_at
            "#,
        )
        .bind(&profile_id_text)
        .bind(&report.candidate_profile_version_hash)
        .bind(&candidate_capability_hash)
        .bind(binding_payload)
        .bind(eval_run_id)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO expert_activation_journal
                (profile_id, capability_revision_hash, previous_capability_hash,
                 eval_run_id, action, created_at)
            VALUES (?, ?, ?, ?, 'activate', ?)
            "#,
        )
        .bind(&profile_id_text)
        .bind(&candidate_capability_hash)
        .bind(current.as_ref().map(|item| &item.capability_revision_hash))
        .bind(eval_run_id)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(StoredProfileActivation {
            profile_id: profile_id_text,
            profile_version_hash: report.candidate_profile_version_hash,
            capability_revision_hash: candidate_capability_hash,
            eval_run_id,
            status: "active".to_string(),
            superseded_reason: None,
            activated_at: now,
        })
    }

    pub async fn get_profile_activation(
        pool: &SqlitePool,
        profile_id: Uuid,
    ) -> Result<Option<StoredProfileActivation>, ExpertProfileRepositoryError> {
        Ok(sqlx::query_as(
            r#"
            SELECT profile_id, profile_version_hash, capability_revision_hash,
                   eval_run_id, status, superseded_reason, activated_at
            FROM expert_profile_activations WHERE profile_id = ?
            "#,
        )
        .bind(profile_id.to_string())
        .fetch_optional(pool)
        .await?)
    }

    pub async fn get_activation_binding(
        pool: &SqlitePool,
        profile_id: Uuid,
    ) -> Result<Option<ModelGenerationBinding>, ExpertProfileRepositoryError> {
        let payload: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT model_binding_payload FROM expert_profile_activations WHERE profile_id = ?",
        )
        .bind(profile_id.to_string())
        .fetch_optional(pool)
        .await?;
        payload
            .map(|payload| {
                serde_json::from_slice(&payload).map_err(|error| {
                    ExpertProfileRepositoryError::Database(sqlx::Error::Protocol(format!(
                        "invalid stored model binding JSON: {error}"
                    )))
                })
            })
            .transpose()
    }

    pub async fn mark_activation_superseded(
        pool: &SqlitePool,
        profile_id: Uuid,
        reason: &str,
    ) -> Result<Option<StoredProfileActivation>, ExpertProfileRepositoryError> {
        let profile_id_text = profile_id.to_string();
        let mut transaction = pool.begin().await?;
        let current: Option<StoredProfileActivation> = sqlx::query_as(
            r#"
            SELECT profile_id, profile_version_hash, capability_revision_hash,
                   eval_run_id, status, superseded_reason, activated_at
            FROM expert_profile_activations WHERE profile_id = ?
            "#,
        )
        .bind(&profile_id_text)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(mut current) = current else {
            transaction.rollback().await?;
            return Ok(None);
        };
        if current.status == "superseded" && current.superseded_reason.as_deref() == Some(reason) {
            transaction.rollback().await?;
            return Ok(Some(current));
        }

        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "UPDATE expert_profile_activations SET status = 'superseded', superseded_reason = ? WHERE profile_id = ?",
        )
        .bind(reason)
        .bind(&profile_id_text)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO expert_activation_journal
                (profile_id, capability_revision_hash, previous_capability_hash,
                 eval_run_id, action, created_at)
            VALUES (?, ?, ?, ?, 'supersede', ?)
            "#,
        )
        .bind(&profile_id_text)
        .bind(&current.capability_revision_hash)
        .bind(&current.capability_revision_hash)
        .bind(current.eval_run_id)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        current.status = "superseded".to_string();
        current.superseded_reason = Some(reason.to_string());
        Ok(Some(current))
    }

    pub async fn require_active_binding(
        pool: &SqlitePool,
        profile_id: Uuid,
        current_binding: &ModelGenerationBinding,
    ) -> Result<StoredProfileActivation, ExpertProfileRepositoryError> {
        let activation = Self::get_profile_activation(pool, profile_id)
            .await?
            .ok_or(ExpertProfileRepositoryError::ProfileNotFound(profile_id))?;
        if activation.status != "active" {
            return Err(ExpertProfileRepositoryError::BindingSuperseded(
                activation
                    .superseded_reason
                    .unwrap_or_else(|| "profile requires re-evaluation".to_string()),
            ));
        }
        let stored_payload: Vec<u8> = sqlx::query_scalar(
            "SELECT model_binding_payload FROM expert_profile_activations WHERE profile_id = ?",
        )
        .bind(profile_id.to_string())
        .fetch_one(pool)
        .await?;
        let stored: ModelGenerationBinding =
            serde_json::from_slice(&stored_payload).map_err(|error| {
                ExpertProfileRepositoryError::Database(sqlx::Error::Protocol(format!(
                    "invalid stored model binding JSON: {error}"
                )))
            })?;
        if hash_model_binding(&stored)? != hash_model_binding(current_binding)? {
            return Err(ExpertProfileRepositoryError::BindingSuperseded(
                "provider, model, endpoint, parameters, renderer, or parser changed".to_string(),
            ));
        }
        Ok(activation)
    }

    pub async fn retire_profile(
        pool: &SqlitePool,
        profile_id: Uuid,
    ) -> Result<(), ExpertProfileRepositoryError> {
        let profile_id_text = profile_id.to_string();
        let mut transaction = pool.begin().await?;
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM expert_profiles WHERE id = ?)")
                .bind(&profile_id_text)
                .fetch_one(&mut *transaction)
                .await?;
        if !exists {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::ProfileNotFound(profile_id));
        }
        let activation: Option<StoredProfileActivation> = sqlx::query_as(
            r#"
            SELECT profile_id, profile_version_hash, capability_revision_hash,
                   eval_run_id, status, superseded_reason, activated_at
            FROM expert_profile_activations WHERE profile_id = ?
            "#,
        )
        .bind(&profile_id_text)
        .fetch_optional(&mut *transaction)
        .await?;
        let now = Utc::now().to_rfc3339();
        if let Some(activation) = activation {
            sqlx::query(
                r#"
                INSERT INTO expert_activation_journal
                    (profile_id, capability_revision_hash, previous_capability_hash,
                     eval_run_id, action, created_at)
                VALUES (?, ?, ?, ?, 'retire', ?)
                "#,
            )
            .bind(&profile_id_text)
            .bind(&activation.capability_revision_hash)
            .bind(&activation.capability_revision_hash)
            .bind(activation.eval_run_id)
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
            sqlx::query("DELETE FROM expert_profile_activations WHERE profile_id = ?")
                .bind(&profile_id_text)
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query("UPDATE expert_profiles SET retired_at = ?, updated_at = ? WHERE id = ?")
            .bind(&now)
            .bind(&now)
            .bind(&profile_id_text)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn restore_profile(
        pool: &SqlitePool,
        profile_id: Uuid,
    ) -> Result<(), ExpertProfileRepositoryError> {
        let result = sqlx::query(
            "UPDATE expert_profiles SET retired_at = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(profile_id.to_string())
        .execute(pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(ExpertProfileRepositoryError::ProfileNotFound(profile_id));
        }
        Ok(())
    }

    pub async fn delete_profile(
        pool: &SqlitePool,
        profile_id: Uuid,
    ) -> Result<(), ExpertProfileRepositoryError> {
        let profile_id_text = profile_id.to_string();
        let mut transaction = pool.begin().await?;
        let active: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM expert_profile_activations WHERE profile_id = ?)",
        )
        .bind(&profile_id_text)
        .fetch_one(&mut *transaction)
        .await?;
        if active {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::ProfileActive);
        }
        let last_hash: Option<String> = sqlx::query_scalar(
            r#"
            SELECT version_hash FROM expert_profile_versions
            WHERE profile_id = ? ORDER BY seq DESC LIMIT 1
            "#,
        )
        .bind(&profile_id_text)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(last_hash) = last_hash else {
            transaction.rollback().await?;
            return Err(ExpertProfileRepositoryError::ProfileNotFound(profile_id));
        };
        sqlx::query(
            r#"
            INSERT INTO expert_activation_journal
                (profile_id, capability_revision_hash, previous_capability_hash,
                 eval_run_id, action, created_at)
            VALUES (?, ?, NULL, NULL, 'delete', ?)
            "#,
        )
        .bind(&profile_id_text)
        .bind(last_hash)
        .bind(Utc::now().to_rfc3339())
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM expert_profiles WHERE id = ?")
            .bind(&profile_id_text)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    async fn get_eval_plan_record(
        pool: &SqlitePool,
        plan_id: Uuid,
        content_hash: &str,
    ) -> Result<Option<StoredEvalPlan>, ExpertProfileRepositoryError> {
        Ok(sqlx::query_as::<_, StoredEvalPlan>(
            r#"
            SELECT id, profile_id, content_hash, schema_version, created_at
            FROM expert_eval_plans
            WHERE id = ? AND content_hash = ?
            "#,
        )
        .bind(plan_id.to_string())
        .bind(content_hash)
        .fetch_optional(pool)
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_support::TestDatabase;
    use crate::expert_profiles::evaluation::{EvalRunOutcome, EvaluationReport};
    use crate::expert_profiles::hashing::{
        hash_capability_revision, hash_eval_plan, hash_model_binding, prompt_renderer_hash,
    };
    use crate::expert_profiles::models::{
        EffectiveCapabilityRevision, GenerationParameters, ModelGenerationBinding,
    };
    use crate::expert_profiles::safety_gate::SAFETY_GATE_VERSION;
    use crate::expert_profiles::tests::{sample_eval_plan, sample_profile};

    #[tokio::test]
    async fn profile_creation_persists_one_immutable_version_and_plan() {
        let database = TestDatabase::new().await;
        let profile_id = Uuid::new_v4();
        let plan_id = Uuid::new_v4();
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);

        let (version, stored_plan) = ExpertProfilesRepository::create_profile_with_plan(
            database.pool(),
            profile_id,
            plan_id,
            &profile,
            &plan,
        )
        .await
        .expect("profile creation should succeed");

        assert_eq!(version.seq, 1);
        assert_eq!(stored_plan.profile_id, profile_id.to_string());
        assert_eq!(
            ExpertProfilesRepository::get_profile_version(
                database.pool(),
                profile_id,
                &version.version_hash,
            )
            .await
            .unwrap(),
            Some(profile)
        );
        assert_eq!(
            ExpertProfilesRepository::get_eval_plan(
                database.pool(),
                plan_id,
                &stored_plan.content_hash,
            )
            .await
            .unwrap(),
            Some(plan)
        );
        assert_eq!(
            ExpertProfilesRepository::list_profiles(database.pool())
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn edits_create_new_versions_without_mutating_the_original() {
        let database = TestDatabase::new().await;
        let profile_id = Uuid::new_v4();
        let original = sample_profile();
        let plan = sample_eval_plan(&original);
        let (first, _) = ExpertProfilesRepository::create_profile_with_plan(
            database.pool(),
            profile_id,
            Uuid::new_v4(),
            &original,
            &plan,
        )
        .await
        .unwrap();

        let mut edited = original.clone();
        edited.style.tone = "concise and candid".to_string();
        let second =
            ExpertProfilesRepository::create_profile_version(database.pool(), profile_id, &edited)
                .await
                .unwrap();

        assert_eq!(second.seq, 2);
        assert_ne!(first.version_hash, second.version_hash);
        assert_eq!(
            ExpertProfilesRepository::get_profile_version(
                database.pool(),
                profile_id,
                &first.version_hash,
            )
            .await
            .unwrap(),
            Some(original)
        );
        assert_eq!(
            ExpertProfilesRepository::get_profile_version(
                database.pool(),
                profile_id,
                &second.version_hash,
            )
            .await
            .unwrap(),
            Some(edited)
        );
    }

    #[tokio::test]
    async fn saving_identical_content_deduplicates_without_advancing_sequence() {
        let database = TestDatabase::new().await;
        let profile_id = Uuid::new_v4();
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        let (first, _) = ExpertProfilesRepository::create_profile_with_plan(
            database.pool(),
            profile_id,
            Uuid::new_v4(),
            &profile,
            &plan,
        )
        .await
        .unwrap();

        let duplicate =
            ExpertProfilesRepository::create_profile_version(database.pool(), profile_id, &profile)
                .await
                .unwrap();

        assert_eq!(duplicate, first);
        assert_eq!(
            ExpertProfilesRepository::list_profile_versions(database.pool(), profile_id)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn database_trigger_rejects_in_place_version_mutation() {
        let database = TestDatabase::new().await;
        let profile_id = Uuid::new_v4();
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        let (version, _) = ExpertProfilesRepository::create_profile_with_plan(
            database.pool(),
            profile_id,
            Uuid::new_v4(),
            &profile,
            &plan,
        )
        .await
        .unwrap();

        let error = sqlx::query(
            "UPDATE expert_profile_versions SET content_payload = ? WHERE profile_id = ? AND version_hash = ?",
        )
        .bind(b"{}".to_vec())
        .bind(profile_id.to_string())
        .bind(&version.version_hash)
        .execute(database.pool())
        .await
        .expect_err("immutable version trigger should reject updates");

        assert!(error.to_string().contains("immutable"));
    }

    #[tokio::test]
    async fn invalid_eval_plan_has_zero_persistence_side_effects() {
        let database = TestDatabase::new().await;
        let profile_id = Uuid::new_v4();
        let profile = sample_profile();
        let mut plan = sample_eval_plan(&profile);
        plan.cases.clear();

        let error = ExpertProfilesRepository::create_profile_with_plan(
            database.pool(),
            profile_id,
            Uuid::new_v4(),
            &profile,
            &plan,
        )
        .await
        .expect_err("empty evaluation plan must be rejected");
        assert!(matches!(error, ExpertProfileRepositoryError::Validation(_)));

        let profile_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM expert_profiles")
            .fetch_one(database.pool())
            .await
            .unwrap();
        let version_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM expert_profile_versions")
            .fetch_one(database.pool())
            .await
            .unwrap();
        assert_eq!((profile_count, version_count), (0, 0));
    }

    #[tokio::test]
    async fn content_digest_is_rechecked_when_an_immutable_version_is_loaded() {
        let database = TestDatabase::new().await;
        let profile_id = Uuid::new_v4();
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        ExpertProfilesRepository::create_profile_with_plan(
            database.pool(),
            profile_id,
            Uuid::new_v4(),
            &profile,
            &plan,
        )
        .await
        .unwrap();

        let false_hash = "sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        sqlx::query(
            r#"
            INSERT INTO expert_profile_versions
                (profile_id, version_hash, seq, content_payload, schema_version, created_at)
            VALUES (?, ?, 2, ?, 1, ?)
            "#,
        )
        .bind(profile_id.to_string())
        .bind(false_hash)
        .bind(canonical_json(&profile).unwrap())
        .bind(Utc::now().to_rfc3339())
        .execute(database.pool())
        .await
        .unwrap();

        let error =
            ExpertProfilesRepository::get_profile_version(database.pool(), profile_id, false_hash)
                .await
                .unwrap_err();
        assert!(matches!(
            error,
            ExpertProfileRepositoryError::StoredContentIntegrity { .. }
        ));
    }

    fn model_binding(model: &str) -> ModelGenerationBinding {
        ModelGenerationBinding {
            provider: "custom-openai".to_string(),
            model: model.to_string(),
            provider_record_id: None,
            provider_configuration_hash: None,
            credential_revision: None,
            model_artifact_hash: None,
            endpoint_fingerprint: Some("sha256:local-test-endpoint".to_string()),
            generation_parameters: GenerationParameters {
                temperature: 0.0,
                top_p: None,
                max_tokens: 1024,
                reasoning_effort: None,
            },
            prompt_renderer_hash: prompt_renderer_hash(),
            output_parser_version: OUTPUT_PARSER_VERSION,
        }
    }

    fn evaluation_report(
        profile_id: Uuid,
        profile: &ExpertProfileVersion,
        plan: &EvalPlan,
        binding: &ModelGenerationBinding,
        outcome: EvalRunOutcome,
    ) -> EvaluationReport {
        let profile_hash = hash_profile_version(profile).unwrap();
        let plan_hash = hash_eval_plan(plan).unwrap();
        let binding_hash = hash_model_binding(binding).unwrap();
        let mut playbook_ids: Vec<_> = profile
            .playbooks
            .iter()
            .map(|playbook| playbook.id)
            .collect();
        playbook_ids.sort();
        let revision = EffectiveCapabilityRevision {
            profile_id,
            profile_version_hash: profile_hash.clone(),
            playbook_ids: playbook_ids.clone(),
            model_binding_hash: binding_hash.clone(),
            eval_plan_hash: plan_hash.clone(),
            safety_gate_version: SAFETY_GATE_VERSION.to_string(),
        };
        EvaluationReport {
            qualifying: true,
            candidate_profile_version_hash: profile_hash,
            baseline_profile_version_hash: None,
            candidate_capability_hash: hash_capability_revision(&revision).unwrap(),
            candidate_capability_revision: revision,
            baseline_capability_hash: None,
            eval_plan_hash: plan_hash,
            model_binding_hash: binding_hash,
            model_binding: Some(binding.clone()),
            safety_gate_version: SAFETY_GATE_VERSION.to_string(),
            repetitions: Vec::new(),
            baseline_missing_playbooks: playbook_ids,
            removed_playbooks: Vec::new(),
            outcome,
            reasons: Vec::new(),
        }
    }

    #[tokio::test]
    async fn qualifying_eval_activates_atomically_and_pins_the_exact_binding() {
        let database = TestDatabase::new().await;
        let profile_id = Uuid::new_v4();
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        let binding = model_binding("model-a");
        ExpertProfilesRepository::create_profile_with_plan(
            database.pool(),
            profile_id,
            Uuid::new_v4(),
            &profile,
            &plan,
        )
        .await
        .unwrap();
        let report = evaluation_report(
            profile_id,
            &profile,
            &plan,
            &binding,
            EvalRunOutcome::BaselineMissing,
        );
        let run = ExpertProfilesRepository::persist_evaluation_report(
            database.pool(),
            profile_id,
            &report,
        )
        .await
        .unwrap();

        let activation = ExpertProfilesRepository::activate_profile(
            database.pool(),
            profile_id,
            run.id,
            &binding,
            None,
        )
        .await
        .unwrap();
        assert_eq!(activation.status, "active");
        assert_eq!(
            activation.capability_revision_hash,
            report.candidate_capability_hash
        );
        assert!(ExpertProfilesRepository::require_active_binding(
            database.pool(),
            profile_id,
            &binding,
        )
        .await
        .is_ok());

        let changed_binding = model_binding("model-b");
        assert!(matches!(
            ExpertProfilesRepository::require_active_binding(
                database.pool(),
                profile_id,
                &changed_binding,
            )
            .await,
            Err(ExpertProfileRepositoryError::BindingSuperseded(_))
        ));
        let journal_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM expert_activation_journal WHERE profile_id = ? AND action = 'activate'",
        )
        .bind(profile_id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(journal_count, 1);
    }

    #[tokio::test]
    async fn failed_eval_and_stale_activation_pointer_never_change_active_state() {
        let database = TestDatabase::new().await;
        let profile_id = Uuid::new_v4();
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        let binding = model_binding("model-a");
        ExpertProfilesRepository::create_profile_with_plan(
            database.pool(),
            profile_id,
            Uuid::new_v4(),
            &profile,
            &plan,
        )
        .await
        .unwrap();

        let failed = evaluation_report(profile_id, &profile, &plan, &binding, EvalRunOutcome::Fail);
        let failed_run = ExpertProfilesRepository::persist_evaluation_report(
            database.pool(),
            profile_id,
            &failed,
        )
        .await
        .unwrap();
        assert!(matches!(
            ExpertProfilesRepository::activate_profile(
                database.pool(),
                profile_id,
                failed_run.id,
                &binding,
                None,
            )
            .await,
            Err(ExpertProfileRepositoryError::EvalRunNotQualifying { .. })
        ));
        assert!(
            ExpertProfilesRepository::get_profile_activation(database.pool(), profile_id)
                .await
                .unwrap()
                .is_none()
        );

        let passing = evaluation_report(
            profile_id,
            &profile,
            &plan,
            &binding,
            EvalRunOutcome::BaselineMissing,
        );
        let passing_run = ExpertProfilesRepository::persist_evaluation_report(
            database.pool(),
            profile_id,
            &passing,
        )
        .await
        .unwrap();
        assert!(matches!(
            ExpertProfilesRepository::activate_profile(
                database.pool(),
                profile_id,
                passing_run.id,
                &binding,
                Some("sha256:stale-pointer"),
            )
            .await,
            Err(ExpertProfileRepositoryError::ActivationInputChanged(_))
        ));
        assert!(
            ExpertProfilesRepository::get_profile_activation(database.pool(), profile_id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn persistent_reconfiguration_marks_binding_superseded_but_is_idempotent() {
        let database = TestDatabase::new().await;
        let profile_id = Uuid::new_v4();
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        let binding = model_binding("model-a");
        ExpertProfilesRepository::create_profile_with_plan(
            database.pool(),
            profile_id,
            Uuid::new_v4(),
            &profile,
            &plan,
        )
        .await
        .unwrap();
        let report = evaluation_report(
            profile_id,
            &profile,
            &plan,
            &binding,
            EvalRunOutcome::BaselineMissing,
        );
        let run = ExpertProfilesRepository::persist_evaluation_report(
            database.pool(),
            profile_id,
            &report,
        )
        .await
        .unwrap();
        ExpertProfilesRepository::activate_profile(
            database.pool(),
            profile_id,
            run.id,
            &binding,
            None,
        )
        .await
        .unwrap();

        let superseded = ExpertProfilesRepository::mark_activation_superseded(
            database.pool(),
            profile_id,
            "default model changed",
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(superseded.status, "superseded");
        ExpertProfilesRepository::mark_activation_superseded(
            database.pool(),
            profile_id,
            "default model changed",
        )
        .await
        .unwrap();

        let supersede_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM expert_activation_journal WHERE profile_id = ? AND action = 'supersede'",
        )
        .bind(profile_id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!(supersede_count, 1);
    }

    #[tokio::test]
    async fn active_profile_must_be_retired_before_atomic_deletion() {
        let database = TestDatabase::new().await;
        let profile_id = Uuid::new_v4();
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        let binding = model_binding("model-a");
        ExpertProfilesRepository::create_profile_with_plan(
            database.pool(),
            profile_id,
            Uuid::new_v4(),
            &profile,
            &plan,
        )
        .await
        .unwrap();
        let report = evaluation_report(
            profile_id,
            &profile,
            &plan,
            &binding,
            EvalRunOutcome::BaselineMissing,
        );
        let run = ExpertProfilesRepository::persist_evaluation_report(
            database.pool(),
            profile_id,
            &report,
        )
        .await
        .unwrap();
        ExpertProfilesRepository::activate_profile(
            database.pool(),
            profile_id,
            run.id,
            &binding,
            None,
        )
        .await
        .unwrap();

        assert!(matches!(
            ExpertProfilesRepository::delete_profile(database.pool(), profile_id).await,
            Err(ExpertProfileRepositoryError::ProfileActive)
        ));
        ExpertProfilesRepository::retire_profile(database.pool(), profile_id)
            .await
            .unwrap();
        assert!(
            ExpertProfilesRepository::get_profile_activation(database.pool(), profile_id)
                .await
                .unwrap()
                .is_none()
        );
        ExpertProfilesRepository::restore_profile(database.pool(), profile_id)
            .await
            .unwrap();
        ExpertProfilesRepository::retire_profile(database.pool(), profile_id)
            .await
            .unwrap();
        ExpertProfilesRepository::delete_profile(database.pool(), profile_id)
            .await
            .unwrap();

        assert!(ExpertProfilesRepository::list_profiles(database.pool())
            .await
            .unwrap()
            .is_empty());
        let actions: Vec<String> = sqlx::query_scalar(
            "SELECT action FROM expert_activation_journal WHERE profile_id = ? ORDER BY id",
        )
        .bind(profile_id.to_string())
        .fetch_all(database.pool())
        .await
        .unwrap();
        assert_eq!(actions, ["activate", "retire", "delete"]);
    }
}
