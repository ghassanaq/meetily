use anyhow::{anyhow, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityScopePolicyMode {
    Offline,
    Advisory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthorityScopePolicyStatus {
    pub identity_id: Uuid,
    pub identity_version_hash: String,
    pub configured: bool,
    pub mode: AuthorityScopePolicyMode,
    pub activated_at: Option<String>,
    pub total_dismissals: u64,
}

pub struct AuthorityScopeRepository;

impl AuthorityScopeRepository {
    pub async fn status(
        pool: &SqlitePool,
        identity_id: Uuid,
        version_hash: &str,
        configured: bool,
    ) -> Result<AuthorityScopePolicyStatus> {
        if configured {
            let now = Utc::now().to_rfc3339();
            sqlx::query(
                "INSERT OR IGNORE INTO authority_scope_policy_state
                 (identity_id, identity_version_hash, mode, activated_at, created_at, updated_at)
                 VALUES (?, ?, 'offline', NULL, ?, ?)",
            )
            .bind(identity_id.to_string())
            .bind(version_hash)
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await?;
        }
        let row = sqlx::query(
            "SELECT mode, activated_at FROM authority_scope_policy_state
             WHERE identity_id = ? AND identity_version_hash = ?",
        )
        .bind(identity_id.to_string())
        .bind(version_hash)
        .fetch_optional(pool)
        .await?;
        let (mode, activated_at) = match row {
            Some(row) => {
                let mode = match row.try_get::<String, _>("mode")?.as_str() {
                    "offline" => AuthorityScopePolicyMode::Offline,
                    "advisory" => AuthorityScopePolicyMode::Advisory,
                    other => return Err(anyhow!("unknown authority policy mode '{other}'")),
                };
                (mode, row.try_get("activated_at")?)
            }
            None => (AuthorityScopePolicyMode::Offline, None),
        };
        let total = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(SUM(dismissal_count), 0) FROM authority_scope_rule_feedback
             WHERE identity_id = ? AND identity_version_hash = ?",
        )
        .bind(identity_id.to_string())
        .bind(version_hash)
        .fetch_one(pool)
        .await?;
        Ok(AuthorityScopePolicyStatus {
            identity_id,
            identity_version_hash: version_hash.to_string(),
            configured,
            mode,
            activated_at,
            total_dismissals: total.try_into().unwrap_or(0),
        })
    }

    pub async fn activate(pool: &SqlitePool, identity_id: Uuid, version_hash: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE authority_scope_policy_state SET mode = 'advisory', activated_at = ?, updated_at = ?
             WHERE identity_id = ? AND identity_version_hash = ?",
        )
        .bind(&now)
        .bind(&now)
        .bind(identity_id.to_string())
        .bind(version_hash)
        .execute(pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(anyhow!(
                "authority rules are not configured for this exact identity version"
            ));
        }
        Ok(())
    }

    pub async fn record_dismissal(
        pool: &SqlitePool,
        identity_id: Uuid,
        version_hash: &str,
        rule_id: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO authority_scope_rule_feedback
             (identity_id, identity_version_hash, rule_id, dismissal_count, last_dismissed_at)
             VALUES (?, ?, ?, 1, ?)
             ON CONFLICT(identity_id, identity_version_hash, rule_id) DO UPDATE SET
             dismissal_count = dismissal_count + 1, last_dismissed_at = excluded.last_dismissed_at",
        )
        .bind(identity_id.to_string())
        .bind(version_hash)
        .bind(rule_id)
        .bind(now)
        .execute(pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::test_support::TestDatabase;

    async fn seed_version(database: &TestDatabase, identity_id: Uuid, hash: &str) {
        let now = Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO professional_identities (id, name, created_at, updated_at) VALUES (?, 'Test', ?, ?)")
            .bind(identity_id.to_string()).bind(&now).bind(&now)
            .execute(database.pool()).await.unwrap();
        sqlx::query("INSERT INTO professional_identity_versions (identity_id, version_hash, seq, content_payload, schema_version, created_at) VALUES (?, ?, 1, x'7B7D', 2, ?)")
            .bind(identity_id.to_string()).bind(hash).bind(&now)
            .execute(database.pool()).await.unwrap();
    }

    #[tokio::test]
    async fn configured_versions_default_offline_and_activation_is_hash_bound() {
        let database = TestDatabase::new().await;
        let id = Uuid::new_v4();
        seed_version(&database, id, "sha256:first").await;
        let initial = AuthorityScopeRepository::status(database.pool(), id, "sha256:first", true)
            .await
            .unwrap();
        assert_eq!(initial.mode, AuthorityScopePolicyMode::Offline);
        AuthorityScopeRepository::activate(database.pool(), id, "sha256:first")
            .await
            .unwrap();
        assert_eq!(
            AuthorityScopeRepository::status(database.pool(), id, "sha256:first", true)
                .await
                .unwrap()
                .mode,
            AuthorityScopePolicyMode::Advisory
        );

        let now = Utc::now().to_rfc3339();
        sqlx::query("INSERT INTO professional_identity_versions (identity_id, version_hash, seq, content_payload, schema_version, created_at) VALUES (?, 'sha256:second', 2, x'7B7D', 2, ?)")
            .bind(id.to_string()).bind(now).execute(database.pool()).await.unwrap();
        assert_eq!(
            AuthorityScopeRepository::status(database.pool(), id, "sha256:second", true)
                .await
                .unwrap()
                .mode,
            AuthorityScopePolicyMode::Offline
        );
    }

    #[tokio::test]
    async fn dismissal_is_atomic_does_not_activate_and_delete_cascades() {
        let database = TestDatabase::new().await;
        let id = Uuid::new_v4();
        seed_version(&database, id, "sha256:first").await;
        AuthorityScopeRepository::status(database.pool(), id, "sha256:first", true)
            .await
            .unwrap();
        AuthorityScopeRepository::record_dismissal(database.pool(), id, "sha256:first", "rule-a")
            .await
            .unwrap();
        AuthorityScopeRepository::record_dismissal(database.pool(), id, "sha256:first", "rule-a")
            .await
            .unwrap();
        let status = AuthorityScopeRepository::status(database.pool(), id, "sha256:first", true)
            .await
            .unwrap();
        assert_eq!(status.total_dismissals, 2);
        assert_eq!(status.mode, AuthorityScopePolicyMode::Offline);
        sqlx::query("DELETE FROM professional_identities WHERE id = ?")
            .bind(id.to_string())
            .execute(database.pool())
            .await
            .unwrap();
        let policy_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM authority_scope_policy_state WHERE identity_id = ?",
        )
        .bind(id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        let feedback_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM authority_scope_rule_feedback WHERE identity_id = ?",
        )
        .bind(id.to_string())
        .fetch_one(database.pool())
        .await
        .unwrap();
        assert_eq!((policy_count, feedback_count), (0, 0));
    }
}
