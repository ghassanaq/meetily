use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use crate::database::repositories::expert_profile::{
    ExpertProfileRepositoryError, ExpertProfilesRepository, StoredEvalPlan, StoredProfileVersion,
};

use super::hashing::{hash_eval_plan, hash_profile_version, HashError};
use super::models::{EvalPlan, ExpertProfileVersion};
use super::validation::{Validate, ValidationErrors};

pub const BUNDLE_FORMAT: &str = "meetily-profile";
pub const BUNDLE_FORMAT_VERSION: u32 = 1;
const MAX_BUNDLE_BYTES: usize = 1024 * 1024;
const MAX_BUNDLE_DEPTH: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ExpertProfileBundle {
    pub format: String,
    pub format_version: u32,
    pub manifest: BundleManifest,
    pub digests: BundleDigests,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub schema_version: u32,
    pub profile: BundleProfile,
    pub eval_plan: BundleEvalPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BundleProfile {
    pub id: Uuid,
    pub version_hash: String,
    pub content: ExpertProfileVersion,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BundleEvalPlan {
    pub id: Uuid,
    pub content_hash: String,
    pub content: EvalPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BundleDigests {
    pub profile: String,
    pub eval_plan: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ImportIdentityMode {
    Clone,
    RestoreIdentity,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportResult {
    pub profile_id: Uuid,
    pub plan_id: Uuid,
    pub profile_version: StoredProfileVersion,
    pub eval_plan: StoredEvalPlan,
    pub playbook_id_remap: HashMap<Uuid, Uuid>,
}

#[derive(Debug, Error)]
pub enum BundleError {
    #[error("LIMIT_EXCEEDED: profile bundle exceeds the 1 MiB limit")]
    LimitExceeded,
    #[error("LIMIT_EXCEEDED: profile bundle nesting exceeds depth 32")]
    DepthExceeded,
    #[error("UNSUPPORTED_FORMAT_VERSION: expected {BUNDLE_FORMAT} v{BUNDLE_FORMAT_VERSION}")]
    UnsupportedFormat,
    #[error("SCHEMA_MISMATCH: {0}")]
    Schema(String),
    #[error("DIGEST_MISMATCH: {0}")]
    DigestMismatch(&'static str),
    #[error("restore identity conflicts with an existing profile")]
    IdentityConflict,
    #[error(transparent)]
    Validation(#[from] ValidationErrors),
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error(transparent)]
    Repository(#[from] ExpertProfileRepositoryError),
}

impl ExpertProfileBundle {
    pub fn new(
        profile_id: Uuid,
        version_hash: String,
        profile: ExpertProfileVersion,
        plan_id: Uuid,
        plan_hash: String,
        plan: EvalPlan,
    ) -> Result<Self, BundleError> {
        profile.validate()?;
        plan.validate_for_profile(&profile)?;
        let actual_profile_hash = hash_profile_version(&profile)?;
        let actual_plan_hash = hash_eval_plan(&plan)?;
        if actual_profile_hash != version_hash {
            return Err(BundleError::DigestMismatch("profile version"));
        }
        if actual_plan_hash != plan_hash {
            return Err(BundleError::DigestMismatch("evaluation plan"));
        }
        Ok(Self {
            format: BUNDLE_FORMAT.to_string(),
            format_version: BUNDLE_FORMAT_VERSION,
            manifest: BundleManifest {
                schema_version: 1,
                profile: BundleProfile {
                    id: profile_id,
                    version_hash: version_hash.clone(),
                    content: profile,
                },
                eval_plan: BundleEvalPlan {
                    id: plan_id,
                    content_hash: plan_hash.clone(),
                    content: plan,
                },
            },
            digests: BundleDigests {
                profile: version_hash,
                eval_plan: plan_hash,
            },
        })
    }

    pub fn validate(&self) -> Result<(), BundleError> {
        if self.format != BUNDLE_FORMAT
            || self.format_version != BUNDLE_FORMAT_VERSION
            || self.manifest.schema_version != 1
        {
            return Err(BundleError::UnsupportedFormat);
        }
        self.manifest.profile.content.validate()?;
        self.manifest
            .eval_plan
            .content
            .validate_for_profile(&self.manifest.profile.content)?;
        let profile_hash = hash_profile_version(&self.manifest.profile.content)?;
        let plan_hash = hash_eval_plan(&self.manifest.eval_plan.content)?;
        if profile_hash != self.manifest.profile.version_hash
            || profile_hash != self.digests.profile
        {
            return Err(BundleError::DigestMismatch("profile"));
        }
        if plan_hash != self.manifest.eval_plan.content_hash || plan_hash != self.digests.eval_plan
        {
            return Err(BundleError::DigestMismatch("evaluation plan"));
        }
        Ok(())
    }
}

pub fn parse_bundle_json(input: &str) -> Result<ExpertProfileBundle, BundleError> {
    if input.len() > MAX_BUNDLE_BYTES {
        return Err(BundleError::LimitExceeded);
    }
    if json_depth(input) > MAX_BUNDLE_DEPTH {
        return Err(BundleError::DepthExceeded);
    }
    let bundle: ExpertProfileBundle =
        serde_json::from_str(input).map_err(|error| BundleError::Schema(error.to_string()))?;
    bundle.validate()?;
    Ok(bundle)
}

pub async fn export_bundle(
    pool: &sqlx::SqlitePool,
    profile_id: Uuid,
    version_hash: &str,
    plan_id: Uuid,
    plan_hash: &str,
) -> Result<ExpertProfileBundle, BundleError> {
    let profile = ExpertProfilesRepository::get_profile_version(pool, profile_id, version_hash)
        .await?
        .ok_or_else(|| ExpertProfileRepositoryError::VersionNotFound {
            profile_id,
            version_hash: version_hash.to_string(),
        })?;
    let plan = ExpertProfilesRepository::get_eval_plan(pool, plan_id, plan_hash)
        .await?
        .ok_or_else(|| ExpertProfileRepositoryError::EvalPlanNotFound {
            plan_id,
            content_hash: plan_hash.to_string(),
        })?;
    ExpertProfileBundle::new(
        profile_id,
        version_hash.to_string(),
        profile,
        plan_id,
        plan_hash.to_string(),
        plan,
    )
}

pub async fn import_bundle(
    pool: &sqlx::SqlitePool,
    mut bundle: ExpertProfileBundle,
    mode: ImportIdentityMode,
) -> Result<ImportResult, BundleError> {
    bundle.validate()?;
    let mut playbook_id_remap = HashMap::new();
    let (profile_id, plan_id) = match mode {
        ImportIdentityMode::Clone => {
            for playbook in &mut bundle.manifest.profile.content.playbooks {
                let new_id = Uuid::new_v4();
                playbook_id_remap.insert(playbook.id, new_id);
                playbook.id = new_id;
            }
            for case in &mut bundle.manifest.eval_plan.content.cases {
                case.playbook_id = *playbook_id_remap
                    .get(&case.playbook_id)
                    .expect("validated case references an embedded playbook");
            }
            (Uuid::new_v4(), Uuid::new_v4())
        }
        ImportIdentityMode::RestoreIdentity => {
            let existing = ExpertProfilesRepository::list_profiles(pool).await?;
            if existing
                .iter()
                .any(|profile| profile.id == bundle.manifest.profile.id.to_string())
            {
                return Err(BundleError::IdentityConflict);
            }
            (bundle.manifest.profile.id, bundle.manifest.eval_plan.id)
        }
    };

    let (profile_version, eval_plan) = ExpertProfilesRepository::create_profile_with_plan(
        pool,
        profile_id,
        plan_id,
        &bundle.manifest.profile.content,
        &bundle.manifest.eval_plan.content,
    )
    .await?;
    Ok(ImportResult {
        profile_id,
        plan_id,
        profile_version,
        eval_plan,
        playbook_id_remap,
    })
}

fn json_depth(input: &str) -> usize {
    let mut current = 0usize;
    let mut maximum = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for character in input.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
            continue;
        }
        match character {
            '"' => in_string = true,
            '{' | '[' => {
                current += 1;
                maximum = maximum.max(current);
            }
            '}' | ']' => current = current.saturating_sub(1),
            _ => {}
        }
    }
    maximum
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_support::TestDatabase;
    use crate::expert_profiles::hashing::{hash_eval_plan, hash_profile_version};
    use crate::expert_profiles::tests::{sample_eval_plan, sample_profile};

    fn sample_bundle() -> ExpertProfileBundle {
        let profile = sample_profile();
        let plan = sample_eval_plan(&profile);
        ExpertProfileBundle::new(
            Uuid::new_v4(),
            hash_profile_version(&profile).unwrap(),
            profile,
            Uuid::new_v4(),
            hash_eval_plan(&plan).unwrap(),
            plan,
        )
        .unwrap()
    }

    #[test]
    fn bundle_round_trip_preserves_content_hashes() {
        let bundle = sample_bundle();
        let encoded = serde_json::to_string(&bundle).unwrap();
        let decoded = parse_bundle_json(&encoded).unwrap();
        assert_eq!(decoded, bundle);
    }

    #[test]
    fn tampered_bundle_digest_fails_before_persistence() {
        let mut bundle = sample_bundle();
        bundle.manifest.profile.content.style.tone = "tampered".to_string();
        assert!(matches!(
            bundle.validate(),
            Err(BundleError::DigestMismatch("profile"))
        ));
    }

    #[tokio::test]
    async fn clone_import_remaps_profile_plan_and_playbook_identity_and_never_activates() {
        let database = TestDatabase::new().await;
        let bundle = sample_bundle();
        let old_profile_id = bundle.manifest.profile.id;
        let old_plan_id = bundle.manifest.eval_plan.id;
        let old_playbook_id = bundle.manifest.profile.content.playbooks[0].id;

        let imported = import_bundle(database.pool(), bundle, ImportIdentityMode::Clone)
            .await
            .unwrap();
        assert_ne!(imported.profile_id, old_profile_id);
        assert_ne!(imported.plan_id, old_plan_id);
        assert_ne!(
            imported.playbook_id_remap[&old_playbook_id],
            old_playbook_id
        );
        assert!(ExpertProfilesRepository::get_profile_activation(
            database.pool(),
            imported.profile_id,
        )
        .await
        .unwrap()
        .is_none());
    }

    #[tokio::test]
    async fn restore_identity_is_explicit_and_rejects_a_local_conflict() {
        let database = TestDatabase::new().await;
        let bundle = sample_bundle();
        let profile_id = bundle.manifest.profile.id;
        let first = import_bundle(
            database.pool(),
            bundle.clone(),
            ImportIdentityMode::RestoreIdentity,
        )
        .await
        .unwrap();
        assert_eq!(first.profile_id, profile_id);
        assert!(matches!(
            import_bundle(database.pool(), bundle, ImportIdentityMode::RestoreIdentity).await,
            Err(BundleError::IdentityConflict)
        ));
    }
}
