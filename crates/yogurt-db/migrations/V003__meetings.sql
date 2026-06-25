-- Legacy storage may already have a meetings table without starred, created_at, or updated_at.
-- SQLite lacks ADD COLUMN IF NOT EXISTS, so migrations.rs adds missing columns after the SQL migrations.
-- Indexes on those columns must also wait for that Rust backfill.

CREATE TABLE IF NOT EXISTS meetings (
    id                TEXT PRIMARY KEY,
    title             TEXT,
    started_at        INTEGER NOT NULL DEFAULT 0,
    ended_at          INTEGER,
    notes_md          TEXT NOT NULL DEFAULT '',
    enriched_md       TEXT,
    transcript_json   TEXT NOT NULL DEFAULT '[]',
    enriched_doc_json TEXT,
    starred           INTEGER NOT NULL DEFAULT 0,
    created_at        INTEGER NOT NULL DEFAULT 0,
    updated_at        INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_meetings_started
    ON meetings(started_at DESC);


INSERT OR IGNORE INTO settings(key, value) VALUES
    ('first_run_completed', 'false');
