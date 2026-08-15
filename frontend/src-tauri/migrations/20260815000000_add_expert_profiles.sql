-- Declarative Expert Profiles and embedded Meeting Playbooks.
-- Payload columns use the personal local-storage baseline documented in
-- EXPERT_PROFILES_DESIGN.md.

CREATE TABLE IF NOT EXISTS expert_profiles (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    retired_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS expert_profile_versions (
    profile_id TEXT NOT NULL,
    version_hash TEXT NOT NULL,
    seq INTEGER NOT NULL CHECK (seq > 0),
    content_payload BLOB NOT NULL,
    schema_version INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (profile_id, version_hash),
    UNIQUE (profile_id, seq),
    FOREIGN KEY (profile_id) REFERENCES expert_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS expert_eval_plans (
    id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    content_payload BLOB NOT NULL,
    schema_version INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (id, content_hash),
    FOREIGN KEY (profile_id) REFERENCES expert_profiles(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_expert_eval_plans_profile
    ON expert_eval_plans(profile_id, created_at DESC);

CREATE TABLE IF NOT EXISTS expert_eval_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id TEXT NOT NULL,
    candidate_capability_hash TEXT NOT NULL,
    baseline_capability_hash TEXT,
    eval_plan_hash TEXT NOT NULL,
    safety_gate_version TEXT NOT NULL,
    model_binding_hash TEXT NOT NULL,
    adjudicator_binding_hash TEXT,
    results_payload BLOB NOT NULL,
    outcome TEXT NOT NULL CHECK (
        outcome IN ('pass', 'fail', 'rejected', 'inconclusive', 'baseline_missing')
    ),
    created_at TEXT NOT NULL,
    FOREIGN KEY (profile_id) REFERENCES expert_profiles(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS expert_profile_activations (
    profile_id TEXT PRIMARY KEY,
    profile_version_hash TEXT NOT NULL,
    capability_revision_hash TEXT NOT NULL,
    model_binding_payload BLOB NOT NULL,
    eval_run_id INTEGER NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('active', 'superseded')),
    superseded_reason TEXT,
    activated_at TEXT NOT NULL,
    FOREIGN KEY (profile_id, profile_version_hash)
        REFERENCES expert_profile_versions(profile_id, version_hash),
    FOREIGN KEY (eval_run_id) REFERENCES expert_eval_runs(id)
);

CREATE TABLE IF NOT EXISTS expert_activation_journal (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    profile_id TEXT NOT NULL,
    capability_revision_hash TEXT NOT NULL,
    previous_capability_hash TEXT,
    eval_run_id INTEGER,
    action TEXT NOT NULL CHECK (
        action IN ('activate', 'supersede', 'retire', 'restore', 'delete')
    ),
    created_at TEXT NOT NULL
);

-- Journal identifiers deliberately have no foreign keys. Profile deletion removes
-- mutable/content-bearing rows while retaining this immutable hash-only audit trail.

-- Content-addressed versions, plans, eval results, and journal rows are
-- immutable. New information is inserted as a new row.
CREATE TRIGGER IF NOT EXISTS expert_profile_versions_no_update
BEFORE UPDATE ON expert_profile_versions
BEGIN
    SELECT RAISE(ABORT, 'expert profile versions are immutable');
END;

CREATE TRIGGER IF NOT EXISTS expert_eval_plans_no_update
BEFORE UPDATE ON expert_eval_plans
BEGIN
    SELECT RAISE(ABORT, 'expert evaluation plans are immutable');
END;

CREATE TRIGGER IF NOT EXISTS expert_eval_runs_no_update
BEFORE UPDATE ON expert_eval_runs
BEGIN
    SELECT RAISE(ABORT, 'expert evaluation runs are immutable');
END;

CREATE TRIGGER IF NOT EXISTS expert_activation_journal_no_update
BEFORE UPDATE ON expert_activation_journal
BEGIN
    SELECT RAISE(ABORT, 'expert activation journal rows are immutable');
END;
