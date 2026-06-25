//! The Voice hotkey, stored as a string in `general.voice_hotkey`: `fn`, a lone modifier such as `right_option`, or a combo such as `option+e` or `right_option+m`.

/// `CGEventFlags` bits for the four combo modifiers.
pub const SHIFT: u64 = 0x2_0000;
pub const CONTROL: u64 = 0x4_0000;
pub const OPTION: u64 = 0x8_0000;
pub const COMMAND: u64 = 0x10_0000;
pub const COMBO_MODIFIERS: u64 = SHIFT | CONTROL | OPTION | COMMAND;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    /// Fn or a lone modifier, seen through `flagsChanged`.
    /// `mask` holds the device-dependent flag bits, so left and right sides stay distinct.
    Modifier { mask: u64 },
    /// A key pressed with exactly `mods` held, seen through `keyDown` and `keyUp`.
    /// `sides` holds the device-dependent bits of any modifier named with a side, such as `right_option`.
    Combo { keycode: u16, mods: u64, sides: u64 },
}

impl Hotkey {
    /// A file-name-safe key that shortcuts firing on the same press share, such as `option+m` and `right_option+m`, or `option` and `right_option`.
    /// Sides are left out, so `left_option+m` and `right_option+m` also share one, though no single press fires both.
    /// shortcut: a lone modifier and a combo on it, such as `right_option` and `right_option+m`, get different ids although one press can fire both; give them one if anyone runs that pair.
    pub fn id(&self) -> String {
        match self {
            Hotkey::Modifier { mask } => {
                let class = [0x2001, 0x6, 0x18, 0x60, 0x80_0000]
                    .into_iter()
                    .filter(|both| mask & both != 0)
                    .fold(0, |acc, both| acc | both);
                format!("mod-{class:x}")
            }
            Hotkey::Combo { keycode, mods, .. } => format!("combo-{keycode}-{mods:x}"),
        }
    }
}

/// Whether `flags` hold exactly a combo's modifiers, with each sided one down on its own side.
pub fn combo_mods_held(mods: u64, sides: u64, flags: u64) -> bool {
    flags & COMBO_MODIFIERS == mods && flags & sides == sides
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid voice hotkey '{0}': use fn, a modifier such as right_option, or a combo with Control, Option, or Command such as option+e or right_option+m")]
pub struct ParseError(String);

impl std::str::FromStr for Hotkey {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, ParseError> {
        let err = || ParseError(s.to_string());
        let lower = s.trim().to_ascii_lowercase();
        let parts: Vec<&str> = lower.split('+').map(str::trim).collect();
        match parts.as_slice() {
            [single] => lone_modifier_mask(single)
                .map(|mask| Hotkey::Modifier { mask })
                .ok_or_else(err),
            [mods @ .., key] => {
                let (mut bits, mut sides) = (0, 0);
                for m in mods {
                    let (bit, side) = combo_modifier(m).ok_or_else(err)?;
                    bits |= bit;
                    sides |= side;
                }
                // The tap swallows the combo everywhere, so Shift+E alone would eat every capital E.
                if bits == SHIFT {
                    return Err(err());
                }
                let keycode = keycode(key).ok_or_else(err)?;
                Ok(Hotkey::Combo {
                    keycode,
                    mods: bits,
                    sides,
                })
            }
            [] => Err(err()),
        }
    }
}

/// Device-dependent bits from IOKit's `IOLLEvent.h`, plus `kCGEventFlagMaskSecondaryFn` for Fn.
fn lone_modifier_mask(name: &str) -> Option<u64> {
    Some(match name {
        "fn" | "globe" => 0x80_0000,
        "left_control" => 0x1,
        "right_control" => 0x2000,
        "control" | "ctrl" => 0x1 | 0x2000,
        "left_shift" => 0x2,
        "right_shift" => 0x4,
        "shift" => 0x2 | 0x4,
        "left_command" => 0x8,
        "right_command" => 0x10,
        "command" | "cmd" => 0x8 | 0x10,
        "left_option" => 0x20,
        "right_option" => 0x40,
        "option" | "alt" | "opt" => 0x20 | 0x40,
        _ => return None,
    })
}

/// The generic flag bit and, for a name with a side such as `right_option`, that side's device-dependent bit.
fn combo_modifier(name: &str) -> Option<(u64, u64)> {
    let (side, generic) = match name.split_once('_') {
        Some((side @ ("left" | "right"), generic)) => (Some(side), generic),
        _ => (None, name),
    };
    let (bit, canonical) = match generic {
        "shift" => (SHIFT, "shift"),
        "control" | "ctrl" => (CONTROL, "control"),
        "option" | "alt" | "opt" => (OPTION, "option"),
        "command" | "cmd" => (COMMAND, "command"),
        _ => return None,
    };
    let side_bit = match side {
        Some(side) => lone_modifier_mask(&format!("{side}_{canonical}"))?,
        None => 0,
    };
    Some((bit, side_bit))
}

/// ANSI virtual keycodes from `HIToolbox/Events.h`.
pub(crate) fn keycode(name: &str) -> Option<u16> {
    const LETTERS: [u16; 26] = [
        0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16,
        6,
    ];
    const DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
    match name.as_bytes() {
        [c @ b'a'..=b'z'] => Some(LETTERS[(c - b'a') as usize]),
        [c @ b'0'..=b'9'] => Some(DIGITS[(c - b'0') as usize]),
        _ => match name {
            "space" => Some(49),
            "return" | "enter" => Some(36),
            "tab" => Some(48),
            _ => None,
        },
    }
}

pub const ESCAPE_KEYCODE: u16 = 53;
pub const V_KEYCODE: u16 = 9;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_shape() {
        assert_eq!("fn".parse(), Ok(Hotkey::Modifier { mask: 0x80_0000 }));
        assert_eq!("Option".parse(), Ok(Hotkey::Modifier { mask: 0x60 }));
        assert_eq!("right_option".parse(), Ok(Hotkey::Modifier { mask: 0x40 }));
        assert_eq!(
            "option+e".parse(),
            Ok(Hotkey::Combo {
                keycode: 14,
                mods: OPTION,
                sides: 0
            })
        );
        assert_eq!(
            "ctrl + space".parse(),
            Ok(Hotkey::Combo {
                keycode: 49,
                mods: CONTROL,
                sides: 0
            })
        );
        assert_eq!(
            "cmd+shift+1".parse(),
            Ok(Hotkey::Combo {
                keycode: 18,
                mods: COMMAND | SHIFT,
                sides: 0
            })
        );
        assert_eq!(
            "right_option+m".parse(),
            Ok(Hotkey::Combo {
                keycode: 46,
                mods: OPTION,
                sides: 0x40
            })
        );
        assert_eq!(
            "Left_Command + Shift + k".parse(),
            Ok(Hotkey::Combo {
                keycode: 40,
                mods: COMMAND | SHIFT,
                sides: 0x8
            })
        );
    }

    #[test]
    fn a_side_takes_every_alias() {
        for s in ["right_alt+m", "right_opt+m", "Right_Option + M"] {
            assert_eq!(s.parse(), "right_option+m".parse::<Hotkey>(), "{s}");
        }
        assert_eq!("right_ctrl+e".parse(), "right_control+e".parse::<Hotkey>());
        assert_eq!("left_cmd+k".parse(), "left_command+k".parse::<Hotkey>());
    }

    #[test]
    fn a_sided_combo_fires_only_on_its_side() {
        let Ok(Hotkey::Combo { mods, sides, .. }) = "right_option+m".parse() else {
            panic!("right_option+m should parse to a combo");
        };
        assert!(combo_mods_held(mods, sides, OPTION | 0x40));
        assert!(
            !combo_mods_held(mods, sides, OPTION | 0x20),
            "Left Option+M"
        );
        assert!(!combo_mods_held(mods, sides, OPTION | 0x40 | SHIFT | 0x2));
        let Ok(Hotkey::Combo { mods, sides, .. }) = "option+m".parse() else {
            panic!("option+m should parse to a combo");
        };
        assert!(combo_mods_held(mods, sides, OPTION | 0x20));
        assert!(combo_mods_held(mods, sides, OPTION | 0x40));
    }

    #[test]
    fn shortcuts_one_press_could_fire_share_an_id() {
        let id = |s: &str| s.parse::<Hotkey>().unwrap().id();
        assert_eq!(id("option+a"), id("Alt + A"));
        assert_eq!(id("option+m"), id("right_option+m"));
        assert_eq!(id("right_option"), id("option"));
        assert_ne!(id("option+a"), id("option+m"));
        assert_ne!(id("right_option"), id("right_command"));
        assert_ne!(id("fn"), id("option"));
    }

    #[test]
    fn rejects_garbage() {
        for s in [
            "",
            "e",
            "option+",
            "hyper+e",
            "option+f13",
            "fn+e",
            "shift+e",
            "shift+space",
            "right_shift+e",
            "right_fn+e",
            "middle_option+e",
        ] {
            assert!(s.parse::<Hotkey>().is_err(), "{s} should not parse");
        }
    }
}
