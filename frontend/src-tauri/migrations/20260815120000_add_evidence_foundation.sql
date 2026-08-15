-- Immutable recording and transcript foundations for evidence-linked intelligence.
-- Paths are storage hints only; artifact identity is an application-minted UUID
-- plus a content digest for the exact recording bytes.

CREATE TABLE IF NOT EXISTS recording_artifacts (
    id TEXT PRIMARY KEY,
    meeting_id TEXT UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('captured', 'imported')),
    created_at TEXT NOT NULL,
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS recording_artifact_versions (
    artifact_id TEXT NOT NULL,
    version_hash TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length >= 0),
    media_type TEXT NOT NULL,
    duration_ms INTEGER NOT NULL CHECK (duration_ms > 0),
    created_at TEXT NOT NULL,
    PRIMARY KEY (artifact_id, version_hash),
    FOREIGN KEY (artifact_id) REFERENCES recording_artifacts(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS recording_artifact_locations (
    artifact_id TEXT NOT NULL,
    version_hash TEXT NOT NULL,
    path TEXT NOT NULL,
    state TEXT NOT NULL CHECK (
        state IN ('available', 'source_missing', 'artifact_mismatch')
    ),
    last_verified_at TEXT,
    PRIMARY KEY (artifact_id, version_hash),
    FOREIGN KEY (artifact_id, version_hash)
        REFERENCES recording_artifact_versions(artifact_id, version_hash)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS transcript_versions (
    id TEXT PRIMARY KEY,
    recording_artifact_id TEXT NOT NULL,
    recording_version_hash TEXT NOT NULL,
    version_hash TEXT NOT NULL,
    schema_version INTEGER NOT NULL CHECK (schema_version > 0),
    language TEXT,
    engine TEXT NOT NULL,
    model TEXT NOT NULL,
    configuration_hash TEXT,
    content_payload BLOB NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (recording_artifact_id, version_hash),
    FOREIGN KEY (recording_artifact_id, recording_version_hash)
        REFERENCES recording_artifact_versions(artifact_id, version_hash)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_transcript_versions_recording_created
    ON transcript_versions(recording_artifact_id, created_at DESC);

CREATE TABLE IF NOT EXISTS transcript_version_segments (
    transcript_version_id TEXT NOT NULL,
    segment_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    start_ms INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK (end_ms > start_ms),
    text TEXT NOT NULL CHECK (length(trim(text)) > 0),
    speaker TEXT,
    source TEXT,
    PRIMARY KEY (transcript_version_id, segment_id),
    UNIQUE (transcript_version_id, ordinal),
    FOREIGN KEY (transcript_version_id) REFERENCES transcript_versions(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_transcript_version_segments_timeline
    ON transcript_version_segments(transcript_version_id, start_ms, end_ms, ordinal);

CREATE TABLE IF NOT EXISTS recording_transcript_heads (
    recording_artifact_id TEXT PRIMARY KEY,
    transcript_version_id TEXT NOT NULL UNIQUE,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (recording_artifact_id) REFERENCES recording_artifacts(id) ON DELETE CASCADE,
    FOREIGN KEY (transcript_version_id) REFERENCES transcript_versions(id) ON DELETE CASCADE
);

-- Version payloads and their relational segment projection are immutable. New
-- interpretations are inserted as new transcript versions and the head moves.
CREATE TRIGGER IF NOT EXISTS recording_artifact_versions_no_update
BEFORE UPDATE ON recording_artifact_versions
BEGIN
    SELECT RAISE(ABORT, 'recording artifact versions are immutable');
END;

CREATE TRIGGER IF NOT EXISTS transcript_versions_no_update
BEFORE UPDATE ON transcript_versions
BEGIN
    SELECT RAISE(ABORT, 'transcript versions are immutable');
END;

CREATE TRIGGER IF NOT EXISTS transcript_version_segments_no_update
BEFORE UPDATE ON transcript_version_segments
BEGIN
    SELECT RAISE(ABORT, 'transcript version segments are immutable');
END;
