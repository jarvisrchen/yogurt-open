//! A server whose own build is deleted exits, so a removed worktree's dev server cannot outlive it.

use std::path::Path;
use std::time::Duration;

use crate::state::AppState;

const CHECK: Duration = Duration::from_secs(30);

/// Only a binary under a `target/` directory: Homebrew deletes the old Cellar folder on upgrade, and that yogurt must keep running.
fn watched(exe: &Path) -> bool {
    exe.components().any(|c| c.as_os_str() == "target")
}

pub(crate) async fn busy(state: &AppState) -> bool {
    use futures_util::FutureExt;
    // A pause that cannot start at once means Pro is mid-work.
    state.voice.dictating()
        || state.meetings.busy().await
        || state.pro.pause(state).now_or_never().is_none()
}

/// Resolves once this binary is deleted and no meeting, summary, or dictation is in progress. Never resolves for an installed binary.
pub async fn wait(state: AppState) {
    let exe = match std::env::current_exe().and_then(|p| p.canonicalize()) {
        Ok(exe) if watched(&exe) => exe,
        _ => return std::future::pending().await,
    };
    let mut tick = tokio::time::interval(CHECK);
    let mut waiting = false;
    loop {
        tick.tick().await;
        if exe.exists() {
            continue;
        }
        // A rebuild unlinks the binary for an instant before linking the new one.
        tokio::time::sleep(Duration::from_secs(1)).await;
        if exe.exists() {
            continue;
        }
        if busy(&state).await {
            if !waiting {
                tracing::warn!(exe = %exe.display(), "this build was deleted, so the server exits once the meeting, its summary, or the dictation ends");
                waiting = true;
            }
            continue;
        }
        // One more check after the last busy one, since the browser asks for the summary only after the stop returns.
        if waiting {
            waiting = false;
            continue;
        }
        tracing::warn!(exe = %exe.display(), "exiting because this build was deleted");
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::watched;
    use std::path::Path;

    #[test]
    fn only_a_build_under_target_is_watched() {
        assert!(watched(Path::new(
            "/Users/r/code/yogurt-worktrees/ui-30/target/debug/yogurt"
        )));
        assert!(watched(Path::new("/tmp/x/target/release/yogurt")));
        assert!(!watched(Path::new(
            "/opt/homebrew/Cellar/yogurt/3.1.0/bin/yogurt"
        )));
        assert!(!watched(Path::new(
            "/Applications/Yogurt.app/Contents/MacOS/yogurt"
        )));
        assert!(!watched(Path::new("/Users/r/targets/yogurt")));
    }
}
