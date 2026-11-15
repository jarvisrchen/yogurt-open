use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};

/// Kills the server on drop, so a failing assertion still cleans up.
pub struct Server {
    pub port: u16,
    child: tokio::process::Child,
}

impl Drop for Server {
    fn drop(&mut self) {
        // Drop cannot await, and a SIGKILLed child holding no listeners is harmless.
        let _ = self.child.start_kill();
    }
}

/// Runs `yogurt start --port 0 --no-open`, with `configure` adding the test's env and flags, and reads the port it bound from its stdout.
/// A cold CI runner can take well over 5 s to answer, so it waits up to 30 s for the port line and 30 s more for health.
pub async fn start(configure: impl Fn(&mut tokio::process::Command)) -> Server {
    let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_yogurt"));
    cmd.args(["start", "--port", "0", "--no-open"]);
    configure(&mut cmd);
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn yogurt start");

    // Both pipes are drained for the child's whole life, so its logging never blocks on a full pipe.
    let mut stderr = child.stderr.take().expect("stderr piped");
    let stderr = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf).await;
        String::from_utf8_lossy(&buf).into_owned()
    });
    let mut stdout = BufReader::new(child.stdout.take().expect("stdout piped")).lines();
    let port = tokio::time::timeout(Duration::from_secs(30), async {
        while let Ok(Some(line)) = stdout.next_line().await {
            if let Some(port) = line.strip_prefix("yogurt is listening on http://127.0.0.1:") {
                return port.parse::<u16>().ok();
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    tokio::spawn(async move { while let Ok(Some(_)) = stdout.next_line().await {} });

    if let Some(port) = port {
        // The port is bound before the server initializes, so a request without a timeout would wait forever on a hung boot.
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .expect("reqwest client");
        for _ in 0..600 {
            if child.try_wait().expect("try_wait").is_some() {
                break;
            }
            if let Ok(resp) = http
                .get(format!("http://127.0.0.1:{port}/api/health"))
                .send()
                .await
            {
                if resp.status().is_success() {
                    return Server { port, child };
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
    let stderr = stderr.await.unwrap_or_default();
    panic!("server never reported healthy (port {port:?}); stderr:\n{stderr}");
}
