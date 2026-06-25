//! The Voice cleanup settings and `/api/voice/cleanup/test` against a fake OpenAI-compatible provider.

use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use yogurt_server::state::AppState;
use yogurt_server::{session, storage, Mode};

async fn boot() -> (AppState, String, String) {
    let path = tempfile::tempdir().expect("tempdir").keep();
    let storage = Arc::new(storage::Storage::init_at(&path.join("db.sqlite")).expect("storage"));
    let session = Arc::new(session::load_or_create(&path.join("session-token")).expect("session"));
    let token = session.as_str().to_string();
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let state = AppState::in_memory(
        Mode::Release,
        storage,
        session,
        addr.port(),
        path.join("notes"),
    )
    .expect("state");
    let app = yogurt_server::__test_router(state.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (state, token, format!("http://{addr}"))
}

struct Api {
    http: reqwest::Client,
    token: String,
    base: String,
}

impl Api {
    async fn send(&self, m: reqwest::Method, p: &str, body: Value) -> (u16, Value) {
        let res = self
            .http
            .request(m, format!("{}{p}", self.base))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = res.status().as_u16();
        (status, res.json().await.unwrap_or(Value::Null))
    }

    async fn patch(&self, body: Value) -> (u16, Value) {
        self.send(reqwest::Method::PATCH, "/api/settings", body)
            .await
    }

    async fn test_cleanup(&self) -> Value {
        let (status, v) = self
            .send(reqwest::Method::POST, "/api/voice/cleanup/test", json!({}))
            .await;
        assert_eq!(status, 200, "{v}");
        v
    }

    async fn provider(&self, base_url: &str) -> String {
        let (_, p) = self
            .send(
                reqwest::Method::POST,
                "/api/settings/providers",
                json!({ "name": "Fast", "base_url": base_url, "model": "fast-1" }),
            )
            .await;
        let id = p["id"].as_str().unwrap().to_string();
        self.send(
            reqwest::Method::POST,
            &format!("/api/settings/providers/{id}/key"),
            json!({ "api_key": "sk-test-1234" }),
        )
        .await;
        id
    }
}

fn sse(deltas: &[&str]) -> String {
    let mut body: String = deltas
        .iter()
        .map(|d| {
            format!(
                "data: {}\n\n",
                json!({ "choices": [{ "delta": { "content": d }, "finish_reason": null }] })
            )
        })
        .collect();
    body.push_str("data: [DONE]\n\n");
    body
}

async fn llm(deltas: &[&str], delay: Duration) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(sse(deltas))
                .set_delay(delay),
        )
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn cleanup_defaults_to_builtin_which_reports_itself_unavailable() {
    // The built-in model downloads to the real ~/.yogurt/models, so a Mac that has it would load 2.5 GB here.
    // tests/local_models.rs covers the model itself.
    if yogurt_stt::models::resolve_model(&yogurt_stt::models::CLEANUP_MODEL).is_some() {
        return;
    }
    let (_s, token, base) = boot().await;
    let api = Api {
        http: reqwest::Client::new(),
        token,
        base,
    };
    let (_, settings) = api
        .send(reqwest::Method::GET, "/api/settings", Value::Null)
        .await;
    assert_eq!(settings["general"]["voice_cleanup"], false);
    assert_eq!(settings["general"]["voice_cleanup_provider"], "builtin");
    let v = api.test_cleanup().await;
    assert_eq!(v["outcome"], "unavailable");
    assert_eq!(v["words"], 45);
    assert!(v["error"].as_str().unwrap().contains("not downloaded yet"));
}

#[tokio::test]
async fn the_picker_takes_only_builtin_or_a_provider_and_falls_back_when_it_is_deleted() {
    let (s, token, base) = boot().await;
    let api = Api {
        http: reqwest::Client::new(),
        token,
        base,
    };
    let (status, _) = api.patch(json!({ "voice_cleanup_provider": "nope" })).await;
    assert_eq!(status, 400);
    // Stored directly, since only Pro can add one through the API and a free build can still hold one.
    let cli = yogurt_db::providers::insert(
        &s.db,
        yogurt_db::providers::NewProvider {
            name: "Claude Code".into(),
            base_url: String::new(),
            model: "claude".into(),
            adapter: yogurt_db::providers::adapter::CLI.into(),
            cli_model: String::new(),
        },
    )
    .unwrap();
    let (status, err) = api.patch(json!({ "voice_cleanup_provider": cli })).await;
    assert_eq!(status, 400, "a CLI cannot stream: {err}");
    let id = api.provider("https://example.test/v1").await;
    let (status, g) = api
        .patch(json!({ "voice_cleanup": true, "voice_cleanup_provider": id }))
        .await;
    assert_eq!(status, 200);
    assert_eq!(g["voice_cleanup_provider"], id.as_str());

    api.send(
        reqwest::Method::DELETE,
        &format!("/api/settings/providers/{id}"),
        Value::Null,
    )
    .await;
    let (_, settings) = api
        .send(reqwest::Method::GET, "/api/settings", Value::Null)
        .await;
    assert_eq!(settings["general"]["voice_cleanup_provider"], "builtin");
}

#[tokio::test]
async fn a_fast_provider_returns_its_cleaned_text_and_time() {
    let server = llm(&["So I think", " we should ship."], Duration::ZERO).await;
    let (_s, token, base) = boot().await;
    let api = Api {
        http: reqwest::Client::new(),
        token,
        base,
    };
    let id = api.provider(&format!("{}/v1", server.uri())).await;
    api.patch(json!({ "voice_cleanup_provider": id })).await;
    let v = api.test_cleanup().await;
    assert_eq!(v["outcome"], "done", "{v}");
    assert_eq!(v["text"], "So I think we should ship.");
    assert_eq!(v["model"], "fast-1");
    assert!(v["ms"].as_u64().unwrap() < 1_500);

    let sent: Value = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .find(|r| r.method.as_str() == "POST")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .unwrap();
    assert_eq!(sent["stream"], true);
    let messages = sent["messages"].as_array().unwrap();
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[1]["role"], "user");
    assert_eq!(messages[2]["role"], "assistant");
    let last = messages.last().unwrap();
    assert_eq!(last["role"], "user");
    assert!(last["content"]
        .as_str()
        .unwrap()
        .starts_with("<dictation>so um I think"));
}

#[tokio::test]
async fn a_provider_slower_than_1_5_s_to_answer_is_too_slow_with_its_real_time() {
    let server = llm(&["late"], Duration::from_millis(1_800)).await;
    let (_s, token, base) = boot().await;
    let api = Api {
        http: reqwest::Client::new(),
        token,
        base,
    };
    let id = api.provider(&format!("{}/v1", server.uri())).await;
    api.patch(json!({ "voice_cleanup_provider": id })).await;
    let v = api.test_cleanup().await;
    assert_eq!(v["outcome"], "too_slow", "{v}");
    assert_eq!(v["limit"], "first_token");
    assert!(v["ms"].as_u64().unwrap() >= 1_800, "{v}");
}

#[tokio::test]
async fn a_try_without_pro_cleans_again_without_the_stored_lists_and_saves_nothing() {
    let server = llm(
        &["Could you ask Xiaoyu", " to send the deck?"],
        Duration::ZERO,
    )
    .await;
    let (state, token, base) = boot().await;
    let api = Api {
        http: reqwest::Client::new(),
        token,
        base,
    };
    let id = api.provider(&format!("{}/v1", server.uri())).await;
    api.patch(json!({ "voice_cleanup": true, "voice_cleanup_provider": id }))
        .await;
    yogurt_db::style::update(&state.db, |s| {
        s.styles.insert(
            yogurt_db::style::Kind::Email,
            yogurt_db::style::Style::Formal,
        );
    })
    .unwrap();
    yogurt_db::dictionary::add(
        &state.db,
        "Xiaoyu",
        Some("shao yoo"),
        yogurt_db::dictionary::Source::Added,
        1,
    )
    .unwrap();

    let (status, v) = api
        .send(
            reqwest::Method::POST,
            "/api/voice/try",
            json!({ "kind": "work" }),
        )
        .await;
    assert_eq!(status, 204, "{v}");
    assert_eq!(state.voice.armed_try(), Some(yogurt_db::style::Kind::Work));
    api.send(
        reqwest::Method::POST,
        "/api/voice/try",
        json!({ "kind": null }),
    )
    .await;
    assert_eq!(state.voice.armed_try(), None);

    let raw = "um so can you ask shao yoo to send the deck";
    let (status, v) = api
        .send(
            reqwest::Method::POST,
            "/api/voice/try/clean",
            json!({ "raw": raw, "kind": "email" }),
        )
        .await;
    assert_eq!(status, 200, "{v}");
    assert_eq!(v["text"], "Could you ask Xiaoyu to send the deck?");
    assert_eq!(v["cleanup"], "done");
    assert_eq!(v["cleanup_model"], "fast-1");
    assert_eq!(v["style"], "normal");
    assert_eq!(v["words"], json!([]));
    assert_eq!(v["fillers"], json!(["um"]));
    let sent: Value = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .rev()
        .find(|r| r.method.as_str() == "POST")
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .unwrap();
    let messages = sent["messages"].to_string();
    assert!(!messages.contains("formal") && !messages.contains("Xiaoyu"));
    assert!(messages.contains("shao yoo"));

    let stats =
        yogurt_db::dictations::stats(&state.db, 0, yogurt_db::stats_range::Range::All).unwrap();
    assert_eq!(stats.count, 0, "a try is never saved");
}
