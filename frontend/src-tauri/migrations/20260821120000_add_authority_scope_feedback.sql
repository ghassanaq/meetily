-- Advisory authority-scope policy is version-bound. Generated answers and
-- evidence excerpts are deliberately never persisted here.

CREATE TABLE IF NOT EXISTS authority_scope_policy_state (
    identity_id TEXT NOT NULL,
    identity_version_hash TEXT NOT NULL,
    mode TEXT NOT NULL DEFAULT 'offline' CHECK (mode IN ('offline', 'advisory')),
    activated_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (identity_id, identity_version_hash),
    FOREIGN KEY (identity_id, identity_version_hash)
        REFERENCES professional_identity_versions(identity_id, version_hash)
        ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS authority_scope_rule_feedback (
    identity_id TEXT NOT NULL,
    identity_version_hash TEXT NOT NULL,
    rule_id TEXT NOT NULL,
    dismissal_count INTEGER NOT NULL DEFAULT 0 CHECK (dismissal_count >= 0),
    last_dismissed_at TEXT NOT NULL,
    PRIMARY KEY (identity_id, identity_version_hash, rule_id),
    FOREIGN KEY (identity_id, identity_version_hash)
        REFERENCES professional_identity_versions(identity_id, version_hash)
        ON DELETE CASCADE
);
