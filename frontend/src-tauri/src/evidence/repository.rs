use chrono::Utc;
use serde::Serialize;
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
use thiserror::Error;
use uuid::Uuid;

use crate::expert_profiles::hashing::HashError;

use super::hashing::{canonical_transcript_payload, hash_transcript_version};
use super::models::{
    EvidenceValidationError, RecordingArtifactKind, RecordingVersionSpec, TranscriptVersionContent,
};

pub struct EvidenceRepository;

#[derive(Debug, Error)]
pub enum EvidenceRepositoryError {
    #[error(transparent)]
    Validation(#[from] EvidenceValidationError),
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error("meeting {0} was not found")]
    MeetingNotFound(String),
    #[error("recording artifact {0} was not found")]
    ArtifactNotFound(Uuid),
    #[error("recording version {version_hash} was not found for artifact {artifact_id}")]
    RecordingVersionNotFound {
        artifact_id: Uuid,
        version_hash: String,
    },
    #[error("recording artifact identity conflicts with existing data")]
    ArtifactIdentityConflict,
    #[error("recording version metadata conflicts with existing data")]
    RecordingVersionConflict,
    #[error("segment {index} ends after the recording duration")]
    SegmentBeyondRecording { index: usize },
    #[error("stored {kind} content does not match its recorded digest")]
    StoredContentIntegrity { kind: &'static str },
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct StoredRecordingArtifact {
    pub id: String,
    pub meeting_id: Option<String>,
    pub kind: String,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct StoredRecordingVersion {
    pub artifact_id: String,
    pub version_hash: String,
    pub byte_length: i64,
    pub media_type: String,
    pub duration_ms: i64,
    pub created_at: String,
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct StoredRecordingLocation {
    pub artifact_id: String,
    pub version_hash: String,
    pub path: String,
    pub state: String,
    pub last_verified_at: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, PartialEq, Eq)]
pub struct StoredTranscriptVersion {
    pub id: String,
    pub recording_artifact_id: String,
    pub recording_version_hash: String,
    pub version_hash: String,
    pub schema_version: i64,
    pub language: Option<String>,
    pub engine: String,
    pub model: String,
    pub configuration_hash: Option<String>,
    pub created_at: String,
}

#[derive(Debug, FromRow)]
pub(crate) struct TranscriptVersionRow {
    id: String,
    recording_artifact_id: String,
    recording_version_hash: String,
    version_hash: String,
    schema_version: i64,
    language: Option<String>,
    engine: String,
    model: String,
    configuration_hash: Option<String>,
    content_payload: Vec<u8>,
    created_at: String,
}

impl TranscriptVersionRow {
    fn stored(&self) -> StoredTranscriptVersion {
        StoredTranscriptVersion {
            id: self.id.clone(),
            recording_artifact_id: self.recording_artifact_id.clone(),
            recording_version_hash: self.recording_version_hash.clone(),
            version_hash: self.version_hash.clone(),
            schema_version: self.schema_version,
            language: self.language.clone(),
            engine: self.engine.clone(),
            model: self.model.clone(),
            configuration_hash: self.configuration_hash.clone(),
            created_at: self.created_at.clone(),
        }
    }
}

impl EvidenceRepository {
    #[allow(clippy::too_many_arguments)]
    pub async fn create_recording_with_version(
        pool: &SqlitePool,
        artifact_id: Uuid,
        meeting_id: &str,
        kind: RecordingArtifactKind,
        version: &RecordingVersionSpec,
        location_path: Option<&str>,
    ) -> Result<(StoredRecordingArtifact, StoredRecordingVersion), EvidenceRepositoryError> {
        version.validate()?;
        let byte_length = i64::try_from(version.byte_length).map_err(|_| {
            EvidenceValidationError::UnsafeInteger {
                field: "recording byte length",
            }
        })?;
        let duration_ms = i64::try_from(version.duration_ms).map_err(|_| {
            EvidenceValidationError::UnsafeInteger {
                field: "recording duration",
            }
        })?;
        let artifact_id_text = artifact_id.to_string();
        let now = Utc::now().to_rfc3339();
        let mut transaction = pool.begin().await?;

        let meeting_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE id = ?)")
                .bind(meeting_id)
                .fetch_one(&mut *transaction)
                .await?;
        if !meeting_exists {
            transaction.rollback().await?;
            return Err(EvidenceRepositoryError::MeetingNotFound(
                meeting_id.to_string(),
            ));
        }

        let existing_artifact = sqlx::query_as::<_, StoredRecordingArtifact>(
            "SELECT id, meeting_id, kind, created_at FROM recording_artifacts WHERE id = ? OR meeting_id = ?",
        )
        .bind(&artifact_id_text)
        .bind(meeting_id)
        .fetch_optional(&mut *transaction)
        .await?;

        let artifact = if let Some(existing) = existing_artifact {
            if existing.id != artifact_id_text
                || existing.meeting_id.as_deref() != Some(meeting_id)
                || existing.kind != kind.as_db_str()
            {
                transaction.rollback().await?;
                return Err(EvidenceRepositoryError::ArtifactIdentityConflict);
            }
            existing
        } else {
            sqlx::query(
                "INSERT INTO recording_artifacts (id, meeting_id, kind, created_at) VALUES (?, ?, ?, ?)",
            )
            .bind(&artifact_id_text)
            .bind(meeting_id)
            .bind(kind.as_db_str())
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
            StoredRecordingArtifact {
                id: artifact_id_text.clone(),
                meeting_id: Some(meeting_id.to_string()),
                kind: kind.as_db_str().to_string(),
                created_at: now.clone(),
            }
        };

        let existing_version = sqlx::query_as::<_, StoredRecordingVersion>(
            r#"
            SELECT artifact_id, version_hash, byte_length, media_type, duration_ms, created_at
            FROM recording_artifact_versions
            WHERE artifact_id = ? AND version_hash = ?
            "#,
        )
        .bind(&artifact_id_text)
        .bind(&version.version_hash)
        .fetch_optional(&mut *transaction)
        .await?;

        let stored_version = if let Some(existing) = existing_version {
            if existing.byte_length != byte_length
                || existing.media_type != version.media_type
                || existing.duration_ms != duration_ms
            {
                transaction.rollback().await?;
                return Err(EvidenceRepositoryError::RecordingVersionConflict);
            }
            existing
        } else {
            sqlx::query(
                r#"
                INSERT INTO recording_artifact_versions
                    (artifact_id, version_hash, byte_length, media_type, duration_ms, created_at)
                VALUES (?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&artifact_id_text)
            .bind(&version.version_hash)
            .bind(byte_length)
            .bind(&version.media_type)
            .bind(duration_ms)
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
            StoredRecordingVersion {
                artifact_id: artifact_id_text.clone(),
                version_hash: version.version_hash.clone(),
                byte_length,
                media_type: version.media_type.clone(),
                duration_ms,
                created_at: now.clone(),
            }
        };

        if let Some(path) = location_path {
            sqlx::query(
                r#"
                INSERT INTO recording_artifact_locations
                    (artifact_id, version_hash, path, state, last_verified_at)
                VALUES (?, ?, ?, 'available', ?)
                ON CONFLICT(artifact_id, version_hash) DO UPDATE SET
                    path = excluded.path,
                    state = 'available',
                    last_verified_at = excluded.last_verified_at
                "#,
            )
            .bind(&artifact_id_text)
            .bind(&version.version_hash)
            .bind(path)
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
        }

        transaction.commit().await?;
        Ok((artifact, stored_version))
    }

    pub async fn install_transcript_version(
        pool: &SqlitePool,
        transcript_version_id: Uuid,
        content: &TranscriptVersionContent,
    ) -> Result<StoredTranscriptVersion, EvidenceRepositoryError> {
        let mut transaction = pool.begin().await?;
        let stored = Self::install_transcript_version_in_transaction(
            &mut transaction,
            transcript_version_id,
            content,
        )
        .await?;
        transaction.commit().await?;
        Ok(stored)
    }

    pub(crate) async fn install_transcript_version_in_transaction(
        transaction: &mut Transaction<'_, Sqlite>,
        transcript_version_id: Uuid,
        content: &TranscriptVersionContent,
    ) -> Result<StoredTranscriptVersion, EvidenceRepositoryError> {
        content.validate()?;
        let version_hash = hash_transcript_version(content)?;
        let payload = canonical_transcript_payload(content)?;
        let artifact_id = content.recording_artifact_id.to_string();

        let recording_duration: Option<i64> = sqlx::query_scalar(
            r#"
            SELECT duration_ms
            FROM recording_artifact_versions
            WHERE artifact_id = ? AND version_hash = ?
            "#,
        )
        .bind(&artifact_id)
        .bind(&content.recording_version_hash)
        .fetch_optional(&mut **transaction)
        .await?;
        let Some(recording_duration) = recording_duration else {
            return Err(EvidenceRepositoryError::RecordingVersionNotFound {
                artifact_id: content.recording_artifact_id,
                version_hash: content.recording_version_hash.clone(),
            });
        };
        let allowed_end = u64::try_from(recording_duration)
            .unwrap_or_default()
            .saturating_add(1);
        if let Some((index, _)) = content
            .segments
            .iter()
            .enumerate()
            .find(|(_, segment)| segment.end_ms > allowed_end)
        {
            return Err(EvidenceRepositoryError::SegmentBeyondRecording { index });
        }

        if let Some(existing) =
            Self::find_transcript_row_by_hash(transaction, &artifact_id, &version_hash).await?
        {
            let stored = Self::verify_transcript_row(existing)?.0;
            Self::set_transcript_head(
                transaction,
                &artifact_id,
                &stored.id,
                &Utc::now().to_rfc3339(),
            )
            .await?;
            return Ok(stored);
        }

        let transcript_version_id = transcript_version_id.to_string();
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO transcript_versions
                (id, recording_artifact_id, recording_version_hash, version_hash,
                 schema_version, language, engine, model, configuration_hash,
                 content_payload, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&transcript_version_id)
        .bind(&artifact_id)
        .bind(&content.recording_version_hash)
        .bind(&version_hash)
        .bind(i64::from(content.schema_version))
        .bind(&content.language)
        .bind(&content.engine)
        .bind(&content.model)
        .bind(&content.configuration_hash)
        .bind(payload)
        .bind(&now)
        .execute(&mut **transaction)
        .await?;

        for (ordinal, segment) in content.segments.iter().enumerate() {
            sqlx::query(
                r#"
                INSERT INTO transcript_version_segments
                    (transcript_version_id, segment_id, ordinal, start_ms, end_ms,
                     text, speaker, source)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&transcript_version_id)
            .bind(segment.segment_id.to_string())
            .bind(i64::try_from(ordinal).expect("segment ordinal must fit in SQLite INTEGER"))
            .bind(i64::try_from(segment.start_ms).expect("validated milliseconds fit in i64"))
            .bind(i64::try_from(segment.end_ms).expect("validated milliseconds fit in i64"))
            .bind(&segment.text)
            .bind(&segment.speaker)
            .bind(&segment.source)
            .execute(&mut **transaction)
            .await?;
        }

        Self::set_transcript_head(transaction, &artifact_id, &transcript_version_id, &now).await?;

        Ok(StoredTranscriptVersion {
            id: transcript_version_id,
            recording_artifact_id: artifact_id,
            recording_version_hash: content.recording_version_hash.clone(),
            version_hash,
            schema_version: i64::from(content.schema_version),
            language: content.language.clone(),
            engine: content.engine.clone(),
            model: content.model.clone(),
            configuration_hash: content.configuration_hash.clone(),
            created_at: now,
        })
    }

    pub async fn get_active_transcript_version(
        pool: &SqlitePool,
        artifact_id: Uuid,
    ) -> Result<Option<(StoredTranscriptVersion, TranscriptVersionContent)>, EvidenceRepositoryError>
    {
        let row = sqlx::query_as::<_, TranscriptVersionRow>(
            r#"
            SELECT v.id, v.recording_artifact_id, v.recording_version_hash,
                   v.version_hash, v.schema_version, v.language, v.engine, v.model,
                   v.configuration_hash, v.content_payload, v.created_at
            FROM recording_transcript_heads h
            JOIN transcript_versions v ON v.id = h.transcript_version_id
            WHERE h.recording_artifact_id = ?
            "#,
        )
        .bind(artifact_id.to_string())
        .fetch_optional(pool)
        .await?;
        row.map(Self::verify_transcript_row).transpose()
    }

    pub async fn get_recording_for_meeting(
        pool: &SqlitePool,
        meeting_id: &str,
    ) -> Result<Option<StoredRecordingArtifact>, EvidenceRepositoryError> {
        Ok(sqlx::query_as::<_, StoredRecordingArtifact>(
            "SELECT id, meeting_id, kind, created_at FROM recording_artifacts WHERE meeting_id = ?",
        )
        .bind(meeting_id)
        .fetch_optional(pool)
        .await?)
    }

    pub async fn get_recording_version(
        pool: &SqlitePool,
        artifact_id: Uuid,
        version_hash: &str,
    ) -> Result<Option<StoredRecordingVersion>, EvidenceRepositoryError> {
        Ok(sqlx::query_as::<_, StoredRecordingVersion>(
            r#"
            SELECT artifact_id, version_hash, byte_length, media_type, duration_ms, created_at
            FROM recording_artifact_versions
            WHERE artifact_id = ? AND version_hash = ?
            "#,
        )
        .bind(artifact_id.to_string())
        .bind(version_hash)
        .fetch_optional(pool)
        .await?)
    }

    pub async fn get_recording_location(
        pool: &SqlitePool,
        artifact_id: Uuid,
        version_hash: &str,
    ) -> Result<Option<StoredRecordingLocation>, EvidenceRepositoryError> {
        Ok(sqlx::query_as::<_, StoredRecordingLocation>(
            r#"
            SELECT artifact_id, version_hash, path, state, last_verified_at
            FROM recording_artifact_locations
            WHERE artifact_id = ? AND version_hash = ?
            "#,
        )
        .bind(artifact_id.to_string())
        .bind(version_hash)
        .fetch_optional(pool)
        .await?)
    }

    pub async fn get_transcript_version(
        pool: &SqlitePool,
        transcript_version_id: Uuid,
    ) -> Result<Option<(StoredTranscriptVersion, TranscriptVersionContent)>, EvidenceRepositoryError>
    {
        let row = sqlx::query_as::<_, TranscriptVersionRow>(
            r#"
            SELECT id, recording_artifact_id, recording_version_hash, version_hash,
                   schema_version, language, engine, model, configuration_hash,
                   content_payload, created_at
            FROM transcript_versions
            WHERE id = ?
            "#,
        )
        .bind(transcript_version_id.to_string())
        .fetch_optional(pool)
        .await?;
        row.map(Self::verify_transcript_row).transpose()
    }

    pub async fn list_transcript_versions(
        pool: &SqlitePool,
        artifact_id: Uuid,
    ) -> Result<Vec<StoredTranscriptVersion>, EvidenceRepositoryError> {
        Ok(sqlx::query_as::<_, StoredTranscriptVersion>(
            r#"
            SELECT id, recording_artifact_id, recording_version_hash, version_hash,
                   schema_version, language, engine, model, configuration_hash, created_at
            FROM transcript_versions
            WHERE recording_artifact_id = ?
            ORDER BY created_at ASC, id ASC
            "#,
        )
        .bind(artifact_id.to_string())
        .fetch_all(pool)
        .await?)
    }

    pub(crate) async fn find_transcript_row_by_hash(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        artifact_id: &str,
        version_hash: &str,
    ) -> Result<Option<TranscriptVersionRow>, sqlx::Error> {
        sqlx::query_as::<_, TranscriptVersionRow>(
            r#"
            SELECT id, recording_artifact_id, recording_version_hash, version_hash,
                   schema_version, language, engine, model, configuration_hash,
                   content_payload, created_at
            FROM transcript_versions
            WHERE recording_artifact_id = ? AND version_hash = ?
            "#,
        )
        .bind(artifact_id)
        .bind(version_hash)
        .fetch_optional(&mut **transaction)
        .await
    }

    async fn set_transcript_head(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        artifact_id: &str,
        transcript_version_id: &str,
        updated_at: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO recording_transcript_heads
                (recording_artifact_id, transcript_version_id, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(recording_artifact_id) DO UPDATE SET
                transcript_version_id = excluded.transcript_version_id,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(artifact_id)
        .bind(transcript_version_id)
        .bind(updated_at)
        .execute(&mut **transaction)
        .await?;
        Ok(())
    }

    pub(crate) fn verify_transcript_row(
        row: TranscriptVersionRow,
    ) -> Result<(StoredTranscriptVersion, TranscriptVersionContent), EvidenceRepositoryError> {
        let content: TranscriptVersionContent = serde_json::from_slice(&row.content_payload)
            .map_err(|error| {
                EvidenceRepositoryError::Database(sqlx::Error::Protocol(format!(
                    "invalid stored transcript version JSON: {error}"
                )))
            })?;
        content.validate()?;
        if hash_transcript_version(&content)? != row.version_hash
            || content.recording_artifact_id.to_string() != row.recording_artifact_id
            || content.recording_version_hash != row.recording_version_hash
            || i64::from(content.schema_version) != row.schema_version
            || content.language != row.language
            || content.engine != row.engine
            || content.model != row.model
            || content.configuration_hash != row.configuration_hash
        {
            return Err(EvidenceRepositoryError::StoredContentIntegrity {
                kind: "transcript version",
            });
        }
        let stored = row.stored();
        Ok((stored, content))
    }
}
