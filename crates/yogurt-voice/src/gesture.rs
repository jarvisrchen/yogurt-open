//! Tells a hold from a tap from a shortcut, with no mode setting.
//!
//! Recording starts on the hotkey's press, so no words are lost while the gesture is still ambiguous.
//! A release at [`HOLD_THRESHOLD`] or later was a hold and finishes the clip.
//! An earlier release switches to hands-free, and the next press finishes the clip.
//! Any other key going down while the hotkey is held makes the press a shortcut, which is dropped.

use std::time::{Duration, Instant};

pub const HOLD_THRESHOLD: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    HotkeyDown,
    HotkeyUp,
    Escape,
    OtherKeyDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Start,
    /// A tap: the clip keeps recording until the next press.
    HandsFree,
    /// Transcribe the clip and paste it.
    Finish,
    /// Drop the clip, or the paste a cleanup is waiting on.
    Cancel,
    /// Paste the raw text without waiting for the cleanup.
    Skip,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Idle,
    Held(Instant),
    HandsFree,
}

#[derive(Debug, Default)]
pub struct Gesture {
    state: State,
}

impl Gesture {
    pub fn on(&mut self, key: Key, now: Instant) -> Option<Action> {
        let (next, action) = match (self.state, key) {
            (State::Idle, Key::HotkeyDown) => (State::Held(now), Some(Action::Start)),
            (State::Idle, _) => (State::Idle, None),
            (State::Held(since), Key::HotkeyDown) => (State::Held(since), None),
            (State::Held(since), Key::HotkeyUp) if now.duration_since(since) >= HOLD_THRESHOLD => {
                (State::Idle, Some(Action::Finish))
            }
            (State::Held(_), Key::HotkeyUp) => (State::HandsFree, Some(Action::HandsFree)),
            (State::Held(_), Key::Escape | Key::OtherKeyDown) => {
                (State::Idle, Some(Action::Cancel))
            }
            (State::HandsFree, Key::HotkeyDown) => (State::Idle, Some(Action::Finish)),
            (State::HandsFree, Key::Escape) => (State::Idle, Some(Action::Cancel)),
            (State::HandsFree, Key::HotkeyUp | Key::OtherKeyDown) => (State::HandsFree, None),
        };
        self.state = next;
        action
    }

    pub fn is_active(&self) -> bool {
        self.state != State::Idle
    }

    pub fn reset(&mut self) {
        self.state = State::Idle;
    }
}

/// While no clip records and a finished one is being cleaned up, the hotkey skips the wait and Esc cancels the paste.
pub fn while_cleaning_up(key: Key) -> Option<Action> {
    match key {
        Key::HotkeyDown => Some(Action::Skip),
        Key::Escape => Some(Action::Cancel),
        Key::HotkeyUp | Key::OtherKeyDown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Action::*;
    use Key::*;

    fn run(steps: &[(u64, Key)]) -> Vec<Option<Action>> {
        let t0 = Instant::now();
        let mut g = Gesture::default();
        steps
            .iter()
            .map(|&(ms, k)| g.on(k, t0 + Duration::from_millis(ms)))
            .collect()
    }

    #[test]
    fn hold_finishes_on_release() {
        assert_eq!(
            run(&[(0, HotkeyDown), (900, HotkeyUp)]),
            [Some(Start), Some(Finish)]
        );
    }

    #[test]
    fn release_at_exactly_the_threshold_is_a_hold() {
        assert_eq!(
            run(&[(0, HotkeyDown), (250, HotkeyUp)]),
            [Some(Start), Some(Finish)]
        );
    }

    #[test]
    fn tap_goes_hands_free_and_the_next_press_finishes() {
        assert_eq!(
            run(&[
                (0, HotkeyDown),
                (249, HotkeyUp),
                (5_000, HotkeyDown),
                (5_100, HotkeyUp),
            ]),
            [Some(Start), Some(HandsFree), Some(Finish), None]
        );
    }

    #[test]
    fn hands_free_ignores_typing() {
        assert_eq!(
            run(&[
                (0, HotkeyDown),
                (100, HotkeyUp),
                (300, OtherKeyDown),
                (900, HotkeyDown),
            ]),
            [Some(Start), Some(HandsFree), None, Some(Finish)]
        );
    }

    #[test]
    fn another_key_while_held_is_a_shortcut() {
        assert_eq!(
            run(&[(0, HotkeyDown), (80, OtherKeyDown), (400, HotkeyUp)]),
            [Some(Start), Some(Cancel), None]
        );
    }

    #[test]
    fn escape_cancels_while_held_and_hands_free() {
        assert_eq!(
            run(&[(0, HotkeyDown), (600, Escape)]),
            [Some(Start), Some(Cancel)]
        );
        assert_eq!(
            run(&[(0, HotkeyDown), (100, HotkeyUp), (2_000, Escape)]),
            [Some(Start), Some(HandsFree), Some(Cancel)]
        );
    }

    #[test]
    fn idle_ignores_everything_but_the_hotkey() {
        assert_eq!(
            run(&[(0, Escape), (1, OtherKeyDown), (2, HotkeyUp)]),
            [None; 3]
        );
    }

    #[test]
    fn cleaning_up_turns_the_hotkey_into_skip_and_ignores_typing() {
        assert_eq!(while_cleaning_up(HotkeyDown), Some(Skip));
        assert_eq!(while_cleaning_up(Escape), Some(Cancel));
        assert_eq!(while_cleaning_up(HotkeyUp), None);
        assert_eq!(while_cleaning_up(OtherKeyDown), None);
    }

    #[test]
    fn repeat_while_held_does_not_restart() {
        assert_eq!(
            run(&[(0, HotkeyDown), (500, HotkeyDown), (600, HotkeyUp)]),
            [Some(Start), None, Some(Finish)]
        );
    }
}
