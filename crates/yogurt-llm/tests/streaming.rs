//! SSE streaming round-trip against a wiremock stand-in OpenAI server.

use futures_util::StreamExt;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use yogurt_llm::{ChatMessage, ChatRequest, LlmClient, OpenAiCompatClient};

#[tokio::test]
async fn it_streams_sse_chunks_into_chat_chunks() {
    // The fourth event carries `finish_reason: "stop"` with an empty delta, as real providers send just before `[DONE]`.
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"lo \"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"yogurt.\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = OpenAiCompatClient::new(server.uri(), "sk-test".into(), "gpt-4o-mini".into());
    let mut stream = client
        .stream(ChatRequest {
            messages: vec![ChatMessage::user("hi")],
            stream: true,
        })
        .await
        .expect("stream opens");

    let mut deltas: Vec<String> = Vec::new();
    let mut saw_done = false;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("chunk ok");
        if !chunk.delta.is_empty() {
            deltas.push(chunk.delta);
        }
        if chunk.done {
            saw_done = true;
        }
    }
    assert_eq!(
        deltas.join(""),
        "Hello yogurt.",
        "deltas should accumulate to the upstream text"
    );
    assert!(
        saw_done,
        "stream should emit at least one terminal `done=true` chunk"
    );
}

#[tokio::test]
async fn it_surfaces_non_2xx_stream_open_as_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .mount(&server)
        .await;

    let client = OpenAiCompatClient::new(server.uri(), "sk-bad".into(), "m".into());
    // `BoxStream` is not `Debug`, so `.expect_err` won't compile.
    let result = client
        .stream(ChatRequest {
            messages: vec![ChatMessage::user("hi")],
            stream: true,
        })
        .await;
    let err = match result {
        Ok(_) => panic!("expected stream-open to fail on 429"),
        Err(e) => e,
    };
    let msg = format!("{err:#}");
    assert!(
        msg.contains("429") || msg.contains("rate limited"),
        "expected status or body in error, got: {msg}"
    );
}

/// Reasoning models leak `<think>…</think>` into `delta.content` when the provider ignores `reasoning_split`, so the stream must strip it.
#[tokio::test]
async fn it_strips_leading_think_blocks_from_streamed_deltas() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<think>\\nlet me think...\\n</think>\\n\\n\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"They agreed on 25% Monday.\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = OpenAiCompatClient::new(server.uri(), "sk-test".into(), "MiniMax-M3".into());
    let mut stream = client
        .stream(ChatRequest {
            messages: vec![ChatMessage::user("what was decided?")],
            stream: true,
        })
        .await
        .expect("stream opens");

    let mut accumulated = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("chunk ok");
        accumulated.push_str(&chunk.delta);
    }
    assert_eq!(
        accumulated, "They agreed on 25% Monday.",
        "think block + whitespace must be stripped; got: {accumulated:?}"
    );
}

/// A think-tag opening split across two SSE deltas must be buffered so the user never sees the partial tag.
#[tokio::test]
async fn it_strips_think_tags_split_across_chunk_boundaries() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<think>\\nrea\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"soning</think>\\n\\n\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"Final answer.\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
        "data: [DONE]\n\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = OpenAiCompatClient::new(server.uri(), "sk-test".into(), "MiniMax-M3".into());
    let mut stream = client
        .stream(ChatRequest {
            messages: vec![ChatMessage::user("hi")],
            stream: true,
        })
        .await
        .expect("stream opens");

    let mut accumulated = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("chunk ok");
        accumulated.push_str(&chunk.delta);
    }
    assert_eq!(
        accumulated, "Final answer.",
        "split-tag thinking must be elided; got: {accumulated:?}"
    );
}

#[tokio::test]
async fn it_keeps_answer_when_final_chunk_completes_close_tag() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"<think>reasoning</th\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"ink>Final answer.\"},\"finish_reason\":\"stop\"}]}\n\n",
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = OpenAiCompatClient::new(server.uri(), "sk-test".into(), "MiniMax-M3".into());
    let mut stream = client
        .stream(ChatRequest {
            messages: vec![ChatMessage::user("hi")],
            stream: true,
        })
        .await
        .expect("stream opens");

    let mut accumulated = String::new();
    while let Some(chunk) = stream.next().await {
        accumulated.push_str(&chunk.expect("chunk ok").delta);
    }
    assert_eq!(accumulated, "Final answer.");
}

/// OpenAI's way: the usage arrives in a chunk of its own with no choices, after the `finish_reason` one.
#[tokio::test]
async fn a_client_asks_for_usage_and_reads_its_chunk() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"Hi.\"},\"finish_reason\":\"stop\"}]}\n\n",
        "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":612,\"completion_tokens\":18,\"total_tokens\":630}}\n\n",
        "data: [DONE]\n\n",
    );
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(
            serde_json::json!({ "stream_options": { "include_usage": true } }),
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(body),
        )
        .mount(&server)
        .await;

    let chunks: Vec<_> = OpenAiCompatClient::new(server.uri(), "sk-test".into(), "m".into())
        .stream(ChatRequest {
            messages: vec![ChatMessage::user("hi")],
            stream: true,
        })
        .await
        .expect("stream opens")
        .map(|c| c.expect("chunk ok"))
        .collect()
        .await;
    assert_eq!(
        chunks.iter().find_map(|c| c.usage),
        Some(yogurt_llm::Usage {
            input_tokens: 612,
            output_tokens: 18
        })
    );
}

#[tokio::test]
async fn a_server_that_rejects_the_usage_option_is_asked_again_without_it() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_partial_json(
            serde_json::json!({ "stream_options": { "include_usage": true } }),
        ))
        .respond_with(ResponseTemplate::new(400).set_body_string("unknown field stream_options"))
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(concat!(
                    "data: {\"choices\":[{\"delta\":{\"content\":\"Hi.\"},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n",
                )),
        )
        .mount(&server)
        .await;

    let text: String = OpenAiCompatClient::new(server.uri(), "sk-test".into(), "m".into())
        .stream(ChatRequest {
            messages: vec![ChatMessage::user("hi")],
            stream: true,
        })
        .await
        .expect("stream opens on the second try")
        .map(|c| c.expect("chunk ok").delta)
        .collect()
        .await;
    assert_eq!(text, "Hi.");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[test]
fn only_a_loopback_server_runs_locally() {
    let local =
        |url: &str| OpenAiCompatClient::new(url.into(), String::new(), "m".into()).runs_locally();
    assert!(local("http://localhost:11434/v1"));
    assert!(local("http://127.0.0.1:1234/v1"));
    assert!(local("http://[::1]:8080/v1"));
    assert!(!local("https://api.deepseek.com/v1"));
    assert!(!local("https://localhost.example.com/v1"));
    assert!(!local(""));
}
