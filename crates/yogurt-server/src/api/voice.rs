use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use yogurt_db::dictations::{self, DictationFilter, DictationPage, HistoryCounts};
use yogurt_db::style::Kind;
use yogurt_voice::{Hotkey, SharedRecorder, Step};

use crate::api::{blocking, ApiError};
use crate::entitlements::limits;
use crate::state::AppState;

/// The page renews the recording every few seconds, so a tab closed mid-recording frees the keyboard soon after.
const RECORD_LEASE: Duration = Duration::from_secs(10);

/// Shorter than the recording lease, since a page that died with the Try field focused would swallow real dictations until it runs out.
const TRY_LEASE: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct VoiceShared {
    /// The event tap reads it on every key. `None` passes every key through.
    pub hotkey: Arc<Mutex<Option<Hotkey>>>,
    pub status: Mutex<String>,
    /// While Settings records a new shortcut, the tap records it and the hotkey stays off.
    pub recording: SharedRecorder,
    /// Makes the voice task reconcile now rather than on its next poll.
    pub wake: tokio::sync::Notify,
    /// While the Try Voice field has focus: a clip that ends goes to the page, cleaned up as for this kind of app.
    pub try_until: Mutex<Option<(Instant, Kind)>>,
    /// Replaces `general.voice_hotkey` for this process, so a dev yogurt keeps off the main one's shortcut.
    pub shortcut: Option<String>,
    /// Each clip holds a clone from its key press until its dictation is pasted or dropped.
    pub clips: Arc<()>,
}

impl VoiceShared {
    pub fn dictating(&self) -> bool {
        Arc::strong_count(&self.clips) > 1
    }

    /// The live recording's current step.
    pub fn recording(&self) -> Option<Step> {
        lock(&self.recording)
            .as_ref()
            .filter(|(until, _)| Instant::now() < *until)
            .map(|(_, r)| r.step())
    }

    /// Starts or renews a recording, or ends it.
    fn record(&self, on: bool) {
        let mut slot = lock(&self.recording);
        let now = Instant::now();
        // A renewal keeps what was pressed so far. A lapsed or finished recording starts over, so a new one never reports an old result.
        let kept = slot
            .take()
            .filter(|(until, r)| now < *until && !r.is_finished());
        *slot = on.then(|| (now + RECORD_LEASE, kept.map(|(_, r)| r).unwrap_or_default()));
    }

    pub fn armed_try(&self) -> Option<Kind> {
        lock(&self.try_until).and_then(|(until, kind)| (Instant::now() < until).then_some(kind))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// macOS reports only trusted or not, so a denial and a never-asked state both read as missing.
pub async fn get_status(State(s): State<AppState>) -> Json<Value> {
    #[cfg(target_os = "macos")]
    let trusted = yogurt_voice::is_trusted();
    #[cfg(not(target_os = "macos"))]
    let trusted = true;
    Json(json!({
        "accessibility": if trusted { "granted" } else { "missing" },
        "running": lock(&s.voice.hotkey).is_some(),
        "status": *lock(&s.voice.status),
    }))
}

#[derive(Deserialize)]
pub struct RecordBody {
    recording: bool,
}

pub async fn record(State(s): State<AppState>, Json(body): Json<RecordBody>) -> StatusCode {
    s.voice.record(body.recording);
    // The voice task disarms the hotkey and starts the tap, even with Voice off, so the recording has something to record with.
    s.voice.wake.notify_one();
    StatusCode::NO_CONTENT
}

/// The held keys while the recording runs, then the hotkey it recorded or a cancel.
pub async fn recorded(State(s): State<AppState>) -> Json<Value> {
    Json(match s.voice.recording() {
        Some(Step::Done(hotkey)) => json!({ "kind": "done", "hotkey": hotkey }),
        Some(Step::Cancel) => json!({ "kind": "cancel" }),
        Some(Step::Pending(held)) => json!({ "kind": "pending", "held": held }),
        None => json!({ "kind": "pending", "held": [] }),
    })
}

#[derive(Deserialize)]
struct ArmBody {
    /// `None` disarms.
    kind: Option<Kind>,
}

/// The page sends it on focus and renews it while the Try field keeps focus, and sends `null` on blur.
async fn arm_try(State(s): State<AppState>, Json(body): Json<ArmBody>) -> StatusCode {
    *lock(&s.voice.try_until) = body.kind.map(|k| (Instant::now() + TRY_LEASE, k));
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
struct CleanBody {
    raw: String,
    kind: Kind,
}

/// Clean up as: the last try's raw text cleaned again for another kind, without talking again.
async fn clean_try(
    State(s): State<AppState>,
    Json(body): Json<CleanBody>,
) -> Result<Json<crate::voice_cleanup::Tried>, ApiError> {
    Ok(Json(
        crate::voice::retry(&s, body.raw, body.kind)
            .await
            .map_err(ApiError::Internal)?,
    ))
}

const PAGE_SIZE: usize = 25;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/voice/dictations", get(list).delete(delete_all))
        .route("/api/voice/dictations/{id}", delete(delete_one))
        .route("/api/voice/cleanup/test", post(test_cleanup))
        .route("/api/voice/try", post(arm_try))
        .route("/api/voice/try/clean", post(clean_try))
}

/// `hands_free=true` keeps only hands-free rows, and `false` does not filter.
#[derive(Debug, Deserialize)]
struct ListQuery {
    #[serde(default)]
    q: String,
    app: Option<String>,
    #[serde(default)]
    hands_free: bool,
    #[serde(default)]
    page: usize,
}

/// `history` counts every row the plan shows, whatever the filter. `window_hours` is how far back that reaches, `None` for all of it.
#[derive(Serialize)]
struct ListPage {
    #[serde(flatten)]
    page: DictationPage,
    history: HistoryCounts,
    window_hours: Option<u32>,
}

/// The plan's history window, and the Unix ms it starts at.
///
/// **Blocking:** it reads and verifies the license.
fn window(s: &AppState) -> (Option<u32>, Option<i64>) {
    let hours = limits(&s.pro.entitlements(s)).dictation_hours;
    let since = hours.map(|h| crate::routes::now_unix_ms() - i64::from(h) * 3_600_000);
    (hours, since)
}

async fn list(
    State(s): State<AppState>,
    Query(qs): Query<ListQuery>,
) -> Result<Json<ListPage>, ApiError> {
    let offset = qs.page.saturating_mul(PAGE_SIZE);
    Ok(Json(
        blocking(move || {
            let (window_hours, since) = window(&s);
            let filter = DictationFilter {
                q: qs.q,
                app: qs.app.filter(|a| !a.is_empty()),
                hands_free: qs.hands_free,
                since,
            };
            Ok(ListPage {
                page: dictations::list(&s.db, &filter, PAGE_SIZE, offset)?,
                history: dictations::history(&s.db, since)?,
                window_hours,
            })
        })
        .await?,
    ))
}

async fn delete_one(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    if blocking(move || dictations::delete(&s.db, &id, window(&s).1)).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

/// Within a history window it deletes only what the window shows, so the older text and the stats wait for Pro.
async fn delete_all(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    let deleted = blocking(move || match window(&s).1 {
        Some(t) => dictations::delete_since(&s.db, t),
        None => dictations::delete_all(&s.db),
    })
    .await?;
    Ok(Json(json!({ "deleted": deleted })))
}

/// 45 words with the fillers, the repeats, and the self-correction the cleanup prompt is for.
const SAMPLE: &str = "so um I think we should we should move the launch to uh Tuesday no wait Wednesday because like the the QA team still needs a couple more days to finish the regression pass and um I don't want to ship it with known bugs";

/// Runs [`SAMPLE`] through the saved cleanup model and reports its real time, even past the limit, and which part of the limit it broke.
/// 200 with `outcome` set to `done`, `too_slow`, `failed`, or `unavailable`, unless the prompt cannot be read.
async fn test_cleanup(State(s): State<AppState>) -> Result<Json<Value>, ApiError> {
    use crate::voice_cleanup::{clean, model, Outcome};
    let provider = yogurt_db::settings::load_general(&s.db)?.voice_cleanup_provider;
    let words = SAMPLE.split_whitespace().count();
    let base = json!({
        "raw": SAMPLE,
        "words": words,
        "total_ms": crate::voice_cleanup::total(words).as_millis() as u64,
    });
    let with = |extra: Value| {
        let mut v = base.clone();
        if let (Some(v), Value::Object(extra)) = (v.as_object_mut(), extra) {
            v.extend(extra);
        }
        Json(v)
    };
    let client = match model(&s, &provider) {
        Ok(c) => c,
        Err(why) => return Ok(with(json!({ "outcome": "unavailable", "error": why }))),
    };
    let prompt = s.prompts.voice_cleanup()?;
    let model_name = client.model_name();
    crate::voice_cleanup::warm(client.as_ref()).await;
    let run = clean(client, prompt, SAMPLE, &Default::default(), true).await;
    let ms = run.elapsed.as_millis() as u64;
    Ok(with(match run.outcome {
        Outcome::Cleaned(text) => {
            json!({ "outcome": "done", "ms": ms, "model": model_name, "text": text })
        }
        Outcome::TooSlow(limit) => {
            json!({ "outcome": "too_slow", "ms": ms, "model": model_name, "limit": limit })
        }
        Outcome::Failed(e) => {
            json!({ "outcome": "failed", "ms": ms, "model": model_name, "error": format!("{e:#}") })
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recording_lapses_when_the_lease_runs_out() {
        let v = VoiceShared::default();
        assert_eq!(v.recording(), None);
        v.record(true);
        assert_eq!(v.recording(), Some(Step::Pending(vec![])));
        lock(&v.recording).as_mut().unwrap().0 = Instant::now() - Duration::from_millis(1);
        assert_eq!(v.recording(), None);
        v.record(true);
        v.record(false);
        assert_eq!(v.recording(), None);
    }

    #[test]
    fn a_renewal_keeps_the_held_keys_and_a_lapsed_recording_starts_over() {
        let v = VoiceShared::default();
        v.record(true);
        lock(&v.recording).as_mut().unwrap().1.flags(0x80_0000);
        v.record(true);
        assert_eq!(v.recording(), Some(Step::Pending(vec!["fn"])));
        lock(&v.recording).as_mut().unwrap().0 = Instant::now() - Duration::from_millis(1);
        v.record(true);
        assert_eq!(v.recording(), Some(Step::Pending(vec![])));
        lock(&v.recording).as_mut().unwrap().1.key_down(53, 0);
        assert_eq!(v.recording(), Some(Step::Cancel));
        v.record(true);
        assert_eq!(v.recording(), Some(Step::Pending(vec![])));
    }

    #[test]
    fn dictating_while_a_clip_holds_its_token() {
        let v = VoiceShared::default();
        assert!(!v.dictating());
        let clip = v.clips.clone();
        assert!(v.dictating());
        drop(clip);
        assert!(!v.dictating());
    }
}
