//! `settings(key, value)` holds non-secret configuration. Secrets live in the key file, never here.

use crate::Db;
use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

/// The `voice_cleanup_provider` value for the model built into yogurt rather than a provider row.
pub const VOICE_CLEANUP_BUILTIN: &str = "builtin";

pub fn get(db: &Db, key: &str) -> Result<Option<String>> {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT value FROM settings WHERE key=?1",
            params![key],
            |r| r.get(0),
        )
        .optional()
    })
    .map_err(Into::into)
}

pub fn set(db: &Db, key: &str, value: &str) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )
    })?;
    Ok(())
}

/// Typed projection of the well-known `general.*` + `audio.*` keys.
/// `first_run_completed` lives here so the Welcome flow can flip it in one `PATCH /api/settings` alongside other general state. The migration seeds it to `"false"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct General {
    pub port: u16,
    pub open_browser_on_start: bool,
    pub audio_input_device: String,
    /// `true` once the user has completed `/welcome`. `useFirstRunRedirect` reads this to land the SPA on `/welcome` or `/`.
    pub first_run_completed: bool,
    /// Which STT engine `meetings/start.rs` constructs: `"cloud"` (`DeepgramStt`) or `"local"` (`LocalModel`). Seeded to `"cloud"` by V005.
    pub stt_provider: String,
    /// Local model to load when `stt_provider == "local"`. Must match a `name` in `yogurt_stt::models::REGISTRY`.
    /// Seeded to `"small.en"` by V005, and V027 makes a new install's `"parakeet-ultra"`.
    pub stt_model: String,
    /// Watch for meeting app windows and offer to start recording. Detection never starts a recording on its own;
    /// it only surfaces the prompt (and stops a recording once the detected window closes). No seed row; absent means `true`.
    pub meeting_detection: bool,
    /// Raise the installed app when a meeting is detected. No seed row;
    /// absent means `true`.
    pub meeting_detection_focus: bool,
    /// Stop the recording when the meeting window closes or both channels
    /// stay silent. No seed row; absent means `true`.
    pub meeting_auto_stop: bool,
    /// Float the meeting bubble over other apps while a meeting records. No seed row; absent means `true`.
    pub meeting_bubble: bool,
    /// Send the latest transcript to the LLM provider every few minutes while
    /// recording and show the bullets it returns. It has no seed row, so absent means `true`.
    pub live_recap: bool,
    /// Where the recap shows. Key `general.live_recap_placement`; absent or unrecognized means `Dock`.
    pub live_recap_placement: RecapPlacement,
    /// Yogurt Voice dictation. No seed row; absent means `false`.
    pub voice_enabled: bool,
    /// A `yogurt_voice::Hotkey` string such as `fn` or `option+e`. No seed row; absent means `fn`.
    pub voice_hotkey: String,
    /// How long `dictations` keeps the text of each paste. No seed row; absent or unrecognized means 30 days.
    pub voice_retention: VoiceRetention,
    /// Run each dictation through a cleanup model before the paste. No seed row; absent means `false`.
    pub voice_cleanup: bool,
    /// `builtin` or a provider id. No seed row; absent means `builtin`.
    pub voice_cleanup_provider: String,
    /// The model Voice transcribes with. No seed row; absent or `""` means [`VOICE_STT_DEFAULT`], [`VOICE_STT_MEETINGS`] follows `stt_model`, and [`VOICE_STT_DEEPGRAM`] is Deepgram.
    pub voice_stt_model: String,
    /// Voice's own mic. No seed row; absent or `""` follows `audio_input_device`.
    pub voice_input_device: String,
}

/// The `voice_stt_model` an unset row resolves to. Where it loads a model, the server goes through
/// `yogurt_stt::models::voice_default`, which keeps a Mac with only parakeet-v3 on v3 until this is downloaded.
pub const VOICE_STT_DEFAULT: &str = "parakeet-ultra";
pub const VOICE_STT_MEETINGS: &str = "meetings";
/// Uses the Deepgram key and model that meetings use.
pub const VOICE_STT_DEEPGRAM: &str = "deepgram";

impl General {
    /// The local model the voice task loads, or [`VOICE_STT_DEEPGRAM`].
    /// Server code calls `voice::voice_model` instead, which resolves `""` against the models on disk.
    pub fn voice_model(&self) -> &str {
        match self.voice_stt_model.as_str() {
            "" => VOICE_STT_DEFAULT,
            VOICE_STT_MEETINGS => &self.stt_model,
            m => m,
        }
    }

    /// The mic a Voice clip opens. `""` is the system default.
    pub fn voice_mic(&self) -> &str {
        if self.voice_input_device.is_empty() {
            &self.audio_input_device
        } else {
            &self.voice_input_device
        }
    }
}

/// Presentation only: the recap task never reads it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecapPlacement {
    /// A view in the transcript dock.
    #[default]
    Dock,
    /// A card above the notes.
    Main,
}

impl RecapPlacement {
    /// The stored and wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            RecapPlacement::Dock => "dock",
            RecapPlacement::Main => "main",
        }
    }
}

/// The stats rollup is never trimmed, so only the text goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VoiceRetention {
    #[default]
    #[serde(rename = "30d")]
    Days30,
    #[serde(rename = "7d")]
    Days7,
    #[serde(rename = "24h")]
    Hours24,
    #[serde(rename = "never")]
    Never,
}

impl VoiceRetention {
    const ALL: [Self; 4] = [Self::Days30, Self::Days7, Self::Hours24, Self::Never];

    /// The stored and wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Days30 => "30d",
            Self::Days7 => "7d",
            Self::Hours24 => "24h",
            Self::Never => "never",
        }
    }

    /// How long a text row lives. 0 for Never, which keeps none.
    pub fn keep_ms(self) -> i64 {
        const HOUR: i64 = 3_600_000;
        match self {
            Self::Days30 => 30 * 24 * HOUR,
            Self::Days7 => 7 * 24 * HOUR,
            Self::Hours24 => 24 * HOUR,
            Self::Never => 0,
        }
    }
}

/// Load the `General` struct from the KV table, falling back to defaults
/// for missing or malformed values.
pub fn load_general(db: &Db) -> Result<General> {
    Ok(General {
        port: get(db, "general.port")?
            .as_deref()
            .and_then(|s| s.parse().ok())
            .unwrap_or(7878),
        open_browser_on_start: get(db, "general.open_browser_on_start")?
            .as_deref()
            .map(|s| s == "true")
            .unwrap_or(true),
        audio_input_device: get(db, "audio.input_device")?.unwrap_or_default(),
        first_run_completed: get(db, "first_run_completed")?
            .as_deref()
            .map(|s| s == "true")
            .unwrap_or(false),
        // Fallbacks keep the projection working on DBs where V005 has not run.
        stt_provider: get(db, "stt.provider")?.unwrap_or_else(|| "cloud".to_string()),
        stt_model: get(db, "stt.model")?.unwrap_or_else(|| "parakeet-v3".to_string()),
        meeting_detection: get(db, "general.meeting_detection")?
            .as_deref()
            .map(|s| s == "true")
            .unwrap_or(true),
        meeting_detection_focus: get(db, "general.meeting_detection_focus")?
            .as_deref()
            .map(|s| s == "true")
            .unwrap_or(true),
        meeting_auto_stop: get(db, "general.meeting_auto_stop")?
            .as_deref()
            .map(|s| s == "true")
            .unwrap_or(true),
        meeting_bubble: get(db, "general.meeting_bubble")?
            .as_deref()
            .map(|s| s == "true")
            .unwrap_or(true),
        live_recap: get(db, "general.live_recap")?
            .as_deref()
            .map(|s| s == "true")
            .unwrap_or(true),
        live_recap_placement: match get(db, "general.live_recap_placement")?.as_deref() {
            Some("main") => RecapPlacement::Main,
            _ => RecapPlacement::Dock,
        },
        voice_enabled: get(db, "general.voice_enabled")?
            .as_deref()
            .map(|s| s == "true")
            .unwrap_or(false),
        voice_hotkey: get(db, "general.voice_hotkey")?.unwrap_or_else(|| "fn".to_string()),
        voice_retention: get(db, "general.voice_retention")?
            .and_then(|v| VoiceRetention::ALL.into_iter().find(|r| r.as_str() == v))
            .unwrap_or_default(),
        voice_cleanup: get(db, "general.voice_cleanup")?
            .as_deref()
            .map(|s| s == "true")
            .unwrap_or(false),
        voice_cleanup_provider: get(db, "general.voice_cleanup_provider")?
            .unwrap_or_else(|| VOICE_CLEANUP_BUILTIN.to_string()),
        voice_stt_model: get(db, "general.voice_stt_model")?.unwrap_or_default(),
        voice_input_device: get(db, "general.voice_input_device")?.unwrap_or_default(),
    })
}

/// `None` fields are left untouched.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct GeneralPatch {
    pub port: Option<u16>,
    pub open_browser_on_start: Option<bool>,
    pub audio_input_device: Option<String>,
    pub first_run_completed: Option<bool>,
    /// `"cloud"` (Deepgram) or `"local"` (`LocalModel`).
    pub stt_provider: Option<String>,
    /// One of the names in `yogurt_stt::models::REGISTRY`.
    pub stt_model: Option<String>,
    /// Silences the meeting-detected prompt.
    pub meeting_detection: Option<bool>,
    pub meeting_detection_focus: Option<bool>,
    pub meeting_auto_stop: Option<bool>,
    pub meeting_bubble: Option<bool>,
    pub live_recap: Option<bool>,
    pub live_recap_placement: Option<RecapPlacement>,
    pub voice_enabled: Option<bool>,
    pub voice_hotkey: Option<String>,
    pub voice_retention: Option<VoiceRetention>,
    pub voice_cleanup: Option<bool>,
    pub voice_cleanup_provider: Option<String>,
    /// `""` follows `stt_model`.
    pub voice_stt_model: Option<String>,
    /// `""` follows `audio_input_device`.
    pub voice_input_device: Option<String>,
}

/// Apply `patch` and return the resulting `General`. Loading after the write avoids assuming the patch landed when a concurrent writer overwrote the row.
pub fn save_general_patch(db: &Db, patch: GeneralPatch) -> Result<General> {
    if let Some(p) = patch.port {
        set(db, "general.port", &p.to_string())?;
    }
    if let Some(o) = patch.open_browser_on_start {
        set(
            db,
            "general.open_browser_on_start",
            if o { "true" } else { "false" },
        )?;
    }
    if let Some(d) = patch.audio_input_device {
        set(db, "audio.input_device", &d)?;
    }
    if let Some(f) = patch.first_run_completed {
        set(db, "first_run_completed", if f { "true" } else { "false" })?;
    }
    if let Some(p) = patch.stt_provider {
        set(db, "stt.provider", &p)?;
    }
    if let Some(m) = patch.stt_model {
        set(db, "stt.model", &m)?;
    }
    if let Some(d) = patch.meeting_detection {
        set(
            db,
            "general.meeting_detection",
            if d { "true" } else { "false" },
        )?;
    }
    if let Some(d) = patch.meeting_detection_focus {
        set(
            db,
            "general.meeting_detection_focus",
            if d { "true" } else { "false" },
        )?;
    }
    if let Some(d) = patch.meeting_auto_stop {
        set(
            db,
            "general.meeting_auto_stop",
            if d { "true" } else { "false" },
        )?;
    }
    if let Some(b) = patch.meeting_bubble {
        set(
            db,
            "general.meeting_bubble",
            if b { "true" } else { "false" },
        )?;
    }
    if let Some(r) = patch.live_recap {
        set(db, "general.live_recap", if r { "true" } else { "false" })?;
    }
    if let Some(p) = patch.live_recap_placement {
        set(db, "general.live_recap_placement", p.as_str())?;
    }
    if let Some(v) = patch.voice_enabled {
        set(
            db,
            "general.voice_enabled",
            if v { "true" } else { "false" },
        )?;
    }
    if let Some(h) = patch.voice_hotkey {
        set(db, "general.voice_hotkey", &h)?;
    }
    if let Some(r) = patch.voice_retention {
        set(db, "general.voice_retention", r.as_str())?;
    }
    if let Some(c) = patch.voice_cleanup {
        set(
            db,
            "general.voice_cleanup",
            if c { "true" } else { "false" },
        )?;
    }
    if let Some(p) = patch.voice_cleanup_provider {
        set(db, "general.voice_cleanup_provider", &p)?;
    }
    if let Some(m) = patch.voice_stt_model {
        set(db, "general.voice_stt_model", &m)?;
    }
    if let Some(d) = patch.voice_input_device {
        set(db, "general.voice_input_device", &d)?;
    }
    load_general(db)
}
