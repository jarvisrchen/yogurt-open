//! The log of LLM calls made for meetings, which the Stats page prices.

use anyhow::Result;
use rusqlite::params;

use crate::Db;

pub const KIND_API: &str = "api";
pub const KIND_LOCAL: &str = "local";
pub const KIND_CLI: &str = "cli";
pub const OUTCOME_ERROR: &str = "error";

#[derive(Debug, Clone, PartialEq)]
pub struct NewCall<'a> {
    /// Unix ms when the call started.
    pub at: i64,
    pub purpose: &'a str,
    pub meeting_id: Option<&'a str>,
    pub model: &'a str,
    pub kind: &'a str,
    pub input_tokens: i64,
    pub output_tokens: i64,
    /// `None` when the price table lacked the model.
    pub cost_usd: Option<f64>,
    pub elapsed_ms: i64,
    pub outcome: &'a str,
}

pub fn insert(db: &Db, c: &NewCall) -> Result<()> {
    db.with_conn(|conn| {
        conn.prepare_cached(
            "INSERT INTO llm_calls (at, purpose, meeting_id, model, kind, input_tokens, output_tokens, cost_usd, elapsed_ms, outcome) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        )?
        .execute(params![
            c.at,
            c.purpose,
            c.meeting_id,
            c.model,
            c.kind,
            c.input_tokens,
            c.output_tokens,
            c.cost_usd,
            c.elapsed_ms,
            c.outcome
        ])?;
        Ok(())
    })
}
