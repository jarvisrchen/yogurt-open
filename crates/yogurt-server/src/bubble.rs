//! The meeting bubble's driver. The bubble floats over other apps while a meeting records and no Yogurt page has focus, and this decides when.
//!
//! Pages report focus over `/ws` (`{"type":"page_focus","focused":bool}`). The close button hides the bubble until a page has focus again,
//! and Settings > General turns it off. A click brings back the app that last showed a Yogurt page, and `bubble_return` on `/ws` lets that page call attention to itself.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::state::AppState;

pub const SETTING_KEY: &str = "general.meeting_bubble";
/// Where a drag left the pill's bottom-right corner, as `right,bottom` in screen points.
pub const POSITION_KEY: &str = "general.meeting_bubble_position";

#[derive(Debug, Default)]
pub struct BubbleShared {
    /// Open `/ws` sockets, one per yogurt page.
    pages: AtomicUsize,
    /// Those whose page has focus and is visible.
    focused: AtomicUsize,
    dismissed: AtomicBool,
}

impl BubbleShared {
    pub fn page_opened(&self) {
        self.pages.fetch_add(1, Ordering::Relaxed);
    }

    pub fn page_closed(&self) {
        self.pages.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn has_pages(&self) -> bool {
        self.pages.load(Ordering::Relaxed) > 0
    }

    /// One socket's page went from `was` to `now` focused. Gaining focus ends a dismissal, so the bubble returns the next time the user leaves.
    pub fn page_focus(&self, was: bool, now: bool) {
        match (was, now) {
            (false, true) => {
                self.focused.fetch_add(1, Ordering::Relaxed);
                self.dismissed.store(false, Ordering::Relaxed);
            }
            (true, false) => {
                self.focused.fetch_sub(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    pub fn dismiss(&self) {
        self.dismissed.store(true, Ordering::Relaxed);
    }

    /// Whether the bubble should be up.
    pub fn wanted(&self, recording: bool, enabled: bool) -> bool {
        self.focused.load(Ordering::Relaxed) == 0
            && recording
            && enabled
            && !self.dismissed.load(Ordering::Relaxed)
    }
}

/// What the driver tells the panel on one tick.
#[derive(Debug, PartialEq)]
enum Step {
    /// Show it for this recording, which also moves a bubble that is up onto a newer one.
    Show(uuid::Uuid),
    /// The user is back in yogurt or closed it, and the recording goes on.
    Hide,
    /// The recording it showed has stopped.
    Saved,
    Stay,
}

fn step(wanted: bool, shown: Option<uuid::Uuid>, active: Option<uuid::Uuid>) -> Step {
    match (wanted, shown, active) {
        (true, shown, Some(id)) if shown != Some(id) => Step::Show(id),
        (false, Some(id), active) if active == Some(id) => Step::Hide,
        (false, Some(_), _) => Step::Saved,
        _ => Step::Stay,
    }
}

/// Feeds one frame's peak to the bubble's bars. A paused frame is silent on purpose and never counts as missing audio.
pub fn level(mic: bool, peak: f32, paused: bool) {
    #[cfg(target_os = "macos")]
    if paused {
        yogurt_voice::bubble::paused_frame();
    } else {
        yogurt_voice::bubble::level(mic, peak);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (mic, peak, paused);
}

/// A page gained focus, so its app is the one a click on the bubble brings back.
pub fn page_focused() {
    #[cfg(target_os = "macos")]
    yogurt_voice::bubble::remember_front();
}

fn position(db: &yogurt_db::Db) -> Option<(f64, f64)> {
    let v = yogurt_db::settings::get(db, POSITION_KEY).ok().flatten()?;
    let (right, bottom) = v.split_once(',')?;
    Some((right.parse().ok()?, bottom.parse().ok()?))
}

#[cfg(target_os = "macos")]
pub fn spawn(state: AppState) {
    use yogurt_voice::bubble::{self as panel, Event};

    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    panel::on_event(move |e| {
        let _ = tx.send(e);
    });
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_millis(200));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let (controls, pending) = control_worker(state.clone());
        let mut shown: Option<uuid::Uuid> = None;
        // The card's lines come from the shown meeting's transcript, seeded from its stored finals.
        let mut card = panel::Card::default();
        let mut seeded_to: SeededTo = [None; 2];
        let mut card_sent: Option<panel::Card> = None;
        let mut transcript: Option<tokio::sync::broadcast::Receiver<yogurt_stt::TranscriptEvent>> =
            None;
        let mut ticks: u64 = 0;
        // What the panel last heard about the shown meeting's pause and mute.
        let mut sent: Option<Status> = None;
        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                event = next_line(&mut transcript), if transcript.is_some() => {
                    match event {
                        Ok(e) if unseen(&e, seeded_to) => card.merge(e.channel == yogurt_stt::Channel::Mic, &e.text, e.is_final),
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => transcript = None,
                    }
                    continue;
                }
                Some(event) = events.recv() => {
                    match event {
                        Event::Return { raised } => {
                            let _ = state.app_events_tx.send(serde_json::json!({ "type": "bubble_return" }));
                            // No app came forward, or none has a yogurt page: open the meeting itself.
                            if let Some(id) = shown.filter(|_| !raised || !state.bubble.has_pages()) {
                                panel::open_url(&format!("http://localhost:{}/meeting/{id}", state.bind_port));
                            }
                        }
                        Event::Dismiss => state.bubble.dismiss(),
                        // Applied one at a time in click order. No status goes out while one is pending, and the panel already shows what was asked.
                        Event::Pause(at) => {
                            if let Some(id) = shown {
                                sent = sent.map(|(_, muted)| (at, muted));
                                pending.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                let _ = controls.send(Control::Pause(id, at));
                            }
                        }
                        Event::Mute(muted) => {
                            if let Some(id) = shown {
                                sent = sent.map(|(paused, _)| (paused, muted));
                                pending.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                                let _ = controls.send(Control::Mute(id, muted));
                            }
                        }
                        Event::Moved { right, bottom } => {
                            if let Err(e) = yogurt_db::settings::set(&state.db, POSITION_KEY, &format!("{right},{bottom}")) {
                                tracing::warn!(error = %e, "could not save the bubble's position");
                            }
                        }
                    }
                    continue;
                }
            }
            let active = state.meetings.active_recording().await;
            if active.is_none() && shown.is_none() {
                continue;
            }
            let enabled = crate::detect::setting_enabled(&state.db, SETTING_KEY);
            match step(
                state.bubble.wanted(active.is_some(), enabled),
                shown,
                active,
            ) {
                Step::Show(id) => {
                    shown = Some(id);
                    sent = None;
                    // Subscribe before reading the stored lines, so no final falls between the two.
                    transcript = state.meetings.subscribe(&id).await;
                    let row = row(&state, id).await;
                    (card, seeded_to) = seeded(&row);
                    card_sent = None;
                    // The Pro part is read below on this same tick, so the first card already has it.
                    ticks = 0;
                    panel::show(row.started_at, position(&state.db));
                }
                Step::Hide => {
                    shown = None;
                    transcript = None;
                    panel::hide();
                }
                Step::Saved => {
                    shown = None;
                    transcript = None;
                    panel::saved();
                }
                Step::Stay => {}
            }
            // Read before the status, so a request the worker finishes in between cannot slip a stale status out.
            let pending = pending.load(std::sync::atomic::Ordering::Relaxed);
            if let Some(m) = match shown {
                Some(id) => state.meetings.get(&id).await,
                None => None,
            } {
                if let Some(now) = push(pending, sent, status_of(&m).await) {
                    sent = Some(now);
                    panel::status(now.0, now.1);
                }
                // The recap changes every few minutes, so once a second is plenty for its license and setting reads.
                ticks += 1;
                if ticks % 5 == 1 {
                    let (s, meeting) = (state.clone(), m.clone());
                    // It reads keys.json and checks the license's signature.
                    card.pro = tokio::task::spawn_blocking(move || s.pro.bubble_card(&s, &meeting))
                        .await
                        .ok()
                        .flatten();
                }
                if card_sent.as_ref() != Some(&card) {
                    card_sent = Some(card.clone());
                    panel::card(card.clone());
                }
            }
        }
    });
}

/// A pause or mute from the bubble's buttons.
#[cfg(target_os = "macos")]
enum Control {
    Pause(uuid::Uuid, Option<u64>),
    Mute(uuid::Uuid, bool),
}

/// Applies the bubble's pause and mute requests one at a time, so a double click lands in order, off the driver's loop, since a mute waits on the capture thread for up to 5 s.
/// The count is the requests not yet applied.
#[cfg(target_os = "macos")]
fn control_worker(
    state: AppState,
) -> (
    tokio::sync::mpsc::UnboundedSender<Control>,
    std::sync::Arc<AtomicUsize>,
) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let pending = std::sync::Arc::new(AtomicUsize::new(0));
    let left = pending.clone();
    tokio::spawn(async move {
        while let Some(control) = rx.recv().await {
            let result = match control {
                Control::Pause(id, at) => state.meetings.set_paused_at(&id, at).await,
                Control::Mute(id, muted) => state.meetings.set_mic_muted(&id, muted).await,
            };
            if let Err(e) = result {
                tracing::warn!(error = ?e, "the bubble's pause or mute did not apply");
            }
            left.fetch_sub(1, Ordering::Relaxed);
        }
    });
    (tx, pending)
}

type Status = (Option<u64>, bool);

/// What the panel should hear: the meeting's status `now`, unless it already shows that or a click is still being applied.
/// A click updates `sent` to what it asked for, so a request that fails shows up here as a difference and snaps the panel back.
fn push(pending: usize, sent: Option<Status>, now: Status) -> Option<Status> {
    (pending == 0 && sent != Some(now)).then_some(now)
}

/// A meeting's pause, as the Unix ms it began or `None`, and its mic mute, as the bubble draws them.
async fn status_of(m: &crate::meetings::Meeting) -> Status {
    let paused = m.paused_since_ms.load(std::sync::atomic::Ordering::Relaxed);
    ((paused != 0).then_some(paused), *m.mic_muted.lock().await)
}

/// What the bubble reads from the meeting's row when it shows.
#[derive(Debug, Default)]
struct Row {
    /// Unix ms, which a restart keeps, so the bubble's timer matches the page's.
    started_at: u64,
    title: String,
    transcript_json: String,
}

async fn row(state: &AppState, id: uuid::Uuid) -> Row {
    let repo = state.meeting_repo.clone();
    let row = tokio::task::spawn_blocking(move || repo.get(&id.to_string()))
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten();
    match row {
        Some(m) => Row {
            started_at: m.started_at as u64,
            title: m.title,
            transcript_json: m.transcript_json,
        },
        None => Row {
            started_at: crate::meetings::now_ms(),
            ..Default::default()
        },
    }
}

fn title_of(row: &Row) -> String {
    if row.title.trim().is_empty() {
        "Recording".to_string()
    } else {
        row.title.clone()
    }
}

/// The card for a meeting that shows mid-recording: its title and the last stored lines.
/// Each channel's newest stored final, them then me, which the live feed can deliver again right after a show.
type SeededTo = [Option<u64>; 2];

/// Whether a live event is newer than what the card was seeded with on its channel.
fn unseen(e: &yogurt_stt::TranscriptEvent, seeded_to: SeededTo) -> bool {
    let me = e.channel == yogurt_stt::Channel::Mic;
    seeded_to[usize::from(me)].is_none_or(|t| e.ts_ms > t)
}

fn seeded(row: &Row) -> (yogurt_voice::bubble::Card, SeededTo) {
    let mut card = yogurt_voice::bubble::Card {
        title: title_of(row),
        ..Default::default()
    };
    let segments: Vec<serde_json::Value> =
        serde_json::from_str(&row.transcript_json).unwrap_or_default();
    let from = segments
        .len()
        .saturating_sub(yogurt_voice::bubble::CARD_LINES);
    let mut seeded_to: SeededTo = [None; 2];
    for seg in &segments[from..] {
        if let Some(text) = seg["text"].as_str() {
            let me = seg["channel"] == "me";
            card.merge(me, text, true);
            seeded_to[usize::from(me)] = seg["ts_ms"].as_u64();
        }
    }
    (card, seeded_to)
}

/// The next transcript event, for `select!` while a meeting is shown.
#[cfg(target_os = "macos")]
async fn next_line(
    rx: &mut Option<tokio::sync::broadcast::Receiver<yogurt_stt::TranscriptEvent>>,
) -> Result<yogurt_stt::TranscriptEvent, tokio::sync::broadcast::error::RecvError> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_shows_only_while_recording_enabled_and_no_page_has_focus() {
        let b = BubbleShared::default();
        assert!(b.wanted(true, true));
        assert!(!b.wanted(false, true));
        assert!(!b.wanted(true, false));
        b.page_focus(false, true);
        assert!(!b.wanted(true, true));
        b.page_focus(true, false);
        assert!(b.wanted(true, true));
    }

    #[test]
    fn a_second_focused_page_keeps_it_hidden_after_the_first_blurs() {
        let b = BubbleShared::default();
        b.page_focus(false, true);
        b.page_focus(false, true);
        b.page_focus(true, false);
        assert!(!b.wanted(true, true));
        b.page_focus(true, true);
        assert!(!b.wanted(true, true));
    }

    #[test]
    fn closing_it_lasts_until_a_page_has_focus_again() {
        let b = BubbleShared::default();
        b.dismiss();
        assert!(!b.wanted(true, true));
        b.page_focus(false, true);
        assert!(!b.wanted(true, true));
        b.page_focus(true, false);
        assert!(b.wanted(true, true));
    }

    #[test]
    fn each_tick_shows_hides_or_says_saved() {
        let (a, b) = (uuid::Uuid::now_v7(), uuid::Uuid::now_v7());
        for (wanted, shown, active, want) in [
            (true, None, Some(a), Step::Show(a)),
            (true, Some(a), Some(a), Step::Stay),
            (true, Some(a), Some(b), Step::Show(b)),
            (false, Some(a), Some(a), Step::Hide),
            (false, Some(a), None, Step::Saved),
            (false, Some(a), Some(b), Step::Saved),
            (false, None, Some(a), Step::Stay),
            (false, None, None, Step::Stay),
        ] {
            assert_eq!(
                step(wanted, shown, active),
                want,
                "{wanted} {shown:?} {active:?}"
            );
        }
    }

    #[test]
    fn pages_are_counted_as_they_open_and_close() {
        let b = BubbleShared::default();
        assert!(!b.has_pages());
        b.page_opened();
        assert!(b.has_pages());
        b.page_closed();
        assert!(!b.has_pages());
    }

    #[test]
    fn the_panel_hears_a_change_once_nothing_is_pending() {
        let (live, muted) = ((None, false), (None, true));
        assert_eq!(
            push(0, Some(live), muted),
            Some(muted),
            "a mute from the page"
        );
        assert_eq!(push(0, Some(muted), muted), None, "already shown");
        assert_eq!(push(1, Some(muted), live), None, "a click still applying");
        assert_eq!(
            push(0, Some(muted), live),
            Some(live),
            "a click that failed snaps back"
        );
        assert_eq!(push(0, None, live), Some(live), "a fresh show");
    }

    #[tokio::test]
    async fn a_running_meeting_reads_as_unpaused_and_a_pause_reads_as_its_start() {
        let registry = crate::meetings::Registry::new();
        let m = registry.create().await;
        assert_eq!(status_of(&m).await, (None, false));
        m.paused_since_ms
            .store(1_794_823_376_871, std::sync::atomic::Ordering::Relaxed);
        *m.mic_muted.lock().await = true;
        assert_eq!(status_of(&m).await, (Some(1_794_823_376_871), true));
    }

    #[test]
    fn a_card_shown_mid_meeting_starts_from_the_stored_lines() {
        let n = yogurt_voice::bubble::CARD_LINES as u64 + 1;
        let segments: Vec<_> = (1..=n)
            .map(|i| serde_json::json!({"ts_ms": i, "channel": if i % 2 == 0 { "me" } else { "them" }, "text": format!("Line {i}")}))
            .collect();
        let row = Row {
            started_at: 0,
            title: String::new(),
            transcript_json: serde_json::to_string(&segments).unwrap(),
        };
        let (card, seeded_to) = seeded(&row);
        assert_eq!(seeded_to, [Some(n), Some(n - 1)]);
        assert_eq!(card.title, "Recording");
        assert_eq!(
            card.lines
                .iter()
                .map(|l| (l.me, l.text.clone()))
                .collect::<Vec<_>>(),
            (2..=n)
                .map(|i| (i % 2 == 0, format!("Line {i}")))
                .collect::<Vec<_>>(),
        );
        assert!(seeded(&Row {
            transcript_json: "not json".into(),
            ..row
        })
        .0
        .lines
        .is_empty());
    }

    #[test]
    fn the_live_feed_skips_only_what_the_seed_already_holds_on_its_channel() {
        let ev = |channel, ts_ms| yogurt_stt::TranscriptEvent {
            ts_ms,
            channel,
            text: "Yeah.".into(),
            is_final: true,
            confidence: None,
        };
        let (them, me) = (yogurt_stt::Channel::System, yogurt_stt::Channel::Mic);
        let seeded_to = [Some(5_000), Some(3_000)];
        assert!(
            !unseen(&ev(them, 5_000), seeded_to),
            "the seeded final again"
        );
        assert!(
            unseen(&ev(them, 6_000), seeded_to),
            "the same words said again later"
        );
        // A mic line that started before the newest seeded line on the other channel.
        assert!(unseen(&ev(me, 4_000), seeded_to));
        assert!(!unseen(&ev(me, 3_000), seeded_to));
        assert!(unseen(&ev(me, 0), [None; 2]), "nothing seeded");
    }

    #[test]
    fn a_saved_position_parses_and_junk_is_ignored() {
        let db = yogurt_db::Db::open_in_memory().unwrap();
        assert_eq!(position(&db), None);
        yogurt_db::settings::set(&db, POSITION_KEY, "1708.5,40").unwrap();
        assert_eq!(position(&db), Some((1708.5, 40.0)));
        yogurt_db::settings::set(&db, POSITION_KEY, "left").unwrap();
        assert_eq!(position(&db), None);
    }
}
