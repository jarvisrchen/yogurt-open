//! macOS glue for Yogurt Voice. It knows nothing about audio, STT, or settings storage.

pub mod gesture;
pub mod hotkey;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod panel;
pub mod pill;
mod record;

pub use gesture::{Action, Gesture, Key};
pub use hotkey::Hotkey;
#[cfg(target_os = "macos")]
pub use macos::{frontmost, is_trusted, paste, run_app, spawn_tap, Pasted, SharedHotkey, TapError};
#[cfg(target_os = "macos")]
pub use panel::{level, show};
pub use pill::PillState;
pub use record::{Recorder, SharedRecorder, Step};
