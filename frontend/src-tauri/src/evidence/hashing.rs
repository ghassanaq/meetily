use crate::expert_profiles::hashing::{canonical_json, hash_serializable, HashError};

use super::models::TranscriptVersionContent;

const TRANSCRIPT_VERSION_HASH_DOMAIN: &[u8] = b"meeting-assistant-transcript-version-v1\0";

pub fn canonical_transcript_payload(
    transcript: &TranscriptVersionContent,
) -> Result<Vec<u8>, HashError> {
    canonical_json(transcript)
}

pub fn hash_transcript_version(transcript: &TranscriptVersionContent) -> Result<String, HashError> {
    hash_serializable(TRANSCRIPT_VERSION_HASH_DOMAIN, transcript)
}
