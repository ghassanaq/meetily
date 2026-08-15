use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, SqlitePool};
use thiserror::Error;
use uuid::Uuid;

use crate::expert_profiles::hashing::{canonical_json, HashError};

use super::{
    build_audio_citation_for_interval, hash_citation_envelope, resolve_current_citation,
    CitationEnvelope, CitationResolutionStatus, EvidenceLocator, EvidenceRepository,
    EvidenceRepositoryError, RecordingSourceState, StoredTranscriptVersion,
    TranscriptVersionContent,
};

const DERIVED_ARTIFACT_HASH_DOMAIN: &[u8] = b"meeting-assistant-derived-artifact-v1\0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DerivedArtifactKind {
    Summary,
    Intelligence,
}

impl DerivedArtifactKind {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Intelligence => "intelligence",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CitationRole {
    Supporting,
    Context,
}

impl CitationRole {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Supporting => "supporting",
            Self::Context => "context",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DerivedArtifactSpec {
    pub id: Uuid,
    pub version_hash: String,
    pub meeting_id: String,
    pub kind: DerivedArtifactKind,
    pub content_payload: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CitationDependency {
    pub citation_id: Uuid,
    pub role: CitationRole,
}

#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct StoredEvidenceCitation {
    pub id: String,
    pub citation_digest: String,
    pub recording_artifact_id: String,
    pub recording_version_hash: String,
    pub transcript_version_hash: String,
    pub locator_type: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptInstallOutcome {
    pub transcript: StoredTranscriptVersion,
    pub invalidations_created: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LegacyTranscriptProjection {
    pub id: String,
    pub text: String,
    pub timestamp: String,
    pub audio_start_time: Option<f64>,
    pub audio_end_time: Option<f64>,
    pub duration: Option<f64>,
}

#[derive(Debug, Error)]
pub enum ProvenanceRepositoryError {
    #[error(transparent)]
    Evidence(#[from] EvidenceRepositoryError),
    #[error(transparent)]
    Hash(#[from] HashError),
    #[error(transparent)]
    Database(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("a citation can only be persisted after current evidence verification")]
    CitationNotVerified,
    #[error("stored citation content does not match its digest")]
    CitationIntegrity,
    #[error("derived artifact content does not match its version hash")]
    DerivedArtifactIntegrity,
    #[error("meeting {0} was not found")]
    MeetingNotFound(String),
    #[error("citation {0} was not found")]
    CitationNotFound(Uuid),
}

#[derive(Debug, FromRow)]
struct StoredCitationRow {
    id: String,
    citation_digest: String,
    recording_artifact_id: String,
    recording_version_hash: String,
    transcript_version_hash: String,
    locator_type: String,
    envelope_payload: Vec<u8>,
    created_at: String,
}

impl StoredCitationRow {
    fn public(&self) -> StoredEvidenceCitation {
        StoredEvidenceCitation {
            id: self.id.clone(),
            citation_digest: self.citation_digest.clone(),
            recording_artifact_id: self.recording_artifact_id.clone(),
            recording_version_hash: self.recording_version_hash.clone(),
            transcript_version_hash: self.transcript_version_hash.clone(),
            locator_type: self.locator_type.clone(),
            created_at: self.created_at.clone(),
        }
    }
}

#[derive(Debug, FromRow)]
struct DependentCitationRow {
    derived_artifact_id: String,
    derived_artifact_version_hash: String,
    citation_digest: String,
    envelope_payload: Vec<u8>,
}

pub struct ProvenanceRepository;

impl ProvenanceRepository {
    #[allow(clippy::too_many_arguments)]
    pub async fn persist_verified_citation(
        pool: &SqlitePool,
        citation: &CitationEnvelope,
        recording_duration_ms: u64,
        source_state: &RecordingSourceState,
        pinned_transcript: &TranscriptVersionContent,
        active_transcript: &TranscriptVersionContent,
    ) -> Result<StoredEvidenceCitation, ProvenanceRepositoryError> {
        if resolve_current_citation(
            citation,
            recording_duration_ms,
            source_state,
            Some(pinned_transcript),
            Some(active_transcript),
        ) != CitationResolutionStatus::Verified
        {
            return Err(ProvenanceRepositoryError::CitationNotVerified);
        }

        let digest = hash_citation_envelope(citation)?;
        let payload = canonical_json(citation)?;
        let locator_type = match citation.locator {
            EvidenceLocator::AudioTimeline { .. } => "audio_timeline",
            EvidenceLocator::DocumentPassage { .. } => "document_passage",
        };
        let now = Utc::now().to_rfc3339();
        let mut transaction = pool.begin().await?;

        if let Some(existing) = Self::find_citation_by_digest(&mut transaction, &digest).await? {
            Self::verify_citation_row(&existing)?;
            transaction.commit().await?;
            return Ok(existing.public());
        }

        sqlx::query(
            r#"
            INSERT INTO evidence_citations
                (id, citation_digest, recording_artifact_id, recording_version_hash,
                 transcript_version_hash, locator_type, envelope_payload, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(citation.citation_id.to_string())
        .bind(&digest)
        .bind(citation.artifact.id.to_string())
        .bind(&citation.artifact.version_hash)
        .bind(&citation.resolution.transcript_version_hash)
        .bind(locator_type)
        .bind(payload)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;

        Ok(StoredEvidenceCitation {
            id: citation.citation_id.to_string(),
            citation_digest: digest,
            recording_artifact_id: citation.artifact.id.to_string(),
            recording_version_hash: citation.artifact.version_hash.clone(),
            transcript_version_hash: citation.resolution.transcript_version_hash.clone(),
            locator_type: locator_type.to_owned(),
            created_at: now,
        })
    }

    pub async fn register_derived_artifact(
        pool: &SqlitePool,
        artifact: &DerivedArtifactSpec,
        citations: &[CitationDependency],
    ) -> Result<(), ProvenanceRepositoryError> {
        if hash_derived_artifact_payload(&artifact.content_payload) != artifact.version_hash {
            return Err(ProvenanceRepositoryError::DerivedArtifactIntegrity);
        }
        let mut transaction = pool.begin().await?;
        let meeting_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM meetings WHERE id = ?)")
                .bind(&artifact.meeting_id)
                .fetch_one(&mut *transaction)
                .await?;
        if !meeting_exists {
            return Err(ProvenanceRepositoryError::MeetingNotFound(
                artifact.meeting_id.clone(),
            ));
        }

        for dependency in citations {
            let citation_exists: bool =
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM evidence_citations WHERE id = ?)")
                    .bind(dependency.citation_id.to_string())
                    .fetch_one(&mut *transaction)
                    .await?;
            if !citation_exists {
                return Err(ProvenanceRepositoryError::CitationNotFound(
                    dependency.citation_id,
                ));
            }
        }

        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO derived_artifacts
                (id, version_hash, meeting_id, kind, content_payload, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(artifact.id.to_string())
        .bind(&artifact.version_hash)
        .bind(&artifact.meeting_id)
        .bind(artifact.kind.as_db_str())
        .bind(&artifact.content_payload)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        for dependency in citations {
            sqlx::query(
                r#"
                INSERT INTO derived_artifact_citations
                    (derived_artifact_id, derived_artifact_version_hash, citation_id, role)
                VALUES (?, ?, ?, ?)
                "#,
            )
            .bind(artifact.id.to_string())
            .bind(&artifact.version_hash)
            .bind(dependency.citation_id.to_string())
            .bind(dependency.role.as_db_str())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn install_transcript_version_and_invalidate(
        pool: &SqlitePool,
        transcript_version_id: Uuid,
        content: &TranscriptVersionContent,
    ) -> Result<TranscriptInstallOutcome, ProvenanceRepositoryError> {
        Self::install_transcript_transaction(pool, transcript_version_id, content, None).await
    }

    pub async fn install_retranscription_and_invalidate(
        pool: &SqlitePool,
        transcript_version_id: Uuid,
        content: &TranscriptVersionContent,
        meeting_id: &str,
        legacy_segments: &[LegacyTranscriptProjection],
    ) -> Result<TranscriptInstallOutcome, ProvenanceRepositoryError> {
        Self::install_transcript_transaction(
            pool,
            transcript_version_id,
            content,
            Some((meeting_id, legacy_segments)),
        )
        .await
    }

    async fn install_transcript_transaction(
        pool: &SqlitePool,
        transcript_version_id: Uuid,
        content: &TranscriptVersionContent,
        legacy_projection: Option<(&str, &[LegacyTranscriptProjection])>,
    ) -> Result<TranscriptInstallOutcome, ProvenanceRepositoryError> {
        let mut transaction = pool.begin().await?;
        let transcript = EvidenceRepository::install_transcript_version_in_transaction(
            &mut transaction,
            transcript_version_id,
            content,
        )
        .await?;

        let duration_ms: i64 = sqlx::query_scalar(
            r#"
            SELECT duration_ms FROM recording_artifact_versions
            WHERE artifact_id = ? AND version_hash = ?
            "#,
        )
        .bind(content.recording_artifact_id.to_string())
        .bind(&content.recording_version_hash)
        .fetch_one(&mut *transaction)
        .await?;
        let duration_ms = u64::try_from(duration_ms).unwrap_or_default();
        let dependencies = sqlx::query_as::<_, DependentCitationRow>(
            r#"
            SELECT DISTINCT
                d.derived_artifact_id,
                d.derived_artifact_version_hash,
                c.citation_digest,
                c.envelope_payload
            FROM derived_artifact_citations d
            JOIN evidence_citations c ON c.id = d.citation_id
            WHERE c.recording_artifact_id = ?
            "#,
        )
        .bind(content.recording_artifact_id.to_string())
        .fetch_all(&mut *transaction)
        .await?;

        let source_state = RecordingSourceState::Available {
            actual_version_hash: content.recording_version_hash.clone(),
        };
        let now = Utc::now().to_rfc3339();
        let mut invalidations_created = 0;
        for dependency in dependencies {
            let citation: CitationEnvelope = serde_json::from_slice(&dependency.envelope_payload)?;
            if hash_citation_envelope(&citation)? != dependency.citation_digest {
                return Err(ProvenanceRepositoryError::CitationIntegrity);
            }
            let Some(pinned_row) = EvidenceRepository::find_transcript_row_by_hash(
                &mut transaction,
                &content.recording_artifact_id.to_string(),
                &citation.resolution.transcript_version_hash,
            )
            .await?
            else {
                return Err(ProvenanceRepositoryError::CitationIntegrity);
            };
            let (_, pinned) = EvidenceRepository::verify_transcript_row(pinned_row)?;
            let status = resolve_current_citation(
                &citation,
                duration_ms,
                &source_state,
                Some(&pinned),
                Some(content),
            );
            let reason = match status {
                CitationResolutionStatus::EvidenceChanged => "evidence_changed",
                CitationResolutionStatus::VersionMissing => "version_missing",
                CitationResolutionStatus::Unresolvable => "unresolvable",
                _ => continue,
            };
            let new_span_hash = match citation.locator {
                EvidenceLocator::AudioTimeline { start_ms, end_ms } => {
                    build_audio_citation_for_interval(
                        Uuid::new_v4(),
                        content,
                        duration_ms,
                        start_ms,
                        end_ms,
                    )
                    .ok()
                    .map(|resolved| resolved.snapshot.content_hash)
                }
                EvidenceLocator::DocumentPassage { .. } => None,
            };
            let result = sqlx::query(
                r#"
                INSERT OR IGNORE INTO derived_artifact_invalidations
                    (derived_artifact_id, derived_artifact_version_hash,
                     prior_citation_digest, new_transcript_version_hash, reason,
                     old_span_hash, new_span_hash, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&dependency.derived_artifact_id)
            .bind(&dependency.derived_artifact_version_hash)
            .bind(&dependency.citation_digest)
            .bind(&transcript.version_hash)
            .bind(reason)
            .bind(&citation.snapshot.content_hash)
            .bind(new_span_hash)
            .bind(&now)
            .execute(&mut *transaction)
            .await?;
            invalidations_created += result.rows_affected();
        }

        if let Some((meeting_id, segments)) = legacy_projection {
            sqlx::query("DELETE FROM transcripts WHERE meeting_id = ?")
                .bind(meeting_id)
                .execute(&mut *transaction)
                .await?;
            for segment in segments {
                sqlx::query(
                    r#"
                    INSERT INTO transcripts
                        (id, meeting_id, transcript, timestamp, audio_start_time,
                         audio_end_time, duration)
                    VALUES (?, ?, ?, ?, ?, ?, ?)
                    "#,
                )
                .bind(&segment.id)
                .bind(meeting_id)
                .bind(&segment.text)
                .bind(&segment.timestamp)
                .bind(segment.audio_start_time)
                .bind(segment.audio_end_time)
                .bind(segment.duration)
                .execute(&mut *transaction)
                .await?;
            }
        }

        transaction.commit().await?;
        Ok(TranscriptInstallOutcome {
            transcript,
            invalidations_created,
        })
    }

    async fn find_citation_by_digest(
        transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        digest: &str,
    ) -> Result<Option<StoredCitationRow>, sqlx::Error> {
        sqlx::query_as::<_, StoredCitationRow>(
            r#"
            SELECT id, citation_digest, recording_artifact_id, recording_version_hash,
                   transcript_version_hash, locator_type, envelope_payload, created_at
            FROM evidence_citations WHERE citation_digest = ?
            "#,
        )
        .bind(digest)
        .fetch_optional(&mut **transaction)
        .await
    }

    fn verify_citation_row(
        row: &StoredCitationRow,
    ) -> Result<CitationEnvelope, ProvenanceRepositoryError> {
        let citation: CitationEnvelope = serde_json::from_slice(&row.envelope_payload)?;
        if hash_citation_envelope(&citation)? != row.citation_digest
            || citation.citation_id.to_string() != row.id
            || citation.artifact.id.to_string() != row.recording_artifact_id
            || citation.artifact.version_hash != row.recording_version_hash
            || citation.resolution.transcript_version_hash != row.transcript_version_hash
        {
            return Err(ProvenanceRepositoryError::CitationIntegrity);
        }
        Ok(citation)
    }
}

pub fn hash_derived_artifact_payload(payload: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DERIVED_ARTIFACT_HASH_DOMAIN);
    hasher.update(payload);
    format!("sha256:{:x}", hasher.finalize())
}
