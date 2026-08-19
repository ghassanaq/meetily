-- Versioned professional identity data is kept separate from Expert Profiles,
-- which remain the declarative meeting-lens layer.

CREATE TABLE IF NOT EXISTS professional_identities (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    retired_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS professional_identity_versions (
    identity_id TEXT NOT NULL,
    version_hash TEXT NOT NULL,
    seq INTEGER NOT NULL CHECK (seq > 0),
    content_payload BLOB NOT NULL,
    schema_version INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (identity_id, version_hash),
    UNIQUE (identity_id, seq),
    FOREIGN KEY (identity_id) REFERENCES professional_identities(id) ON DELETE CASCADE
);

CREATE TRIGGER IF NOT EXISTS professional_identity_versions_no_update
BEFORE UPDATE ON professional_identity_versions
BEGIN
    SELECT RAISE(ABORT, 'professional identity versions are immutable');
END;
