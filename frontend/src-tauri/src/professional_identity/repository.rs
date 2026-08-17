use chrono::Utc;
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};
use thiserror::Error;
use uuid::Uuid;

use crate::expert_profiles::hashing::canonical_json;

use super::{hash_identity_version, validate_identity, ProfessionalIdentityVersion};

pub struct ProfessionalIdentityRepository;

#[derive(Debug, Error)]
pub enum ProfessionalIdentityRepositoryError {
    #[error(transparent)]
    Validation(#[from] anyhow::Error),
    #[error(transparent)]
    Hash(#[from] crate::expert_profiles::hashing::HashError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("professional identity {0} was not found")]
    IdentityNotFound(Uuid),
    #[error("professional identity version {version_hash} was not found for {identity_id}")]
    VersionNotFound {
        identity_id: Uuid,
        version_hash: String,
    },
    #[error("stored professional identity content does not match its recorded digest")]
    StoredContentIntegrity,
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct ProfessionalIdentitySummary {
    pub id: String,
    pub name: String,
    pub retired_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct StoredProfessionalIdentityVersion {
    pub identity_id: String,
    pub version_hash: String,
    pub seq: i64,
    pub schema_version: i64,
    pub created_at: String,
}

impl ProfessionalIdentityRepository {
    pub async fn create(
        pool: &SqlitePool,
        identity_id: Uuid,
        content: &ProfessionalIdentityVersion,
    ) -> Result<StoredProfessionalIdentityVersion, ProfessionalIdentityRepositoryError> {
        validate_identity(content)?;
        let version_hash = hash_identity_version(content)?;
        let payload = canonical_json(content)?;
        let identity_id = identity_id.to_string();
        let now = Utc::now().to_rfc3339();
        let mut transaction = pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO professional_identities (id, name, retired_at, created_at, updated_at)
            VALUES (?, ?, NULL, ?, ?)
            "#,
        )
        .bind(&identity_id)
        .bind(&content.identity.display_name)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO professional_identity_versions
                (identity_id, version_hash, seq, content_payload, schema_version, created_at)
            VALUES (?, ?, 1, ?, ?, ?)
            "#,
        )
        .bind(&identity_id)
        .bind(&version_hash)
        .bind(payload)
        .bind(i64::from(content.schema_version))
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(StoredProfessionalIdentityVersion {
            identity_id,
            version_hash,
            seq: 1,
            schema_version: i64::from(content.schema_version),
            created_at: now,
        })
    }

    pub async fn create_version(
        pool: &SqlitePool,
        identity_id: Uuid,
        content: &ProfessionalIdentityVersion,
    ) -> Result<StoredProfessionalIdentityVersion, ProfessionalIdentityRepositoryError> {
        validate_identity(content)?;
        let identity_id_text = identity_id.to_string();
        let version_hash = hash_identity_version(content)?;
        let payload = canonical_json(content)?;
        let now = Utc::now().to_rfc3339();
        let mut transaction = pool.begin().await?;

        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM professional_identities WHERE id = ?)")
                .bind(&identity_id_text)
                .fetch_one(&mut *transaction)
                .await?;
        if !exists {
            transaction.rollback().await?;
            return Err(ProfessionalIdentityRepositoryError::IdentityNotFound(
                identity_id,
            ));
        }

        if let Some(existing) = sqlx::query_as::<_, StoredProfessionalIdentityVersion>(
            r#"
            SELECT identity_id, version_hash, seq, schema_version, created_at
            FROM professional_identity_versions
            WHERE identity_id = ? AND version_hash = ?
            "#,
        )
        .bind(&identity_id_text)
        .bind(&version_hash)
        .fetch_optional(&mut *transaction)
        .await?
        {
            transaction.rollback().await?;
            return Ok(existing);
        }

        let next_seq: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM professional_identity_versions WHERE identity_id = ?",
        )
        .bind(&identity_id_text)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            r#"
            INSERT INTO professional_identity_versions
                (identity_id, version_hash, seq, content_payload, schema_version, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&identity_id_text)
        .bind(&version_hash)
        .bind(next_seq)
        .bind(payload)
        .bind(i64::from(content.schema_version))
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("UPDATE professional_identities SET name = ?, updated_at = ? WHERE id = ?")
            .bind(&content.identity.display_name)
            .bind(&now)
            .bind(&identity_id_text)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;

        Ok(StoredProfessionalIdentityVersion {
            identity_id: identity_id_text,
            version_hash,
            seq: next_seq,
            schema_version: i64::from(content.schema_version),
            created_at: now,
        })
    }

    pub async fn list(
        pool: &SqlitePool,
    ) -> Result<Vec<ProfessionalIdentitySummary>, ProfessionalIdentityRepositoryError> {
        Ok(sqlx::query_as::<_, ProfessionalIdentitySummary>(
            r#"
            SELECT id, name, retired_at, created_at, updated_at
            FROM professional_identities
            ORDER BY retired_at IS NOT NULL, updated_at DESC, id
            "#,
        )
        .fetch_all(pool)
        .await?)
    }

    pub async fn list_versions(
        pool: &SqlitePool,
        identity_id: Uuid,
    ) -> Result<Vec<StoredProfessionalIdentityVersion>, ProfessionalIdentityRepositoryError> {
        Ok(sqlx::query_as::<_, StoredProfessionalIdentityVersion>(
            r#"
            SELECT identity_id, version_hash, seq, schema_version, created_at
            FROM professional_identity_versions
            WHERE identity_id = ?
            ORDER BY seq DESC
            "#,
        )
        .bind(identity_id.to_string())
        .fetch_all(pool)
        .await?)
    }

    pub async fn get(
        pool: &SqlitePool,
        identity_id: Uuid,
        version_hash: &str,
    ) -> Result<Option<ProfessionalIdentityVersion>, ProfessionalIdentityRepositoryError> {
        let payload: Option<Vec<u8>> = sqlx::query_scalar(
            r#"
            SELECT content_payload
            FROM professional_identity_versions
            WHERE identity_id = ? AND version_hash = ?
            "#,
        )
        .bind(identity_id.to_string())
        .bind(version_hash)
        .fetch_optional(pool)
        .await?;
        let Some(payload) = payload else {
            return Ok(None);
        };
        let content: ProfessionalIdentityVersion = serde_json::from_slice(&payload)
            .map_err(|error| ProfessionalIdentityRepositoryError::Validation(error.into()))?;
        validate_identity(&content)?;
        if hash_identity_version(&content)? != version_hash {
            return Err(ProfessionalIdentityRepositoryError::StoredContentIntegrity);
        }
        Ok(Some(content))
    }

    pub async fn retire(
        pool: &SqlitePool,
        identity_id: Uuid,
    ) -> Result<(), ProfessionalIdentityRepositoryError> {
        let result = sqlx::query(
            "UPDATE professional_identities SET retired_at = ?, updated_at = ? WHERE id = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(Utc::now().to_rfc3339())
        .bind(identity_id.to_string())
        .execute(pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(ProfessionalIdentityRepositoryError::IdentityNotFound(
                identity_id,
            ));
        }
        Ok(())
    }

    pub async fn restore(
        pool: &SqlitePool,
        identity_id: Uuid,
    ) -> Result<(), ProfessionalIdentityRepositoryError> {
        let result = sqlx::query(
            "UPDATE professional_identities SET retired_at = NULL, updated_at = ? WHERE id = ?",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(identity_id.to_string())
        .execute(pool)
        .await?;
        if result.rows_affected() == 0 {
            return Err(ProfessionalIdentityRepositoryError::IdentityNotFound(
                identity_id,
            ));
        }
        Ok(())
    }

    pub async fn delete(
        pool: &SqlitePool,
        identity_id: Uuid,
    ) -> Result<(), ProfessionalIdentityRepositoryError> {
        let result = sqlx::query("DELETE FROM professional_identities WHERE id = ?")
            .bind(identity_id.to_string())
            .execute(pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(ProfessionalIdentityRepositoryError::IdentityNotFound(
                identity_id,
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_support::TestDatabase;
    use crate::professional_identity::{
        IdentityRecord, IdentityRecordCategory, IdentitySource, ProfessionalIdentityHeader,
        PROFESSIONAL_IDENTITY_SCHEMA_VERSION,
    };

    fn sample() -> ProfessionalIdentityVersion {
        ProfessionalIdentityVersion {
            schema_version: PROFESSIONAL_IDENTITY_SCHEMA_VERSION,
            identity: ProfessionalIdentityHeader {
                display_name: "Ghassan".to_string(),
                role_title: "Head of Mission".to_string(),
                organization: "Mission".to_string(),
                professional_summary: "I lead the mission.".to_string(),
            },
            records: vec![IdentityRecord {
                id: Uuid::from_u128(10),
                category: IdentityRecordCategory::Authority,
                title: "Approval authority".to_string(),
                content: "I can approve operational spending up to the recorded limit.".to_string(),
                source: IdentitySource {
                    label: "Delegation matrix".to_string(),
                    revision: "2026".to_string(),
                },
                updated_at: "2026-08-12T00:00:00Z".to_string(),
                valid_until: None,
                conflict_key: None,
                tags: vec!["approval".to_string()],
            }],
            projects: vec![],
        }
    }

    #[tokio::test]
    async fn versions_are_immutable_content_addressed_and_round_trip() {
        let database = TestDatabase::new().await;
        let identity_id = Uuid::new_v4();
        let first = sample();
        let stored = ProfessionalIdentityRepository::create(database.pool(), identity_id, &first)
            .await
            .unwrap();
        let duplicate =
            ProfessionalIdentityRepository::create_version(database.pool(), identity_id, &first)
                .await
                .unwrap();
        assert_eq!(stored.version_hash, duplicate.version_hash);
        assert_eq!(duplicate.seq, 1);

        let loaded =
            ProfessionalIdentityRepository::get(database.pool(), identity_id, &stored.version_hash)
                .await
                .unwrap()
                .unwrap();
        assert_eq!(loaded, first);
    }

    #[tokio::test]
    async fn database_trigger_rejects_in_place_identity_version_mutation() {
        let database = TestDatabase::new().await;
        let identity_id = Uuid::new_v4();
        let stored =
            ProfessionalIdentityRepository::create(database.pool(), identity_id, &sample())
                .await
                .unwrap();
        let error = sqlx::query(
            "UPDATE professional_identity_versions SET schema_version = 99 WHERE identity_id = ? AND version_hash = ?",
        )
        .bind(identity_id.to_string())
        .bind(stored.version_hash)
        .execute(database.pool())
        .await
        .unwrap_err();
        assert!(error.to_string().contains("versions are immutable"));
    }
}
