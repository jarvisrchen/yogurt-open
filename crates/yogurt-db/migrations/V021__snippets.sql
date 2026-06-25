-- Voice snippets: a spoken cue and the text it pastes, which may hold line breaks. Times are unix ms.
-- `key` is the cue lowercased with everything but letters and digits gone, as Rust computes it, so two cues that sound the same cannot both exist.
CREATE TABLE snippets (
  id         TEXT PRIMARY KEY,
  key        TEXT NOT NULL UNIQUE,
  cue        TEXT NOT NULL,
  text       TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
