-- meetings has no transcript_text column, so FTS stores its own content.
-- External-content mode would try to read that missing column on MATCH.

CREATE VIRTUAL TABLE meetings_fts USING fts5(
  title,
  notes_md,
  transcript_text,
  tokenize='unicode61 remove_diacritics 2'
);

-- Seed any existing rows (no-op on fresh DB; non-empty on upgrade where
-- Phase 7 Plan 07-01 has already created rows before this migration ran).
-- `transcript_text` seeds as '' here — the row's first PATCH that touches
-- `transcript_json` will replace it with the flattened text.
INSERT INTO meetings_fts(rowid, title, notes_md, transcript_text)
SELECT rowid, title, notes_md, '' FROM meetings;

-- AFTER INSERT — index the new row with an empty transcript_text. The
-- MeetingRepo::create path writes empty transcript_json ("[]") on create,
-- so an empty transcript_text here is correct; patch() maintains the
-- column afterwards.
CREATE TRIGGER meetings_ai AFTER INSERT ON meetings BEGIN
  INSERT INTO meetings_fts(rowid, title, notes_md, transcript_text)
  VALUES (new.rowid, new.title, new.notes_md, '');
END;

-- AFTER DELETE — for a self-contained FTS5 table, a plain DELETE on
-- the virtual table works (the `'delete'` command-form is reserved for
-- contentless tables and would error here).
CREATE TRIGGER meetings_ad AFTER DELETE ON meetings BEGIN
  DELETE FROM meetings_fts WHERE rowid = old.rowid;
END;

-- AFTER UPDATE — drop+re-insert. `transcript_text` is '' here for the same
-- reason as AFTER INSERT; MeetingRepo::patch issues an explicit UPDATE on
-- meetings_fts immediately after to fill it in from the new transcript_json.
CREATE TRIGGER meetings_au AFTER UPDATE ON meetings BEGIN
  DELETE FROM meetings_fts WHERE rowid = old.rowid;
  INSERT INTO meetings_fts(rowid, title, notes_md, transcript_text)
  VALUES (new.rowid, new.title, new.notes_md, '');
END;
