//! LLM client resolution for the enhance handler.
//!
//! There is deliberately no automatic fallback from an unreachable `http` provider to a
//! local CLI. Silently rerouting a meeting's content to an unvetted backend because of a
//! network hiccup is a change the user must opt into by picking the CLI as the active provider.

use std::sync::Arc;

use yogurt_db::llm_calls;
pub use yogurt_llm::OpenAiCompatClient;

/// Shared by enhance and chat so they always use the same model.
///
/// Priority:
///   1. `state.llm_override`, test injection only.
///   2. `YOGURT_LLM_*` env vars.
///   3. Active provider row. An unreadable key or a CLI missing from `$PATH` is a hard `Err`.
///   4. `MockLlm` when nothing is configured, so first-run users still see the hero flow.
pub async fn resolve(
    state: &crate::state::AppState,
    purpose: &'static str,
    meeting_id: Option<String>,
) -> anyhow::Result<std::sync::Arc<dyn yogurt_llm::LlmClient>> {
    match resolve_configured(state, purpose, meeting_id).await? {
        Some(client) => Ok(client),
        None => {
            tracing::warn!("no LLM provider configured; falling back to MockLlm");
            Ok(Arc::new(crate::llm_mock::MockLlm))
        }
    }
}

/// Steps 1 to 3 of [`resolve`], with `Ok(None)` where it would fall back to the mock.
/// `purpose` and `meeting_id` label the client's `llm_call` log lines.
pub async fn resolve_configured(
    state: &crate::state::AppState,
    purpose: &'static str,
    meeting_id: Option<String>,
) -> anyhow::Result<Option<Arc<dyn yogurt_llm::LlmClient>>> {
    let inner = if let Some(overridden) = &state.llm_override {
        overridden.clone()
    } else if let Some(c) = from_env() {
        Arc::new(c)
    } else {
        match from_active_provider(state).await? {
            Some(c) => c,
            None => return Ok(None),
        }
    };
    Ok(Some(Arc::new(Logged {
        inner,
        purpose,
        meeting_id,
        db: state.db.clone(),
    })))
}

/// Voice cleanup reads its own usage and prices each dictation, so its calls stay out of `llm_calls`.
const VOICE_CLEANUP: &str = "voice_cleanup";

/// How long to keep reading a stream past its last word for the chunk that carries the usage, which OpenAI-style providers send after it.
const USAGE_GRACE: std::time::Duration = std::time::Duration::from_secs(2);

/// Writes one `llm_call` line per request, so a log shows how often each model was called for a meeting and how each call ended, and one `llm_calls` row, which the Stats page prices.
struct Logged {
    inner: Arc<dyn yogurt_llm::LlmClient>,
    purpose: &'static str,
    meeting_id: Option<String>,
    db: yogurt_db::Db,
}

impl Logged {
    fn call(&self, req: &yogurt_llm::ChatRequest) -> Call {
        let model = self.inner.model_name();
        let kind = if model.starts_with("cli:") {
            llm_calls::KIND_CLI
        } else if self.inner.runs_locally() {
            llm_calls::KIND_LOCAL
        } else {
            llm_calls::KIND_API
        };
        Call {
            db: self.db.clone(),
            purpose: self.purpose,
            meeting_id: self.meeting_id.clone(),
            model,
            kind,
            at: crate::routes::now_unix_ms(),
            started: std::time::Instant::now(),
            elapsed_ms: None,
            input_chars: req.messages.iter().map(|m| m.content.chars().count()).sum(),
            output_chars: 0,
            usage: None,
            logged: false,
        }
    }
}

struct Call {
    db: yogurt_db::Db,
    purpose: &'static str,
    meeting_id: Option<String>,
    model: String,
    kind: &'static str,
    at: i64,
    started: std::time::Instant,
    /// Set once the answer ended, so waiting for the usage chunk does not count as the model's time.
    elapsed_ms: Option<u64>,
    input_chars: usize,
    output_chars: usize,
    usage: Option<yogurt_llm::Usage>,
    logged: bool,
}

impl Call {
    fn saw(&mut self, chunk: &yogurt_llm::ChatChunk) {
        self.output_chars += chunk.delta.chars().count();
        self.usage = chunk.usage.or(self.usage);
        if chunk.done && self.elapsed_ms.is_none() {
            self.elapsed_ms = Some(self.started.elapsed().as_millis() as u64);
        }
    }

    fn finish(&mut self, outcome: &str, error: Option<&anyhow::Error>) {
        if std::mem::replace(&mut self.logged, true) {
            return;
        }
        let elapsed_ms = self
            .elapsed_ms
            .unwrap_or_else(|| self.started.elapsed().as_millis() as u64);
        tracing::info!(
            event = "llm_call",
            purpose = self.purpose,
            meeting_id = self.meeting_id.as_deref(),
            model = %self.model,
            elapsed_ms,
            outcome,
            error = error.map(|e| format!("{e:#}")).as_deref(),
            "llm call"
        );
        if self.purpose == VOICE_CLEANUP {
            return;
        }
        let failed = outcome == llm_calls::OUTCOME_ERROR;
        // A provider that names `usage` but leaves out its fields reads as 0, which is no report.
        let usage = match self.usage.filter(|u| u.input_tokens > 0) {
            _ if failed => yogurt_llm::Usage::default(),
            Some(reported) => reported,
            None => crate::voice_cleanup::estimate(self.input_chars, self.output_chars),
        };
        let cost_usd = match self.kind {
            _ if failed => None,
            llm_calls::KIND_API => crate::voice_cleanup::price(&self.model, usage),
            _ => Some(0.0),
        };
        let (db, at, purpose, kind) = (self.db.clone(), self.at, self.purpose, self.kind);
        let (meeting_id, model, outcome) = (
            self.meeting_id.clone(),
            self.model.clone(),
            outcome.to_string(),
        );
        let write = move || {
            let row = llm_calls::NewCall {
                at,
                purpose,
                meeting_id: meeting_id.as_deref(),
                model: &model,
                kind,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                cost_usd,
                elapsed_ms: elapsed_ms as i64,
                outcome: &outcome,
            };
            if let Err(e) = llm_calls::insert(&db, &row) {
                tracing::warn!(error = %e, "llm call not recorded");
            }
        };
        // The Stats page holds the database while it reads every meeting, which must not stall an async worker here.
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => drop(runtime.spawn_blocking(write)),
            Err(_) => write(),
        }
    }
}

/// A caller that times out, a client that disconnects, or a stream with no `done` chunk drops the call before it finished.
impl Drop for Call {
    fn drop(&mut self) {
        self.finish("unfinished", None);
    }
}

#[async_trait::async_trait]
impl yogurt_llm::LlmClient for Logged {
    async fn complete(
        &self,
        req: yogurt_llm::ChatRequest,
    ) -> anyhow::Result<yogurt_llm::ChatResponse> {
        let mut call = self.call(&req);
        let res = self.inner.complete(req).await;
        match &res {
            Ok(r) => {
                call.output_chars = r.content.chars().count();
                call.usage = r.usage;
                call.finish("ok", None)
            }
            Err(e) => call.finish(llm_calls::OUTCOME_ERROR, Some(e)),
        }
        res
    }

    async fn stream(
        &self,
        req: yogurt_llm::ChatRequest,
    ) -> anyhow::Result<
        futures_util::stream::BoxStream<'static, anyhow::Result<yogurt_llm::ChatChunk>>,
    > {
        use futures_util::StreamExt;
        let mut call = self.call(&req);
        let mut stream = self
            .inner
            .stream(req)
            .await
            .inspect_err(|e| call.finish(llm_calls::OUTCOME_ERROR, Some(e)))?;
        if self.purpose == VOICE_CLEANUP {
            return Ok(stream
                .inspect(move |chunk| match chunk {
                    Ok(c) if c.done => call.finish("ok", None),
                    Err(e) => call.finish(llm_calls::OUTCOME_ERROR, Some(e)),
                    Ok(_) => {}
                })
                .boxed());
        }
        // A caller stops reading at the last word, and the usage comes after it, so a task of its own reads the stream to the end.
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            let mut done = false;
            loop {
                let next = if done {
                    match tokio::time::timeout(USAGE_GRACE, stream.next()).await {
                        Ok(next) => next,
                        Err(_) => break,
                    }
                } else {
                    // A caller that went away before the answer ended cancels the request.
                    tokio::select! {
                        next = stream.next() => next,
                        _ = tx.closed() => break,
                    }
                };
                let Some(chunk) = next else { break };
                match &chunk {
                    Ok(c) => {
                        call.saw(c);
                        done |= c.done;
                    }
                    Err(e) if !done => call.finish(llm_calls::OUTCOME_ERROR, Some(e)),
                    Err(_) => {}
                }
                let failed = chunk.is_err();
                let sent = tx.send(chunk).await.is_ok();
                if failed || (done && call.usage.is_some()) || (!done && !sent) {
                    break;
                }
            }
            if done {
                call.finish("ok", None);
            }
        });
        Ok(futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| (chunk, rx))
        })
        .boxed())
    }

    fn model_name(&self) -> String {
        self.inner.model_name()
    }

    fn response_timeout(&self) -> std::time::Duration {
        self.inner.response_timeout()
    }

    async fn warm(&self) {
        self.inner.warm().await
    }

    fn runs_locally(&self) -> bool {
        self.inner.runs_locally()
    }
}

/// Developer override. `None` unless all of `YOGURT_LLM_BASE_URL`, `YOGURT_LLM_API_KEY`
/// and `YOGURT_LLM_MODEL` are set.
pub fn from_env() -> Option<OpenAiCompatClient> {
    let base_url = std::env::var("YOGURT_LLM_BASE_URL").ok()?;
    let api_key = std::env::var("YOGURT_LLM_API_KEY").ok()?;
    let model = std::env::var("YOGURT_LLM_MODEL").ok()?;
    Some(OpenAiCompatClient::new(base_url, api_key, model))
}

/// `Ok(None)` means no active provider, so the caller falls back to mock.
/// `Err` means the active provider is unusable (unreadable key, CLI not on `$PATH`) and the
/// caller must surface it (502) rather than fall back.
async fn from_active_provider(
    state: &crate::state::AppState,
) -> anyhow::Result<Option<Arc<dyn yogurt_llm::LlmClient>>> {
    yogurt_db::providers::active(&state.db)?
        .map(|p| from_provider(state, p, false))
        .transpose()
}

/// The client for one provider row, wrapped like [`resolve`]'s so its calls are logged.
pub fn for_provider(
    state: &crate::state::AppState,
    provider: yogurt_db::providers::Provider,
    purpose: &'static str,
) -> anyhow::Result<Arc<dyn yogurt_llm::LlmClient>> {
    let inner = match &state.llm_override {
        Some(overridden) => overridden.clone(),
        None => from_provider(state, provider, purpose == "voice_cleanup")?,
    };
    Ok(Arc::new(Logged {
        inner,
        purpose,
        meeting_id: None,
        db: state.db.clone(),
    }))
}

/// `usage` asks the provider's streams for token counts, which Voice cleanup prices.
fn from_provider(
    state: &crate::state::AppState,
    provider: yogurt_db::providers::Provider,
    usage: bool,
) -> anyhow::Result<Arc<dyn yogurt_llm::LlmClient>> {
    if provider.adapter == yogurt_db::providers::adapter::CLI {
        return state.pro.llm_for(state, &provider);
    }
    // The error names the provider, never the key.
    match state.keys.get(&provider.id)? {
        Some(key) => {
            let client = OpenAiCompatClient::new(provider.base_url, key, provider.model);
            Ok(Arc::new(if usage { client.with_usage() } else { client }))
        }
        None => anyhow::bail!(
            "provider '{}' ({}) has an API key that could not be read \
             from the key store - re-enter it in Settings",
            provider.name,
            provider.id
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use yogurt_db::providers::{insert, set_active, NewProvider};
    use yogurt_db::Db;

    fn fixture() -> crate::state::AppState {
        let dir = tempfile::tempdir().unwrap().keep();
        crate::state::AppState::in_memory(
            crate::Mode::Release,
            Arc::new(crate::storage::Storage::init_at(&dir.join("db.sqlite")).unwrap()),
            Arc::new(crate::session::load_or_create(&dir.join("session-token")).unwrap()),
            0,
            dir.join("notes"),
        )
        .unwrap()
    }

    fn insert_active_provider(db: &Db) -> String {
        let id = insert(
            db,
            NewProvider {
                name: "Minimax".to_string(),
                base_url: "https://api.minimax.io/v1".to_string(),
                model: "MiniMax-Text-01".to_string(),
                adapter: yogurt_db::providers::adapter::HTTP.to_string(),
                cli_model: String::new(),
            },
        )
        .unwrap();
        set_active(db, &id).unwrap();
        id
    }

    #[tokio::test]
    async fn no_provider_returns_none() {
        let got = from_active_provider(&fixture()).await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn provider_with_key_returns_client() {
        let state = fixture();
        let id = insert_active_provider(&state.db);
        state.keys.set(&id, "sk-test").unwrap();
        // `Arc<dyn LlmClient>` isn't `Debug`, so `unwrap` is unavailable.
        let client = match from_active_provider(&state).await {
            Ok(Some(c)) => c,
            other => panic!("expected Ok(Some(_)), got {}", other.is_ok()),
        };
        assert_eq!(client.model_name(), "MiniMax-Text-01");
    }

    #[tokio::test]
    async fn provider_without_key_errors_naming_provider() {
        let state = fixture();
        insert_active_provider(&state.db);
        let err = match from_active_provider(&state).await {
            Err(e) => e,
            Ok(_) => panic!("expected Err when key is missing"),
        };
        let msg = err.to_string();
        assert!(msg.contains("Minimax"), "error should name provider: {msg}");
        assert!(msg.to_lowercase().contains("key"), "actionable msg: {msg}");
    }

    /// Without Pro, an active coding agent errors and never falls back to another provider or the mock.
    #[tokio::test]
    async fn an_active_cli_provider_without_pro_errors_naming_settings() {
        let state = fixture();
        let id = insert(
            &state.db,
            NewProvider {
                name: "Claude Code (local CLI)".to_string(),
                base_url: String::new(),
                model: "claude".to_string(),
                adapter: yogurt_db::providers::adapter::CLI.to_string(),
                cli_model: String::new(),
            },
        )
        .unwrap();
        set_active(&state.db, &id).unwrap();
        let err = match from_active_provider(&state).await {
            Err(e) => e,
            Ok(_) => panic!("expected Err for a coding agent without Pro"),
        };
        let msg = err.to_string();
        assert!(msg.contains("Claude Code (local CLI)"), "{msg}");
        assert!(msg.contains("Settings > Model"), "{msg}");
    }

    struct Fails;

    #[async_trait::async_trait]
    impl yogurt_llm::LlmClient for Fails {
        async fn complete(
            &self,
            _: yogurt_llm::ChatRequest,
        ) -> anyhow::Result<yogurt_llm::ChatResponse> {
            Err(anyhow::anyhow!("deadline has elapsed").context("error sending request"))
        }
        async fn stream(
            &self,
            _: yogurt_llm::ChatRequest,
        ) -> anyhow::Result<
            futures_util::stream::BoxStream<'static, anyhow::Result<yogurt_llm::ChatChunk>>,
        > {
            anyhow::bail!("unused")
        }
        fn model_name(&self) -> String {
            "fails".into()
        }
    }

    #[derive(Clone, Default)]
    struct Lines(Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for Lines {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn every_call_logs_one_llm_call_line_with_how_it_ended() {
        use futures_util::StreamExt;
        use yogurt_llm::{ChatMessage, ChatRequest, LlmClient};
        let out = Lines::default();
        let writer = out.clone();
        let _guard = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_writer(move || writer.clone())
                .with_ansi(false)
                .finish(),
        );
        // While one subscriber exists, tracing decides whether the `llm_call` line is on from the thread that reaches it first, and a test on another thread has none. A second one makes it ask every subscriber.
        let _second = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
        let req = || ChatRequest {
            messages: vec![ChatMessage::user("hi")],
            stream: false,
        };
        let db = Db::open_in_memory().unwrap();
        let logged = |inner: Arc<dyn LlmClient>| Logged {
            inner,
            purpose: "recap",
            meeting_id: Some("m1".into()),
            db: db.clone(),
        };

        let mock = logged(Arc::new(crate::llm_mock::MockLlm));
        mock.complete(req()).await.unwrap();
        let mut s = mock.stream(req()).await.unwrap();
        while s.next().await.is_some() {}
        drop(s);
        drop(mock.stream(req()).await.unwrap());
        // The task that reads the stream notices the caller went away the next time it runs.
        tokio::task::yield_now().await;
        assert!(logged(Arc::new(Fails)).complete(req()).await.is_err());

        let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<_> = text.lines().filter(|l| l.contains("llm_call")).collect();
        assert_eq!(lines.len(), 4, "{text}");
        assert!(
            lines
                .iter()
                .all(|l| l.contains("purpose=\"recap\"") && l.contains("meeting_id=\"m1\"")),
            "{text}"
        );
        assert!(lines[0].contains("outcome=\"ok\""), "{}", lines[0]);
        assert!(lines[1].contains("outcome=\"ok\""), "{}", lines[1]);
        assert!(lines[2].contains("outcome=\"unfinished\""), "{}", lines[2]);
        assert!(
            lines[3].contains("outcome=\"error\"")
                && lines[3].contains("error sending request: deadline has elapsed"),
            "{}",
            lines[3]
        );

        logged_rows(&db, 4).await;
        let rows: Vec<(String, i64, Option<f64>)> = db.with_conn(|c| {
            // Blocking threads write the rows, so their ids follow no call order.
            c.prepare(
                "SELECT outcome, input_tokens, cost_usd FROM llm_calls
                 ORDER BY CASE outcome WHEN 'ok' THEN 0 WHEN 'unfinished' THEN 1 ELSE 2 END",
            )
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
        });
        assert_eq!(
            rows.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(),
            ["ok", "ok", "unfinished", "error"]
        );
        assert_eq!(
            (rows[0].1, rows[0].2),
            (1, None),
            "no usage reported, so two characters round up to a token, and the mock has no price"
        );
        assert_eq!(rows[3].1, 0, "a failed call bills nothing");
    }

    /// Waits for the rows, which a blocking thread writes after the call returns, then a moment more so a row too many would show.
    async fn logged_rows(db: &Db, want: i64) {
        let count = || {
            db.with_conn(|c| {
                c.query_row("SELECT COUNT(*) FROM llm_calls", [], |r| r.get::<_, i64>(0))
                    .unwrap()
            })
        };
        for _ in 0..400 {
            if count() >= want {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(count(), want);
    }

    /// Ends its answer, then sends the usage in a chunk of its own, as OpenAI-style providers do.
    struct UsageAfterDone(&'static str);

    #[async_trait::async_trait]
    impl yogurt_llm::LlmClient for UsageAfterDone {
        async fn complete(
            &self,
            _: yogurt_llm::ChatRequest,
        ) -> anyhow::Result<yogurt_llm::ChatResponse> {
            unreachable!()
        }

        async fn stream(
            &self,
            _: yogurt_llm::ChatRequest,
        ) -> anyhow::Result<
            futures_util::stream::BoxStream<'static, anyhow::Result<yogurt_llm::ChatChunk>>,
        > {
            use futures_util::StreamExt;
            use yogurt_llm::{ChatChunk, Usage};
            Ok(futures_util::stream::iter([
                ChatChunk {
                    delta: "hello".into(),
                    done: true,
                    usage: None,
                },
                ChatChunk {
                    usage: Some(Usage {
                        input_tokens: 1_000_000,
                        output_tokens: 1_000_000,
                    }),
                    ..Default::default()
                },
            ])
            .map(Ok)
            .boxed())
        }

        fn model_name(&self) -> String {
            self.0.into()
        }
    }

    #[tokio::test]
    async fn a_stream_the_caller_stops_reading_at_its_last_word_still_records_its_usage() {
        use futures_util::StreamExt;
        use yogurt_llm::{ChatMessage, ChatRequest, LlmClient};
        let db = Db::open_in_memory().unwrap();
        for (model, purpose) in [
            ("MiniMax-M3", "enhance"),
            ("cli:claude", "chat"),
            ("MiniMax-M3", VOICE_CLEANUP),
        ] {
            let logged = Logged {
                inner: Arc::new(UsageAfterDone(model)),
                purpose,
                meeting_id: None,
                db: db.clone(),
            };
            let mut s = logged
                .stream(ChatRequest {
                    messages: vec![ChatMessage::user("hi")],
                    stream: true,
                })
                .await
                .unwrap();
            assert!(s.next().await.unwrap().unwrap().done);
            drop(s);
            tokio::task::yield_now().await;
        }

        logged_rows(&db, 2).await;
        let rows: Vec<(String, String, i64, Option<f64>)> = db.with_conn(|c| {
            c.prepare("SELECT purpose, kind, output_tokens, cost_usd FROM llm_calls ORDER BY id")
                .unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .map(Result::unwrap)
                .collect()
        });
        assert_eq!(
            rows,
            [
                ("enhance".into(), "api".into(), 1_000_000, Some(1.5)),
                ("chat".into(), "cli".into(), 1_000_000, Some(0.0)),
            ],
            "MiniMax-M3 lists at $0.30 in and $1.20 out a million, a CLI call costs nothing here, and Voice cleanup is not logged"
        );
    }
}
