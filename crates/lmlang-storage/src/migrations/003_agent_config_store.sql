-- Agent configuration persistence for dashboard/operator workflows.
-- Stores agent registration metadata and provider settings (including API key)
-- so configuration survives server restarts.

CREATE TABLE IF NOT EXISTS agent_configs (
    agent_id      TEXT PRIMARY KEY,
    name          TEXT,
    provider      TEXT,
    model         TEXT,
    api_base_url  TEXT,
    api_key       TEXT,
    system_prompt TEXT,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_agent_configs_name ON agent_configs(name);
CREATE INDEX IF NOT EXISTS idx_agent_configs_provider ON agent_configs(provider);
