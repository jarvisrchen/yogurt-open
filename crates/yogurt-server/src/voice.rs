//! Yogurt Voice: hold or tap the hotkey, talk, and the transcript is pasted where the cursor is.
//!
//! The `yogurt_voice` tap turns key events into Start, HandsFree, Finish, Cancel, and Skip, and this task shows each clip's state on the pill.
//! This task owns the mic clip, the transcription model, the cleanup, and the paste, and re-reads `general.voice_*` every [`POLL`].
//! The clip lives only in memory and is dropped once it is transcribed.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, mpsc, oneshot};
use tokio::task::JoinHandle;
use yogurt_llm::LlmClient;
use yogurt_stt::deepgram::DeepgramStt;
use yogurt_stt::LocalModel;
use yogurt_voice::{Action, Hotkey, PillState, SharedHotkey};

use crate::state::AppState;
use crate::voice_cleanup::{Cleaned, Interrupt, Tried};
use yogurt_db::style::Kind;

/// Also how soon Voice starts after the user grants Accessibility.
const POLL: Duration = Duration::from_secs(2);

/// How long after the release Deepgram has to return the text before the local model takes over.
/// Streaming leaves only the tail to flush, which took about 0.2 s here, but 1 run in 30 stalled for 48 s.
const DEEPGRAM_WAIT: Duration = Duration::from_secs(2);

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        trim_history(&state).await;
        run(state).await
    });
}

/// Deletes the dictation text older than the retention setting allows. Logs instead of failing, since the next save retries.
pub async fn trim_history(state: &AppState) {
    let db = state.db.clone();
    let trimmed = tokio::task::spawn_blocking(move || {
        let retention = yogurt_db::settings::load_general(&db)?.voice_retention;
        yogurt_db::dictations::trim(&db, retention, crate::routes::now_unix_ms())
    })
    .await;
    match trimmed {
        Ok(Ok(0)) => {}
        Ok(Ok(n)) => tracing::info!(deleted = n, "voice: trimmed the dictation history"),
        Ok(Err(e)) => tracing::warn!(error = %e, "voice: could not trim the dictation history"),
        Err(e) => tracing::warn!(error = %e, "voice: trimming the dictation history panicked"),
    }
}

struct Clip {
    /// Numbers clips for the pill, which drops states from an older clip.
    id: u64,
    /// Dropping it closes the mic.
    _stop: std::sync::mpsc::Sender<()>,
    pcm: JoinHandle<Vec<i16>>,
    /// Unix ms when the hotkey went down.
    started_at: i64,
    hands_free: bool,
    mic: MicOutcome,
    /// Resolved at key press so its connection warms while the user talks. `None` with cleanup off.
    cleanup: Option<Result<Arc<dyn LlmClient>, String>>,
    /// Fixed at key press, so switching engines mid-clip cannot drop the clip. On Deepgram the socket opens then and is fed the audio as it is recorded.
    stt: Option<Stt>,
    _busy: Arc<()>,
}

/// A Deepgram dictation in flight. Dropping it, as Esc or a dropped shortcut does, closes the socket and discards the text.
struct Streaming {
    model: String,
    task: JoinHandle<anyhow::Result<String>>,
}

impl Drop for Streaming {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone)]
enum Engine {
    /// The model name and the loaded model, kept while Voice is on because loading costs 70 to 750 ms.
    Local(String, Arc<LocalModel>),
    Deepgram(Arc<DeepgramStt>),
}

/// What a finished clip is transcribed with.
enum Stt {
    Local(String, Arc<LocalModel>),
    Deepgram(Streaming),
}

/// Where a cleanup waits for the hotkey or Esc. Set while the pill says Cleaning up.
type Cleaning = Arc<Mutex<Option<oneshot::Sender<Interrupt>>>>;

/// Pastes in the order the clips finished.
struct Order {
    after: Option<oneshot::Receiver<()>>,
    /// Dropped when this clip's `finish` returns, which lets the next clip paste.
    _pasted: oneshot::Sender<()>,
}

/// Whichever comes first wins: the clip ending, or its mic failing to open.
/// So an error never shows after a cancel, and the pill never hides an error the user has yet to read.
#[derive(Clone, Default)]
struct MicOutcome(Arc<AtomicU8>);

impl MicOutcome {
    const LIVE: u8 = 0;
    const ENDED: u8 = 1;
    const FAILED: u8 = 2;

    fn settle(&self, to: u8) -> bool {
        self.0
            .compare_exchange(Self::LIVE, to, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// `false` if the mic failed first, and the pill already shows that.
    fn end(&self) -> bool {
        self.settle(Self::ENDED)
    }

    /// `false` if the clip already ended, so the failure is only logged.
    fn fail(&self) -> bool {
        self.settle(Self::FAILED)
    }

    fn failed(&self) -> bool {
        self.0.load(Ordering::SeqCst) == Self::FAILED
    }
}

#[derive(Default)]
struct Voice {
    hotkey: SharedHotkey,
    tap_started: bool,
    engine: Option<Engine>,
    /// A model file that failed to load, and why, so the poll does not reload it every tick.
    failed_load: Option<(PathBuf, String)>,
    clip: Option<Clip>,
    last_clip: u64,
    cleaning: Cleaning,
    /// Resolves once the last finished clip has pasted or given up, so a short clip cannot paste ahead of a longer one still cleaning up.
    last_paste: Option<oneshot::Receiver<()>>,
    /// Logged when it changes, so a 2 s poll does not repeat itself.
    status: String,
    /// Proof this yogurt owns its shortcut on this machine, so no other yogurt answers the same press.
    lock: Option<crate::voice_lock::ShortcutLock>,
}

const NOT_TRUSTED: &str = "waiting for Accessibility: turn on the terminal that runs yogurt in System Settings > Privacy & Security > Accessibility";

async fn run(state: AppState) {
    let (tx, mut rx) = mpsc::unbounded_channel::<(Action, Instant)>();
    let mut voice = Voice {
        hotkey: state.voice.hotkey.clone(),
        ..Voice::default()
    };
    let mut tick = tokio::time::interval(POLL);
    loop {
        tokio::select! {
            _ = tick.tick() => {}
            _ = state.voice.wake.notified() => {}
            Some((action, at)) = rx.recv() => {
                voice.act(&state, action, at);
                continue;
            }
        }
        let status = voice.reconcile(&state, &tx).await;
        if status != voice.status {
            tracing::info!(status = %status, "voice");
            *state.voice.status.lock().unwrap_or_else(|e| e.into_inner()) = status.clone();
            voice.status = status;
        }
    }
}

impl Voice {
    fn set_hotkey(&self, hotkey: Option<Hotkey>) {
        *self.hotkey.lock().unwrap_or_else(|e| e.into_inner()) = hotkey;
    }

    /// Stops answering the shortcut and frees it for another yogurt.
    fn disarm(&mut self) {
        self.set_hotkey(None);
        self.lock = None;
    }

    /// Claimed after the checks that do no work, and given up when the model is missing or will not load, so a yogurt that cannot dictate does not keep the shortcut from one that can.
    /// A yogurt that loses also drops its engine, so it holds no model while it waits its turn.
    fn claim(&mut self, state: &AppState, hotkey: &Hotkey, shortcut: &str) -> Result<(), String> {
        if self.lock.as_ref().is_some_and(|l| l.is_for(hotkey)) {
            return Ok(());
        }
        self.disarm();
        let claimed = crate::voice_lock::default_dir()
            .and_then(|dir| Ok(crate::voice_lock::claim(&dir, hotkey, state.bind_port)?));
        let why = match claimed {
            Ok(Ok(lock)) => {
                self.lock = Some(lock);
                return Ok(());
            }
            Ok(Err(Some(port))) => format!("{shortcut} belongs to the yogurt on port {port}"),
            Ok(Err(None)) => format!("{shortcut} belongs to another yogurt"),
            Err(e) => format!("could not claim {shortcut}: {e:#}"),
        };
        self.engine = None;
        Err(why)
    }

    /// The tap goes in only after the claim, so a yogurt that never owns a shortcut never installs one.
    fn arm(
        &mut self,
        state: &AppState,
        tx: &mpsc::UnboundedSender<(Action, Instant)>,
        hotkey: Hotkey,
        shortcut: &str,
    ) -> Result<(), String> {
        self.claim(state, &hotkey, shortcut)?;
        if let Err(e) = self.start_tap(state, tx) {
            self.disarm();
            return Err(e);
        }
        self.set_hotkey(Some(hotkey));
        Ok(())
    }

    async fn reconcile(
        &mut self,
        state: &AppState,
        tx: &mpsc::UnboundedSender<(Action, Instant)>,
    ) -> String {
        let g = match yogurt_db::settings::load_general(&state.db) {
            Ok(g) => g,
            Err(e) => return format!("could not read settings: {e}"),
        };
        // Before every early return, so a paused or waiting Voice still loads or frees the model.
        crate::voice_cleanup::keep_builtin(
            g.voice_enabled
                && g.voice_cleanup
                && g.voice_cleanup_provider == yogurt_db::settings::VOICE_CLEANUP_BUILTIN,
        );
        let recording = state.voice.recording().is_some();
        if !g.voice_enabled {
            self.disarm();
            self.engine = None;
            self.drop_clip();
            if !recording {
                return "off".into();
            }
        }
        if recording {
            self.set_hotkey(None);
            if let Err(e) = self.start_tap(state, tx) {
                return e;
            }
            return "paused while Settings records a new shortcut".into();
        }
        let shortcut = state.voice.shortcut.as_deref().unwrap_or(&g.voice_hotkey);
        let hotkey: Hotkey = match shortcut.parse() {
            Ok(h) => h,
            Err(e) => {
                self.disarm();
                return e.to_string();
            }
        };
        if !self.tap_started && !yogurt_voice::is_trusted() {
            self.disarm();
            return NOT_TRUSTED.into();
        }
        let mut model = g.voice_model().to_string();
        if g.voice_stt_model.is_empty() {
            // The default depends on which models are downloaded, so only the yogurt that owns the shortcut checks the disk on every poll.
            if let Err(why) = self.claim(state, &hotkey, shortcut) {
                self.set_hotkey(None);
                self.engine = None;
                return why;
            }
            model = voice_model(&g).await;
        }
        if model == yogurt_db::settings::VOICE_STT_DEEPGRAM {
            let Some(key) = crate::meetings::deepgram_key(state.keys.as_ref()) else {
                self.disarm();
                self.engine = None;
                return "no Deepgram key stored; add one in Settings > Transcription".into();
            };
            let dg = match &self.engine {
                Some(Engine::Deepgram(dg)) if dg.api_key == key => dg.clone(),
                _ => Arc::new(DeepgramStt::new(key)),
            };
            let status = format!("on, hotkey {shortcut}, Deepgram {}", dg.model);
            self.engine = Some(Engine::Deepgram(dg));
            if let Err(why) = self.arm(state, tx, hotkey, shortcut) {
                return why;
            }
            return status;
        }
        if !matches!(&self.engine, Some(Engine::Local(m, _)) if *m == model) {
            self.set_hotkey(None);
            self.engine = None;
            // Before the lookup, so a yogurt that does not own the shortcut does no model work on every poll.
            if let Err(why) = self.claim(state, &hotkey, shortcut) {
                return why;
            }
            let found = tokio::task::spawn_blocking({
                let model = model.clone();
                move || {
                    let spec = yogurt_stt::models::lookup(&model)?;
                    Some((spec, yogurt_stt::models::resolve_model(spec)?))
                }
            })
            .await
            .ok()
            .flatten();
            let Some((spec, path)) = found else {
                self.disarm();
                return format!(
                    "the local model {model} is not downloaded; download it in Settings > Voice"
                );
            };
            if let Some((failed, why)) = &self.failed_load {
                if *failed == path {
                    let why = why.clone();
                    self.disarm();
                    return why;
                }
            }
            let loaded = tokio::task::spawn_blocking({
                let path = path.clone();
                move || LocalModel::load(spec, path)
            })
            .await;
            match loaded {
                Ok(Ok(w)) => self.engine = Some(Engine::Local(model.clone(), Arc::new(w))),
                Ok(Err(e)) => {
                    self.disarm();
                    let why = format!("could not load {}: {e:#}", path.display());
                    self.failed_load = Some((path, why.clone()));
                    return why;
                }
                Err(e) => {
                    self.disarm();
                    return format!("loading the transcription model panicked: {e}");
                }
            }
        }
        if let Err(why) = self.arm(state, tx, hotkey, shortcut) {
            return why;
        }
        format!("on, hotkey {shortcut}, model {model}")
    }

    /// Once per process: the tap lives until exit.
    fn start_tap(
        &mut self,
        state: &AppState,
        tx: &mpsc::UnboundedSender<(Action, Instant)>,
    ) -> Result<(), String> {
        if self.tap_started {
            return Ok(());
        }
        if !yogurt_voice::is_trusted() {
            return Err(NOT_TRUSTED.into());
        }
        let tx = tx.clone();
        let cleaning = self.cleaning.clone();
        let cleaning_up = move || {
            lock(&cleaning)
                .as_ref()
                .is_some_and(|waiting| !waiting.is_closed())
        };
        yogurt_voice::spawn_tap(
            self.hotkey.clone(),
            state.voice.recording.clone(),
            cleaning_up,
            move |a, at| {
                let _ = tx.send((a, at));
            },
        )
        .map_err(|e| e.to_string())?;
        self.tap_started = true;
        Ok(())
    }

    fn drop_clip(&mut self) -> bool {
        let Some(clip) = self.clip.take() else {
            return false;
        };
        if clip.mic.end() {
            yogurt_voice::show(clip.id, PillState::Hidden);
        }
        true
    }

    fn act(&mut self, state: &AppState, action: Action, at: Instant) {
        match action {
            Action::Start => {
                self.clip = None;
                self.last_clip += 1;
                let id = self.last_clip;
                yogurt_voice::show(id, PillState::Recording);
                let mic = MicOutcome::default();
                let g = yogurt_db::settings::load_general(&state.db).ok();
                let device = g
                    .as_ref()
                    .map(|g| g.voice_mic().to_string())
                    .unwrap_or_default();
                let (frames_tx, mut frames) =
                    broadcast::channel::<yogurt_audio::Frame>(yogurt_audio::BROADCAST_CAPACITY);
                let (stop, stopped) = std::sync::mpsc::channel::<()>();
                let rt = tokio::runtime::Handle::current();
                // `MicCapture` holds a `cpal::Stream`, which is not `Send`, so it lives on its own thread until the clip ends.
                let outcome = mic.clone();
                std::thread::spawn(move || {
                    let _rt = rt.enter();
                    let device = Some(device.as_str()).filter(|d| !d.is_empty());
                    let opened = yogurt_audio::spawn_mic_capture(frames_tx.clone(), device)
                        .or_else(|e| match (device, e) {
                            (Some(name), e @ yogurt_audio::AudioError::MicUnavailable(_)) => {
                                tracing::warn!(error = %e, device = name, "voice: mic unavailable, using the system default");
                                yogurt_audio::spawn_mic_capture(frames_tx, None)
                            }
                            (_, e) => Err(e),
                        });
                    match opened {
                        Ok(mic) => {
                            let _ = stopped.recv();
                            drop(mic);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "voice: could not open the mic");
                            if outcome.fail() {
                                yogurt_voice::show(id, PillState::Error("Mic unavailable"));
                            }
                        }
                    }
                });
                // After the mic thread starts, so reading the provider and its key does not hold up opening the mic.
                let cleanup = g.filter(|g| g.voice_cleanup).map(|g| {
                    let model = crate::voice_cleanup::model(state, &g.voice_cleanup_provider);
                    if let Ok(client) = &model {
                        let client = client.clone();
                        tokio::spawn(
                            async move { crate::voice_cleanup::warm(client.as_ref()).await },
                        );
                    }
                    model
                });
                let (audio, stt) = match &self.engine {
                    Some(Engine::Deepgram(dg)) => {
                        let (audio, rx) = mpsc::unbounded_channel();
                        let dg = dg.clone();
                        let model = dg.model.clone();
                        let s = state.clone();
                        let task = tokio::spawn(async move {
                            let words = tokio::task::spawn_blocking(move || {
                                yogurt_db::dictionary::engine_words(&s.pro.dictionary(&s))
                            })
                            .await?;
                            dg.dictate(rx, &words).await
                        });
                        (Some(audio), Some(Stt::Deepgram(Streaming { model, task })))
                    }
                    Some(Engine::Local(model, m)) => {
                        (None, Some(Stt::Local(model.clone(), m.clone())))
                    }
                    None => (None, None),
                };
                // Ends when dropping the mic drops the frame sender, and then drops `audio`, which ends the Deepgram stream.
                let pcm = tokio::spawn(async move {
                    let mut pcm = Vec::new();
                    loop {
                        match frames.recv().await {
                            Ok(f) => {
                                yogurt_voice::level(crate::meetings::peak_amplitude(&f.samples));
                                pcm.extend_from_slice(&f.samples);
                                if let Some(audio) = &audio {
                                    let _ = audio.send(f.samples);
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(_)) => {}
                            Err(broadcast::error::RecvError::Closed) => return pcm,
                        }
                    }
                });
                self.clip = Some(Clip {
                    id,
                    _stop: stop,
                    pcm,
                    started_at: crate::routes::now_unix_ms(),
                    hands_free: false,
                    mic,
                    cleanup,
                    stt,
                    _busy: state.voice.clips.clone(),
                });
            }
            Action::HandsFree => {
                if let Some(clip) = &mut self.clip {
                    clip.hands_free = true;
                    if !clip.mic.failed() {
                        yogurt_voice::show(clip.id, PillState::HandsFree);
                    }
                }
            }
            Action::Cancel => {
                if self.drop_clip() || self.interrupt(Interrupt::Cancel) {
                    tracing::info!("voice: cancelled");
                }
            }
            Action::Skip => {
                if self.interrupt(Interrupt::Skip) {
                    tracing::info!("voice: skipped the cleanup");
                }
            }
            Action::Finish => {
                let Some(mut clip) = self.clip.take() else {
                    return;
                };
                if !clip.mic.end() {
                    return;
                }
                let id = clip.id;
                let Some(stt) = clip.stt.take() else {
                    return yogurt_voice::show(id, PillState::Hidden);
                };
                yogurt_voice::show(id, PillState::Transcribing);
                let (pasted, turn) = oneshot::channel();
                let order = Order {
                    after: self.last_paste.replace(turn),
                    _pasted: pasted,
                };
                tokio::spawn(finish(
                    state.clone(),
                    clip,
                    stt,
                    self.cleaning.clone(),
                    order,
                    at,
                    state.voice.armed_try(),
                ));
            }
        }
    }

    fn interrupt(&self, i: Interrupt) -> bool {
        lock(&self.cleaning)
            .take()
            .is_some_and(|waiting| waiting.send(i).is_ok())
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `released` is the key event that ended the clip, so the logged latency covers everything after it.
/// With `tried`, the clip ended in the Try Voice field: it is cleaned up as for that kind of app and goes to the page instead of being pasted or saved.
async fn finish(
    state: AppState,
    clip: Clip,
    stt: Stt,
    cleaning: Cleaning,
    order: Order,
    released: Instant,
    tried: Option<Kind>,
) {
    let Clip {
        id,
        _stop,
        pcm,
        started_at,
        hands_free,
        mic: _,
        cleanup,
        stt: _,
        _busy,
    } = clip;
    drop(_stop);
    let Ok(pcm) = pcm.await else {
        yogurt_voice::show(id, PillState::Hidden);
        return;
    };
    let clip_ms = pcm.len() as u64 * 1000 / u64::from(yogurt_audio::SAMPLE_RATE_HZ);
    let s = state.clone();
    let dictionary = tokio::task::spawn_blocking(move || s.pro.dictionary(&s))
        .await
        .unwrap_or_else(|e| {
            tracing::warn!(error = %e, "voice: reading the dictionary panicked");
            vec![]
        });
    let transcribed = transcribe(
        stt,
        pcm,
        released,
        yogurt_db::dictionary::engine_words(&dictionary),
    )
    .await;
    let (raw, transcribe_ms, engine) = match transcribed {
        Ok(Some(t)) if !t.0.is_empty() => t,
        Ok(_) => {
            tracing::info!(clip_ms, "voice: no speech, nothing pasted");
            return yogurt_voice::show(id, PillState::Hidden);
        }
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "voice: transcription failed");
            return yogurt_voice::show(id, PillState::Error("Transcription failed"));
        }
    };

    let (skip, skipped) = oneshot::channel();
    // A newer clip's cleanup took the slot, so this one runs to its own limit.
    let interrupt = async {
        match skipped.await {
            Ok(i) => i,
            Err(_) => std::future::pending().await,
        }
    };
    let (cleaned, style, dictionary) = clean_up(
        &state,
        cleanup,
        raw.clone(),
        dictionary,
        move |s| match tried {
            Some(kind) => s.style(kind),
            None => s.style_for(yogurt_voice::frontmost().as_deref()),
        },
        || {
            // Only once the model runs, so a short clip never takes the slot from a longer one still cleaning up.
            *lock(&cleaning) = Some(skip);
            yogurt_voice::show(id, PillState::CleaningUp);
        },
        interrupt,
    )
    .await;
    lock(&cleaning).take_if(|waiting| waiting.is_closed());
    let Some(cleaned) = cleaned else {
        tracing::info!("voice: cancelled during cleanup");
        return yogurt_voice::show(id, PillState::Hidden);
    };
    // A try still reaches the page, which shows what was heard.
    if cleaned.text.is_empty() && tried.is_none() {
        tracing::info!(clip_ms, "voice: only filler words, nothing pasted");
        return yogurt_voice::show(id, PillState::Hidden);
    }

    let Order { after, _pasted } = order;
    if let Some(after) = after {
        let _ = after.await;
    }
    if tried.is_some() {
        yogurt_voice::show(id, PillState::Pasted);
        tracing::info!(
            transcribe_ms,
            engine,
            cleanup = cleaned.kind,
            clip_ms,
            "voice: tried"
        );
        let tried = crate::voice_cleanup::Tried::new(&raw, cleaned, style, &dictionary);
        return crate::ws::send_event(
            &state.app_events_tx,
            crate::ws::WsEvent::VoiceTry {
                raw,
                stt_engine: engine,
                stt_ms: transcribe_ms,
                tried,
            },
        );
    }
    let text = cleaned.text.clone();
    let pasted = tokio::task::spawn_blocking(move || {
        // Before the paste, which blocks for the clipboard restore, so the check lands with the text.
        yogurt_voice::show(id, PillState::Pasted);
        yogurt_voice::paste(&text)
    })
    .await;
    drop(_pasted);
    let pasted = match pasted {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "voice: paste task panicked");
            return yogurt_voice::show(id, PillState::Hidden);
        }
    };
    tracing::info!(
        app = pasted.app_name.as_deref().unwrap_or("?"),
        bundle_id = pasted.bundle_id.as_deref().unwrap_or("?"),
        release_to_paste_ms = pasted.posted_at.duration_since(released).as_millis() as u64,
        transcribe_ms,
        engine,
        cleanup = cleaned.kind,
        cleanup_ms = cleaned.ms,
        clip_ms,
        words = yogurt_db::dictations::word_count(&cleaned.text),
        "voice: pasted"
    );
    let row = yogurt_db::dictations::NewDictation {
        created_at: started_at,
        app_bundle_id: pasted.bundle_id,
        app_name: pasted.app_name,
        hands_free,
        audio_ms: clip_ms as i64,
        raw_text: raw,
        text: cleaned.text,
        stt_engine: engine,
        stt_ms: transcribe_ms as i64,
        cleanup: cleaned.kind.into(),
        cleanup_model: cleaned.model,
        cleanup_ms: cleaned.ms,
        input_tokens: cleaned.usage.map(|u| u.input_tokens),
        output_tokens: cleaned.usage.map(|u| u.output_tokens),
        cost_usd: cleaned.cost_usd,
    };
    save(&state, row).await;
}

/// The text, the ms it took, and the engine as `dictations.stt_engine` records it, or `None` for a clip with no speech.
/// A local model's time is its decode, and Deepgram's runs from the release, since its wait includes the network.
/// A fallback records its decode only, so a slow network never inflates a local model's average.
async fn transcribe(
    stt: Stt,
    pcm: Vec<i16>,
    released: Instant,
    words: Vec<String>,
) -> anyhow::Result<Option<(String, u64, String)>> {
    let (pcm, speech) = tokio::task::spawn_blocking(move || {
        let speech = yogurt_stt::vad::has_speech(&pcm);
        (pcm, speech)
    })
    .await?;
    // Dropping `stt` here closes a Deepgram socket without waiting for its text.
    if !speech {
        return Ok(None);
    }
    let decode = |model: &str, m: &LocalModel, pcm: &[i16], words: &[String]| {
        let started = Instant::now();
        let text = m.transcribe(pcm, words)?;
        Ok::<_, anyhow::Error>((
            text,
            started.elapsed().as_millis() as u64,
            format!("local:{model}"),
        ))
    };
    match stt {
        Stt::Local(model, m) => Ok(Some(
            tokio::task::spawn_blocking(move || decode(&model, &m, &pcm, &words)).await??,
        )),
        Stt::Deepgram(streaming) => {
            let released = tokio::time::Instant::from_std(released);
            let heard = deepgram_or_local(streaming, released, move || {
                let model = yogurt_stt::models::voice_default();
                let spec = yogurt_stt::models::lookup(model)
                    .ok_or_else(|| anyhow::anyhow!("{model} is not a known model"))?;
                let path = yogurt_stt::models::resolve_model(spec).ok_or_else(|| {
                    anyhow::anyhow!("{model} is not downloaded, so there is no local fallback")
                })?;
                decode(model, &LocalModel::load(spec, path)?, &pcm, &words)
            })
            .await?;
            Ok(Some(heard))
        }
    }
}

/// [`yogurt_db::settings::General::voice_model`], with the default resolved against what is downloaded by
/// [`yogurt_stt::models::voice_default`]. Everything that loads or guards Voice's model goes through this.
pub(crate) async fn voice_model(g: &yogurt_db::settings::General) -> String {
    if !g.voice_stt_model.is_empty() {
        return g.voice_model().to_string();
    }
    tokio::task::spawn_blocking(yogurt_stt::models::voice_default)
        .await
        .unwrap_or(yogurt_db::settings::VOICE_STT_DEFAULT)
        .to_string()
}

/// Deepgram's text, or `local`'s when Deepgram fails or misses [`DEEPGRAM_WAIT`] after `released`, as [`transcribe`] returns them.
async fn deepgram_or_local(
    mut streaming: Streaming,
    released: tokio::time::Instant,
    local: impl FnOnce() -> anyhow::Result<(String, u64, String)> + Send + 'static,
) -> anyhow::Result<(String, u64, String)> {
    let failed = match tokio::time::timeout_at(released + DEEPGRAM_WAIT, &mut streaming.task).await
    {
        Ok(Ok(Ok(text))) => {
            let ms = released.elapsed().as_millis() as u64;
            return Ok((text, ms, format!("deepgram:{}", streaming.model)));
        }
        Ok(Ok(Err(e))) => e,
        Ok(Err(e)) => anyhow::anyhow!("the Deepgram task ended: {e}"),
        Err(_) => anyhow::anyhow!("no result within {DEEPGRAM_WAIT:?} of the release"),
    };
    drop(streaming);
    tracing::warn!(error = %format!("{failed:#}"), "voice: Deepgram failed, trying the local model");
    tokio::task::spawn_blocking(local)
        .await?
        .map_err(|e| e.context(format!("Deepgram failed ({failed:#})")))
}

/// Cleans `raw` again as a dictation into an app of `kind`, for Try Voice's Clean up as.
pub async fn retry(state: &AppState, raw: String, kind: Kind) -> anyhow::Result<Tried> {
    let s = state.clone();
    let (g, dictionary) = tokio::task::spawn_blocking(move || {
        Ok::<_, anyhow::Error>((
            yogurt_db::settings::load_general(&s.db)?,
            s.pro.dictionary(&s),
        ))
    })
    .await??;
    let cleanup = g
        .voice_cleanup
        .then(|| crate::voice_cleanup::model(state, &g.voice_cleanup_provider));
    let (cleaned, style, dictionary) = clean_up(
        state,
        cleanup,
        raw.clone(),
        dictionary,
        move |s| s.style(kind),
        || {},
        std::future::pending(),
    )
    .await;
    let cleaned = cleaned.ok_or_else(|| anyhow::anyhow!("the cleanup was cancelled"))?;
    Ok(Tried::new(&raw, cleaned, style, &dictionary))
}

/// What `raw` becomes, with the style it was cleaned in: a cue said as the whole dictation gives its snippet as written, and anything else is cleaned up.
/// `None` means `interrupt` cancelled it.
async fn clean_up(
    state: &AppState,
    cleanup: Option<Result<Arc<dyn LlmClient>, String>>,
    raw: String,
    dictionary: Vec<yogurt_db::dictionary::Entry>,
    style_of: impl FnOnce(&yogurt_db::style::Setup) -> yogurt_db::style::Style + Send + 'static,
    on_model: impl FnOnce(),
    interrupt: impl std::future::Future<Output = Interrupt>,
) -> (
    Option<Cleaned>,
    yogurt_db::style::Style,
    Vec<yogurt_db::dictionary::Entry>,
) {
    let raw = yogurt_db::dictionary::respell(&dictionary, &raw);
    let model_runs = matches!(cleanup, Some(Ok(_))) && !crate::voice_cleanup::short_clip(&raw);
    let (s, raw_text) = (state.clone(), raw.clone());
    let read = tokio::task::spawn_blocking(move || {
        let (whole, lists) =
            s.pro
                .voice_lists(&s, &dictionary, &raw_text, model_runs, Box::new(style_of));
        (whole, lists, dictionary)
    })
    .await;
    let (whole, lists, dictionary) = read.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "voice: reading the cleanup lists panicked");
        Default::default()
    });
    let style = lists.style;
    let cleaned = match whole {
        Some(text) => Some(Cleaned {
            text,
            kind: "snippet",
            ..Default::default()
        }),
        None => {
            crate::voice_cleanup::dictation(
                cleanup,
                || Ok((state.prompts.voice_cleanup()?, lists)),
                raw,
                on_model,
                interrupt,
            )
            .await
        }
    };
    (cleaned, style, dictionary)
}

/// Always counts in the stats. The text is kept unless the retention is Never, and older text is trimmed.
/// The text was already pasted, so a failure here only logs.
async fn save(state: &AppState, row: yogurt_db::dictations::NewDictation) {
    let db = state.db.clone();
    let saved = tokio::task::spawn_blocking(move || {
        let retention = yogurt_db::settings::load_general(&db)?.voice_retention;
        yogurt_db::dictations::insert(
            &db,
            row,
            retention != yogurt_db::settings::VoiceRetention::Never,
        )
    })
    .await;
    match saved {
        Ok(Ok(d)) => crate::ws::send_event(
            &state.app_events_tx,
            crate::ws::WsEvent::DictationSaved { id: d.id },
        ),
        Ok(Err(e)) => tracing::warn!(error = %e, "voice: could not save the dictation"),
        Err(e) => tracing::warn!(error = %e, "voice: saving the dictation panicked"),
    }
    trim_history(state).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Never runs with `tried` off: that would paste into whatever window has focus.
    #[tokio::test]
    async fn a_clip_ended_in_the_try_field_goes_to_the_page_and_is_never_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Arc::new(crate::storage::Storage::init_at(&tmp.path().join("db.sqlite")).unwrap());
        let session =
            Arc::new(crate::session::load_or_create(&tmp.path().join("session-token")).unwrap());
        let state = AppState::in_memory(
            crate::Mode::Release,
            storage,
            session,
            7878,
            tmp.path().join("notes"),
        )
        .unwrap();
        let mut events = state.app_events_tx.subscribe();
        // "Um, can you send me the deck before the meeting on Wednesday?", spoken by macOS `say`.
        let wav = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/dictation.wav"
        ))
        .unwrap();
        let data = wav.windows(4).position(|w| w == b"data").unwrap() + 8;
        let pcm: Vec<i16> = wav[data..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        let heard = "um can you send me the deck before the meeting on wednesday";
        let clip = Clip {
            id: 1,
            _stop: std::sync::mpsc::channel().0,
            pcm: tokio::spawn(async move { pcm }),
            started_at: 0,
            hands_free: false,
            mic: MicOutcome::default(),
            cleanup: None,
            stt: None,
            _busy: Arc::default(),
        };
        let stt = Stt::Deepgram(Streaming {
            model: "nova-3".into(),
            task: tokio::spawn(async move { Ok(heard.to_string()) }),
        });
        let order = Order {
            after: None,
            _pasted: oneshot::channel().0,
        };
        finish(
            state.clone(),
            clip,
            stt,
            Cleaning::default(),
            order,
            Instant::now(),
            Some(Kind::Email),
        )
        .await;

        let event = events.try_recv().unwrap();
        assert_eq!(event["type"], "voice_try");
        assert_eq!(event["raw"], heard);
        assert_eq!(event["text"], heard);
        assert_eq!(event["stt_engine"], "deepgram:nova-3");
        assert_eq!(event["cleanup"], "off");
        assert!(events.try_recv().is_err(), "no dictation_saved follows");
        let stats = yogurt_db::dictations::stats(
            &state.db,
            crate::routes::now_unix_ms(),
            yogurt_db::stats_range::Range::All,
        )
        .unwrap();
        assert_eq!(stats.count, 0);
    }

    #[tokio::test]
    async fn save_counts_every_paste_but_keeps_text_only_within_the_retention() {
        let tmp = tempfile::tempdir().unwrap();
        let storage =
            Arc::new(crate::storage::Storage::init_at(&tmp.path().join("db.sqlite")).unwrap());
        let session =
            Arc::new(crate::session::load_or_create(&tmp.path().join("session-token")).unwrap());
        let state = AppState::in_memory(
            crate::Mode::Release,
            storage,
            session,
            7878,
            tmp.path().join("notes"),
        )
        .unwrap();
        let mut events = state.app_events_tx.subscribe();
        let now = crate::routes::now_unix_ms();
        let row = |created_at| yogurt_db::dictations::NewDictation {
            created_at,
            text: "hello there".into(),
            raw_text: "hello there".into(),
            cleanup: "off".into(),
            ..Default::default()
        };
        let all = || {
            yogurt_db::dictations::list(&state.db, &Default::default(), 10, 0)
                .unwrap()
                .dictations
        };
        let counted = || {
            yogurt_db::dictations::stats(&state.db, now, yogurt_db::stats_range::Range::All)
                .unwrap()
                .count
        };
        let retain = |r| {
            yogurt_db::settings::save_general_patch(
                &state.db,
                yogurt_db::settings::GeneralPatch {
                    voice_retention: Some(r),
                    ..Default::default()
                },
            )
            .unwrap();
        };

        save(&state, row(now - 2 * 86_400_000)).await;
        save(&state, row(now)).await;
        let saved = all();
        assert_eq!((saved.len(), counted()), (2, 2));
        assert_eq!(events.try_recv().unwrap()["type"], "dictation_saved");
        assert_eq!(events.try_recv().unwrap()["id"], saved[0].id.as_str());

        retain(yogurt_db::settings::VoiceRetention::Hours24);
        save(&state, row(now)).await;
        assert_eq!(
            (all().len(), counted()),
            (2, 3),
            "the 2-day-old text is trimmed on save"
        );

        retain(yogurt_db::settings::VoiceRetention::Never);
        save(&state, row(now)).await;
        assert_eq!(
            (all().len(), counted()),
            (0, 4),
            "Never keeps no text and still counts"
        );
        assert!(events.try_recv().is_ok());
    }

    fn streaming(
        task: impl std::future::Future<Output = anyhow::Result<String>> + Send + 'static,
    ) -> Streaming {
        Streaming {
            model: "nova-3".into(),
            task: tokio::spawn(task),
        }
    }

    fn now() -> tokio::time::Instant {
        tokio::time::Instant::now()
    }

    fn local_text() -> anyhow::Result<(String, u64, String)> {
        Ok(("from parakeet".into(), 200, "local:parakeet-v3".into()))
    }

    #[tokio::test]
    async fn deepgram_text_wins_and_the_local_model_never_runs() {
        let heard = deepgram_or_local(streaming(async { Ok("hi".into()) }), now(), || {
            panic!("no fallback when Deepgram answers")
        })
        .await
        .unwrap();
        assert_eq!(
            (heard.0.as_str(), heard.2.as_str()),
            ("hi", "deepgram:nova-3")
        );
    }

    #[tokio::test]
    async fn a_failed_deepgram_falls_back_to_the_local_model() {
        let failing = streaming(async { anyhow::bail!("Deepgram rejected the key") });
        let heard = deepgram_or_local(failing, now(), local_text).await.unwrap();
        assert_eq!(
            heard,
            ("from parakeet".into(), 200, "local:parakeet-v3".into()),
            "a fallback records only its own decode time"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_deepgram_past_the_deadline_is_closed_and_falls_back() {
        let (alive, closed) = oneshot::channel::<()>();
        let hung = streaming(async move {
            let _alive = alive;
            std::future::pending().await
        });
        let started = tokio::time::Instant::now();
        let heard = deepgram_or_local(hung, now(), local_text).await.unwrap();
        assert_eq!(heard.2, "local:parakeet-v3");
        assert_eq!(started.elapsed(), DEEPGRAM_WAIT);
        assert!(closed.await.is_err(), "the Deepgram task was aborted");
    }

    #[tokio::test]
    async fn no_local_model_after_a_failed_deepgram_is_an_error() {
        let failing = streaming(async { anyhow::bail!("no network") });
        let err = deepgram_or_local(failing, now(), || {
            anyhow::bail!("parakeet-v3 is not downloaded")
        })
        .await
        .unwrap_err();
        let msg = format!("{err:#}");
        assert!(
            msg.contains("no network") && msg.contains("not downloaded"),
            "{msg}"
        );
    }

    #[tokio::test]
    async fn dropping_a_clip_closes_its_deepgram_stream() {
        let (alive, closed) = oneshot::channel::<()>();
        let s = streaming(async move {
            let _alive = alive;
            std::future::pending().await
        });
        drop(s);
        assert!(closed.await.is_err(), "a cancel aborts the Deepgram task");
    }

    #[tokio::test]
    async fn a_clip_with_no_speech_pastes_nothing_and_closes_the_stream() {
        let (alive, closed) = oneshot::channel::<()>();
        let s = streaming(async move {
            let _alive = alive;
            std::future::pending().await
        });
        let silence = vec![0i16; 16_000];
        assert!(
            transcribe(Stt::Deepgram(s), silence, Instant::now(), vec![])
                .await
                .unwrap()
                .is_none()
        );
        assert!(closed.await.is_err());
    }

    #[test]
    fn the_first_of_end_and_fail_wins() {
        let ended = MicOutcome::default();
        assert!(ended.end());
        assert!(!ended.fail(), "a mic failing after a cancel shows no error");
        assert!(!ended.failed());

        let failed = MicOutcome::default();
        assert!(failed.fail());
        assert!(
            !failed.end(),
            "ending a clip whose mic failed leaves the error up"
        );
        assert!(failed.failed());
    }
}
