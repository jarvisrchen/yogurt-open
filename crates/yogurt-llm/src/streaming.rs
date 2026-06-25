use crate::{types, ChatChunk, ChatRequest, OpenAiCompatClient};
use anyhow::{anyhow, Result};
use eventsource_stream::Eventsource;
use futures_util::stream::{BoxStream, StreamExt};
use yogurt_prompts::thinking::ThinkStripper;

pub(crate) async fn stream(
    client: &OpenAiCompatClient,
    req: ChatRequest,
) -> Result<BoxStream<'static, Result<ChatChunk>>> {
    let open = |usage: bool| {
        let body = types::OpenAiRequest {
            model: client.model_for_streaming(),
            messages: &req.messages,
            stream: true,
            reasoning_split: client.split_reasoning(),
            thinking: client.thinking(),
            stream_options: usage.then_some(types::StreamOptions {
                include_usage: true,
            }),
        };
        crate::send(
            client
                .http_for_streaming()
                .post(format!(
                    "{}/chat/completions",
                    client.base_url_for_streaming()
                ))
                .bearer_auth(client.api_key_for_streaming())
                .json(&body),
        )
    };

    let usage = client.usage_for_streaming();
    let failed = |status: reqwest::StatusCode, body: &str| {
        anyhow!("LLM stream open failed: {status} - {}", client.scrub(body))
    };
    let mut resp = open(usage).await?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        // A strict OpenAI-compatible server rejects `stream_options` as an unknown field and names it, and the answer matters more than its token count.
        // shortcut: such a server gets two requests on every call, remember the rejection per client if one turns out to be common.
        let rejected_usage = usage
            && matches!(
                status,
                reqwest::StatusCode::BAD_REQUEST | reqwest::StatusCode::UNPROCESSABLE_ENTITY
            )
            && (body.contains("stream_options") || body.contains("include_usage"));
        if !rejected_usage {
            return Err(failed(status, &body));
        }
        resp = open(false).await?;
        if !resp.status().is_success() {
            let status = resp.status();
            return Err(failed(status, &resp.text().await.unwrap_or_default()));
        }
    }

    let byte_stream = resp.bytes_stream();
    let events = byte_stream.eventsource();

    let mapped = events.map(|ev| -> Result<ChatChunk> {
        let ev = ev.map_err(|e| match &e {
            // reqwest reports its own timeout as "error decoding response body", which reads like a bad payload.
            eventsource_stream::EventStreamError::Transport(t) if t.is_timeout() => anyhow!(
                "LLM stream timed out: the provider sent nothing for {}s, or the reply passed {}s",
                crate::HTTP_TIMEOUT.as_secs(),
                crate::GENERATION_TIMEOUT.as_secs()
            ),
            _ => anyhow!("SSE parse error: {e}"),
        })?;
        // `[DONE]` terminates the stream. A `finish_reason` chunk may already have set `done`, so consumers must treat repeats as idempotent.
        if ev.data.trim() == "[DONE]" {
            return Ok(ChatChunk {
                done: true,
                ..Default::default()
            });
        }
        // The payload is the model's answer, which quotes the user's meeting or dictation, so it stays out of the error and the logs.
        let chunk: types::OpenAiStreamChunk =
            serde_json::from_str(&ev.data).map_err(|e| anyhow!("invalid chunk JSON: {e}"))?;
        let usage = chunk.usage;
        let (delta, done) = match chunk.choices.into_iter().next() {
            Some(choice) => (
                choice.delta.content.unwrap_or_default(),
                choice.finish_reason.is_some(),
            ),
            // Heartbeat or metadata-only frame: empty mid-stream chunk so the caller can tick its activity timer.
            None => (String::new(), false),
        };
        Ok(ChatChunk { delta, done, usage })
    });

    // Stateful so a tag split across chunk boundaries is still stripped.
    let mut stripper = ThinkStripper::new();
    let stripped = mapped.map(move |chunk_res| {
        let mut chunk = chunk_res?;
        chunk.delta = stripper.push(&chunk.delta);
        if chunk.done {
            chunk.delta.push_str(&stripper.flush());
        }
        Ok(chunk)
    });

    Ok(stripped.boxed())
}
