//! Updates a Homebrew install from inside the app: a daily check of the tap's formula, then
//! `brew upgrade` and a re-exec into the new binary once nothing is recording.
//!
//! The check reads the tap rather than GitHub Releases, so it only offers a version brew can install.
//! Re-exec keeps the PID and the terminal that launched yogurt, which macOS credits with the mic and
//! Screen Recording grants, so an update never asks for them again.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::Duration;

use axum::{extract::State, routing::get, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use crate::api::ApiError;
use crate::state::AppState;
use crate::ws::{self, WsEvent};
use crate::Mode;

const FORMULA_URL: &str =
    "https://raw.githubusercontent.com/jarvisrchen/homebrew-yogurt/main/Formula/yogurt.rb";
const FORMULA: &str = "jarvisrchen/yogurt/yogurt";
const AUTO_CHECK_KEY: &str = "general.update_check";
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
// Tokio's clock stops while the Mac sleeps, so the daily check compares wall-clock time on a short tick.
const CHECK_TICK: Duration = Duration::from_secs(60 * 60);
const IDLE_POLL: Duration = Duration::from_secs(2);
const BREW_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const DOWNLOAD_POLL: Duration = Duration::from_millis(250);
// The size only feeds the percentage, so a slow GitHub never holds brew back for it.
const SIZE_TIMEOUT: Duration = Duration::from_secs(5);
const ARCH: &str = if cfg!(target_arch = "aarch64") {
    "aarch64"
} else {
    "x86_64"
};
const CURRENT: &str = env!("CARGO_PKG_VERSION");
const NOT_BREW: &str = "this copy of yogurt was not installed with Homebrew";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Idle,
    /// Waiting for the meeting, summary, or dictation to end. `installed` says brew already ran.
    Waiting,
    Installing,
    Restarting,
    Failed,
}

/// What brew is doing while the phase is `Installing`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Step {
    Checking,
    Downloading,
    Installing,
}

#[derive(Debug, Clone, Serialize)]
pub struct Status {
    current: &'static str,
    latest: Option<String>,
    /// `latest` is newer than `current`.
    available: bool,
    /// Unix ms of the last check that reached GitHub.
    checked_at: Option<i64>,
    check_error: Option<String>,
    auto_check: bool,
    /// A Homebrew keg in release mode. Anything else is updated by hand.
    installable: bool,
    phase: Phase,
    installed: bool,
    error: Option<String>,
    /// `None` outside an install, and during one whose formula names no download for this Mac.
    step: Option<Step>,
    /// Bytes of the release archive in Homebrew's cache so far.
    downloaded: Option<u64>,
    /// The archive's size as GitHub reports it.
    total: Option<u64>,
}

#[derive(Default)]
struct Inner {
    latest: Option<String>,
    /// The release archive the formula names for this Mac.
    asset: Option<String>,
    checked_at: Option<i64>,
    /// Unix ms of the last check, reached or not, which paces the daily one.
    tried_at: Option<i64>,
    check_error: Option<String>,
    phase: Phase,
    installed: bool,
    error: Option<String>,
    step: Option<Step>,
    downloaded: Option<u64>,
    total: Option<u64>,
    /// Bumped by each Update and Cancel, so a cancelled install that wakes from its wait stops.
    run: u64,
}

impl Inner {
    fn available(&self) -> bool {
        self.latest.as_deref().is_some_and(|v| newer(v, CURRENT))
    }

    /// Starts an install and returns its run, or `None` when one is already going.
    fn begin(&mut self, busy: bool) -> Result<Option<u64>, &'static str> {
        if !self.available() {
            return Err("no newer version to install");
        }
        if matches!(
            self.phase,
            Phase::Waiting | Phase::Installing | Phase::Restarting
        ) {
            return Ok(None);
        }
        self.phase = if busy {
            Phase::Waiting
        } else {
            Phase::Installing
        };
        self.error = None;
        // Set here too, so the click's own response never shows less than the frame the install pushes next.
        self.step = (!busy).then_some(Step::Checking);
        self.downloaded = None;
        self.total = None;
        self.run += 1;
        Ok(Some(self.run))
    }

    /// Cancel stops a wait that has not installed anything yet, and clears a failure.
    fn cancel(&mut self) -> Result<(), &'static str> {
        match self.phase {
            Phase::Waiting if !self.installed => {
                self.phase = Phase::Idle;
                self.run += 1;
            }
            Phase::Failed => {
                self.phase = Phase::Idle;
                self.error = None;
            }
            Phase::Waiting | Phase::Installing | Phase::Restarting => {
                return Err("the update is already installing");
            }
            Phase::Idle => {}
        }
        Ok(())
    }
}

static INNER: LazyLock<Mutex<Inner>> = LazyLock::new(Mutex::default);
static RESTART: OnceLock<PathBuf> = OnceLock::new();
static RESTART_NOW: Notify = Notify::const_new();

/// The binary to exec once the server has stopped, set only after a finished upgrade.
pub fn restart_target() -> Option<PathBuf> {
    RESTART.get().cloned()
}

/// Resolves once an installed update is ready to restart into.
pub(crate) async fn restarting() {
    RESTART_NOW.notified().await;
}

/// A Homebrew keg: `<prefix>/Cellar/yogurt/<version>/bin/yogurt`.
#[derive(Debug, PartialEq, Eq)]
struct Keg {
    prefix: PathBuf,
    version: String,
}

fn keg_of(exe: &Path) -> Option<Keg> {
    let parts: Vec<_> = exe.components().collect();
    let n = parts.len();
    if n < 6 {
        return None;
    }
    let tail: Vec<_> = parts[n - 5..].iter().map(|c| c.as_os_str()).collect();
    if tail[0] != "Cellar" || tail[1] != "yogurt" || tail[3] != "bin" || tail[4] != "yogurt" {
        return None;
    }
    Some(Keg {
        prefix: parts[..n - 5].iter().collect(),
        version: tail[2].to_str()?.to_string(),
    })
}

static EXE_KEG: LazyLock<Option<Keg>> = LazyLock::new(|| {
    std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .ok()
        .and_then(|p| keg_of(&p))
});

fn own_keg(mode: Mode) -> Option<&'static Keg> {
    EXE_KEG.as_ref().filter(|_| mode == Mode::Release)
}

/// Whether dotted version `a` is newer than `b`. A missing part counts as 0.
fn newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| {
        s.split('.')
            .map(str::parse::<u64>)
            .collect::<Result<Vec<_>, _>>()
    };
    let (Ok(x), Ok(y)) = (parse(a), parse(b)) else {
        return false;
    };
    (0..x.len().max(y.len()))
        .map(|i| (x.get(i).unwrap_or(&0), y.get(i).unwrap_or(&0)))
        .find(|(p, q)| p != q)
        .is_some_and(|(p, q)| p > q)
}

fn formula_version(rb: &str) -> Option<String> {
    rb.lines().find_map(|l| {
        let v = l.trim().strip_prefix("version \"")?.strip_suffix('"')?;
        Some(v.to_string())
    })
}

fn formula_asset(rb: &str) -> Option<String> {
    rb.lines().find_map(|l| {
        let u = l.trim().strip_prefix("url \"")?.split('"').next()?;
        u.contains(ARCH).then(|| u.to_string())
    })
}

/// Where brew keeps a download: `<cache>/downloads/<sha256 of the url>--<file name>`, with `.incomplete` added until it is whole.
// shortcut: misses a HOMEBREW_CACHE set in a brew.env file, which leaves the bar on Downloading; read those files if someone reports it.
fn cached(url: &str) -> Option<PathBuf> {
    let cache = match std::env::var_os("HOMEBREW_CACHE") {
        Some(c) => PathBuf::from(c),
        None => directories::BaseDirs::new()?
            .home_dir()
            .join("Library/Caches/Homebrew"),
    };
    let name = url.rsplit('/').next()?;
    Some(
        cache
            .join("downloads")
            .join(format!("{:x}--{name}", Sha256::digest(url))),
    )
}

async fn asset_size(url: &str) -> Option<u64> {
    let r = HTTP.head(url).send().await.ok()?.error_for_status().ok()?;
    r.headers()
        .get(reqwest::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

fn auto_check(state: &AppState) -> bool {
    yogurt_db::settings::get(&state.db, AUTO_CHECK_KEY)
        .ok()
        .flatten()
        .is_none_or(|v| v == "true")
}

fn status(state: &AppState) -> Status {
    let auto_check = auto_check(state);
    let installable = own_keg(state.mode).is_some();
    let s = INNER.lock().unwrap();
    Status {
        current: CURRENT,
        latest: s.latest.clone(),
        available: s.available(),
        checked_at: s.checked_at,
        check_error: s.check_error.clone(),
        auto_check,
        installable,
        phase: s.phase,
        installed: s.installed,
        error: s.error.clone(),
        step: s.step,
        downloaded: s.downloaded,
        total: s.total,
    }
}

fn set(state: &AppState, f: impl FnOnce(&mut Inner)) {
    f(&mut INNER.lock().unwrap());
    push(state);
}

/// Sends the install's state to every open tab.
fn push(state: &AppState) {
    ws::send_event(
        &state.app_events_tx,
        WsEvent::UpdateStatus {
            status: status(state),
        },
    );
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("yogurt")
        .build()
        .expect("a client with a timeout and user agent always builds")
});

fn check_error(e: &anyhow::Error) -> String {
    match e.downcast_ref::<reqwest::Error>() {
        Some(r) => match r.status() {
            Some(code) => format!("GitHub answered {code}"),
            None => "Couldn't reach GitHub".into(),
        },
        None => "The tap's formula has no version".into(),
    }
}

async fn check() {
    INNER.lock().unwrap().tried_at = Some(now_ms());
    let result = async {
        let rb = HTTP
            .get(FORMULA_URL)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        let v = formula_version(&rb)
            .ok_or_else(|| anyhow::anyhow!("the formula has no version line"))?;
        Ok((v, formula_asset(&rb)))
    }
    .await;
    let mut s = INNER.lock().unwrap();
    match result {
        Ok((v, asset)) => {
            s.latest = Some(v);
            s.asset = asset;
            s.checked_at = Some(now_ms());
            s.check_error = None;
        }
        Err(e) => {
            tracing::info!(error = %e, "update check failed");
            s.check_error = Some(check_error(&e));
        }
    }
}

/// Checks at start and then daily, while the setting is on. Never runs for a build brew does not own.
pub(crate) fn spawn(state: AppState) {
    if own_keg(state.mode).is_none() {
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(CHECK_TICK);
        loop {
            tick.tick().await;
            let due = INNER
                .lock()
                .unwrap()
                .tried_at
                .is_none_or(|t| now_ms() - t >= DAY_MS);
            if due && auto_check(&state) {
                check().await;
            }
        }
    });
}

/// What brew's own `Error:` line says, since warnings and support notes can follow it. Its last line when it has none.
fn brew_error(stderr: &str) -> &str {
    let mut lines = stderr.lines().rev().map(str::trim);
    lines
        .clone()
        .find_map(|l| l.strip_prefix("Error: "))
        .or_else(|| lines.find(|l| !l.is_empty()))
        .unwrap_or("")
}

/// A brew past the timeout is killed, so Try again never runs a second brew beside it.
async fn run_brew(brew: &Path, args: &[&str]) -> Result<(), String> {
    let mut cmd = tokio::process::Command::new(brew);
    cmd.args(args)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .env("HOMEBREW_NO_AUTO_UPDATE", "1")
        .env("HOMEBREW_NO_INSTALL_CLEANUP", "1")
        .env("HOMEBREW_NO_ENV_HINTS", "1");
    let out = tokio::time::timeout(BREW_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("brew {} took longer than 15 minutes", args[0]))?
        .map_err(|e| format!("could not run {}: {e}", brew.display()))?;
    if out.status.success() {
        return Ok(());
    }
    Err(format!(
        "brew {} failed: {}",
        args[0],
        brew_error(&String::from_utf8_lossy(&out.stderr))
    ))
}

/// Reads the download's progress off Homebrew's cache. The whole file there means brew has moved on to installing it.
fn progress(file: &Path) -> (Step, Option<u64>) {
    if file.exists() {
        return (Step::Installing, None);
    }
    let part = PathBuf::from(format!("{}.incomplete", file.display()));
    (
        Step::Downloading,
        std::fs::metadata(part).ok().map(|m| m.len()),
    )
}

fn watch(state: &AppState, file: &Path) {
    let (step, downloaded) = progress(file);
    let changed = {
        let mut s = INNER.lock().unwrap();
        let downloaded = downloaded.or(s.downloaded);
        let changed = (s.step, s.downloaded) != (Some(step), downloaded);
        s.step = Some(step);
        s.downloaded = downloaded;
        changed
    };
    if changed {
        push(state);
    }
}

/// Waits until nothing is recording, summarizing, or dictating. False once Cancel ended this run.
async fn wait_idle(state: &AppState, run: u64) -> bool {
    let mut waited = false;
    loop {
        let idle = !crate::build_gone::busy(state).await;
        {
            let mut s = INNER.lock().unwrap();
            if s.run != run {
                return false;
            }
            // One more poll after a busy spell, since the browser asks for the summary only after the stop returns.
            if idle && !waited {
                return true;
            }
            if s.phase == Phase::Installing {
                s.phase = Phase::Waiting;
            }
        }
        waited = !idle;
        tokio::time::sleep(IDLE_POLL).await;
    }
}

async fn install(state: AppState, keg: &'static Keg, run: u64) {
    if !wait_idle(&state, run).await {
        return;
    }
    {
        let mut s = INNER.lock().unwrap();
        if s.run != run {
            return;
        }
        s.phase = Phase::Installing;
        s.step = Some(Step::Checking);
    }
    push(&state);
    let brew = keg.prefix.join("bin/brew");
    // A failed `brew update` can still have pulled this tap, so the version on the link decides.
    // The formula is read again because brew installs the tap's version now, which a release since the last check has moved.
    let size = tokio::time::timeout(SIZE_TIMEOUT, async {
        check().await;
        let asset = INNER.lock().unwrap().asset.clone();
        asset_size(&asset?).await
    });
    let (updated, size) = tokio::join!(run_brew(&brew, &["update"]), size);
    let total = size.ok().flatten();
    let file = INNER.lock().unwrap().asset.as_deref().and_then(cached);
    set(&state, |s| {
        s.step = file.as_ref().map(|_| Step::Downloading);
        s.total = total;
    });
    let upgraded = {
        let upgrade = run_brew(&brew, &["upgrade", FORMULA]);
        tokio::pin!(upgrade);
        let mut tick = tokio::time::interval(DOWNLOAD_POLL);
        loop {
            tokio::select! {
                r = &mut upgrade => break r,
                _ = tick.tick() => if let Some(f) = &file { watch(&state, f) },
            }
        }
    };
    let link = keg.prefix.join("bin/yogurt");
    let now = link.canonicalize().ok().and_then(|p| keg_of(&p));
    // Any other keg on the link is the upgrade, including a revision such as `3.3.0_1`.
    match now {
        Some(k) if k.version != keg.version => {
            tracing::info!(version = %k.version, "Homebrew installed the update");
            set(&state, |s| s.installed = true);
        }
        _ => {
            let why = upgraded
                .err()
                .or(updated.err())
                .unwrap_or_else(|| format!("Homebrew still has yogurt {CURRENT}"));
            tracing::warn!(error = %why, "update did not install");
            set(&state, |s| {
                s.phase = Phase::Failed;
                s.error = Some(why);
            });
            return;
        }
    }
    // A recording can start during the install. Cancel refuses once `installed` is set, so this wait cannot end early.
    // Pro stays paused through the exec, since a Cloud token refresh cut off there would lose the rotated token.
    loop {
        wait_idle(&state, run).await;
        let paused = state.pro.pause(&state).await;
        if !state.voice.dictating() && !state.meetings.busy().await {
            std::mem::forget(paused);
            break;
        }
    }
    set(&state, |s| s.phase = Phase::Restarting);
    let _ = RESTART.set(link);
    RESTART_NOW.notify_one();
}

async fn get_status(State(state): State<AppState>) -> Json<Status> {
    Json(status(&state))
}

async fn post_check(State(state): State<AppState>) -> Result<Json<Status>, ApiError> {
    if own_keg(state.mode).is_none() {
        return Err(ApiError::Conflict(NOT_BREW.into()));
    }
    check().await;
    Ok(Json(status(&state)))
}

async fn post_install(State(state): State<AppState>) -> Result<Json<Status>, ApiError> {
    let keg = own_keg(state.mode).ok_or_else(|| ApiError::Conflict(NOT_BREW.into()))?;
    let busy = crate::build_gone::busy(&state).await;
    let run = INNER
        .lock()
        .unwrap()
        .begin(busy)
        .map_err(|e| ApiError::Conflict(e.into()))?;
    if let Some(run) = run {
        tokio::spawn(install(state.clone(), keg, run));
    }
    Ok(Json(status(&state)))
}

async fn post_cancel(State(state): State<AppState>) -> Result<Json<Status>, ApiError> {
    INNER
        .lock()
        .unwrap()
        .cancel()
        .map_err(|e| ApiError::Conflict(e.into()))?;
    Ok(Json(status(&state)))
}

#[derive(Deserialize)]
struct Patch {
    auto_check: bool,
}

async fn patch(
    State(state): State<AppState>,
    Json(p): Json<Patch>,
) -> Result<Json<Status>, ApiError> {
    yogurt_db::settings::set(
        &state.db,
        AUTO_CHECK_KEY,
        if p.auto_check { "true" } else { "false" },
    )?;
    Ok(Json(status(&state)))
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/update", get(get_status).patch(patch))
        .route("/api/update/check", post(post_check))
        .route("/api/update/install", post(post_install))
        .route("/api/update/cancel", post(post_cancel))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_keg_is_read_from_the_cellar_path() {
        assert_eq!(
            keg_of(Path::new("/opt/homebrew/Cellar/yogurt/3.2.2/bin/yogurt")),
            Some(Keg {
                prefix: "/opt/homebrew".into(),
                version: "3.2.2".into()
            })
        );
        assert_eq!(
            keg_of(Path::new("/usr/local/Cellar/yogurt/3.10.0/bin/yogurt")).map(|k| k.prefix),
            Some(PathBuf::from("/usr/local"))
        );
        assert_eq!(
            keg_of(Path::new("/opt/homebrew/Cellar/yogurt/3.3.0_1/bin/yogurt")).map(|k| k.version),
            Some("3.3.0_1".to_string())
        );
        assert_eq!(keg_of(Path::new("/opt/homebrew/bin/yogurt")), None);
        assert_eq!(keg_of(Path::new("/x/target/release/yogurt")), None);
        assert_eq!(
            keg_of(Path::new("/opt/homebrew/Cellar/other/1.0/bin/yogurt")),
            None
        );
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(newer("3.3.0", "3.2.2"));
        assert!(newer("3.10.0", "3.9.9"));
        assert!(newer("3.2.2.1", "3.2.2"));
        assert!(!newer("3.2.2", "3.2.2"));
        assert!(!newer("3.2", "3.2.0"));
        assert!(!newer("3.2.1", "3.2.2"));
        assert!(!newer("3.3.0-rc1", "3.2.2"));
    }

    #[test]
    fn the_formula_version_line_is_found() {
        let rb = "class Yogurt < Formula\n  desc \"x\"\n  version \"3.2.2\"\n  license \"MIT\"\n";
        assert_eq!(formula_version(rb).as_deref(), Some("3.2.2"));
        assert_eq!(formula_version("class Yogurt < Formula\nend\n"), None);
    }

    #[test]
    fn the_download_is_found_in_the_formula_and_the_cache() {
        let rb = "  on_macos do\n    if Hardware::CPU.arm?\n      url \"https://x/v1/yogurt-aarch64-apple-darwin.tar.gz\", using: :curl\n    else\n      url \"https://x/v1/yogurt-x86_64-apple-darwin.tar.gz\"\n";
        let url = formula_asset(rb).unwrap();
        assert_eq!(
            url,
            format!("https://x/v1/yogurt-{ARCH}-apple-darwin.tar.gz")
        );
        assert_eq!(formula_asset("version \"3.2.2\"\n"), None);

        // The name brew 7.0.13 gave this download in a real cache.
        let real = "https://github.com/jarvisrchen/yogurt/releases/download/v3.5.0/yogurt-aarch64-apple-darwin.tar.gz";
        assert!(cached(real).unwrap().ends_with(
            "downloads/2a6346d9a37c52002cca5e5c8ec4a21c94d31e0d602264023092b40ed3dd2a5c--yogurt-aarch64-apple-darwin.tar.gz"
        ));
    }

    #[test]
    fn the_cache_file_tells_the_download_from_the_install() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("abc--yogurt.tar.gz");
        let part = dir.path().join("abc--yogurt.tar.gz.incomplete");
        assert_eq!(progress(&file), (Step::Downloading, None));
        std::fs::write(&part, [0; 7]).unwrap();
        assert_eq!(progress(&file), (Step::Downloading, Some(7)));
        std::fs::rename(&part, &file).unwrap();
        assert_eq!(progress(&file), (Step::Installing, None));
    }

    #[test]
    fn the_error_line_wins_over_what_brew_prints_after_it() {
        let stderr = "Warning: old Xcode\nError: Failed to download resource \"yogurt\"\n\nThis is a Tier 3 configuration:\nRead the above document before opening any issues or PRs.\n";
        assert_eq!(brew_error(stderr), "Failed to download resource \"yogurt\"");
        assert_eq!(brew_error("fatal: no network\n\n"), "fatal: no network");
        assert_eq!(brew_error(""), "");
    }

    #[test]
    fn update_waits_while_busy_and_cancel_stops_only_that_run() {
        let mut s = Inner::default();
        assert!(s.begin(false).is_err(), "nothing newer to install");

        s.latest = Some("999.0.0".into());
        assert_eq!(s.begin(true), Ok(Some(1)));
        assert_eq!((s.phase, s.step), (Phase::Waiting, None));
        assert_eq!(
            s.begin(true),
            Ok(None),
            "a second click joins the running install"
        );

        assert_eq!(s.cancel(), Ok(()));
        assert_eq!(
            (s.phase, s.run),
            (Phase::Idle, 2),
            "the cancelled run 1 can no longer proceed"
        );
        assert_eq!(s.begin(false), Ok(Some(3)));
        assert_eq!((s.phase, s.step), (Phase::Installing, Some(Step::Checking)));
        assert!(s.cancel().is_err(), "brew is already running");

        s.phase = Phase::Waiting;
        s.installed = true;
        assert!(
            s.cancel().is_err(),
            "an installed update only waits to restart"
        );

        s.phase = Phase::Failed;
        s.error = Some("brew upgrade failed".into());
        assert_eq!(s.cancel(), Ok(()));
        assert_eq!((s.phase, s.error.as_deref()), (Phase::Idle, None));
    }
}
