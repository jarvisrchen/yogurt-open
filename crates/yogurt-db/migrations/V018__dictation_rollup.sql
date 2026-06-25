-- The Voice stats, summed per local day and app, so they outlive the text rows that retention trims.
-- Numbers and a model name only, never dictated text. `app_name` is '' when the app was not known.
-- `cleaned_words` and `fillers_removed` sum over rows cleaned by the model or the rule pass.
-- The migration's Rust hook backfills both tables from `dictations`.
CREATE TABLE dictation_days (
  day              TEXT NOT NULL,
  app_name         TEXT NOT NULL,
  count            INTEGER NOT NULL DEFAULT 0,
  hands_free_count INTEGER NOT NULL DEFAULT 0,
  words            INTEGER NOT NULL DEFAULT 0,
  raw_words        INTEGER NOT NULL DEFAULT 0,
  audio_ms         INTEGER NOT NULL DEFAULT 0,
  stt_ms           INTEGER NOT NULL DEFAULT 0,
  cleanup_ms       INTEGER NOT NULL DEFAULT 0,
  off_count        INTEGER NOT NULL DEFAULT 0,
  done_count       INTEGER NOT NULL DEFAULT 0,
  rules_count      INTEGER NOT NULL DEFAULT 0,
  too_slow_count   INTEGER NOT NULL DEFAULT 0,
  cleaned_words    INTEGER NOT NULL DEFAULT 0,
  fillers_removed  INTEGER NOT NULL DEFAULT 0,
  cleanup_model    TEXT,
  cleanup_model_at INTEGER,
  PRIMARY KEY (day, app_name)
);

-- Release-to-paste waits counted per 10 ms step, for the median.
CREATE TABLE dictation_waits (
  day       TEXT NOT NULL,
  wait_10ms INTEGER NOT NULL,
  count     INTEGER NOT NULL,
  PRIMARY KEY (day, wait_10ms)
);

-- The history toggle becomes a retention: off keeps no text, on keeps 30 days, which is also the default when absent.
INSERT OR IGNORE INTO settings (key, value)
  SELECT 'general.voice_retention', 'never' FROM settings
  WHERE key = 'general.voice_history' AND value = 'false';
DELETE FROM settings WHERE key = 'general.voice_history';
