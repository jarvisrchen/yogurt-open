-- One row per pasted Voice dictation. A cancelled or empty dictation leaves no row.
-- Times are unix ms. `app_*` is the frontmost app read right before the Cmd+V.
-- `cleanup` is one of off | done | rules | too_slow.
CREATE TABLE dictations (
  id            TEXT PRIMARY KEY,
  created_at    INTEGER NOT NULL,
  app_bundle_id TEXT,
  app_name      TEXT,
  hands_free    INTEGER NOT NULL,
  audio_ms      INTEGER NOT NULL,
  raw_text      TEXT NOT NULL,
  text          TEXT NOT NULL,
  raw_words     INTEGER NOT NULL,
  words         INTEGER NOT NULL,
  stt_engine    TEXT NOT NULL,
  stt_ms        INTEGER NOT NULL,
  cleanup       TEXT NOT NULL,
  cleanup_model TEXT,
  cleanup_ms    INTEGER
);
CREATE INDEX dictations_created_at ON dictations (created_at);

-- Rows are never updated, so insert and delete triggers keep the index whole.
CREATE VIRTUAL TABLE dictations_fts USING fts5(
  text,
  tokenize='unicode61 remove_diacritics 2'
);

CREATE TRIGGER dictations_ai AFTER INSERT ON dictations BEGIN
  INSERT INTO dictations_fts(rowid, text) VALUES (new.rowid, new.text);
END;

CREATE TRIGGER dictations_ad AFTER DELETE ON dictations BEGIN
  DELETE FROM dictations_fts WHERE rowid = old.rowid;
END;
