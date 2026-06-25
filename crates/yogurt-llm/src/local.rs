//! The built-in cleanup model: llama.cpp in this process, on Metal, over a GGUF file the caller downloaded.
//!
//! One worker thread owns the model and its context, because `LlamaContext` borrows the model and neither may cross threads.
//! The context keeps the KV state of the last prompt and answer, so a call that starts with the same tokens decodes only what follows them.
//! The cleanup system prompt and its example turns are identical on every call, so a dictation decodes just its own words.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, OnceLock};

use anyhow::{anyhow, bail, Context as _, Result};
use async_trait::async_trait;
use futures_util::stream::{self, BoxStream, StreamExt};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use tokio::sync::mpsc as tokio_mpsc;

use crate::{ChatChunk, ChatMessage, ChatRequest, ChatResponse, LlmClient};

/// Prompt plus answer. A 300-word dictation is about 400 tokens each way, and the KV cache costs about 144 KB a token for Qwen3 4B.
const N_CTX: u32 = 4096;

type Chunks = tokio_mpsc::UnboundedSender<Result<ChatChunk>>;

enum Job {
    Warm(tokio::sync::oneshot::Sender<()>),
    Chat(Vec<ChatMessage>, Chunks),
}

/// A local GGUF model behind [`LlmClient`]. The model loads on the first call or [`LlmClient::warm`].
/// Dropping the client waits for the worker to finish its job and free the model.
pub struct LocalLlamaClient {
    name: String,
    jobs: mpsc::Sender<Job>,
    failed: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl LocalLlamaClient {
    /// Starts the worker and returns at once. A file that fails to load fails every call with the reason.
    pub fn new(path: PathBuf, name: impl Into<String>) -> Self {
        let (jobs, rx) = mpsc::channel();
        let failed = Arc::new(AtomicBool::new(false));
        let flag = failed.clone();
        let worker = std::thread::Builder::new()
            .name("yogurt-llama".into())
            .spawn(move || work(&path, rx, &flag))
            .expect("spawn the llama.cpp worker thread");
        Self {
            name: name.into(),
            jobs,
            failed,
            worker: Some(worker),
        }
    }

    /// The model failed to load, so every call fails, and a new client is the only way to try again.
    pub fn load_failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    fn send(&self, job: Job) -> Result<()> {
        self.jobs
            .send(job)
            .map_err(|_| anyhow!("the built-in model's worker stopped"))
    }
}

/// A worker still freeing the model while the process exits races ggml's own teardown, which crashed a test with SIGSEGV.
impl Drop for LocalLlamaClient {
    fn drop(&mut self) {
        // Swapping in a sender whose receiver is already gone closes the worker's queue.
        drop(std::mem::replace(&mut self.jobs, mpsc::channel().0));
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[async_trait]
impl LlmClient for LocalLlamaClient {
    async fn complete(&self, req: ChatRequest) -> Result<ChatResponse> {
        let mut chunks = self.stream(req).await?;
        let mut content = String::new();
        while let Some(chunk) = chunks.next().await {
            content.push_str(&chunk?.delta);
        }
        Ok(ChatResponse {
            content,
            model: self.name.clone(),
            usage: None,
        })
    }

    async fn stream(&self, req: ChatRequest) -> Result<BoxStream<'static, Result<ChatChunk>>> {
        let (tx, rx) = tokio_mpsc::unbounded_channel();
        self.send(Job::Chat(req.messages, tx))?;
        Ok(stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|c| (c, rx)) }).boxed())
    }

    fn model_name(&self) -> String {
        self.name.clone()
    }

    fn runs_locally(&self) -> bool {
        true
    }

    /// Resolves once the model has loaded, or failed to, so a caller can keep the load out of a timed call.
    async fn warm(&self) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if self.send(Job::Warm(tx)).is_ok() {
            let _ = rx.await;
        }
    }
}

/// llama.cpp allows one backend per process, and it lives as long as the process.
fn backend() -> Result<&'static LlamaBackend> {
    static BACKEND: OnceLock<std::result::Result<LlamaBackend, String>> = OnceLock::new();
    BACKEND
        .get_or_init(|| {
            llama_cpp_2::send_logs_to_tracing(llama_cpp_2::LogOptions::default());
            LlamaBackend::init().map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| anyhow!("could not start llama.cpp: {e}"))
}

fn work(path: &std::path::Path, jobs: mpsc::Receiver<Job>, failed: &AtomicBool) {
    let Ok(first) = jobs.recv() else { return };
    let jobs = std::iter::once(first).chain(jobs.iter());
    let started = std::time::Instant::now();
    let model = backend().and_then(|b| {
        // CI only: the runner's virtual GPU ran a 0.6B model at 6.6 s a token, where its CPU is fast enough.
        let gpu_layers = if std::env::var_os("YOGURT_LLAMA_CPU").is_some() {
            0
        } else {
            u32::MAX
        };
        let params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
        LlamaModel::load_from_file(b, path, &params)
            .with_context(|| format!("could not load {}", path.display()))
    });
    let mut ctx = model.as_ref().map_err(|e| format!("{e:#}")).and_then(|m| {
        let params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(N_CTX))
            .with_n_batch(N_CTX);
        m.new_context(backend().map_err(|e| e.to_string())?, params)
            .map_err(|e| format!("could not create a llama.cpp context: {e}"))
    });
    match &ctx {
        Ok(_) => {
            tracing::info!(ms = started.elapsed().as_millis() as u64, path = %path.display(), "built-in model loaded")
        }
        Err(e) => {
            tracing::warn!(error = %e, "built-in model failed to load");
            failed.store(true, Ordering::Relaxed);
        }
    }
    let mut cached = Vec::new();
    for job in jobs {
        let (messages, out) = match job {
            Job::Warm(loaded) => {
                let _ = loaded.send(());
                continue;
            }
            Job::Chat(messages, out) => (messages, out),
        };
        let (model, ctx) = match (&model, &mut ctx) {
            (Ok(m), Ok(c)) => (m, c),
            (_, Err(e)) => {
                let _ = out.send(Err(anyhow!("{e}")));
                continue;
            }
            (Err(_), Ok(_)) => unreachable!("a context needs a model"),
        };
        if let Err(e) = chat(model, ctx, &mut cached, &messages, &out) {
            // A failed decode can leave the KV state half written.
            ctx.clear_kv_cache();
            cached.clear();
            let _ = out.send(Err(e));
        }
    }
}

/// `cached` holds the tokens whose KV state the context already has, in order from position 0.
fn chat(
    model: &LlamaModel,
    ctx: &mut LlamaContext,
    cached: &mut Vec<LlamaToken>,
    messages: &[ChatMessage],
    out: &Chunks,
) -> Result<()> {
    let chat = messages
        .iter()
        .map(|m| LlamaChatMessage::new(m.role.clone(), m.content.clone()))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let prompt = model.apply_chat_template(&model.chat_template(None)?, &chat, true)?;
    let vocab = model.vocab();
    let tokens = vocab.tokenize(prompt.as_bytes(), false, true);
    let n_ctx = ctx.n_ctx() as usize;
    if tokens.is_empty() || tokens.len() >= n_ctx {
        bail!(
            "the prompt is {} tokens, and the built-in model takes under {n_ctx}",
            tokens.len()
        );
    }

    // At least the last prompt token is decoded again, because sampling needs its logits.
    let keep = shared_prefix(cached, &tokens).min(tokens.len() - 1);
    if !ctx.clear_kv_cache_seq(Some(0), Some(keep as u32), None)? {
        bail!("llama.cpp could not trim the KV cache");
    }
    cached.truncate(keep);
    let mut batch = LlamaBatch::new(n_ctx, 1);
    let last = tokens.len() - 1;
    for (pos, &t) in tokens.iter().enumerate().skip(keep) {
        batch.add(t, pos as i32, &[0], pos == last)?;
    }
    ctx.decode(&mut batch)?;
    cached.extend_from_slice(&tokens[keep..]);

    let mut sampler = LlamaSampler::greedy();
    let mut text = Utf8Pieces::default();
    loop {
        let token = sampler.sample(ctx, batch.n_tokens() - 1);
        if vocab.is_eog(token) {
            break;
        }
        if cached.len() >= n_ctx {
            bail!("the answer ran past the built-in model's {n_ctx}-token context");
        }
        let delta = text.push(&vocab.token_to_piece(token, false, None));
        // The caller gave up, so the rest of the answer is wasted work.
        if !delta.is_empty()
            && out
                .send(Ok(ChatChunk {
                    delta,
                    done: false,
                    usage: None,
                }))
                .is_err()
        {
            return Ok(());
        }
        batch.clear();
        batch.add(token, cached.len() as i32, &[0], true)?;
        ctx.decode(&mut batch)?;
        cached.push(token);
    }
    let _ = out.send(Ok(ChatChunk {
        delta: text.rest(),
        done: true,
        usage: None,
    }));
    Ok(())
}

fn shared_prefix(a: &[LlamaToken], b: &[LlamaToken]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| x == y).count()
}

/// A token can end partway through a UTF-8 character, which waits for the next token.
#[derive(Default)]
struct Utf8Pieces(Vec<u8>);

impl Utf8Pieces {
    fn push(&mut self, bytes: &[u8]) -> String {
        self.0.extend_from_slice(bytes);
        let valid = match std::str::from_utf8(&self.0) {
            Ok(s) => s.len(),
            // An incomplete character at the end waits. Bytes that can never be valid become U+FFFD.
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            Err(_) => self.0.len(),
        };
        let done: Vec<u8> = self.0.drain(..valid).collect();
        String::from_utf8_lossy(&done).into_owned()
    }

    fn rest(&mut self) -> String {
        String::from_utf8_lossy(&std::mem::take(&mut self.0)).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_split_across_tokens_waits_for_its_last_byte() {
        let mut t = Utf8Pieces::default();
        let e = "é".as_bytes();
        assert_eq!(t.push(b"caf"), "caf");
        assert_eq!(t.push(&e[..1]), "");
        assert_eq!(t.push(&e[1..]), "é");
        assert_eq!(t.push(&[0xff, b'x']), "\u{fffd}x");
        assert_eq!(t.push(&e[..1]), "");
        assert_eq!(t.rest(), "\u{fffd}");
    }

    #[test]
    fn the_shared_prefix_stops_at_the_first_difference() {
        let t = |v: &[i32]| v.iter().map(|&i| LlamaToken::new(i)).collect::<Vec<_>>();
        assert_eq!(shared_prefix(&t(&[1, 2, 3]), &t(&[1, 2, 4, 5])), 2);
        assert_eq!(shared_prefix(&t(&[]), &t(&[1])), 0);
        assert_eq!(shared_prefix(&t(&[1, 2]), &t(&[1, 2])), 2);
    }
}
