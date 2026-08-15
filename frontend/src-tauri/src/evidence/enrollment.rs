use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;
use uuid::Uuid;

use super::{
    EvidenceRepository, EvidenceRepositoryError, RecordingArtifactKind, RecordingVersionSpec,
    StoredRecordingArtifact, StoredRecordingVersion,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrolledRecording {
    pub artifact: StoredRecordingArtifact,
    pub version: StoredRecordingVersion,
}

#[derive(Debug, Error)]
pub enum RecordingEnrollmentError {
    #[error(transparent)]
    Repository(#[from] EvidenceRepositoryError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("recording hash task failed: {0}")]
    HashTask(String),
    #[error("recording changed while it was being hashed")]
    SourceChanged,
    #[error("stored recording artifact id is invalid")]
    InvalidArtifactId,
    #[error("stored recording artifact kind is invalid")]
    InvalidArtifactKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFingerprint {
    byte_length: u64,
    modified: Option<SystemTime>,
}

pub async fn enroll_recording_file(
    pool: &SqlitePool,
    meeting_id: &str,
    kind: RecordingArtifactKind,
    path: &Path,
    duration_ms: u64,
) -> Result<EnrolledRecording, RecordingEnrollmentError> {
    let path = path.to_path_buf();
    let hashed = tokio::task::spawn_blocking(move || hash_stable_file(&path))
        .await
        .map_err(|error| RecordingEnrollmentError::HashTask(error.to_string()))??;
    let existing = EvidenceRepository::get_recording_for_meeting(pool, meeting_id).await?;
    let (artifact_id, kind) = if let Some(artifact) = existing {
        let artifact_id = Uuid::parse_str(&artifact.id)
            .map_err(|_| RecordingEnrollmentError::InvalidArtifactId)?;
        let kind = match artifact.kind.as_str() {
            "captured" => RecordingArtifactKind::Captured,
            "imported" => RecordingArtifactKind::Imported,
            _ => return Err(RecordingEnrollmentError::InvalidArtifactKind),
        };
        (artifact_id, kind)
    } else {
        (Uuid::new_v4(), kind)
    };
    let media_type = media_type_for_path(&hashed.path);
    let (artifact, version) = EvidenceRepository::create_recording_with_version(
        pool,
        artifact_id,
        meeting_id,
        kind,
        &RecordingVersionSpec {
            version_hash: hashed.version_hash,
            byte_length: hashed.fingerprint.byte_length,
            media_type,
            duration_ms,
        },
        Some(&hashed.path.to_string_lossy()),
    )
    .await?;
    Ok(EnrolledRecording { artifact, version })
}

struct HashedFile {
    path: PathBuf,
    fingerprint: FileFingerprint,
    version_hash: String,
}

fn hash_stable_file(path: &Path) -> Result<HashedFile, RecordingEnrollmentError> {
    let before = fingerprint(path)?;
    let mut reader = BufReader::new(File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let after = fingerprint(path)?;
    if before != after {
        return Err(RecordingEnrollmentError::SourceChanged);
    }
    Ok(HashedFile {
        path: path.to_path_buf(),
        fingerprint: after,
        version_hash: format!("sha256:{:x}", hasher.finalize()),
    })
}

fn fingerprint(path: &Path) -> Result<FileFingerprint, std::io::Error> {
    let metadata = path.metadata()?;
    Ok(FileFingerprint {
        byte_length: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

fn media_type_for_path(path: &Path) -> String {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("wav") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        Some("m4a") | Some("mp4") => "audio/mp4",
        Some("webm") => "audio/webm",
        Some("ogg") | Some("oga") => "audio/ogg",
        Some("flac") => "audio/flac",
        _ => "application/octet-stream",
    }
    .to_owned()
}
