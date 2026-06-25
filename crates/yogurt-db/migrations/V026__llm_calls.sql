-- One row per LLM call made for a meeting or for the chat across meetings, which the Stats page prices.
-- Voice cleanup is left out, since `dictations` and `dictation_costs` already hold its cost.
-- `meeting_id` has no foreign key, so the spend outlives a deleted meeting.
-- `kind` is 'api' for a provider billed per token, 'local' for a model on this Mac, and 'cli' for a coding-agent CLI on the user's own plan.
-- `cost_usd` is the list price when the call ran, 0 for 'local' and 'cli', and NULL when the price table lacked the model.
-- A call that failed keeps its row with no tokens and no cost.
CREATE TABLE llm_calls (
  id            INTEGER PRIMARY KEY,
  at            INTEGER NOT NULL,
  purpose       TEXT NOT NULL,
  meeting_id    TEXT,
  model         TEXT NOT NULL,
  kind          TEXT NOT NULL,
  input_tokens  INTEGER NOT NULL DEFAULT 0,
  output_tokens INTEGER NOT NULL DEFAULT 0,
  cost_usd      REAL,
  elapsed_ms    INTEGER NOT NULL,
  outcome       TEXT NOT NULL
);
CREATE INDEX llm_calls_at ON llm_calls (at);
