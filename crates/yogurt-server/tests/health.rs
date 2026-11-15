use std::net::SocketAddr;
use std::time::Duration;
use yogurt_server::{run_with_config, Mode, RunConfig};

#[tokio::test]
async fn it_responds_to_health() {
    let tmp = tempfile::tempdir().expect("create tempdir");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let addr: SocketAddr = listener.local_addr().expect("local_addr");

    std::env::set_var("YOGURT_MEMORY_KEYSTORE", "1");
    let cfg = RunConfig {
        listener,
        mode: Mode::Release,
        db_path: Some(tmp.path().join("yogurt-test.db")),
        session_token_path: Some(tmp.path().join("session-token")),
        notes_dir: Some(tmp.path().join("notes")),
        app_db_path: Some(tmp.path().join("yogurt-app.sqlite")),
        keys_path: None,
        voice_shortcut: None,
    };
    let handle = tokio::spawn(async move {
        let _ = run_with_config(cfg).await;
    });

    for _ in 0..50 {
        if reqwest::get(format!("http://{addr}/api/health"))
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let body = reqwest::get(format!("http://{addr}/api/health"))
        .await
        .expect("server reachable")
        .json::<serde_json::Value>()
        .await
        .expect("valid JSON");

    assert_eq!(body["status"], "ok");
    assert_eq!(body["service"], "yogurt-server");
    // `yogurt ctl` tells instances apart by version + mode.
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["mode"], "release");
    handle.abort();
}
