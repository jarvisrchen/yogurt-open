use std::cell::{Cell, OnceCell, RefCell};
use std::ffi::c_void;
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::MainThreadMarker;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSPasteboard, NSPasteboardItem,
    NSPasteboardTypeString, NSPasteboardWriting, NSWorkspace,
};
use objc2_core_foundation::{kCFRunLoopCommonModes, CFMachPort, CFRetained, CFRunLoop};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventMask, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType,
};
use objc2_foundation::{NSArray, NSData, NSString};

use crate::gesture::{while_cleaning_up, Action, Gesture, Key};
use crate::hotkey::{combo_mods_held, Hotkey, COMMAND, ESCAPE_KEYCODE, V_KEYCODE};
use crate::record::SharedRecorder;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> bool;
}

/// Whether this process, which macOS charges to the terminal that launched it, has Accessibility.
pub fn is_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Runs `NSApplication` on the main thread with no Dock icon, and never returns.
/// Panics off the main thread.
pub fn run_app() -> ! {
    let mtm = MainThreadMarker::new().expect("run_app must be called on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    app.run();
    std::process::exit(0)
}

/// `None` turns the tap into a pass-through, so Voice off swallows nothing.
pub type SharedHotkey = Arc<Mutex<Option<Hotkey>>>;

#[derive(Debug, thiserror::Error)]
pub enum TapError {
    #[error(
        "could not create the event tap; grant Accessibility to the terminal that runs yogurt"
    )]
    Create,
    #[error("event tap thread failed to start: {0}")]
    Thread(String),
}

/// Starts the hotkey tap on its own thread with its own run loop.
/// `cleaning_up` says a finished clip waits on its cleanup, which turns the hotkey into Skip and Esc into Cancel.
/// While `recording` holds a live recorder, every key goes to it and reaches no app.
/// It and `emit` run inside the tap callback, which macOS disables if it is slow, so they must only read a flag or hand the action off, and the recorder lock is held only for one event.
pub fn spawn_tap(
    hotkey: SharedHotkey,
    recording: SharedRecorder,
    cleaning_up: impl Fn() -> bool + Send + 'static,
    emit: impl Fn(Action, Instant) + Send + 'static,
) -> Result<(), TapError> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("yogurt-voice-tap".into())
        .spawn(move || {
            let tap = Box::new(Tap {
                hotkey,
                recording,
                cleaning_up: Box::new(cleaning_up),
                emit: Box::new(emit),
                current: Cell::new(None),
                gesture: RefCell::default(),
                modifier_down: Cell::new(false),
                combo_down: Cell::new(false),
                port: OnceCell::new(),
            });
            let mask: CGEventMask = [
                CGEventType::KeyDown,
                CGEventType::KeyUp,
                CGEventType::FlagsChanged,
                CGEventType::LeftMouseDown,
                CGEventType::RightMouseDown,
                CGEventType::OtherMouseDown,
            ]
            .iter()
            .fold(0, |m, t| m | 1 << t.0);
            let port = unsafe {
                CGEvent::tap_create(
                    CGEventTapLocation::SessionEventTap,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    mask,
                    Some(callback),
                    &*tap as *const Tap as *mut c_void,
                )
            };
            let Some(port) = port else {
                let _ = ready_tx.send(Err(TapError::Create));
                return;
            };
            let Some(source) = CFMachPort::new_run_loop_source(None, Some(&port), 0) else {
                let _ = ready_tx.send(Err(TapError::Create));
                return;
            };
            let Some(run_loop) = CFRunLoop::current() else {
                let _ = ready_tx.send(Err(TapError::Create));
                return;
            };
            run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
            CGEvent::tap_enable(&port, true);
            // Leaked only once the tap exists: it lives for the rest of the process, and the callback holds a raw pointer to it.
            let tap = Box::leak(tap);
            let _ = tap.port.set(port);
            let _ = ready_tx.send(Ok(()));
            CFRunLoop::run();
        })
        .map_err(|e| TapError::Thread(e.to_string()))?;
    ready_rx
        .recv()
        .map_err(|e| TapError::Thread(e.to_string()))?
}

/// Touched only on the tap thread, except `hotkey`.
struct Tap {
    hotkey: SharedHotkey,
    recording: SharedRecorder,
    cleaning_up: Box<dyn Fn() -> bool + Send>,
    emit: Box<dyn Fn(Action, Instant) + Send>,
    /// The hotkey the gesture state belongs to.
    current: Cell<Option<Hotkey>>,
    gesture: RefCell<Gesture>,
    modifier_down: Cell<bool>,
    combo_down: Cell<bool>,
    port: OnceCell<CFRetained<CFMachPort>>,
}

unsafe extern "C-unwind" fn callback(
    _proxy: CGEventTapProxy,
    ty: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    let tap = unsafe { &*(user_info as *const Tap) };
    if ty == CGEventType::TapDisabledByTimeout || ty == CGEventType::TapDisabledByUserInput {
        // Keys typed while the tap was off went uncounted.
        INPUT.fetch_add(1, Ordering::Relaxed);
        if let Some(port) = tap.port.get() {
            CGEvent::tap_enable(port, true);
        }
        return event.as_ptr();
    }
    if tap.handle(ty, unsafe { event.as_ref() }) {
        null_mut()
    } else {
        event.as_ptr()
    }
}

impl Tap {
    /// Returns `true` to swallow the event.
    fn handle(&self, ty: CGEventType, event: &CGEvent) -> bool {
        let hotkey = *self.hotkey.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        // Voice turned off or the hotkey changed: a clip under the old hotkey could never be finished.
        if self.current.replace(hotkey) != hotkey {
            if self.gesture.borrow().is_active() {
                self.gesture.borrow_mut().reset();
                (self.emit)(Action::Cancel, now);
            }
            self.modifier_down.set(false);
            self.combo_down.set(false);
            // With Voice off the tap counts nothing, so typing then went uncounted.
            INPUT.fetch_add(1, Ordering::Relaxed);
        }
        // Our own paste's Cmd+V, which a recording must not take for the new hotkey.
        if CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData) == OWN_EVENT
        {
            return false;
        }
        if self.record(ty, event, now) {
            return true;
        }
        let Some(hotkey) = hotkey else {
            return false;
        };
        let flags = CGEvent::flags(Some(event)).0;
        let keycode =
            CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode) as u16;
        let repeat =
            CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventAutorepeat) != 0;

        match hotkey {
            Hotkey::Modifier { mask } if ty == CGEventType::FlagsChanged => {
                let down = flags & mask != 0;
                if down != self.modifier_down.replace(down) {
                    self.feed(if down { Key::HotkeyDown } else { Key::HotkeyUp }, now);
                }
                // A modifier types nothing, and swallowing it would break Fn+arrow and the like.
                return false;
            }
            Hotkey::Combo {
                keycode: k,
                mods,
                sides,
            } if ty == CGEventType::KeyDown
                && keycode == k
                && combo_mods_held(mods, sides, flags) =>
            {
                self.combo_down.set(true);
                // A held key repeats, which would restart a clip the press just finished.
                if !repeat {
                    self.feed(Key::HotkeyDown, now);
                }
                return true;
            }
            Hotkey::Combo { keycode: k, .. }
                if ty == CGEventType::KeyUp && keycode == k && self.combo_down.get() =>
            {
                self.combo_down.set(false);
                self.feed(Key::HotkeyUp, now);
                return true;
            }
            _ => {}
        }

        if ty == CGEventType::KeyDown && keycode == ESCAPE_KEYCODE {
            let swallow = self.feed(Key::Escape, now) == Some(Action::Cancel);
            if !swallow {
                INPUT.fetch_add(1, Ordering::Relaxed);
            }
            return swallow;
        }
        if ty != CGEventType::KeyUp && ty != CGEventType::FlagsChanged {
            INPUT.fetch_add(1, Ordering::Relaxed);
            // A click counts too, so an Option-click with Option as the hotkey stays a click.
            self.feed(Key::OtherKeyDown, now);
        }
        false
    }

    /// Returns `true` when Settings is recording a new hotkey, so the key reaches no app.
    fn record(&self, ty: CGEventType, event: &CGEvent, now: Instant) -> bool {
        if ty != CGEventType::KeyDown && ty != CGEventType::KeyUp && ty != CGEventType::FlagsChanged
        {
            return false;
        }
        let mut slot = self.recording.lock().unwrap_or_else(|e| e.into_inner());
        let Some((_, recorder)) = slot
            .as_mut()
            .filter(|(until, r)| now < *until && !r.is_finished())
        else {
            return false;
        };
        let flags = CGEvent::flags(Some(event)).0;
        if ty == CGEventType::FlagsChanged {
            recorder.flags(flags);
        } else if ty == CGEventType::KeyDown {
            let keycode =
                CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode);
            recorder.key_down(keycode as u16, flags);
        }
        true
    }

    fn feed(&self, key: Key, now: Instant) -> Option<Action> {
        let action = if !self.gesture.borrow().is_active() && (self.cleaning_up)() {
            while_cleaning_up(key)
        } else {
            self.gesture.borrow_mut().on(key, now)
        };
        if let Some(a) = action {
            (self.emit)(a, now);
        }
        action
    }
}

/// The app that had focus when the paste went out.
#[derive(Debug, Clone)]
pub struct Pasted {
    pub bundle_id: Option<String>,
    pub app_name: Option<String>,
    /// When Cmd+V was posted, before the clipboard restore wait.
    pub posted_at: Instant,
}

/// Key presses and clicks that reached an app, so a paste can tell the caret has not moved since the last one.
static INPUT: AtomicU64 = AtomicU64::new(0);

/// The previous paste, while nothing typed or clicked since could have moved the caret off its end.
struct Last {
    bundle_id: String,
    input: u64,
    ends: char,
}

/// Marks the Cmd+V that `paste` posts, so the tap does not read it as a shortcut that cancels a clip being held.
const OWN_EVENT: i64 = 0x796f_6775;

/// How long the app being pasted into gets to read the clipboard before the previous contents come back.
const RESTORE_AFTER: Duration = Duration::from_millis(500);

/// The bundle ID of the app with focus. It is read before cleanup, so it can differ from where [`paste`] lands if the user switches apps meanwhile.
pub fn frontmost() -> Option<String> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|a| a.bundleIdentifier())
        .map(|s| s.to_string())
}

/// Puts `text` on the clipboard, posts Cmd+V to the focused app, and restores the previous clipboard.
/// A space goes first when `text` would otherwise run into the previous paste, still right before the caret.
/// Blocks for [`RESTORE_AFTER`], so call it off the async runtime.
pub fn paste(text: &str) -> Pasted {
    // One at a time: a second paste inside the first one's wait would save the first transcript as the clipboard to restore.
    static PASTE: Mutex<Option<Last>> = Mutex::new(None);
    let mut last = PASTE.lock().unwrap_or_else(|e| e.into_inner());
    let front = NSWorkspace::sharedWorkspace().frontmostApplication();
    let bundle_id = front
        .as_ref()
        .and_then(|a| a.bundleIdentifier())
        .map(|s| s.to_string());
    let input = INPUT.load(Ordering::Relaxed);
    // Secure input, such as a terminal's Secure Keyboard Entry, hides key presses from the tap, so the count cannot vouch for the caret.
    let before = if unsafe { IsSecureEventInputEnabled() } {
        None
    } else {
        still_before(last.as_ref(), bundle_id.as_deref(), input)
    };
    let text = joined(before, text);
    *last = bundle_id
        .clone()
        .zip(text.chars().last())
        .map(|(bundle_id, ends)| Last {
            bundle_id,
            input,
            ends,
        });

    let pb = NSPasteboard::generalPasteboard();
    let saved = snapshot(&pb);

    pb.clearContents();
    pb.setString_forType(&NSString::from_str(&text), unsafe {
        NSPasteboardTypeString
    });
    let ours = pb.changeCount();

    for down in [true, false] {
        let ev = CGEvent::new_keyboard_event(None, V_KEYCODE, down);
        // Explicit flags so a hotkey modifier still held, such as Option in hands-free Option+E, does not turn Cmd+V into Cmd+Option+V.
        CGEvent::set_flags(ev.as_deref(), CGEventFlags(COMMAND));
        CGEvent::set_integer_value_field(
            ev.as_deref(),
            CGEventField::EventSourceUserData,
            OWN_EVENT,
        );
        CGEvent::post(CGEventTapLocation::HIDEventTap, ev.as_deref());
    }
    let pasted = Pasted {
        bundle_id,
        app_name: front
            .as_ref()
            .and_then(|a| a.localizedName())
            .map(|s| s.to_string()),
        posted_at: Instant::now(),
    };

    std::thread::sleep(RESTORE_AFTER);
    // Someone else wrote the clipboard in the meantime, and theirs wins.
    if pb.changeCount() == ours {
        restore(&pb, saved);
    }
    pasted
}

/// The previous paste's last character, when it still sits right before the caret in `bundle_id`.
fn still_before(last: Option<&Last>, bundle_id: Option<&str>, input: u64) -> Option<char> {
    last.filter(|l| Some(l.bundle_id.as_str()) == bundle_id && l.input == input)
        .map(|l| l.ends)
}

/// The Android app's `Splice.insert` rule, give or take Unicode's edges: a space when the text starts with a letter or digit and would touch the character before it.
fn joined(before: Option<char>, text: &str) -> String {
    match before {
        Some(c) if !c.is_whitespace() && text.starts_with(char::is_alphanumeric) => {
            format!(" {text}")
        }
        _ => text.to_string(),
    }
}

/// Every type of every item, so images and rich text survive the round trip.
fn snapshot(pb: &NSPasteboard) -> Vec<Vec<(Retained<NSString>, Retained<NSData>)>> {
    let Some(items) = pb.pasteboardItems() else {
        return Vec::new();
    };
    items
        .iter()
        .map(|item| {
            item.types()
                .iter()
                .filter_map(|ty| item.dataForType(&ty).map(|data| (ty, data)))
                .collect()
        })
        .collect()
}

fn restore(pb: &NSPasteboard, saved: Vec<Vec<(Retained<NSString>, Retained<NSData>)>>) {
    pb.clearContents();
    if saved.is_empty() {
        return;
    }
    let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = saved
        .into_iter()
        .map(|types| {
            let item = NSPasteboardItem::new();
            for (ty, data) in types {
                item.setData_forType(&data, &ty);
            }
            ProtocolObject::from_retained(item)
        })
        .collect();
    if !pb.writeObjects(&NSArray::from_retained_slice(&items)) {
        tracing::warn!("could not restore the clipboard after a voice paste");
    }
}

#[cfg(test)]
mod tests {
    use super::{joined, still_before, Last};

    #[test]
    fn only_an_untouched_caret_in_the_same_app_follows_the_last_paste() {
        let last = Last {
            bundle_id: "com.apple.TextEdit".into(),
            input: 7,
            ends: '.',
        };
        for (bundle_id, input, want) in [
            (Some("com.apple.TextEdit"), 7, Some('.')),
            (Some("com.apple.TextEdit"), 8, None),
            (Some("com.apple.Notes"), 7, None),
            (None, 7, None),
        ] {
            assert_eq!(
                still_before(Some(&last), bundle_id, input),
                want,
                "{bundle_id:?} {input}"
            );
        }
        assert_eq!(still_before(None, Some("com.apple.TextEdit"), 7), None);
    }

    #[test]
    fn a_space_separates_text_from_the_previous_paste() {
        for (before, text, want) in [
            (Some('.'), "Next one.", " Next one."),
            (Some('d'), "and more", " and more"),
            (Some('.'), "10 items", " 10 items"),
            (Some(' '), "Next one.", "Next one."),
            (Some('\n'), "Next one.", "Next one."),
            (Some('.'), ", then", ", then"),
            (None, "First one.", "First one."),
        ] {
            assert_eq!(joined(before, text), want, "{before:?} {text:?}");
        }
    }
}
