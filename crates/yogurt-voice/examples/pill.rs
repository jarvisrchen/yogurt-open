//! Holds the pill in one state, with fake speech on the bars, to check it on screen:
//! `cargo run -p yogurt-voice --example pill -- hands-free`.
//! States: recording, hands-free, transcribing, cleaning-up, pasted, error, cycle.
//! The pill appears on the screen under the mouse. Prints the panel's frame in `screencapture -R` coordinates.
//! `PILL_APPEARANCE=light|dark` sets the app's appearance and puts a backdrop in the mock's light or dark wall color behind the pill.

use std::time::{Duration, Instant};

use yogurt_voice::PillState;

const STATES: [(&str, PillState); 6] = [
    ("recording", PillState::Recording),
    ("hands-free", PillState::HandsFree),
    ("transcribing", PillState::Transcribing),
    ("cleaning-up", PillState::CleaningUp),
    ("pasted", PillState::Pasted),
    ("error", PillState::Error("Mic unavailable")),
];

fn main() {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "recording".into());
    let cycle = arg == "cycle";
    let Some(&(_, first)) = STATES
        .iter()
        .find(|(n, _)| *n == arg)
        .or(cycle.then_some(&STATES[0]))
    else {
        eprintln!("unknown state {arg}; one of recording, hands-free, transcribing, cleaning-up, pasted, error, cycle");
        std::process::exit(2);
    };
    let appearance = std::env::var("PILL_APPEARANCE").ok();
    if let Some(a) = appearance.clone() {
        dispatch2::DispatchQueue::main().exec_async(move || set_appearance(&a));
    }
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut state = first;
        let mut shown = Instant::now() - Duration::from_secs(1);
        let mut target = 0.0f32;
        let mut next_burst = Instant::now();
        let mut seed = 0x1234_5678u32;
        let mut rand = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        loop {
            if cycle {
                state = STATES[(started.elapsed().as_secs() / 3) as usize % STATES.len()].1;
            }
            // Pasted and Error hide on their own, so keep showing them.
            if shown.elapsed() >= Duration::from_millis(400) {
                yogurt_voice::show(0, state);
                shown = Instant::now();
            }
            if Instant::now() >= next_burst {
                target = if rand() < 0.2 {
                    0.05
                } else {
                    0.45 + rand() * 0.55
                };
                next_burst = Instant::now() + Duration::from_millis(90 + (rand() * 180.0) as u64);
            }
            // Inverse of the pill's dB mapping, so the bars see `target`.
            yogurt_voice::level(10f32.powf((target * 40.0 - 50.0) / 20.0));
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(600));
        dispatch2::DispatchQueue::main().exec_async(move || on_shown(appearance.as_deref()));
    });
    yogurt_voice::run_app();
}

fn set_appearance(name: &str) {
    use objc2_app_kit::{
        NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    };
    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    let name = unsafe {
        if name == "dark" {
            NSAppearanceNameDarkAqua
        } else {
            NSAppearanceNameAqua
        }
    };
    NSApplication::sharedApplication(mtm)
        .setAppearance(NSAppearance::appearanceNamed(name).as_deref());
}

fn on_shown(appearance: Option<&str>) {
    use objc2::MainThreadOnly;
    use objc2_app_kit::{
        NSApplication, NSBackingStoreType, NSColor, NSScreen, NSWindow, NSWindowStyleMask,
    };
    let mtm = objc2::MainThreadMarker::new().expect("main thread");
    let Some(panel) = NSApplication::sharedApplication(mtm)
        .windows()
        .firstObject()
    else {
        return;
    };
    let f = panel.frame();
    if let Some(a) = appearance {
        // The mock's wall-light and wall-dark, at their midpoints.
        let (r, g, b) = if a == "dark" {
            (0x23, 0x1F, 0x2E)
        } else {
            (0xE7, 0xD5, 0xC0)
        };
        let backdrop = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                f,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        unsafe { backdrop.setReleasedWhenClosed(false) };
        backdrop.setBackgroundColor(Some(&NSColor::colorWithSRGBRed_green_blue_alpha(
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
            1.0,
        )));
        backdrop.setIgnoresMouseEvents(true);
        backdrop.orderFrontRegardless();
        std::mem::forget(backdrop);
    }
    // screencapture's origin is the top left of the main screen, AppKit's the bottom left.
    let main_h = NSScreen::screens(mtm).objectAtIndex(0).frame().size.height;
    println!(
        "frame {},{},{},{}",
        f.origin.x,
        main_h - f.origin.y - f.size.height,
        f.size.width,
        f.size.height
    );
}
