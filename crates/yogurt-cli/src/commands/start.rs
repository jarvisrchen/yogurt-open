use anyhow::Result;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use yogurt_server::{Mode, RunConfig};

pub struct StartArgs {
    pub port: u16,
    pub no_open: bool,
    pub dev: bool,
    pub data_dir: Option<PathBuf>,
    pub voice_shortcut: Option<String>,
}

/// The Voice shortcut a dev instance answers, so it never records alongside the main yogurt.
const DEV_VOICE_SHORTCUT: &str = "right_option+m";

pub async fn run(args: StartArgs) -> Result<()> {
    let addr: SocketAddr = ([127, 0, 0, 1], args.port).into();
    let mode = if args.dev { Mode::Dev } else { Mode::Release };
    let url = format!("http://127.0.0.1:{}", args.port);

    // Relocates both SQLite databases, keys.json, the notes folder, and the session token. Models stay under ~/.yogurt.
    let mut cfg = RunConfig::new(addr, mode);
    let data_dir = crate::data_dir::resolve(args.data_dir)?;
    let own_data = data_dir.as_deref().is_some_and(|d| !is_default_data_dir(d));
    cfg.voice_shortcut = voice_shortcut(args.voice_shortcut, args.dev || own_data);
    if let Some(dir) = data_dir {
        let db_path = crate::data_dir::db_path(&dir);
        cfg.db_path = Some(db_path.clone());
        cfg.app_db_path = Some(db_path);
        cfg.keys_path = Some(crate::data_dir::keys_path(&dir));
        cfg.notes_dir = Some(crate::data_dir::notes_dir(&dir));
        cfg.session_token_path = Some(crate::data_dir::session_token_path(&dir));
        if let Some((own, default)) = crate::data_dir::keys_not_copied(&dir) {
            tracing::warn!(
                "no API keys at {}, and this data dir does not read {}. Copy that file here if this instance should use the same keys, and delete its `yogurt-cloud` entry so this instance does not push as the same device.",
                own.display(),
                default.display()
            );
        }
    }

    if !args.no_open {
        // Poll until the server listens so a slow cold boot doesn't open the browser on "connection refused".
        let url_for_open = url.clone();
        let probe_addr = addr;
        tokio::spawn(async move {
            for _ in 0..40 {
                if tokio::net::TcpStream::connect(probe_addr).await.is_ok() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            if let Err(e) = open::that(&url_for_open) {
                tracing::warn!(?e, "failed to open browser");
            }
        });
    }

    tracing::info!(%url, "yogurt is starting");
    match yogurt_server::run_with_config(cfg).await {
        Ok(()) => Ok(()),
        Err(err) => {
            if is_addr_in_use(&err) {
                let port = args.port;
                // `wrapping_add(1)` on 65535 gives 0, which asks for an ephemeral port. Suggest port-1 at the upper bound.
                let next_port_suggestion = port
                    .checked_add(1)
                    .filter(|p| *p > 0)
                    .map(|p| format!("Try --port {p} or run lsof -i :{port}"))
                    .unwrap_or_else(|| {
                        format!(
                            "No nearby port free -- kill the process holding {port} \
                             with `lsof -i :{port}` and `kill <pid>`"
                        )
                    });
                eprintln!("Port {port} is already in use. {next_port_suggestion}");
                std::process::exit(1);
            }
            Err(err)
        }
    }
}

fn voice_shortcut(flag: Option<String>, dev: bool) -> Option<String> {
    flag.or_else(|| dev.then(|| DEV_VOICE_SHORTCUT.to_string()))
}

fn is_default_data_dir(dir: &std::path::Path) -> bool {
    let canonical = |p: &std::path::Path| std::fs::canonicalize(p).ok();
    yogurt_db::paths::yogurt_dir()
        .ok()
        .and_then(|d| canonical(&d))
        .is_some_and(|d| Some(d) == canonical(dir))
}

fn is_addr_in_use(err: &anyhow::Error) -> bool {
    for cause in err.chain() {
        if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
            if io_err.kind() == std::io::ErrorKind::AddrInUse {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dev_instance_keeps_off_the_main_shortcut() {
        assert_eq!(voice_shortcut(None, false), None);
        assert_eq!(
            voice_shortcut(None, true).as_deref(),
            Some("right_option+m")
        );
        let flag = Some("control+option+m".to_string());
        assert_eq!(voice_shortcut(flag.clone(), true), flag);
        assert_eq!(voice_shortcut(flag.clone(), false), flag);
    }

    #[test]
    fn only_the_home_data_dir_is_the_main_one() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_default_data_dir(tmp.path()));
        let home = yogurt_db::paths::yogurt_dir().unwrap();
        assert!(is_default_data_dir(&home.join(".")));
    }
}
