-- Immutable citations and their dependency graph. Model output never writes
-- these rows directly; application-owned resolvers construct every envelope.

CREATE TABLE IF NOT EXISTS evidence_citations (
    id TEXT PRIMARY KEY,
    citation_digest TEXT NOT NULL UNIQUE,
    recording_artifact_id TEXT NOT NULL,
    recording_version_hash TEXT NOT NULL,
    transcript_version_hash TEXT NOT NULL,
    locator_type TEXT NOT NULL CHECK (
        locator_type IN ('audio_timeline', 'document_passage')
    ),
    envelope_payload BLOB NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (recording_artifact_id, recording_version_hash)
        REFERENCES recording_artifact_versions(artifact_id, version_hash)
        ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_evidence_citations_recording
    ON evidence_citations(recording_artifact_id, transcript_version_hash);

CREATE TABLE IF NOT EXISTS derived_artifacts (
    id TEXT NOT NULL,
    version_hash TEXT NOT NULL,
    meeting_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('summary', 'intelligence')),
    content_payload BLOB NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (id, version_hash),
    FOREIGN KEY (meeting_id) REFERENCES meetings(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS derived_artifact_citations (
    derived_artifact_id TEXT NOT NULL,
    derived_artifact_version_hash TEXT NOT NULL,
    citation_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('supporting', 'context')),
    PRIMARY KEY (
        derived_artifact_id,
        derived_artifact_version_hash,
        citation_id,
        role
    ),
    FOREIGN KEY (derived_artifact_id, derived_artifact_version_hash)
        REFERENCES derived_artifacts(id, version_hash) ON DELETE CASCADE,
    FOREIGN KEY (citation_id) REFERENCES evidence_citations(id) ON DELETE RESTRICT
);

CREATE INDEX IF NOT EXISTS idx_derived_artifact_citations_citation
    ON derived_artifact_citations(citation_id);

CREATE TABLE IF NOT EXISTS derived_artifact_invalidations (
    derived_artifact_id TEXT NOT NULL,
    derived_artifact_version_hash TEXT NOT NULL,
    prior_citation_digest TEXT NOT NULL,
    new_transcript_version_hash TEXT NOT NULL,
    reason TEXT NOT NULL CHECK (
        reason IN ('evidence_changed', 'version_missing', 'unresolvable')
    ),
    old_span_hash TEXT NOT NULL,
    new_span_hash TEXT,
    created_at TEXT NOT NULL,
    PRIMARY KEY (
        derived_artifact_id,
        derived_artifact_version_hash,
        prior_citation_digest,
        new_transcript_version_hash
    ),
    FOREIGN KEY (derived_artifact_id, derived_artifact_version_hash)
        REFERENCES derived_artifacts(id, version_hash) ON DELETE CASCADE
);

CREATE TRIGGER IF NOT EXISTS evidence_citations_no_update
BEFORE UPDATE ON evidence_citations
BEGIN
    SELECT RAISE(ABORT, 'evidence citations are immutable');
END;

CREATE TRIGGER IF NOT EXISTS derived_artifacts_no_update
BEFORE UPDATE ON derived_artifacts
BEGIN
    SELECT RAISE(ABORT, 'derived artifact versions are immutable');
END;

CREATE TRIGGER IF NOT EXISTS derived_artifact_citations_no_update
BEFORE UPDATE ON derived_artifact_citations
BEGIN
    SELECT RAISE(ABORT, 'derived artifact citation links are immutable');
END;

CREATE TRIGGER IF NOT EXISTS derived_artifact_invalidations_no_update
BEFORE UPDATE ON derived_artifact_invalidations
BEGIN
    SELECT RAISE(ABORT, 'derived artifact invalidations are immutable');
END;
