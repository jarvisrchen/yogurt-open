-- The Voice dictionary: words as the user spells them, with an optional "sounds like".
-- `source` is added (typed on the Dictionary page) or learned (picked from a dictation). Times are unix ms.
-- `key` is the word lowercased in Rust, since NOCASE folds only ASCII, so adding "iPhone" over "Iphone" or "émile" over "Émile" respells the row.
CREATE TABLE dictionary (
  id          TEXT PRIMARY KEY,
  key         TEXT NOT NULL UNIQUE,
  word        TEXT NOT NULL,
  sounds_like TEXT,
  source      TEXT NOT NULL,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
