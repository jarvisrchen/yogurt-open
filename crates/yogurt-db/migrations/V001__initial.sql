
CREATE TABLE IF NOT EXISTS settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS providers (
    id            TEXT PRIMARY KEY,           -- ulid
    name          TEXT NOT NULL,              -- e.g. "Minimax", "OpenAI"
    base_url      TEXT NOT NULL,              -- e.g. "https://api.minimax.io/v1"
    model         TEXT NOT NULL DEFAULT '',   -- e.g. "MiniMax-Text-01"
    kind          TEXT NOT NULL DEFAULT 'llm',-- 'llm' | (future: 'stt')
    is_active     INTEGER NOT NULL DEFAULT 0, -- bool; at most one row per kind
    created_at    INTEGER NOT NULL            -- unix millis
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_providers_one_active_per_kind
    ON providers(kind) WHERE is_active = 1;

CREATE INDEX IF NOT EXISTS idx_providers_kind ON providers(kind);

INSERT OR IGNORE INTO settings(key, value) VALUES
    ('general.port', '7878'),
    ('general.open_browser_on_start', 'true'),
    ('audio.input_device', '');
