CREATE TABLE IF NOT EXISTS live_assist_providers (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL,
    provider_kind TEXT NOT NULL CHECK (provider_kind IN ('deepseek', 'kimi', 'openai', 'custom')),
    endpoint TEXT NOT NULL,
    model TEXT NOT NULL,
    credential_revision INTEGER NOT NULL DEFAULT 0 CHECK (credential_revision >= 0),
    last_tested_config_hash TEXT,
    last_tested_at TEXT,
    is_active INTEGER NOT NULL DEFAULT 0 CHECK (is_active IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_live_assist_providers_one_active
    ON live_assist_providers (is_active)
    WHERE is_active = 1;
