//! Migrations are compiled in via `include_str!` so the single static binary ships no migration files.

use anyhow::Result;
use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../migrations/V001__initial.sql")),
        // Also created by `yogurt-server::storage::migrations` (`IF NOT EXISTS`). Whichever runner fires first wins and the columns agree.
        M::up(include_str!("../migrations/V002__chat_messages.sql")),
        // `CREATE TABLE IF NOT EXISTS` is a no-op on DBs that already have the older `meetings` table,
        // so [`backfill_phase7_meeting_columns`] adds `starred`, `created_at`, `updated_at` right after.
        M::up(include_str!("../migrations/V003__meetings.sql")),
        // Rebuild `chat_messages` so its FK on `meetings(id)` is ON DELETE CASCADE. The older storage migration declared it without the cascade,
        // and V002's `IF NOT EXISTS` kept that when storage migrated first.
        M::up(include_str!(
            "../migrations/V003b__chat_messages_cascade.sql"
        )),
        M::up(include_str!("../migrations/V004__meetings_fts.sql")),
        M::up(include_str!("../migrations/V005__stt_provider_model.sql")),
        M::up(include_str!("../migrations/V006__meetings_stt_engine.sql")),
        M::up(include_str!("../migrations/V007__labels.sql")),
        M::up(include_str!("../migrations/V008__meetings_llm_model.sql")),
        M::up(include_str!("../migrations/V009__providers_adapter.sql")),
        M::up(include_str!("../migrations/V010__providers_cli_model.sql")),
        M::up(include_str!("../migrations/V011__meetings_template.sql")),
        M::up(include_str!(
            "../migrations/V012__labels_position_updated_at.sql"
        )),
        M::up(include_str!(
            "../migrations/V013__meetings_fts_enriched.sql"
        )),
        M::up(include_str!("../migrations/V014__meetings_cloud_hash.sql")),
        M::up(include_str!("../migrations/V015__meetings_source.sql")),
        M::up(include_str!("../migrations/V016__meetings_archived.sql")),
        M::up(include_str!("../migrations/V017__dictations.sql")),
        // `dictation_days` and `dictation_waits`: the Voice stats rollup, backfilled in Rust because the filler count is Rust.
        // Also moves `general.voice_history` to `general.voice_retention`.
        M::up_with_hook(
            include_str!("../migrations/V018__dictation_rollup.sql"),
            |tx: &rusqlite::Transaction| Ok(crate::dictations::backfill_rollup(tx)?),
        ),
        M::up(include_str!("../migrations/V019__stt_model_lineup.sql")),
        M::up(include_str!("../migrations/V020__dictionary.sql")),
        M::up(include_str!("../migrations/V021__snippets.sql")),
        M::up(include_str!("../migrations/V022__dictation_cost.sql")),
        M::up(include_str!(
            "../migrations/V023__minimax_retired_models.sql"
        )),
        M::up(include_str!(
            "../migrations/V024__meetings_previous_enhance.sql"
        )),
        M::up(include_str!(
            "../migrations/V025__meetings_title_suggestions.sql"
        )),
        M::up(include_str!("../migrations/V026__llm_calls.sql")),
        M::up(include_str!(
            "../migrations/V027__parakeet_ultra_default.sql"
        )),
    ])
}

/// Idempotent. After V003, adds `starred`, `created_at`, `updated_at` to `meetings` where an older storage migration created the table without them.
/// SQLite has no `ADD COLUMN IF NOT EXISTS`, so the existence check happens in Rust.
pub fn run(conn: &mut Connection) -> Result<()> {
    migrations().to_latest(conn)?;
    backfill_phase7_meeting_columns(conn)?;
    Ok(())
}

fn backfill_phase7_meeting_columns(conn: &Connection) -> Result<()> {
    let existing = existing_columns(conn, "meetings")?;
    // Defaults match the V003 CREATE TABLE so fresh and back-filled DBs end up with identical schemas.
    if !existing.contains(&"starred".to_string()) {
        conn.execute_batch("ALTER TABLE meetings ADD COLUMN starred INTEGER NOT NULL DEFAULT 0")?;
    }
    if !existing.contains(&"created_at".to_string()) {
        conn.execute_batch(
            "ALTER TABLE meetings ADD COLUMN created_at INTEGER NOT NULL DEFAULT 0",
        )?;
    }
    if !existing.contains(&"updated_at".to_string()) {
        conn.execute_batch(
            "ALTER TABLE meetings ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0",
        )?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_meetings_starred \
         ON meetings(starred) WHERE starred = 1",
    )?;
    // `notes_md` / `transcript_json` were nullable in the older schema. No backfill is needed: MeetingRepo always writes a value and coerces NULL to "" / "[]" at read time.
    Ok(())
}

fn existing_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let sql = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(1))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::OptionalExtension;

    #[test]
    fn v015_adds_source_columns_and_a_unique_index_for_imports_only() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations().to_version(&mut conn, 15).unwrap();
        conn.execute_batch(
            "INSERT INTO meetings (id, title, started_at) VALUES ('a', 'Old', 0), ('b', 'Old', 0);",
        )
        .unwrap();
        migrations().to_latest(&mut conn).unwrap();
        let ins = |id: &str, src: Option<&str>, sid: Option<&str>| {
            conn.execute(
                "INSERT INTO meetings (id, title, started_at, source, source_id) VALUES (?1, 't', 0, ?2, ?3)",
                rusqlite::params![id, src, sid],
            )
        };
        assert!(ins("c", Some("granola"), Some("not_1")).is_ok());
        assert!(ins("d", Some("granola"), Some("not_1")).is_err());
        assert!(ins("e", Some("granola"), Some("not_2")).is_ok());
        assert!(ins("f", None, None).is_ok());
        let native: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM meetings WHERE source IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(native, 3);
    }

    #[test]
    fn v013_backfills_enriched_and_transcript_text_for_existing_rows() {
        let mut conn = Connection::open_in_memory().unwrap();
        // Version 13 is V012: V003b counts as its own step.
        migrations().to_version(&mut conn, 13).unwrap();
        conn.execute_batch(
            r#"INSERT INTO meetings (id, title, started_at, notes_md, enriched_md, transcript_json)
               VALUES ('a', 'Old', 0, '', '## Pricing tiers', '[{"text":"homebrew"},{"transcript":"legacy"},"bare"]'),
                      ('b', 'Bad', 0, '', NULL, 'not json');"#,
        )
        .unwrap();
        migrations().to_latest(&mut conn).unwrap();
        let hit = |q: &str| -> Vec<String> {
            let mut stmt = conn
                .prepare("SELECT m.id FROM meetings m JOIN meetings_fts f ON f.rowid = m.rowid WHERE meetings_fts MATCH ?1")
                .unwrap();
            stmt.query_map([q], |r| r.get(0))
                .unwrap()
                .map(|r| r.unwrap())
                .collect()
        };
        assert_eq!(hit("pricing"), ["a"]);
        assert_eq!(hit("homebrew"), ["a"]);
        assert_eq!(hit("legacy"), ["a"]);
        assert_eq!(hit("bad"), ["b"]);
    }

    /// `created_at, app_name, hands_free, audio_ms, raw_text, text, cleanup, cleanup_model, stt_ms, cleanup_ms`.
    type V017Row<'a> = (
        i64,
        Option<&'a str>,
        bool,
        i64,
        &'a str,
        &'a str,
        &'a str,
        Option<&'a str>,
        i64,
        Option<i64>,
    );
    /// `day, app_name`, the numbers in column order, `cleanup_model, cleanup_model_at`.
    type DayRow = (String, String, Vec<i64>, Option<String>, Option<i64>);

    fn v017_with_dictations(rows: &[V017Row]) -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        // V003b makes V017 the 18th migration.
        migrations().to_version(&mut conn, 18).unwrap();
        for (i, (at, app, hf, audio, raw, text, cleanup, model, stt, cms)) in
            rows.iter().enumerate()
        {
            conn.execute(
                "INSERT INTO dictations (id, created_at, app_name, hands_free, audio_ms, raw_text, text, raw_words, words, \
                   stt_engine, stt_ms, cleanup, cleanup_model, cleanup_ms) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'local', ?10, ?11, ?12, ?13)",
                rusqlite::params![
                    i.to_string(), at, app, hf, audio, raw, text,
                    crate::dictations::word_count(raw), crate::dictations::word_count(text),
                    stt, cleanup, model, cms
                ],
            )
            .unwrap();
        }
        conn
    }

    #[test]
    fn v018_backfills_the_rollup_from_saved_dictations() {
        use chrono::TimeZone;
        let at = |d, h| {
            chrono::Local
                .with_ymd_and_hms(2026, 11, d, h, 0, 0)
                .unwrap()
                .timestamp_millis()
        };
        let mut conn = v017_with_dictations(&[
            (
                at(6, 10),
                Some("Slack"),
                false,
                6_000,
                "um so we we ship",
                "So we ship.",
                "done",
                Some("qwen"),
                300,
                Some(200),
            ),
            (
                at(6, 11),
                Some("Slack"),
                true,
                2_000,
                "uh hi",
                "Hi.",
                "rules",
                None,
                250,
                None,
            ),
            (
                at(6, 12),
                None,
                false,
                3_000,
                "one two three",
                "one two three",
                "off",
                None,
                300,
                None,
            ),
            (
                at(6, 13),
                Some("Slack"),
                false,
                1_000,
                "x y",
                "x y",
                "too_slow",
                Some("qwen"),
                300,
                Some(1_500),
            ),
            (
                at(7, 9),
                Some("Slack"),
                false,
                4_000,
                "a b c",
                "A b c.",
                "done",
                Some("gemini"),
                400,
                Some(100),
            ),
        ]);
        migrations().to_latest(&mut conn).unwrap();

        let days: Vec<DayRow> = conn
            .prepare(
                "SELECT day, app_name, count, hands_free_count, words, raw_words, audio_ms, stt_ms, cleanup_ms, \
                   off_count, done_count, rules_count, too_slow_count, cleaned_words, fillers_removed, \
                   cleanup_model, cleanup_model_at \
                 FROM dictation_days ORDER BY day, app_name",
            )
            .unwrap()
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    (2..15).map(|i| r.get(i)).collect::<rusqlite::Result<_>>()?,
                    r.get(15)?,
                    r.get(16)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            days,
            [
                (
                    "2026-11-06".into(),
                    "".into(),
                    vec![1, 0, 3, 3, 3_000, 300, 0, 1, 0, 0, 0, 0, 0],
                    None,
                    None
                ),
                (
                    "2026-11-06".into(),
                    "Slack".into(),
                    vec![3, 1, 6, 9, 9_000, 850, 1_700, 0, 1, 1, 1, 3, 2],
                    Some("qwen".into()),
                    Some(at(6, 10)),
                ),
                (
                    "2026-11-07".into(),
                    "Slack".into(),
                    vec![1, 0, 3, 3, 4_000, 400, 100, 0, 1, 0, 0, 0, 0],
                    Some("gemini".into()),
                    Some(at(7, 9)),
                ),
            ]
        );
        let waits: Vec<(String, i64, i64)> = conn
            .prepare("SELECT day, wait_10ms, count FROM dictation_waits ORDER BY day, wait_10ms")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            waits,
            [
                ("2026-11-06".into(), 25, 1),
                ("2026-11-06".into(), 30, 1),
                ("2026-11-06".into(), 50, 1),
                ("2026-11-06".into(), 180, 1),
                ("2026-11-07".into(), 50, 1),
            ]
        );
    }

    #[test]
    fn v018_turns_the_history_toggle_into_a_retention() {
        let retention_after = |history: Option<&str>| {
            let mut conn = v017_with_dictations(&[]);
            if let Some(v) = history {
                conn.execute(
                    "INSERT INTO settings (key, value) VALUES ('general.voice_history', ?1)",
                    [v],
                )
                .unwrap();
            }
            migrations().to_latest(&mut conn).unwrap();
            let kv = |k: &str| -> Option<String> {
                conn.query_row("SELECT value FROM settings WHERE key = ?1", [k], |r| {
                    r.get(0)
                })
                .optional()
                .unwrap()
            };
            assert_eq!(kv("general.voice_history"), None);
            kv("general.voice_retention")
        };
        assert_eq!(retention_after(Some("false")).as_deref(), Some("never"));
        assert_eq!(
            retention_after(Some("true")),
            None,
            "absent reads as the 30-day default"
        );
        assert_eq!(retention_after(None), None);
    }

    #[test]
    fn v019_moves_removed_models_and_defaults_new_installs_to_parakeet() {
        let after_with = |welcomed: &str, provider: &str, stt: &str, voice: Option<&str>| {
            let mut conn = Connection::open_in_memory().unwrap();
            // V003b makes V018 the 19th migration.
            migrations().to_version(&mut conn, 19).unwrap();
            let set = |k: &str, v: &str| {
                conn.execute(
                    "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                    [k, v],
                )
                .unwrap();
            };
            set("first_run_completed", welcomed);
            set("stt.provider", provider);
            set("stt.model", stt);
            if let Some(v) = voice {
                set("general.voice_stt_model", v);
            }
            migrations().to_version(&mut conn, 20).unwrap();
            let kv = |k: &str| -> Option<String> {
                conn.query_row("SELECT value FROM settings WHERE key = ?1", [k], |r| {
                    r.get(0)
                })
                .optional()
                .unwrap()
            };
            (kv("stt.model").unwrap(), kv("general.voice_stt_model"))
        };
        let after = |welcomed: &str, stt: &str, voice: Option<&str>| {
            after_with(welcomed, "cloud", stt, voice)
        };
        assert_eq!(after("false", "small.en", None).0, "parakeet-v3");
        assert_eq!(
            after_with("false", "local", "small.en", None).0,
            "small.en",
            "a Local pick made before Welcome finished stays"
        );
        assert_eq!(after("true", "small.en", None).0, "small.en");
        assert_eq!(
            after("true", "medium.en", Some("large-v3")),
            ("large-v3-turbo".into(), Some("large-v3-turbo".into()))
        );
        assert_eq!(
            after("true", "tiny.en", Some("meetings")),
            ("tiny.en".into(), Some("meetings".into()))
        );
    }

    #[test]
    fn v027_defaults_new_installs_to_parakeet_ultra_and_keeps_a_picked_v3() {
        let after = |welcomed: &str, provider: &str, stt: &str| {
            let mut conn = Connection::open_in_memory().unwrap();
            // V003b makes V026 the 27th migration.
            migrations().to_version(&mut conn, 27).unwrap();
            for (k, v) in [
                ("first_run_completed", welcomed),
                ("stt.provider", provider),
                ("stt.model", stt),
            ] {
                conn.execute(
                    "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
                    [k, v],
                )
                .unwrap();
            }
            migrations().to_latest(&mut conn).unwrap();
            conn.query_row(
                "SELECT value FROM settings WHERE key = 'stt.model'",
                [],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
        };
        assert_eq!(after("false", "cloud", "parakeet-v3"), "parakeet-ultra");
        assert_eq!(
            after("false", "local", "parakeet-v3"),
            "parakeet-v3",
            "a Local pick made before Welcome finished stays"
        );
        assert_eq!(after("true", "cloud", "parakeet-v3"), "parakeet-v3");
        assert_eq!(after("true", "local", "parakeet-v3"), "parakeet-v3");
    }

    #[test]
    fn fresh_install_defaults_meetings_to_parakeet_ultra() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrations().to_latest(&mut conn).unwrap();
        let model: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'stt.model'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(model, "parakeet-ultra");
    }

    #[test]
    fn v023_moves_minimax_rows_off_retired_models() {
        let mut conn = Connection::open_in_memory().unwrap();
        // V003b makes V022 the 23rd migration.
        migrations().to_version(&mut conn, 23).unwrap();
        for (id, base_url, model) in [
            ("a", "https://api.minimax.io/v1", "MiniMax-Text-01"),
            ("b", "https://api.minimax.io/v1", "abab6.5s-chat"),
            ("c", "https://api.minimax.io/v1", "MiniMax-M3.1-Flash"),
            ("d", "https://api.openai.com/v1", "MiniMax-Text-01"),
        ] {
            conn.execute(
                "INSERT INTO providers (id, name, base_url, model, kind, created_at) VALUES (?1, ?1, ?2, ?3, 'llm', 0)",
                rusqlite::params![id, base_url, model],
            )
            .unwrap();
        }
        migrations().to_latest(&mut conn).unwrap();
        let models: Vec<String> = conn
            .prepare("SELECT model FROM providers ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            models,
            [
                "MiniMax-M3",
                "MiniMax-M3",
                "MiniMax-M3.1-Flash",
                "MiniMax-Text-01"
            ]
        );
    }
}
