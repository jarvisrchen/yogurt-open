use std::time::Duration;

use serde_json::Value;
use yogurt_db::dictations::{self, NewDictation};
use yogurt_server::{run_with_config, Mode, RunConfig};

struct Server {
    addr: std::net::SocketAddr,
    token: String,
    db: yogurt_db::Db,
    _handle: tokio::task::JoinHandle<()>,
    _tmp: tempfile::TempDir,
}

async fn spawn_server() -> Server {
    let tmp = tempfile::tempdir().unwrap();
    let token_path = tmp.path().join("session-token");
    let app_db_path = tmp.path().join("yogurt-app.sqlite");
    let token = yogurt_server::session::load_or_create(&token_path)
        .unwrap()
        .as_str()
        .to_string();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
    let addr = listener.local_addr().unwrap();

    std::env::set_var("YOGURT_MEMORY_KEYSTORE", "1");
    let cfg = RunConfig {
        listener,
        mode: Mode::Release,
        db_path: Some(tmp.path().join("yogurt-test.db")),
        session_token_path: Some(token_path),
        notes_dir: Some(tmp.path().join("notes")),
        app_db_path: Some(app_db_path.clone()),
        keys_path: None,
        voice_shortcut: None,
    };
    let handle = tokio::spawn(async move {
        let _ = run_with_config(cfg).await;
    });
    for _ in 0..400 {
        if reqwest::get(format!("http://{addr}/api/health"))
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
        {
            return Server {
                addr,
                token,
                db: yogurt_db::Db::open(&app_db_path).unwrap(),
                _handle: handle,
                _tmp: tmp,
            };
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("server did not become reachable within 8 seconds");
}

impl Server {
    fn seed(&self, ago_ms: i64, app: Option<&str>, hands_free: bool, text: &str) -> String {
        let now = chrono::Utc::now().timestamp_millis();
        dictations::insert(
            &self.db,
            NewDictation {
                created_at: now - ago_ms,
                app_name: app.map(str::to_string),
                hands_free,
                audio_ms: 4_000,
                raw_text: text.into(),
                text: text.into(),
                stt_engine: "local".into(),
                stt_ms: 350,
                cleanup: "off".into(),
                ..Default::default()
            },
            true,
        )
        .unwrap()
        .id
    }

    async fn get(&self, path: &str) -> Value {
        let res = reqwest::Client::new()
            .get(format!("http://{}{path}", self.addr))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "{path}");
        res.json().await.unwrap()
    }

    async fn patch_settings(&self, body: Value) -> reqwest::Response {
        reqwest::Client::new()
            .patch(format!("http://{}/api/settings", self.addr))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .unwrap()
    }

    async fn delete(&self, path: &str) -> reqwest::Response {
        reqwest::Client::new()
            .delete(format!("http://{}{path}", self.addr))
            .bearer_auth(&self.token)
            .send()
            .await
            .unwrap()
    }
}

fn ids(page: &Value) -> Vec<&str> {
    page["dictations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn history_pages_hold_25_newest_first() {
    let s = spawn_server().await;
    let seeded: Vec<String> = (0..26)
        .map(|i| s.seed(i * 1_000, Some("Notes"), false, &format!("note {i}")))
        .collect();

    let page0 = s.get("/api/voice/dictations").await;
    assert_eq!(page0["total"], 26);
    assert_eq!(
        ids(&page0),
        seeded[..25].iter().map(String::as_str).collect::<Vec<_>>()
    );
    let page1 = s.get("/api/voice/dictations?page=1").await;
    assert_eq!(ids(&page1), [seeded[25].as_str()]);
}

#[tokio::test(flavor = "multi_thread")]
async fn list_filter_search_and_delete() {
    let s = spawn_server().await;
    let slack = s.seed(3_000, Some("Slack"), false, "send the launch deck");
    let notes = s.seed(2_000, Some("Notes"), true, "plan for next week");
    let unknown = s.seed(1_000, None, false, "lunch order");

    let all = s.get("/api/voice/dictations").await;
    assert_eq!(all["total"], 3);
    assert_eq!(ids(&all), [&unknown, &notes, &slack]);
    assert_eq!(all["dictations"][0]["cleanup"], "off");
    assert_eq!(
        (
            all["history"]["count"].as_i64(),
            all["history"]["hands_free_count"].as_i64(),
            all["history"]["apps"].as_array().unwrap().len()
        ),
        (Some(3), Some(1), 3)
    );
    assert_eq!(
        ids(&s.get("/api/voice/dictations?app=Slack").await),
        [&slack]
    );
    assert_eq!(
        ids(&s.get("/api/voice/dictations?app=Unknown").await),
        [&unknown]
    );
    assert_eq!(
        ids(&s.get("/api/voice/dictations?hands_free=true").await),
        [&notes]
    );
    assert_eq!(ids(&s.get("/api/voice/dictations?q=laun").await), [&slack]);
    assert_eq!(
        ids(&s.get("/api/voice/dictations?q=next%20we&app=Notes").await),
        [&notes]
    );
    let page1 = s.get("/api/voice/dictations?page=1").await;
    assert_eq!((page1["total"].as_i64(), ids(&page1).len()), (Some(3), 0));

    s.seed(40 * 86_400_000, Some("Mail"), false, "an old note");
    let bad = s
        .patch_settings(serde_json::json!({ "voice_retention": "forever" }))
        .await;
    assert!(bad.status().is_client_error(), "{}", bad.status());
    let res = s
        .patch_settings(serde_json::json!({ "voice_retention": "24h" }))
        .await;
    assert_eq!(res.status(), 200);
    assert_eq!(res.json::<Value>().await.unwrap()["voice_retention"], "24h");
    assert_eq!(
        dictations::history(&s.db, None).unwrap().count,
        3,
        "the 40-day-old text is gone"
    );
    assert_eq!(
        dictations::stats(
            &s.db,
            chrono::Utc::now().timestamp_millis(),
            yogurt_db::stats_range::Range::All
        )
        .unwrap()
        .count,
        4,
        "its stats stay"
    );

    assert_eq!(
        s.delete(&format!("/api/voice/dictations/{notes}"))
            .await
            .status(),
        204
    );
    assert_eq!(
        s.delete(&format!("/api/voice/dictations/{notes}"))
            .await
            .status(),
        404
    );
    assert!(ids(&s.get("/api/voice/dictations?q=plan").await).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn free_shows_and_deletes_only_the_last_24_hours() {
    let s = spawn_server().await;
    let old = s.seed(25 * 3_600_000, Some("Mail"), false, "from yesterday");
    let new = s.seed(1_000, Some("Slack"), false, "from today");

    let page = s.get("/api/voice/dictations").await;
    assert_eq!(ids(&page), [&new]);
    assert_eq!(
        (page["total"].as_i64(), page["history"]["count"].as_i64()),
        (Some(1), Some(1))
    );
    assert_eq!(page["window_hours"], 24);
    assert!(ids(&s.get("/api/voice/dictations?q=yesterday").await).is_empty());
    assert!(ids(&s.get("/api/voice/dictations?app=Mail").await).is_empty());
    assert_eq!(
        s.delete(&format!("/api/voice/dictations/{old}"))
            .await
            .status(),
        404,
        "a hidden dictation cannot be deleted one by one either"
    );

    let now = chrono::Utc::now().timestamp_millis();
    let before = dictations::stats(&s.db, now, yogurt_db::stats_range::Range::All).unwrap();
    let res = s.delete("/api/voice/dictations").await;
    assert_eq!(res.status(), 200);
    assert_eq!(res.json::<Value>().await.unwrap()["deleted"], 1);

    assert_eq!(s.get("/api/voice/dictations").await["total"], 0);
    let kept = dictations::list(&s.db, &Default::default(), 10, 0).unwrap();
    assert_eq!(
        kept.dictations.iter().map(|d| &d.id).collect::<Vec<_>>(),
        [&old],
        "the older dictation waits for Pro"
    );
    let after = dictations::stats(&s.db, now, yogurt_db::stats_range::Range::All).unwrap();
    assert_eq!(
        dictations::VoiceStats {
            history: before.history.clone(),
            ..after
        },
        before,
        "the rollups are untouched"
    );
}

#[cfg(not(feature = "pro"))]
#[tokio::test(flavor = "multi_thread")]
async fn a_build_without_pro_has_no_voice_stats_or_lists() {
    let s = spawn_server().await;
    for path in [
        "/api/voice/stats",
        "/api/voice/dictionary",
        "/api/voice/snippets",
        "/api/voice/style",
    ] {
        let res = reqwest::Client::new()
            .get(format!("http://{}{path}", s.addr))
            .bearer_auth(&s.token)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 404, "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn voice_routes_need_the_session_token() {
    let s = spawn_server().await;
    let client = reqwest::Client::new();
    for (method, path) in [
        (reqwest::Method::GET, "/api/voice/dictations"),
        (reqwest::Method::DELETE, "/api/voice/dictations"),
        (reqwest::Method::DELETE, "/api/voice/dictations/x"),
    ] {
        let res = client
            .request(method.clone(), format!("http://{}{path}", s.addr))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 403, "{method} {path}");
    }
}
