-- What each cleanup used and cost, priced at the model's list price when it ran, so a later price change leaves past rows as they were.
-- All three are NULL when no model ran, and on rows from before this. `cost_usd` is NULL with tokens set when the price table lacked the model.
ALTER TABLE dictations ADD COLUMN input_tokens INTEGER;
ALTER TABLE dictations ADD COLUMN output_tokens INTEGER;
ALTER TABLE dictations ADD COLUMN cost_usd REAL;

-- Snippet pastes, which the speaking pace and time saved leave out, since a short cue pastes many words.
-- `snippet_wait_ms` is their transcription plus cleanup time, the part of `stt_ms + cleanup_ms` they add.
ALTER TABLE dictation_days ADD COLUMN snippet_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE dictation_days ADD COLUMN snippet_words INTEGER NOT NULL DEFAULT 0;
ALTER TABLE dictation_days ADD COLUMN snippet_audio_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE dictation_days ADD COLUMN snippet_wait_ms INTEGER NOT NULL DEFAULT 0;

-- The cleanup cost summed per local day and model, so it outlives the text rows that retention trims.
-- `usd` sums the priced dictations, and `unpriced_count` counts the ones whose model the price table lacked.
CREATE TABLE dictation_costs (
  day            TEXT NOT NULL,
  model          TEXT NOT NULL,
  count          INTEGER NOT NULL DEFAULT 0,
  input_tokens   INTEGER NOT NULL DEFAULT 0,
  output_tokens  INTEGER NOT NULL DEFAULT 0,
  usd            REAL NOT NULL DEFAULT 0,
  unpriced_count INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (day, model)
);
