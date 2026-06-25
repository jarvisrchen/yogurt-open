use std::net::SocketAddr;
use std::time::Duration;
use yogurt_server::{Mode, RunConfig};

async fn spawn_server() -> (SocketAddr, String, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let token_path = tmp.path().join("session-token");
    let db_path = tmp.path().join("yogurt.sqlite");

    let token = yogurt_server::session::load_or_create(&token_path)
        .expect("seed session token in tempdir")
        .0;

    // Bind-drop-rebind races; the polling loop below tolerates it.
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind ephemeral");
    let addr: SocketAddr = probe.local_addr().expect("local_addr");
    drop(probe);

    std::env::set_var("YOGURT_MEMORY_KEYSTORE", "1");
    let cfg = RunConfig {
        addr,
        mode: Mode::Release,
        db_path: Some(db_path),
        session_token_path: Some(token_path),
        notes_dir: Some(tmp.path().join("notes")),
        // Parallel runs collide on the WAL lock of the real ~/.yogurt/db.sqlite.
        app_db_path: Some(tmp.path().join("yogurt-app.sqlite")),
        keys_path: None,
        voice_shortcut: None,
    };
    tokio::spawn(async move {
        let _ = yogurt_server::run_with_config(cfg).await;
    });

    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/api/health"))
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return (addr, token, tmp);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("server did not become reachable within 1 second");
}

#[tokio::test]
async fn it_lists_audio_devices() {
    let (addr, token, _tmp) = spawn_server().await;

    let client = reqwest::Client::new();
    let body = client
        .get(format!("http://{addr}/api/audio/devices"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("server reachable")
        .json::<serde_json::Value>()
        .await
        .expect("valid JSON");

    let arr = body.as_array().expect("response is a JSON array");

    // CI runners without audio devices may return an empty array.
    for entry in arr {
        assert!(
            entry.get("name").and_then(|v| v.as_str()).is_some(),
            "device entry missing `name` string: {entry:?}"
        );
        assert!(
            entry.get("is_default").and_then(|v| v.as_bool()).is_some(),
            "device entry missing `is_default` bool: {entry:?}"
        );
        // sample_rate may be null when cpal cannot report one.
        assert!(
            entry.get("sample_rate").is_some(),
            "device entry missing `sample_rate` key: {entry:?}"
        );
    }
}

#[tokio::test]
async fn it_reports_permission_status() {
    let (addr, token, _tmp) = spawn_server().await;

    let client = reqwest::Client::new();
    let body = client
        .get(format!("http://{addr}/api/audio/permission"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("server reachable")
        .json::<serde_json::Value>()
        .await
        .expect("valid JSON");

    let screen_recording = body
        .get("screen_recording")
        .and_then(|v| v.as_str())
        .expect("response has `screen_recording` string field");
    let microphone = body
        .get("microphone")
        .and_then(|v| v.as_str())
        .expect("response has `microphone` string field");

    assert!(
        matches!(
            screen_recording,
            "granted" | "denied" | "not_determined" | "not_required"
        ),
        "screen_recording must be a known status, got {screen_recording:?}"
    );
    assert!(
        matches!(
            microphone,
            "granted" | "denied" | "not_determined" | "not_required"
        ),
        "microphone must be a known status, got {microphone:?}"
    );
    assert!(
        body.get("owner").and_then(|v| v.as_str()).is_some(),
        "macOS always has a responsible process to name, got {body}"
    );
}

#[tokio::test]
async fn it_reports_voice_status_and_pauses_behind_the_token() {
    let (addr, token, _tmp) = spawn_server().await;
    let client = reqwest::Client::new();

    let body = client
        .get(format!("http://{addr}/api/voice/status"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("server reachable")
        .json::<serde_json::Value>()
        .await
        .expect("valid JSON");
    assert!(
        matches!(body["accessibility"].as_str(), Some("granted" | "missing")),
        "accessibility must be granted or missing, got {body}"
    );
    // The test server runs no voice task, so nothing arms the hotkey.
    assert_eq!(body["running"], false, "got {body}");
    assert!(body["status"].is_string(), "got {body}");

    for recording in [true, false] {
        let resp = client
            .post(format!("http://{addr}/api/voice/record"))
            .bearer_auth(&token)
            .json(&serde_json::json!({ "recording": recording }))
            .send()
            .await
            .expect("server reachable");
        assert_eq!(resp.status(), reqwest::StatusCode::NO_CONTENT);
        let step = client
            .get(format!("http://{addr}/api/voice/record"))
            .bearer_auth(&token)
            .send()
            .await
            .expect("server reachable")
            .json::<serde_json::Value>()
            .await
            .expect("valid JSON");
        // Nothing presses keys here, so the recording only ever waits.
        assert_eq!(step, serde_json::json!({ "kind": "pending", "held": [] }));
    }
    let resp = reqwest::get(format!("http://{addr}/api/voice/record"))
        .await
        .expect("server reachable");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    let resp = reqwest::get(format!("http://{addr}/api/voice/status"))
        .await
        .expect("server reachable");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
    let resp = client
        .post(format!("http://{addr}/api/voice/record"))
        .json(&serde_json::json!({ "recording": true }))
        .send()
        .await
        .expect("server reachable");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

/// TCC state is environment-dependent, so only the shape is asserted.
#[tokio::test]
async fn it_request_microphone_returns_combined_snapshot() {
    let (addr, token, _tmp) = spawn_server().await;

    let client = reqwest::Client::new();
    let body = client
        .post(format!("http://{addr}/api/audio/microphone/request"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("server reachable")
        .json::<serde_json::Value>()
        .await
        .expect("valid JSON");

    let screen_recording = body
        .get("screen_recording")
        .and_then(|v| v.as_str())
        .expect("response has `screen_recording` string field");
    let microphone = body
        .get("microphone")
        .and_then(|v| v.as_str())
        .expect("response has `microphone` string field");
    assert!(matches!(
        screen_recording,
        "granted" | "denied" | "not_determined" | "not_required"
    ));
    assert!(matches!(
        microphone,
        "granted" | "denied" | "not_determined" | "not_required"
    ));
}

/// TCC state is environment-dependent, so only the shape is asserted.
#[tokio::test]
async fn it_request_screen_recording_returns_combined_snapshot() {
    let (addr, token, _tmp) = spawn_server().await;

    let client = reqwest::Client::new();
    let body = client
        .post(format!("http://{addr}/api/audio/screen-recording/request"))
        .bearer_auth(&token)
        .send()
        .await
        .expect("server reachable")
        .json::<serde_json::Value>()
        .await
        .expect("valid JSON");

    let screen_recording = body
        .get("screen_recording")
        .and_then(|v| v.as_str())
        .expect("response has `screen_recording` string field");
    let microphone = body
        .get("microphone")
        .and_then(|v| v.as_str())
        .expect("response has `microphone` string field");
    assert!(matches!(
        screen_recording,
        "granted" | "denied" | "not_determined" | "not_required"
    ));
    assert!(matches!(
        microphone,
        "granted" | "denied" | "not_determined" | "not_required"
    ));
}

#[tokio::test]
async fn it_rejects_unauthenticated_audio_calls() {
    let (addr, _token, _tmp) = spawn_server().await;

    let resp = reqwest::get(format!("http://{addr}/api/audio/devices"))
        .await
        .expect("server reachable");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);

    let resp = reqwest::get(format!("http://{addr}/api/audio/permission"))
        .await
        .expect("server reachable");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn it_rejects_wrong_token_audio_calls() {
    let (addr, _token, _tmp) = spawn_server().await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/api/audio/devices"))
        .bearer_auth("wrong-token-value-that-does-not-match-the-real-one")
        .send()
        .await
        .expect("server reachable");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn it_accepts_query_string_token_audio_calls() {
    let (addr, token, _tmp) = spawn_server().await;

    let resp = reqwest::get(format!("http://{addr}/api/audio/permission?token={token}"))
        .await
        .expect("server reachable");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
}
