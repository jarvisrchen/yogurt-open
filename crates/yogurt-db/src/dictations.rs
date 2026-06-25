//! The Voice history: one row per pasted dictation, and the stats the Voice page shows.
//! The stats read a daily rollup in `dictation_days` and `dictation_waits`, so they outlive the text rows that retention trims.

use anyhow::Result;
use chrono::{Local, NaiveDate, NaiveTime, TimeZone, Timelike};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::settings::VoiceRetention;
use crate::stats_range::{self, Range, Unit};
use crate::Db;

/// Stands in for a missing app name, in the stats and in the `app` filter.
pub const UNKNOWN_APP: &str = "Unknown";

pub use crate::stats_range::CHART_DAYS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dictation {
    pub id: String,
    /// Unix ms when the shortcut went down.
    pub created_at: i64,
    pub app_bundle_id: Option<String>,
    pub app_name: Option<String>,
    pub hands_free: bool,
    pub audio_ms: i64,
    pub raw_text: String,
    /// What was pasted.
    pub text: String,
    pub raw_words: i64,
    pub words: i64,
    pub stt_engine: String,
    pub stt_ms: i64,
    /// `off`, `done`, `rules`, `too_slow`, or `snippet` for a cue said as the whole dictation.
    pub cleanup: String,
    pub cleanup_model: Option<String>,
    pub cleanup_ms: Option<i64>,
    /// What the cleanup model used, as its provider reported it or estimated. `None` when no model ran.
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    /// At the model's list price when it ran. `None` with tokens set when the price table lacked the model.
    pub cost_usd: Option<f64>,
}

/// A dictation before it is saved. The id and both word counts are derived on insert.
#[derive(Debug, Clone, Default)]
pub struct NewDictation {
    pub created_at: i64,
    pub app_bundle_id: Option<String>,
    pub app_name: Option<String>,
    pub hands_free: bool,
    pub audio_ms: i64,
    pub raw_text: String,
    pub text: String,
    pub stt_engine: String,
    pub stt_ms: i64,
    pub cleanup: String,
    pub cleanup_model: Option<String>,
    pub cleanup_ms: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default)]
pub struct DictationFilter {
    /// Prefix-matched phrase over the pasted text. Empty means no search.
    pub q: String,
    /// An app name, or [`UNKNOWN_APP`] for rows with none.
    pub app: Option<String>,
    pub hands_free: bool,
    /// Unix ms. Only rows created at or after it, and `None` means every row.
    pub since: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DictationPage {
    pub dictations: Vec<Dictation>,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordsBucket {
    /// As [`stats_range::key`] writes it.
    pub key: String,
    pub words: i64,
    pub count: i64,
}

/// The range's words by the bucket [`stats_range::axis`] picks for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WordsChart {
    pub unit: Unit,
    pub buckets: Vec<WordsBucket>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppUse {
    pub name: String,
    pub words: i64,
    pub count: i64,
}

/// Counts of the kept text rows, for the sidebar rows that filter the history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryCounts {
    pub count: i64,
    pub hands_free_count: i64,
    /// Most-used first.
    pub apps: Vec<AppUse>,
    /// Average transcription ms per local model, and every Deepgram model under `deepgram`, for Settings > Voice.
    /// Rows saved before the engine named its model read only `local` and are left out.
    pub stt_avg_ms: std::collections::BTreeMap<String, i64>,
    /// Average cleanup ms per `cleanup_model`, over the dictations the model cleaned up, for Settings > Voice.
    pub cleanup_avg_ms: std::collections::BTreeMap<String, i64>,
}

/// The cleanup cost of one model over the range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelCost {
    pub model: String,
    pub count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// Over the priced dictations only.
    pub usd: f64,
    /// Dictations the price table did not know the model for when they ran.
    pub unpriced_count: i64,
}

/// The snippet pastes within the totals, which the speaking pace and time saved leave out.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SnippetUse {
    pub count: i64,
    pub words: i64,
    pub audio_ms: i64,
    pub wait_ms: i64,
}

/// Everything but `history` covers the range asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceStats {
    pub count: i64,
    pub hands_free_count: i64,
    pub words: i64,
    pub audio_ms: i64,
    /// Transcription plus cleanup time summed over every dictation, the wait after letting go.
    pub wait_ms: i64,
    /// The first local day with a dictation, `YYYY-MM-DD`.
    pub first_day: Option<String>,
    /// Today's hours come from the kept text rows, so a dictation deleted today is missing from its hour, and with none kept, as under Never save, the chart is today as one day.
    pub chart: WordsChart,
    /// Most-used first.
    pub apps: Vec<AppUse>,
    /// Dictations cleaned up by the model or the rule pass.
    pub cleaned_count: i64,
    pub cleaned_words: i64,
    pub fillers_removed: i64,
    pub too_slow_count: i64,
    pub median_wait_ms: Option<i64>,
    /// The model of the newest dictation the model cleaned up.
    pub cleanup_model: Option<String>,
    /// Most expensive first. Dictations from before costs were saved are in none of them.
    pub costs: Vec<ModelCost>,
    pub snippets: SnippetUse,
    pub history: HistoryCounts,
}

/// One local day's counts summed over every app, the row Share daily totals sends to Yogurt Cloud. Counts only, never text or app names.
/// The default, all zeros, stands in for a day that no longer has dictations after Delete all.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DayTotals {
    /// `YYYY-MM-DD`.
    pub day: String,
    pub count: i64,
    pub words: i64,
    pub audio_ms: i64,
    pub wait_ms: i64,
    pub hands_free_count: i64,
    pub cleaned_count: i64,
    pub cleaned_words: i64,
    pub fillers_removed: i64,
    pub too_slow_count: i64,
    pub snippet_count: i64,
    pub snippet_words: i64,
    pub snippet_audio_ms: i64,
    pub snippet_wait_ms: i64,
    pub usd: f64,
    pub unpriced_count: i64,
}

/// The V017 columns, which the V018 backfill reads before the cost columns exist.
const V017_COLUMNS: &str =
    "id, created_at, app_bundle_id, app_name, hands_free, audio_ms, raw_text, text, \
     raw_words, words, stt_engine, stt_ms, cleanup, cleanup_model, cleanup_ms";
const COST_COLUMNS: &str = "input_tokens, output_tokens, cost_usd";

fn row_to_dictation(r: &Row) -> rusqlite::Result<Dictation> {
    Ok(Dictation {
        id: r.get(0)?,
        created_at: r.get(1)?,
        app_bundle_id: r.get(2)?,
        app_name: r.get(3)?,
        hands_free: r.get(4)?,
        audio_ms: r.get(5)?,
        raw_text: r.get(6)?,
        text: r.get(7)?,
        raw_words: r.get(8)?,
        words: r.get(9)?,
        stt_engine: r.get(10)?,
        stt_ms: r.get(11)?,
        cleanup: r.get(12)?,
        cleanup_model: r.get(13)?,
        cleanup_ms: r.get(14)?,
        input_tokens: r.get(15)?,
        output_tokens: r.get(16)?,
        cost_usd: r.get(17)?,
    })
}

pub fn word_count(text: &str) -> i64 {
    text.split_whitespace().count() as i64
}

/// Standalone "um" and "uh" in `text`, the fillers the cleanup rule pass drops.
pub fn count_fillers(text: &str) -> i64 {
    text.split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|w| w == "um" || w == "uh")
        .count() as i64
}

/// Adds the dictation to the stats, and saves its text only when `keep_text`.
pub fn insert(db: &Db, d: NewDictation, keep_text: bool) -> Result<Dictation> {
    let row = Dictation {
        id: ulid::Ulid::new().to_string(),
        created_at: d.created_at,
        app_bundle_id: d.app_bundle_id,
        app_name: d.app_name,
        hands_free: d.hands_free,
        audio_ms: d.audio_ms,
        raw_words: word_count(&d.raw_text),
        words: word_count(&d.text),
        raw_text: d.raw_text,
        text: d.text,
        stt_engine: d.stt_engine,
        stt_ms: d.stt_ms,
        cleanup: d.cleanup,
        cleanup_model: d.cleanup_model,
        cleanup_ms: d.cleanup_ms,
        input_tokens: d.input_tokens,
        output_tokens: d.output_tokens,
        cost_usd: d.cost_usd,
    };
    db.with_conn(|conn| -> Result<()> {
        let tx = conn.unchecked_transaction()?;
        roll_up(&tx, &row)?;
        if !keep_text {
            return Ok(tx.commit()?);
        }
        tx.execute(
            &format!(
                "INSERT INTO dictations ({V017_COLUMNS}, {COST_COLUMNS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)"
            ),
            params![
                row.id,
                row.created_at,
                row.app_bundle_id,
                row.app_name,
                row.hands_free,
                row.audio_ms,
                row.raw_text,
                row.text,
                row.raw_words,
                row.words,
                row.stt_engine,
                row.stt_ms,
                row.cleanup,
                row.cleanup_model,
                row.cleanup_ms,
                row.input_tokens,
                row.output_tokens,
                row.cost_usd,
            ],
        )?;
        Ok(tx.commit()?)
    })?;
    Ok(row)
}

/// Deletes the text rows older than `retention` allows, and returns how many went. The stats keep them.
pub fn trim(db: &Db, retention: VoiceRetention, now_ms: i64) -> Result<usize> {
    let cutoff = match retention {
        VoiceRetention::Never => i64::MAX,
        r => now_ms - r.keep_ms(),
    };
    Ok(db
        .with_conn(|conn| conn.execute("DELETE FROM dictations WHERE created_at < ?1", [cutoff]))?)
}

/// Adds one dictation to `dictation_days` and `dictation_waits`, inside the caller's transaction.
fn roll_up(conn: &Connection, d: &Dictation) -> rusqlite::Result<()> {
    let day = local_date(d.created_at).to_string();
    let (cleaned_words, fillers_removed) = if matches!(d.cleanup.as_str(), "done" | "rules") {
        (
            (d.raw_words - d.words).max(0),
            (count_fillers(&d.raw_text) - count_fillers(&d.text)).max(0),
        )
    } else {
        (0, 0)
    };
    let model_at = (d.cleanup == "done" && d.cleanup_model.is_some()).then_some(d.created_at);
    conn.prepare_cached(
        "INSERT INTO dictation_days (day, app_name, count, hands_free_count, words, raw_words, audio_ms, stt_ms, \
           cleanup_ms, off_count, done_count, rules_count, too_slow_count, cleaned_words, fillers_removed, \
           cleanup_model, cleanup_model_at) \
         VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8, ?9 = 'off', ?9 = 'done', ?9 = 'rules', ?9 = 'too_slow', ?10, ?11, ?12, ?13) \
         ON CONFLICT (day, app_name) DO UPDATE SET \
           count = count + 1, \
           hands_free_count = hands_free_count + excluded.hands_free_count, \
           words = words + excluded.words, \
           raw_words = raw_words + excluded.raw_words, \
           audio_ms = audio_ms + excluded.audio_ms, \
           stt_ms = stt_ms + excluded.stt_ms, \
           cleanup_ms = cleanup_ms + excluded.cleanup_ms, \
           off_count = off_count + excluded.off_count, \
           done_count = done_count + excluded.done_count, \
           rules_count = rules_count + excluded.rules_count, \
           too_slow_count = too_slow_count + excluded.too_slow_count, \
           cleaned_words = cleaned_words + excluded.cleaned_words, \
           fillers_removed = fillers_removed + excluded.fillers_removed, \
           cleanup_model = CASE WHEN excluded.cleanup_model_at >= COALESCE(cleanup_model_at, excluded.cleanup_model_at) \
             THEN excluded.cleanup_model ELSE cleanup_model END, \
           cleanup_model_at = COALESCE(MAX(cleanup_model_at, excluded.cleanup_model_at), cleanup_model_at, excluded.cleanup_model_at)",
    )?
    .execute(params![
        day,
        d.app_name.as_deref().unwrap_or(""),
        d.hands_free,
        d.words,
        d.raw_words,
        d.audio_ms,
        d.stt_ms,
        d.cleanup_ms.unwrap_or(0),
        d.cleanup,
        cleaned_words,
        fillers_removed,
        model_at.and(d.cleanup_model.as_deref()),
        model_at,
    ])?;
    conn.prepare_cached(
        "INSERT INTO dictation_waits (day, wait_10ms, count) VALUES (?1, ?2, 1) \
         ON CONFLICT (day, wait_10ms) DO UPDATE SET count = count + 1",
    )?
    .execute(params![day, (wait_ms(d) + 5) / 10])?;
    // Separate statements, so the V018 backfill, which runs before these columns and tables exist and never has a snippet or tokens, skips them.
    if d.cleanup == "snippet" {
        conn.prepare_cached(
            "UPDATE dictation_days SET snippet_count = snippet_count + 1, snippet_words = snippet_words + ?3, \
               snippet_audio_ms = snippet_audio_ms + ?4, snippet_wait_ms = snippet_wait_ms + ?5 \
             WHERE day = ?1 AND app_name = ?2",
        )?
        .execute(params![
            day,
            d.app_name.as_deref().unwrap_or(""),
            d.words,
            d.audio_ms,
            wait_ms(d)
        ])?;
    }
    if let (Some(model), Some(input), Some(output)) =
        (&d.cleanup_model, d.input_tokens, d.output_tokens)
    {
        conn.prepare_cached(
            "INSERT INTO dictation_costs (day, model, count, input_tokens, output_tokens, usd, unpriced_count) \
             VALUES (?1, ?2, 1, ?3, ?4, COALESCE(?5, 0), ?5 IS NULL) \
             ON CONFLICT (day, model) DO UPDATE SET \
               count = count + 1, \
               input_tokens = input_tokens + excluded.input_tokens, \
               output_tokens = output_tokens + excluded.output_tokens, \
               usd = usd + excluded.usd, \
               unpriced_count = unpriced_count + excluded.unpriced_count",
        )?
        .execute(params![day, model, input, output, d.cost_usd])?;
    }
    Ok(())
}

fn wait_ms(d: &Dictation) -> i64 {
    d.stt_ms + d.cleanup_ms.unwrap_or(0)
}

pub(crate) fn backfill_rollup(conn: &Connection) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {V017_COLUMNS}, NULL, NULL, NULL FROM dictations"
    ))?;
    for d in stmt.query_map([], row_to_dictation)? {
        roll_up(conn, &d?)?;
    }
    Ok(())
}

/// Newest first. The search is one prefix-matched phrase with `"` doubled, so FTS5 operators in `q` search as text.
pub fn list(
    db: &Db,
    filter: &DictationFilter,
    limit: usize,
    offset: usize,
) -> Result<DictationPage> {
    let q = filter.q.trim();
    let match_expr = (!q.is_empty()).then(|| format!("\"{}\"*", q.replace('"', "\"\"")));
    let where_sql = "(:q IS NULL OR rowid IN (SELECT rowid FROM dictations_fts WHERE dictations_fts MATCH :q)) \
         AND (:app IS NULL OR COALESCE(app_name, :unknown) = :app) \
         AND (:hf = 0 OR hands_free = 1) \
         AND (:since IS NULL OR created_at >= :since)";
    db.with_conn(|conn| -> Result<DictationPage> {
        let named = rusqlite::named_params! {
            ":q": match_expr,
            ":app": filter.app,
            ":unknown": UNKNOWN_APP,
            ":hf": filter.hands_free,
            ":since": filter.since,
        };
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM dictations WHERE {where_sql}"),
            named,
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT {V017_COLUMNS}, {COST_COLUMNS} FROM dictations WHERE {where_sql} \
             ORDER BY created_at DESC, id DESC LIMIT :limit OFFSET :offset"
        ))?;
        let rows = stmt.query_map(
            rusqlite::named_params! {
                ":q": match_expr,
                ":app": filter.app,
                ":unknown": UNKNOWN_APP,
                ":hf": filter.hands_free,
                ":since": filter.since,
                ":limit": limit as i64,
                ":offset": offset as i64,
            },
            row_to_dictation,
        )?;
        Ok(DictationPage {
            dictations: rows.collect::<rusqlite::Result<_>>()?,
            total: total as usize,
        })
    })
}

/// `false` when no row created at or after `since` had that id.
pub fn delete(db: &Db, id: &str, since: Option<i64>) -> Result<bool> {
    Ok(db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM dictations WHERE id = ?1 AND created_at >= ?2",
            params![id, since.unwrap_or(i64::MIN)],
        )
    })? > 0)
}

/// Deletes the text rows created at or after `since` and leaves older rows and the stats. Returns how many went.
pub fn delete_since(db: &Db, since: i64) -> Result<usize> {
    Ok(db
        .with_conn(|conn| conn.execute("DELETE FROM dictations WHERE created_at >= ?1", [since]))?)
}

/// Deletes the text and resets the stats. Returns how many text rows went.
pub fn delete_all(db: &Db) -> Result<usize> {
    db.with_conn(|conn| -> Result<usize> {
        let tx = conn.unchecked_transaction()?;
        let deleted = tx.execute("DELETE FROM dictations", [])?;
        tx.execute_batch(
            "DELETE FROM dictation_days; DELETE FROM dictation_waits; DELETE FROM dictation_costs;",
        )?;
        tx.commit()?;
        Ok(deleted)
    })
}

/// `now_ms` sets which local day counts as today.
pub fn stats(db: &Db, now_ms: i64, range: Range) -> Result<VoiceStats> {
    let today = local_date(now_ms);
    let start = range.start(today);
    let since = start.map(|d| d.to_string());
    db.with_conn(|conn| -> Result<VoiceStats> {
        let (count, hands_free_count, words, audio_ms, wait_ms, first_day, done, rules, too_slow_count, cleaned_words, fillers_removed, snippets) =
            conn.query_row(
                "SELECT COALESCE(SUM(count), 0), COALESCE(SUM(hands_free_count), 0), COALESCE(SUM(words), 0), \
                        COALESCE(SUM(audio_ms), 0), COALESCE(SUM(stt_ms + cleanup_ms), 0), MIN(day), \
                        COALESCE(SUM(done_count), 0), COALESCE(SUM(rules_count), 0), COALESCE(SUM(too_slow_count), 0), \
                        COALESCE(SUM(cleaned_words), 0), COALESCE(SUM(fillers_removed), 0), \
                        COALESCE(SUM(snippet_count), 0), COALESCE(SUM(snippet_words), 0), \
                        COALESCE(SUM(snippet_audio_ms), 0), COALESCE(SUM(snippet_wait_ms), 0) \
                 FROM dictation_days WHERE ?1 IS NULL OR day >= ?1",
                [&since],
                |r| {
                    Ok((
                        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get::<_, Option<String>>(5)?,
                        r.get::<_, i64>(6)?, r.get::<_, i64>(7)?, r.get(8)?, r.get(9)?, r.get(10)?,
                        SnippetUse { count: r.get(11)?, words: r.get(12)?, audio_ms: r.get(13)?, wait_ms: r.get(14)? },
                    ))
                },
            )?;

        let (mut unit, keys) = stats_range::axis(start, today, first_day.as_deref().and_then(|d| d.parse().ok()));
        let mut buckets: Vec<WordsBucket> = keys
            .into_iter()
            .map(|key| WordsBucket { key, words: 0, count: 0 })
            .collect();
        let mut fill = |key: String, words: i64, count: i64| {
            if let Some(b) = buckets.iter_mut().find(|b| b.key == key) {
                b.words += words;
                b.count += count;
            }
        };
        if unit == Unit::Hour {
            // A day early in UTC is before local midnight in any zone, and the loop keeps only today's rows.
            let floor = today.and_time(NaiveTime::MIN).and_utc().timestamp_millis() - 86_400_000;
            let mut stmt = conn.prepare_cached("SELECT created_at, words FROM dictations WHERE created_at >= ?1")?;
            for r in stmt.query_map([floor], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
                let (at, words) = r?;
                if let Some(t) = Local.timestamp_millis_opt(at).earliest().filter(|t| t.date_naive() == today) {
                    fill(stats_range::key(unit, today, t.hour()), words, 1);
                }
            }
            // With no text kept, as under Never save, there are no hours to draw, so the chart is today as one day.
            if count > 0 && buckets.iter().all(|b| b.count == 0) {
                unit = Unit::Day;
                buckets = vec![WordsBucket { key: today.to_string(), words, count }];
            }
        } else {
            let mut stmt = conn.prepare_cached(
                "SELECT day, SUM(words), SUM(count) FROM dictation_days WHERE ?1 IS NULL OR day >= ?1 GROUP BY day",
            )?;
            for r in stmt.query_map([&since], |r| Ok((r.get::<_, String>(0)?, r.get(1)?, r.get(2)?)))? {
                let (day, words, count) = r?;
                if let Ok(d) = day.parse::<NaiveDate>() {
                    fill(stats_range::key(unit, d, 0), words, count);
                }
            }
        }

        let apps = app_uses(
            conn,
            "SELECT COALESCE(NULLIF(app_name, ''), ?1) AS name, SUM(words), SUM(count) FROM dictation_days \
             WHERE ?2 IS NULL OR day >= ?2 GROUP BY name ORDER BY SUM(count) DESC, name",
            params![UNKNOWN_APP, since],
        )?;

        let history = history_counts(conn, None)?;

        Ok(VoiceStats {
            count,
            hands_free_count,
            words,
            audio_ms,
            wait_ms,
            first_day,
            chart: WordsChart { unit, buckets },
            apps,
            cleaned_count: done + rules,
            cleaned_words,
            fillers_removed,
            too_slow_count,
            median_wait_ms: median_wait_ms(conn, &since)?,
            cleanup_model: conn
                .query_row(
                    "SELECT cleanup_model FROM dictation_days \
                     WHERE cleanup_model IS NOT NULL AND (?1 IS NULL OR day >= ?1) \
                     ORDER BY cleanup_model_at DESC LIMIT 1",
                    [&since],
                    |r| r.get(0),
                )
                .optional()?,
            costs: conn
                .prepare_cached(
                    "SELECT model, SUM(count), SUM(input_tokens), SUM(output_tokens), SUM(usd), SUM(unpriced_count) \
                     FROM dictation_costs WHERE ?1 IS NULL OR day >= ?1 \
                     GROUP BY model ORDER BY SUM(usd) DESC, SUM(count) DESC, model",
                )?
                .query_map([&since], |r| {
                    Ok(ModelCost {
                        model: r.get(0)?,
                        count: r.get(1)?,
                        input_tokens: r.get(2)?,
                        output_tokens: r.get(3)?,
                        usd: r.get(4)?,
                        unpriced_count: r.get(5)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?,
            snippets,
            history,
        })
    })
}

/// Every day with a dictation, oldest first, from the same rollup as [`stats`].
pub fn day_totals(db: &Db) -> Result<Vec<DayTotals>> {
    db.with_conn(|conn| -> Result<Vec<DayTotals>> {
        let mut stmt = conn.prepare_cached(
            "SELECT d.day, d.n, d.words, d.audio_ms, d.wait_ms, d.hands_free, d.cleaned, d.cleaned_words, d.fillers, \
                    d.too_slow, d.snippets, d.snippet_words, d.snippet_audio_ms, d.snippet_wait_ms, \
                    COALESCE(c.usd, 0), COALESCE(c.unpriced, 0) \
             FROM (SELECT day, SUM(count) AS n, SUM(words) AS words, SUM(audio_ms) AS audio_ms, \
                          SUM(stt_ms + cleanup_ms) AS wait_ms, SUM(hands_free_count) AS hands_free, \
                          SUM(done_count + rules_count) AS cleaned, SUM(cleaned_words) AS cleaned_words, \
                          SUM(fillers_removed) AS fillers, SUM(too_slow_count) AS too_slow, SUM(snippet_count) AS snippets, \
                          SUM(snippet_words) AS snippet_words, SUM(snippet_audio_ms) AS snippet_audio_ms, \
                          SUM(snippet_wait_ms) AS snippet_wait_ms \
                   FROM dictation_days GROUP BY day) d \
             LEFT JOIN (SELECT day, SUM(usd) AS usd, SUM(unpriced_count) AS unpriced FROM dictation_costs GROUP BY day) c \
               ON c.day = d.day \
             ORDER BY d.day",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(DayTotals {
                    day: r.get(0)?,
                    count: r.get(1)?,
                    words: r.get(2)?,
                    audio_ms: r.get(3)?,
                    wait_ms: r.get(4)?,
                    hands_free_count: r.get(5)?,
                    cleaned_count: r.get(6)?,
                    cleaned_words: r.get(7)?,
                    fillers_removed: r.get(8)?,
                    too_slow_count: r.get(9)?,
                    snippet_count: r.get(10)?,
                    snippet_words: r.get(11)?,
                    snippet_audio_ms: r.get(12)?,
                    snippet_wait_ms: r.get(13)?,
                    usd: r.get(14)?,
                    unpriced_count: r.get(15)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    })
}

/// The counts over the kept text rows created at or after `since`, or over every row for `None`.
/// The speed averages always cover every kept row, since Settings > Voice uses them to compare models.
pub fn history(db: &Db, since: Option<i64>) -> Result<HistoryCounts> {
    db.with_conn(|conn| history_counts(conn, since))
}

fn history_counts(conn: &Connection, since: Option<i64>) -> Result<HistoryCounts> {
    let (history_count, history_hands_free) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(hands_free), 0) FROM dictations WHERE ?1 IS NULL OR created_at >= ?1",
        [since],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(HistoryCounts {
        count: history_count,
        hands_free_count: history_hands_free,
        apps: app_uses(
            conn,
            "SELECT COALESCE(app_name, ?1) AS name, SUM(words), COUNT(*) FROM dictations \
             WHERE ?2 IS NULL OR created_at >= ?2 GROUP BY name ORDER BY COUNT(*) DESC, name",
            params![UNKNOWN_APP, since],
        )?,
        stt_avg_ms: conn
            .prepare_cached(
                "SELECT CASE WHEN stt_engine LIKE 'local:%' THEN substr(stt_engine, 7) ELSE 'deepgram' END AS engine, \
                 CAST(ROUND(AVG(stt_ms)) AS INTEGER) FROM dictations \
                 WHERE stt_engine LIKE 'local:%' OR stt_engine LIKE 'deepgram:%' GROUP BY engine",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?,
        cleanup_avg_ms: conn
            .prepare_cached(
                "SELECT cleanup_model, CAST(ROUND(AVG(cleanup_ms)) AS INTEGER) FROM dictations \
                 WHERE cleanup = 'done' AND cleanup_model IS NOT NULL AND cleanup_ms IS NOT NULL \
                 GROUP BY cleanup_model",
            )?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?,
    })
}

fn app_uses(conn: &Connection, sql: &str, p: impl rusqlite::Params) -> Result<Vec<AppUse>> {
    let mut stmt = conn.prepare_cached(sql)?;
    let apps = stmt
        .query_map(p, |r| {
            Ok(AppUse {
                name: r.get(0)?,
                words: r.get(1)?,
                count: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(apps)
}

/// In Rust because SQLite has no median. The upper middle for an even count, to the nearest 10 ms, which the rollup keeps.
fn median_wait_ms(conn: &Connection, since: &Option<String>) -> Result<Option<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT wait_10ms, SUM(count) FROM dictation_waits WHERE ?1 IS NULL OR day >= ?1 \
         GROUP BY wait_10ms ORDER BY wait_10ms",
    )?;
    let steps = stmt
        .query_map([since], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut middle = steps.iter().map(|(_, n)| n).sum::<i64>() / 2;
    for (step, n) in steps {
        if middle < n {
            return Ok(Some(step * 10));
        }
        middle -= n;
    }
    Ok(None)
}

pub(crate) fn local_date(ms: i64) -> NaiveDate {
    Local
        .timestamp_millis_opt(ms)
        .earliest()
        .map_or(NaiveDate::MIN, |t| t.date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;

    fn add(db: &Db, at: i64, app: Option<&str>, raw: &str, text: &str, cleanup: &str) -> Dictation {
        insert(
            db,
            NewDictation {
                created_at: at,
                app_name: app.map(str::to_string),
                hands_free: app == Some("Notes"),
                audio_ms: 6_000,
                raw_text: raw.into(),
                text: text.into(),
                stt_engine: "local:small.en".into(),
                stt_ms: 300,
                cleanup: cleanup.into(),
                ..Default::default()
            },
            true,
        )
        .unwrap()
    }

    #[test]
    fn day_totals_sum_every_app_per_day_with_the_day_s_cost() {
        let db = Db::open_in_memory().unwrap();
        let t0 = 1_800_000_000_000;
        add(
            &db,
            t0,
            Some("Slack"),
            "um so hi there",
            "So hi there.",
            "done",
        );
        add(&db, t0 + 60_000, Some("Notes"), "uh ok", "ok", "too_slow");
        add(&db, t0 + DAY, None, "one two three", "one two three", "off");
        insert(
            &db,
            NewDictation {
                created_at: t0 + 120_000,
                raw_text: "sig".into(),
                text: "Thanks, Richard".into(),
                audio_ms: 900,
                stt_ms: 100,
                cleanup: "snippet".into(),
                ..Default::default()
            },
            false,
        )
        .unwrap();
        for (cost, model) in [(Some(0.002), "gemini"), (None, "mystery")] {
            insert(
                &db,
                NewDictation {
                    created_at: t0 + 180_000,
                    app_name: Some("Mail".into()),
                    raw_text: "a b".into(),
                    text: "A b.".into(),
                    cleanup: "done".into(),
                    cleanup_model: Some(model.into()),
                    cleanup_ms: Some(400),
                    input_tokens: Some(300),
                    output_tokens: Some(10),
                    cost_usd: cost,
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        }

        let days = day_totals(&db).unwrap();
        assert_eq!(days.len(), 2);
        let (first, second) = (&days[0], &days[1]);
        assert_eq!(first.day, local_date(t0).to_string());
        assert_eq!(
            (
                first.count,
                first.words,
                first.audio_ms,
                first.wait_ms,
                first.hands_free_count
            ),
            (
                5,
                3 + 1 + 2 + 2 + 2,
                6_000 + 6_000 + 900,
                300 + 300 + 100 + 800,
                1
            )
        );
        assert_eq!(
            (
                first.cleaned_count,
                first.cleaned_words,
                first.fillers_removed,
                first.too_slow_count
            ),
            (3, 1, 1, 1)
        );
        assert_eq!(
            (
                first.snippet_count,
                first.snippet_words,
                first.snippet_audio_ms,
                first.snippet_wait_ms
            ),
            (1, 2, 900, 100)
        );
        assert_eq!((first.usd, first.unpriced_count), (0.002, 1));
        assert_eq!((second.count, second.words, second.usd), (1, 3, 0.0));

        delete_all(&db).unwrap();
        assert!(day_totals(&db).unwrap().is_empty());
    }

    #[test]
    fn stt_average_is_per_model_over_kept_text_only() {
        let db = Db::open_in_memory().unwrap();
        let row = |engine: &str, stt_ms, keep| {
            insert(
                &db,
                NewDictation {
                    created_at: 1_800_000_000_000,
                    raw_text: "hi".into(),
                    text: "hi".into(),
                    stt_engine: engine.into(),
                    stt_ms,
                    cleanup: "off".into(),
                    ..Default::default()
                },
                keep,
            )
            .unwrap();
        };
        row("local:small.en", 300, true);
        row("local:small.en", 381, true);
        row("local:tiny.en", 150, true);
        row("local", 999, true);
        row("local:medium.en", 900, false);
        row("deepgram:nova-3", 280, true);
        row("deepgram:nova-2", 340, true);
        let avg = stats(&db, 1_800_000_000_000, Range::All)
            .unwrap()
            .history
            .stt_avg_ms;
        assert_eq!(
            avg.into_iter().collect::<Vec<_>>(),
            [
                ("deepgram".to_string(), 310),
                ("small.en".to_string(), 341),
                ("tiny.en".to_string(), 150)
            ]
        );
    }

    #[test]
    fn cleanup_average_is_per_model_over_done_rows_only() {
        let db = Db::open_in_memory().unwrap();
        let row = |cleanup: &str, model: Option<&str>, ms: Option<i64>, keep| {
            insert(
                &db,
                NewDictation {
                    created_at: 1_800_000_000_000,
                    raw_text: "hi".into(),
                    text: "hi".into(),
                    stt_engine: "local:small.en".into(),
                    cleanup: cleanup.into(),
                    cleanup_model: model.map(str::to_string),
                    cleanup_ms: ms,
                    ..Default::default()
                },
                keep,
            )
            .unwrap();
        };
        row("done", Some("qwen3-4b-instruct"), Some(400), true);
        row("done", Some("qwen3-4b-instruct"), Some(451), true);
        row("done", Some("gemini-3.5-flash-lite"), Some(860), true);
        row("too_slow", Some("gemini-3.5-flash-lite"), Some(2_900), true);
        row("off", Some("gemini-3.5-flash-lite"), Some(100), true);
        row("rules", None, Some(0), true);
        row("done", Some("deepseek-v4-flash"), Some(1_300), false);
        let avg = stats(&db, 1_800_000_000_000, Range::All)
            .unwrap()
            .history
            .cleanup_avg_ms;
        assert_eq!(
            avg.into_iter().collect::<Vec<_>>(),
            [
                ("gemini-3.5-flash-lite".to_string(), 860),
                ("qwen3-4b-instruct".to_string(), 426)
            ]
        );
    }

    #[test]
    fn fillers_are_standalone_um_and_uh() {
        assert_eq!(count_fillers("Um, so uh the umbrella is UH... huh"), 3);
    }

    #[test]
    fn list_filters_searches_and_pages_newest_first() {
        let db = Db::open_in_memory().unwrap();
        let now = 1_800_000_000_000;
        let a = add(
            &db,
            now - 3_000,
            Some("Slack"),
            "ship the deck",
            "ship the deck",
            "off",
        );
        let b = add(
            &db,
            now - 2_000,
            Some("Notes"),
            "deck review notes",
            "deck review notes",
            "off",
        );
        let c = add(
            &db,
            now - 1_000,
            None,
            "lunch \"order\"",
            "lunch \"order\"",
            "off",
        );
        let ids = |f: DictationFilter| -> Vec<String> {
            list(&db, &f, 10, 0)
                .unwrap()
                .dictations
                .into_iter()
                .map(|d| d.id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(DictationFilter::default()),
            [c.id.as_str(), b.id.as_str(), a.id.as_str()]
        );
        assert_eq!(
            ids(DictationFilter {
                q: "dec".into(),
                ..Default::default()
            }),
            [b.id.as_str(), a.id.as_str()]
        );
        assert_eq!(
            ids(DictationFilter {
                q: "\"order".into(),
                ..Default::default()
            }),
            [c.id.as_str()]
        );
        assert_eq!(
            ids(DictationFilter {
                app: Some("Slack".into()),
                ..Default::default()
            }),
            [a.id.as_str()]
        );
        assert_eq!(
            ids(DictationFilter {
                app: Some(UNKNOWN_APP.into()),
                ..Default::default()
            }),
            [c.id.as_str()]
        );
        assert_eq!(
            ids(DictationFilter {
                hands_free: true,
                ..Default::default()
            }),
            [b.id.as_str()]
        );
        let page = list(&db, &DictationFilter::default(), 1, 1).unwrap();
        assert_eq!(
            (page.total, page.dictations[0].id.as_str()),
            (3, b.id.as_str())
        );

        assert!(!delete(&db, &a.id, Some(now - 2_000)).unwrap());
        assert!(delete(&db, &b.id, Some(now - 2_000)).unwrap());
        assert!(!delete(&db, &b.id, None).unwrap());
        assert_eq!(
            ids(DictationFilter {
                q: "review".into(),
                ..Default::default()
            }),
            Vec::<String>::new()
        );
        assert_eq!(delete_all(&db).unwrap(), 2);
        assert_eq!(
            list(
                &db,
                &DictationFilter {
                    q: "deck".into(),
                    ..Default::default()
                },
                10,
                0
            )
            .unwrap()
            .total,
            0
        );
    }

    #[test]
    fn today_with_no_text_kept_charts_the_day_as_one_bucket() {
        let db = Db::open_in_memory().unwrap();
        let now = Local
            .with_ymd_and_hms(2026, 11, 7, 15, 0, 0)
            .unwrap()
            .timestamp_millis();
        let unkept = NewDictation {
            created_at: now - 3_600_000,
            app_name: Some("Slack".into()),
            audio_ms: 2_000,
            raw_text: "ship it today".into(),
            text: "ship it today".into(),
            stt_engine: "local".into(),
            cleanup: "off".into(),
            ..Default::default()
        };
        insert(&db, unkept, false).unwrap();
        let s = stats(&db, now, Range::Today).unwrap();
        assert_eq!((s.count, s.words), (1, 3));
        assert_eq!(
            (s.chart.unit, s.chart.buckets),
            (
                Unit::Day,
                vec![WordsBucket {
                    key: "2026-11-07".into(),
                    words: 3,
                    count: 1
                }]
            )
        );

        add(
            &db,
            now - 60_000,
            Some("Notes"),
            "and the deck",
            "and the deck",
            "off",
        );
        let s = stats(&db, now, Range::Today).unwrap();
        assert_eq!(
            (s.count, s.chart.unit),
            (2, Unit::Hour),
            "a kept row draws the hours"
        );
        assert_eq!(s.chart.buckets[14].count, 1);
    }

    #[test]
    fn stats_sum_bucket_and_count_cleanup() {
        let db = Db::open_in_memory().unwrap();
        let empty = stats(&db, 1_800_000_000_000, Range::All).unwrap();
        assert_eq!(
            (empty.count, empty.median_wait_ms, empty.chart.buckets.len()),
            (0, None, 1)
        );

        let now = Local
            .with_ymd_and_hms(2026, 11, 7, 15, 0, 0)
            .unwrap()
            .timestamp_millis();
        add(
            &db,
            now,
            Some("Slack"),
            "um so we we ship",
            "So we ship.",
            "done",
        );
        add(
            &db,
            now - DAY,
            Some("Slack"),
            "one two three",
            "one two three",
            "off",
        );
        add(&db, now - 2 * DAY, Some("Notes"), "uh hi", "Hi.", "rules");
        add(
            &db,
            now - 40 * DAY,
            None,
            "old words here",
            "old words here",
            "too_slow",
        );
        let s = stats(&db, now, Range::All).unwrap();
        assert_eq!(
            (s.count, s.hands_free_count, s.words, s.audio_ms),
            (4, 1, 10, 24_000)
        );
        assert_eq!(s.chart.unit, Unit::Week);
        assert_eq!(s.chart.buckets.iter().map(|b| b.count).sum::<i64>(), 4);
        let days = stats(&db, now, Range::Month).unwrap().chart.buckets;
        assert_eq!(
            days.last().unwrap(),
            &WordsBucket {
                key: "2026-11-07".into(),
                words: 3,
                count: 1
            }
        );
        assert_eq!(days[28].words, 3);
        assert_eq!(days.iter().map(|b| b.count).sum::<i64>(), 3);

        let today = stats(&db, now, Range::Today).unwrap();
        assert_eq!(
            (today.count, today.words),
            (1, 3),
            "the range holds only today"
        );
        assert_eq!(
            (today.chart.unit, today.chart.buckets.len()),
            (Unit::Hour, 24)
        );
        assert_eq!(
            today.chart.buckets[15],
            WordsBucket {
                key: "15".into(),
                words: 3,
                count: 1
            }
        );
        assert_eq!(stats(&db, now, Range::Week).unwrap().chart.buckets.len(), 7);
        assert_eq!(
            s.apps[0],
            AppUse {
                name: "Slack".into(),
                words: 6,
                count: 2
            }
        );
        assert_eq!(s.apps.len(), 3);
        assert_eq!(
            (
                s.cleaned_count,
                s.cleaned_words,
                s.fillers_removed,
                s.too_slow_count
            ),
            (2, 3, 2, 1)
        );
        assert_eq!(s.median_wait_ms, Some(300));
    }

    fn at_3pm_on_nov_7() -> i64 {
        Local
            .with_ymd_and_hms(2026, 11, 7, 15, 0, 0)
            .unwrap()
            .timestamp_millis()
    }

    /// Varied rows over 60 days: every cleanup outcome, missing apps, hands-free, and priced and unpriced models.
    fn seed(db: &Db, now: i64) {
        let apps = [Some("Slack"), Some("Notes"), None, Some("Cursor")];
        let outcomes = ["off", "done", "rules", "too_slow", "snippet"];
        let raws = [
            "um so we we ship it",
            "uh hi there",
            "one two three",
            "um uh um okay",
        ];
        let texts = ["So we ship it.", "Hi there.", "one two three", "Okay."];
        for i in 0..120_i64 {
            let k = i as usize;
            let cleanup = outcomes[k % 5];
            let model_ran = cleanup == "done" || cleanup == "too_slow";
            insert(
                db,
                NewDictation {
                    created_at: now - i * DAY / 2 - (i % 7) * 60_000,
                    app_name: apps[k % 4].map(str::to_string),
                    hands_free: i % 3 == 0,
                    audio_ms: 1_000 + i * 37,
                    raw_text: raws[k % 4].into(),
                    text: if cleanup == "done" || cleanup == "rules" {
                        texts[k % 4]
                    } else {
                        raws[k % 4]
                    }
                    .into(),
                    stt_engine: "local".into(),
                    stt_ms: 200 + i * 13,
                    cleanup: cleanup.into(),
                    cleanup_model: model_ran.then(|| format!("model-{}", i % 3)),
                    cleanup_ms: model_ran.then_some(100 + i * 7),
                    input_tokens: model_ran.then_some(300 + i),
                    output_tokens: model_ran.then_some(20 + i),
                    // Powers of two, so the sums come out exact in any order. model-2 is unpriced.
                    cost_usd: (model_ran && i % 3 != 2).then_some(i as f64 / 1_024.0),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        }
    }

    fn range(all_time: bool) -> Range {
        if all_time {
            Range::All
        } else {
            Range::Month
        }
    }

    /// The stats recomputed straight from the text rows, as an oracle for the rollup.
    fn recount(db: &Db, now: i64, all_time: bool) -> VoiceStats {
        let all = list(db, &DictationFilter::default(), usize::MAX >> 1, 0)
            .unwrap()
            .dictations;
        let first = Range::Month.start(local_date(now)).unwrap();
        let mut rows = all.clone();
        if !all_time {
            rows.retain(|d| local_date(d.created_at) >= first);
        }
        let cleaned: Vec<_> = rows
            .iter()
            .filter(|d| d.cleanup == "done" || d.cleanup == "rules")
            .collect();
        let mut waits: Vec<i64> = rows.iter().map(|d| (wait_ms(d) + 5) / 10 * 10).collect();
        waits.sort();
        let mut apps: Vec<AppUse> = Vec::new();
        for d in &rows {
            let name = d.app_name.clone().unwrap_or_else(|| UNKNOWN_APP.into());
            match apps.iter_mut().find(|a| a.name == name) {
                Some(a) => {
                    a.words += d.words;
                    a.count += 1;
                }
                None => apps.push(AppUse {
                    name,
                    words: d.words,
                    count: 1,
                }),
            }
        }
        apps.sort_by(|a, b| b.count.cmp(&a.count).then(a.name.cmp(&b.name)));
        let full = stats(db, now, range(all_time)).unwrap();
        VoiceStats {
            count: rows.len() as i64,
            hands_free_count: rows.iter().filter(|d| d.hands_free).count() as i64,
            words: rows.iter().map(|d| d.words).sum(),
            audio_ms: rows.iter().map(|d| d.audio_ms).sum(),
            wait_ms: rows.iter().map(wait_ms).sum(),
            first_day: rows
                .iter()
                .map(|d| local_date(d.created_at))
                .min()
                .map(|d| d.to_string()),
            chart: {
                let (unit, keys) = stats_range::axis(
                    (!all_time).then_some(first),
                    local_date(now),
                    rows.iter().map(|d| local_date(d.created_at)).min(),
                );
                let buckets = keys
                    .into_iter()
                    .map(|key| {
                        let on: Vec<_> = rows
                            .iter()
                            .filter(|d| stats_range::key(unit, local_date(d.created_at), 0) == key)
                            .collect();
                        WordsBucket {
                            words: on.iter().map(|d| d.words).sum(),
                            count: on.len() as i64,
                            key,
                        }
                    })
                    .collect();
                WordsChart { unit, buckets }
            },
            apps,
            cleaned_count: cleaned.len() as i64,
            cleaned_words: cleaned.iter().map(|d| (d.raw_words - d.words).max(0)).sum(),
            fillers_removed: cleaned
                .iter()
                .map(|d| (count_fillers(&d.raw_text) - count_fillers(&d.text)).max(0))
                .sum(),
            too_slow_count: rows.iter().filter(|d| d.cleanup == "too_slow").count() as i64,
            median_wait_ms: waits.get(waits.len() / 2).copied(),
            cleanup_model: rows
                .iter()
                .filter(|d| d.cleanup == "done")
                .max_by_key(|d| d.created_at)
                .and_then(|d| d.cleanup_model.clone()),
            costs: {
                let mut costs: Vec<ModelCost> = Vec::new();
                for d in rows.iter().filter(|d| d.input_tokens.is_some()) {
                    let model = d.cleanup_model.clone().unwrap();
                    let i = match costs.iter().position(|c| c.model == model) {
                        Some(i) => i,
                        None => {
                            costs.push(ModelCost {
                                model,
                                count: 0,
                                input_tokens: 0,
                                output_tokens: 0,
                                usd: 0.0,
                                unpriced_count: 0,
                            });
                            costs.len() - 1
                        }
                    };
                    let c = &mut costs[i];
                    c.count += 1;
                    c.input_tokens += d.input_tokens.unwrap();
                    c.output_tokens += d.output_tokens.unwrap();
                    c.usd += d.cost_usd.unwrap_or(0.0);
                    c.unpriced_count += d.cost_usd.is_none() as i64;
                }
                costs.sort_by(|a, b| {
                    b.usd
                        .total_cmp(&a.usd)
                        .then(b.count.cmp(&a.count))
                        .then(a.model.cmp(&b.model))
                });
                costs
            },
            snippets: {
                let snips: Vec<_> = rows.iter().filter(|d| d.cleanup == "snippet").collect();
                SnippetUse {
                    count: snips.len() as i64,
                    words: snips.iter().map(|d| d.words).sum(),
                    audio_ms: snips.iter().map(|d| d.audio_ms).sum(),
                    wait_ms: snips.iter().map(|d| wait_ms(d)).sum(),
                }
            },
            history: full.history,
        }
    }

    #[test]
    fn the_rollup_equals_a_recount_from_the_rows() {
        let db = Db::open_in_memory().unwrap();
        let now = at_3pm_on_nov_7();
        seed(&db, now);
        for all_time in [true, false] {
            assert_eq!(
                stats(&db, now, range(all_time)).unwrap(),
                recount(&db, now, all_time),
                "all_time {all_time}"
            );
        }
        let s = stats(&db, now, Range::All).unwrap();
        assert_eq!((s.count, s.history.count), (120, 120));
        assert_eq!(s.snippets.count, 24);
        assert_eq!(s.costs.iter().map(|c| c.count).sum::<i64>(), 48);
        let unpriced = s.costs.iter().find(|c| c.model == "model-2").unwrap();
        assert_eq!(
            (unpriced.usd, unpriced.unpriced_count),
            (0.0, unpriced.count)
        );
        assert!(stats(&db, now, Range::Month).unwrap().count < 120);
    }

    #[test]
    fn a_trim_keeps_the_retention_and_leaves_the_stats_alone() {
        let now = at_3pm_on_nov_7();
        for (retention, kept) in [
            (VoiceRetention::Days30, 60),
            (VoiceRetention::Days7, 15),
            (VoiceRetention::Hours24, 2),
            (VoiceRetention::Never, 0),
        ] {
            let db = Db::open_in_memory().unwrap();
            seed(&db, now);
            let before = stats(&db, now, Range::All).unwrap();
            let cutoff = match retention {
                VoiceRetention::Never => i64::MAX,
                r => now - r.keep_ms(),
            };
            let expected = list(&db, &DictationFilter::default(), 1_000, 0)
                .unwrap()
                .dictations
                .iter()
                .filter(|d| d.created_at >= cutoff)
                .count();
            assert_eq!(expected, kept, "{retention:?}");
            assert_eq!(
                trim(&db, retention, now).unwrap(),
                120 - kept,
                "{retention:?}"
            );
            assert_eq!(
                list(&db, &DictationFilter::default(), 1_000, 0)
                    .unwrap()
                    .total,
                kept
            );
            let after = stats(&db, now, Range::All).unwrap();
            assert_eq!(after.history.count, kept as i64);
            assert_eq!(
                VoiceStats {
                    history: before.history.clone(),
                    ..after
                },
                before,
                "{retention:?}"
            );
        }
    }

    #[test]
    fn never_save_counts_the_dictation_without_its_text() {
        let db = Db::open_in_memory().unwrap();
        let now = at_3pm_on_nov_7();
        let d = NewDictation {
            created_at: now,
            app_name: Some("Slack".into()),
            raw_text: "um hello there".into(),
            text: "Hello there.".into(),
            stt_ms: 300,
            cleanup: "done".into(),
            cleanup_model: Some("qwen".into()),
            ..Default::default()
        };
        insert(&db, d, false).unwrap();
        assert_eq!(
            list(&db, &DictationFilter::default(), 10, 0).unwrap().total,
            0
        );
        let s = stats(&db, now, Range::Month).unwrap();
        assert_eq!(
            (s.count, s.words, s.fillers_removed, s.history.count),
            (1, 2, 1, 0)
        );
        assert_eq!(s.cleanup_model.as_deref(), Some("qwen"));
        assert_eq!(s.apps[0].name, "Slack");
    }

    #[test]
    fn delete_since_keeps_older_rows_and_every_rollup() {
        let db = Db::open_in_memory().unwrap();
        let now = at_3pm_on_nov_7();
        let old = add(&db, now - 3 * DAY, Some("Mail"), "old", "old", "off");
        add(&db, now - 1_000, Some("Slack"), "new", "new", "off");
        let since = Some(now - DAY);
        let window = DictationFilter {
            since,
            ..Default::default()
        };
        assert_eq!(list(&db, &window, 10, 0).unwrap().total, 1);
        assert_eq!(history(&db, since).unwrap().count, 1);
        let before = stats(&db, now, Range::All).unwrap();

        assert_eq!(delete_since(&db, now - DAY).unwrap(), 1);
        let left = list(&db, &DictationFilter::default(), 10, 0).unwrap();
        assert_eq!(ids_of(&left), [old.id.as_str()]);
        assert_eq!(history(&db, since).unwrap().count, 0);
        let after = stats(&db, now, Range::All).unwrap();
        assert_eq!(after.history.count, 1);
        assert_eq!(
            VoiceStats {
                history: before.history.clone(),
                ..after
            },
            before
        );
    }

    fn ids_of(page: &DictationPage) -> Vec<&str> {
        page.dictations.iter().map(|d| d.id.as_str()).collect()
    }

    #[test]
    fn delete_all_clears_the_text_and_the_stats() {
        let db = Db::open_in_memory().unwrap();
        let now = at_3pm_on_nov_7();
        seed(&db, now);
        trim(&db, VoiceRetention::Days7, now).unwrap();
        assert_eq!(delete_all(&db).unwrap(), 15);
        let s = stats(&db, now, Range::All).unwrap();
        assert_eq!(
            (
                s.count,
                s.words,
                s.first_day,
                s.median_wait_ms,
                s.cleanup_model,
                s.history.count
            ),
            (0, 0, None, None, None, 0)
        );
        assert!(s.apps.is_empty() && s.chart.buckets.iter().all(|b| b.count == 0));
    }
}
