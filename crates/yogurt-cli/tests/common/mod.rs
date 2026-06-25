use std::net::TcpListener;
use std::time::Duration;

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

/// Runs `yogurt start --port <free port> --no-open`, with `configure` adding the test's env and flags.
/// The probe port is released before the child binds it, so a parallel test can take it in between, and a child that exits early is retried on a fresh port.
/// A cold CI runner can take well over 5 s to answer, so each try waits up to 30 s.
pub async fn start(configure: impl Fn(&mut tokio::process::Command)) -> Server {
    let mut last_stderr = String::new();
    for _ in 0..3 {
        let probe = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral");
        let port = probe.local_addr().expect("local_addr").port();
        drop(probe);

        let mut cmd = tokio::process::Command::new(env!("CARGO_BIN_EXE_yogurt"));
        cmd.args(["start", "--port", &port.to_string(), "--no-open"]);
        configure(&mut cmd);
        let mut child = cmd
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("spawn yogurt start");

        for _ in 0..600 {
            if child.try_wait().expect("try_wait").is_some() {
                break;
            }
            if let Ok(resp) = reqwest::get(format!("http://127.0.0.1:{port}/api/health")).await {
                if resp.status().is_success() {
                    return Server { port, child };
                }
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let _ = child.start_kill();
        let out = child.wait_with_output().await.expect("wait_with_output");
        last_stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    }
    panic!("server never reported healthy; last stderr:\n{last_stderr}");
}
