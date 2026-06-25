-- Legacy yogurt-server storage can create this table first, so both definitions must stay compatible.
-- SQLite permits the meetings FK before V003 creates its target table.
-- V003b rebuilds legacy tables that were created without the cascading foreign key.
-- Persist only conversation turns; system and tool roles are request-only.

CREATE TABLE IF NOT EXISTS chat_messages (
    id          TEXT PRIMARY KEY,
    meeting_id  TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    role        TEXT NOT NULL CHECK (role IN ('user', 'assistant')),
    content     TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_chat_meeting
    ON chat_messages(meeting_id, created_at);
