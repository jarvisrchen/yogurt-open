//! LLM client behind the dyn-compatible [`LlmClient`] trait. No axum or web deps.
//! `base_url` always comes from the caller, so no provider host is hardcoded here.

#[cfg(feature = "local")]
mod local;
mod streaming;
mod types;

#[cfg(feature = "local")]
pub use local::LocalLlamaClient;
pub use types::{ChatChunk, ChatMessage, ChatRequest, ChatResponse, Usage};
pub use yogurt_prompts::thinking::strip_thinking;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures_util::stream::BoxStream;
use std::time::Duration;

/// The longest an HTTP provider may go without sending a byte, so a wedged one cannot hang the enhance handler.
/// The timer resets on every read, so a long stream that keeps sending runs to completion.
const HTTP_TIMEOUT: Duration = Duration::from_secs(60);

/// Budget for a client whose `stream` does not actually stream and must finish generating before it yields (the agent CLIs in `cli.rs`).
///
/// Against an HTTP provider `stream()` resolves at the response headers, so `HTTP_TIMEOUT` only has to cover a handshake.
/// `CliClient::stream` runs the subprocess to completion first, so the budget has to cover full generation.
/// `claude --model haiku` ignores `--effort low` and can spend 90+ s thinking on a short answer.
/// A wedged process is still bounded by the adapter's own `CLI_TIMEOUT`.
/// It also caps a whole HTTP call, so a provider that keeps sending bytes, keep-alives included, cannot run or bill forever.
const GENERATION_TIMEOUT: Duration = Duration::from_secs(300);

/// Connect (DNS + TCP + TLS) bound, so a typo'd base_url fails fast instead of stalling for `HTTP_TIMEOUT`.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

const CONNECT_RETRY_DELAY: Duration = Duration::from_secs(1);

/// A request that failed to connect never reached the provider, so one retry is safe and rides out a network blip.
async fn send(rb: reqwest::RequestBuilder) -> reqwest::Result<reqwest::Response> {
    let retry = rb.try_clone();
    match rb.send().await {
        Err(e) if e.is_connect() => match retry {
            Some(rb) => {
                tokio::time::sleep(CONNECT_RETRY_DELAY).await;
                rb.send().await
            }
            None => Err(e),
        },
        res => res,
    }
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    /// Must set `stream=false` on the wire regardless of `req.stream`.
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse>;

    /// Must set `stream=true` on the wire regardless of `req.stream`.
    async fn stream(&self, req: ChatRequest) -> Result<BoxStream<'static, Result<ChatChunk>>>;

    /// The model name sent on the wire, stamped into `meetings.llm_model`.
    fn model_name(&self) -> String;

    /// How long a caller should wait before calling this client failed, both for `stream()` to resolve and between chunks.
    ///
    /// A trait method because only the implementation knows what each await point covers. See [`GENERATION_TIMEOUT`].
    fn response_timeout(&self) -> Duration {
        HTTP_TIMEOUT
    }

    /// Opens the connection ahead of a call that is about to come, so it skips the handshake.
    async fn warm(&self) {}

    /// True for a model on this machine, which costs nothing per call.
    fn runs_locally(&self) -> bool {
        false
    }
}

fn http_client(idle: Duration, total: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .read_timeout(idle)
        .timeout(total)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

/// OpenAI-compatible HTTP adapter for the `/chat/completions` shape (OpenAI, MiniMax, Ollama, LM Studio, vLLM, OpenRouter, and similar).
#[derive(Clone)]
pub struct OpenAiCompatClient {
    base_url: String,
    api_key: String,
    model: String,
    http: reqwest::Client,
    usage: bool,
}

impl OpenAiCompatClient {
    /// Trailing `/` on `base_url` is stripped so the endpoint has exactly one separator.
    ///
    /// A failed client build falls back to `Client::new()` rather than panicking in a constructor.
    pub fn new(base_url: String, api_key: String, model: String) -> Self {
        let http = http_client(HTTP_TIMEOUT, GENERATION_TIMEOUT);
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            model,
            http,
            usage: true,
        }
    }

    /// Does nothing, since every client asks its streams for token `usage`.
    pub fn with_usage(mut self) -> Self {
        self.usage = true;
        self
    }

    pub(crate) fn usage_for_streaming(&self) -> bool {
        self.usage
    }

    fn endpoint(&self) -> String {
        format!("{}/chat/completions", self.base_url)
    }

    #[doc(hidden)]
    pub fn http_for_streaming(&self) -> &reqwest::Client {
        &self.http
    }

    #[doc(hidden)]
    pub fn base_url_for_streaming(&self) -> &str {
        &self.base_url
    }

    #[doc(hidden)]
    pub fn api_key_for_streaming(&self) -> &str {
        &self.api_key
    }

    #[doc(hidden)]
    pub fn model_for_streaming(&self) -> &str {
        &self.model
    }

    #[doc(hidden)]
    pub fn split_reasoning(&self) -> bool {
        self.model.to_ascii_lowercase().starts_with("minimax-")
    }

    /// MiniMax reasoning models spend an unpredictable 700-5700 hidden reasoning tokens per call, which makes a call slow and its length unpredictable.
    /// Enhance and chat are extraction jobs, so thinking is switched off.
    /// `reasoning_effort` is ignored by their API; only `thinking.type = "disabled"` works.
    /// Flash models reject it with a 400 ("requires adaptive thinking"), so they get no `thinking` field.
    #[doc(hidden)]
    pub(crate) fn thinking(&self) -> Option<types::Thinking> {
        (self.split_reasoning() && !self.model.to_ascii_lowercase().contains("flash"))
            .then_some(types::Thinking { r#type: "disabled" })
    }

    /// List the model ids the provider's `/models` endpoint advertises.
    ///
    /// Auth is attached only for a non-empty key: local runtimes (Ollama, LM Studio) need none.
    /// Google's compat shim returns ids like `models/gemini-2.5-flash`, so the `models/` prefix is stripped.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/models", self.base_url);
        let mut req = self.http.get(&url);
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }
        let resp = req.send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("LLM call failed: {status} - {}", self.scrub(&body)));
        }
        let parsed: types::ModelsResponse = resp.json().await?;
        Ok(parsed
            .data
            .into_iter()
            .map(|m| {
                m.id.strip_prefix("models/")
                    .map(str::to_string)
                    .unwrap_or(m.id)
            })
            .collect())
    }

    /// Replace this client's API key in `text` with `[key redacted]`, since providers quote the key back in error bodies.
    ///
    /// Keys under 4 chars are skipped: substring-replacing them would shred unrelated text.
    fn scrub(&self, text: &str) -> String {
        if self.api_key.len() < 4 {
            return text.to_string();
        }
        text.replace(&self.api_key, "[key redacted]")
    }
}

#[async_trait]
impl LlmClient for OpenAiCompatClient {
    fn model_name(&self) -> String {
        self.model.clone()
    }

    /// An Ollama, LM Studio, or llama.cpp server at a loopback address.
    fn runs_locally(&self) -> bool {
        reqwest::Url::parse(&self.base_url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.trim_matches(['[', ']']).to_string()))
            .is_some_and(|h| {
                h == "localhost"
                    || h.parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            })
    }

    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse> {
        let body = types::OpenAiRequest {
            model: &self.model,
            messages: &req.messages,
            stream: false,
            reasoning_split: self.split_reasoning(),
            thinking: self.thinking(),
            stream_options: None,
        };
        let resp = send(
            self.http
                .post(self.endpoint())
                .bearer_auth(&self.api_key)
                .json(&body),
        )
        .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("LLM call failed: {status} - {}", self.scrub(&body)));
        }
        let parsed: types::OpenAiResponse = resp.json().await?;
        let model = parsed.model;
        let usage = parsed.usage;
        let content = parsed
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no choices in response"))?
            .message
            .content;
        let content = strip_thinking(&content);
        Ok(ChatResponse {
            content,
            model,
            usage,
        })
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream<'static, Result<ChatChunk>>> {
        streaming::stream(self, req).await
    }

    /// Any response leaves the TLS connection in the pool, and a HEAD sends nothing but the URL.
    async fn warm(&self) {
        let _ = self.http.head(&self.base_url).send().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Serves one SSE response, sleeping `gaps[i]` before event `i`, then `[DONE]`.
    async fn sse_server(gaps: Vec<Duration>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 8192];
            let _ = sock.read(&mut buf).await;
            sock.write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
            )
            .await
            .unwrap();
            for gap in gaps {
                tokio::time::sleep(gap).await;
                let ev = "data: {\"choices\":[{\"delta\":{\"content\":\"x\"},\"finish_reason\":null}]}\n\n";
                if sock.write_all(ev.as_bytes()).await.is_err() {
                    return;
                }
            }
            let _ = sock.write_all(b"data: [DONE]\n\n").await;
        });
        format!("http://{addr}")
    }

    async fn stream_with_limits(
        base_url: String,
        idle: Duration,
        total: Duration,
    ) -> Result<String> {
        let mut client = OpenAiCompatClient::new(base_url, "sk-test".into(), "m".into());
        client.http = http_client(idle, total);
        let mut stream = client
            .stream(ChatRequest {
                messages: vec![ChatMessage::user("hi")],
                stream: true,
            })
            .await?;
        let mut text = String::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            text.push_str(&chunk.delta);
            if chunk.done {
                break;
            }
        }
        Ok(text)
    }

    #[tokio::test]
    async fn a_stream_longer_than_the_limit_finishes_while_it_keeps_sending() {
        let url = sse_server(vec![Duration::from_millis(300); 6]).await;
        let text = stream_with_limits(url, Duration::from_secs(1), Duration::from_secs(10))
            .await
            .expect("1.8 s of steady chunks must not hit a 1 s idle limit");
        assert_eq!(text, "xxxxxx");
    }

    #[tokio::test]
    async fn a_stream_that_goes_silent_past_the_limit_fails() {
        let url = sse_server(vec![Duration::ZERO, Duration::from_secs(3)]).await;
        let err = stream_with_limits(url, Duration::from_secs(1), Duration::from_secs(10))
            .await
            .expect_err("3 s of silence must hit a 1 s idle limit");
        assert!(format!("{err:#}").contains("timed out"), "{err:#}");
    }

    #[tokio::test]
    async fn a_stream_that_keeps_sending_past_the_total_cap_fails() {
        let url = sse_server(vec![Duration::from_millis(300); 10]).await;
        let err = stream_with_limits(url, Duration::from_secs(1), Duration::from_secs(1))
            .await
            .expect_err("3 s of steady chunks must hit a 1 s total cap");
        assert!(format!("{err:#}").contains("timed out"), "{err:#}");
    }
}
