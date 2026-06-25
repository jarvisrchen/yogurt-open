//! Records a new Voice hotkey from the event tap, which sees Fn where a browser page never does.
//!
//! A single modifier or Fn, pressed and released with nothing else, records as a lone modifier.
//! A key the hotkey parser knows, pressed with Control, Option, or Command held, records as a combo.
//! Esc cancels.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::hotkey::{keycode, COMMAND, CONTROL, ESCAPE_KEYCODE, OPTION, SHIFT};

/// Settings' recording and when its lease runs out. The tap swallows every key while it is live.
pub type SharedRecorder = Arc<Mutex<Option<(Instant, Recorder)>>>;

/// The device-dependent flag bit of each side of each modifier, and Fn's, named as the hotkey parser names them.
const SIDES: [(&str, u64); 9] = [
    ("fn", 0x80_0000),
    ("left_control", 0x1),
    ("right_control", 0x2000),
    ("left_shift", 0x2),
    ("right_shift", 0x4),
    ("left_command", 0x8),
    ("right_command", 0x10),
    ("left_option", 0x20),
    ("right_option", 0x40),
];

/// In the order a combo spells them.
const COMBO: [(&str, u64); 4] = [
    ("control", CONTROL),
    ("option", OPTION),
    ("shift", SHIFT),
    ("command", COMMAND),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The modifiers held now, in the order they went down.
    Pending(Vec<&'static str>),
    Done(String),
    Cancel,
}

#[derive(Debug, Default)]
pub struct Recorder {
    held: Vec<(&'static str, u64)>,
    /// Every side pressed since all were last up, so Option then Shift is not a lone Shift.
    round: u64,
    /// A key went down under a held modifier, so that modifier was not pressed alone.
    mixed: bool,
    end: Option<Step>,
}

impl Recorder {
    pub fn step(&self) -> Step {
        self.end
            .clone()
            .unwrap_or_else(|| Step::Pending(self.held.iter().map(|&(name, _)| name).collect()))
    }

    pub fn is_finished(&self) -> bool {
        self.end.is_some()
    }

    /// A `flagsChanged` event.
    pub fn flags(&mut self, flags: u64) {
        if self.end.is_some() {
            return;
        }
        self.held.retain(|&(_, bit)| flags & bit != 0);
        for side in SIDES {
            if flags & side.1 != 0 && !self.held.contains(&side) {
                self.held.push(side);
                self.round |= side.1;
            }
        }
        if self.held.is_empty() {
            let round = std::mem::take(&mut self.round);
            if !std::mem::take(&mut self.mixed) {
                if let Some(&(name, _)) = SIDES.iter().find(|&&(_, bit)| bit == round) {
                    self.end = Some(Step::Done(name.into()));
                }
            }
        }
    }

    /// A `keyDown` event.
    pub fn key_down(&mut self, code: u16, flags: u64) {
        if self.end.is_some() {
            return;
        }
        // Cmd+Tab means the user is leaving, and as the hotkey it would break app switching everywhere.
        if code == ESCAPE_KEYCODE || (code == TAB && flags & COMMAND != 0) {
            self.end = Some(Step::Cancel);
            return;
        }
        // Shift alone would make the hotkey eat a capital letter everywhere, so the parser refuses it.
        match key_name(code) {
            Some(key) if flags & (CONTROL | OPTION | COMMAND) != 0 => {
                let mut parts: Vec<String> = COMBO
                    .iter()
                    .filter(|&&(_, bit)| flags & bit != 0)
                    .map(|&(name, _)| name.into())
                    .collect();
                parts.push(key);
                self.end = Some(Step::Done(parts.join("+")));
            }
            _ => self.mixed |= !self.held.is_empty(),
        }
    }
}

const TAB: u16 = 48;

fn key_name(code: u16) -> Option<String> {
    ('a'..='z')
        .chain('0'..='9')
        .map(String::from)
        .chain(["space", "return", "tab"].map(String::from))
        .find(|name| keycode(name) == Some(code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Hotkey;

    const E: u16 = 14;
    const ONE: u16 = 18;
    const LEFT_OPTION: u64 = OPTION | 0x20;
    const RIGHT_OPTION: u64 = OPTION | 0x40;
    const LEFT_SHIFT: u64 = SHIFT | 0x2;
    /// `kCGEventFlagMaskSecondaryFn`, which macOS also sets on arrow keys.
    const FN: u64 = 0x80_0000;

    #[test]
    fn fn_alone_records_and_parses() {
        let mut r = Recorder::default();
        r.flags(FN);
        assert_eq!(r.step(), Step::Pending(vec!["fn"]));
        r.flags(0);
        assert_eq!(r.step(), Step::Done("fn".into()));
        assert_eq!("fn".parse(), Ok(Hotkey::Modifier { mask: FN }));
    }

    #[test]
    fn every_lone_side_records_its_own_name() {
        for (name, bit) in SIDES {
            let mut r = Recorder::default();
            r.flags(bit | OPTION);
            r.flags(0);
            assert_eq!(r.step(), Step::Done(name.into()));
            assert_eq!(name.parse(), Ok(Hotkey::Modifier { mask: bit }), "{name}");
        }
    }

    #[test]
    fn two_modifiers_are_not_a_lone_one() {
        let mut r = Recorder::default();
        r.flags(LEFT_OPTION);
        r.flags(LEFT_OPTION | LEFT_SHIFT);
        assert_eq!(r.step(), Step::Pending(vec!["left_option", "left_shift"]));
        r.flags(LEFT_SHIFT);
        assert_eq!(r.step(), Step::Pending(vec!["left_shift"]));
        r.flags(0);
        assert_eq!(r.step(), Step::Pending(vec![]));
        r.flags(RIGHT_OPTION);
        r.flags(0);
        assert_eq!(r.step(), Step::Done("right_option".into()));
    }

    #[test]
    fn a_key_under_a_lone_modifier_spoils_it() {
        let mut r = Recorder::default();
        r.flags(FN);
        r.key_down(E, FN);
        r.flags(0);
        assert_eq!(r.step(), Step::Pending(vec![]));
        r.flags(LEFT_SHIFT);
        r.key_down(E, LEFT_SHIFT);
        r.flags(0);
        assert_eq!(r.step(), Step::Pending(vec![]));
    }

    #[test]
    fn combos_record_on_key_down_and_parse() {
        let mut r = Recorder::default();
        r.flags(LEFT_OPTION);
        r.key_down(E, LEFT_OPTION);
        assert_eq!(r.step(), Step::Done("option+e".into()));
        r.flags(0);
        assert_eq!(r.step(), Step::Done("option+e".into()));

        let mut r = Recorder::default();
        r.key_down(ONE, COMMAND | SHIFT | CONTROL | 0x8 | 0x2 | 0x1);
        let Step::Done(hotkey) = r.step() else {
            panic!("expected a combo, got {:?}", r.step());
        };
        assert_eq!(hotkey, "control+shift+command+1");
        assert!(hotkey.parse::<Hotkey>().is_ok());
    }

    #[test]
    fn keys_the_parser_refuses_do_not_record() {
        let mut r = Recorder::default();
        r.key_down(E, 0);
        r.key_down(E, LEFT_SHIFT);
        r.key_down(122, LEFT_OPTION); // F1
        assert_eq!(r.step(), Step::Pending(vec![]));
    }

    #[test]
    fn escape_cancels_even_under_a_modifier() {
        let mut r = Recorder::default();
        r.flags(LEFT_OPTION);
        r.key_down(ESCAPE_KEYCODE, LEFT_OPTION);
        assert_eq!(r.step(), Step::Cancel);
        assert!(r.is_finished());
    }

    #[test]
    fn command_tab_cancels() {
        let mut r = Recorder::default();
        r.key_down(TAB, COMMAND | 0x8);
        assert_eq!(r.step(), Step::Cancel);
        let mut r = Recorder::default();
        r.key_down(TAB, CONTROL | 0x1);
        assert_eq!(r.step(), Step::Done("control+tab".into()));
    }

    #[test]
    fn caps_lock_is_ignored() {
        let mut r = Recorder::default();
        r.flags(0x1_0000);
        r.flags(0);
        assert_eq!(r.step(), Step::Pending(vec![]));
    }
}
