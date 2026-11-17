//! The meeting bubble: a borderless, non-activating `NSPanel` that floats over every app while a meeting records and no Yogurt page has focus.
//! [`show`], [`hide`], [`saved`], [`level`], [`remember_front`] and [`on_event`] are safe from any thread. Everything else runs on the main thread, which `run_app` owns.

use std::cell::{Cell, OnceCell, RefCell};
use std::f64::consts::PI;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, AnyThread, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAppearance, NSAppearanceCustomization,
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSApplicationActivationOptions,
    NSBackingStoreType, NSColor, NSCursor, NSEvent, NSFloatingWindowLevel, NSFont,
    NSFontAttributeName, NSForegroundColorAttributeName, NSKernAttributeName, NSPanel, NSResponder,
    NSRunningApplication, NSScreen, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowCollectionBehavior, NSWindowSharingType, NSWindowStyleMask, NSWorkspace,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGMutablePath, CGPath};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSNumber, NSObject, NSObjectNSKeyValueCoding,
    NSString, NSURL,
};
use objc2_quartz_core::{
    CABasicAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction, CAShapeLayer, CATextLayer,
    CATransaction,
};

#[path = "bubble_card.rs"]
mod card;

use crate::bubble::{clamp_corner, clock, half_height, Event, AUDIBLE_PEAK, BARS, SILENT_AFTER};
use crate::panel::{bundled_font, color, hex, hosting_view, manrope, Rgba};
use crate::pill::{level_from_peak, smooth};

/// The pill's outer height: the mock's 36 pt row inside a 1 pt border.
const PILL_H: f64 = 38.0;
/// Left and right padding inside the border.
const PAD_L: f64 = 12.0;
const PAD_R: f64 = 14.0;
const GAP: f64 = 7.0;
const DOT: f64 = 8.0;
const HALO: f64 = 3.0;
const BAR_W: f64 = 3.0;
const BAR_GAP: f64 = 2.0;
const BARS_W: f64 = BARS as f64 * BAR_W + (BARS - 1) as f64 * BAR_GAP;
const FLAG: f64 = 14.0;
const CLOSE: f64 = 20.0;
/// The pause and mute buttons that hover unfolds left of the dot, from the mock's `.acts`: 6 pt of padding, two 30 by 28 buttons 1 pt apart, then a 1 pt divider 5 pt on.
const ACTS_W: f64 = 73.0;
const BTN_W: f64 = 30.0;
const BTN_H: f64 = 28.0;
const BTN_ICON: f64 = 16.0;
/// Each button's left edge within the strip, and the top they share in the pill.
const BTN_X: [f64; 2] = [6.0, 6.0 + BTN_W + 1.0];
const BTN_Y: f64 = (PILL_H - BTN_H) / 2.0;
const OPEN: f64 = 0.22;
/// How far the close button reaches past the pill's bottom-right corner.
const CLOSE_OUT: f64 = 9.0;
/// Room around the pill for its shadow, the close button, and the Saved toast above it.
const MARGIN: f64 = 40.0;
/// The default spot's distance from the screen's edges, above the Dock.
const INSET: f64 = 20.0;
const DRAG_SLOP: f64 = 5.0;

const WAIT: Duration = Duration::from_millis(350);
const APPEAR: f64 = 0.26;
const APPEAR_FADE: f64 = 0.2;
const LEAVE: f64 = 0.16;
const PRESS: f64 = 0.09;
const HOVER_FADE: f64 = 0.16;
const SAVED_HOLD: Duration = Duration::from_millis(600);
const START_SCALE: f64 = 0.6;
const PRESS_SCALE: f64 = 0.94;
const TICK: Duration = Duration::from_nanos(1_000_000_000 / 30);
/// Under reduced motion the bars step to the level this often instead of easing.
const STEP_TICKS: u32 = 12;

/// The mock's two bubble palettes, `--b-*` in `.sl` and `.sd`.
struct Palette {
    bg: Rgba,
    edge: Rgba,
    ink: Rgba,
    muted: Rgba,
    mic: Rgba,
    sys: Rgba,
    rec: Rgba,
    halo: Rgba,
    ok: Rgba,
    warn: Rgba,
    /// A button under the pointer.
    hover: Rgba,
    warn_bg: Rgba,
    shadow: Rgba,
    /// The CSS shadow's negative spread, which an inset shadow path stands in for.
    shadow_inset: f64,
}

const LIGHT: Palette = Palette {
    bg: hex(0xFFFFFF, 0.92),
    edge: hex(0xE3DBCC, 1.0),
    ink: hex(0x211D18, 1.0),
    muted: hex(0x7A7264, 1.0),
    mic: hex(0x5B4FC7, 1.0),
    sys: hex(0xA89F90, 1.0),
    rec: hex(0xE07A66, 1.0),
    halo: hex(0xE07A66, 0.22),
    ok: hex(0x3F7A53, 1.0),
    warn: hex(0xA67C1B, 1.0),
    hover: hex(0x211D18, 0.07),
    warn_bg: hex(0xFBF0D6, 1.0),
    shadow: hex(0x211D18, 0.32),
    shadow_inset: 8.0,
};

const DARK: Palette = Palette {
    bg: hex(0x1E1B18, 0.88),
    edge: hex(0xFFFFFF, 0.11),
    ink: hex(0xEDEAE2, 1.0),
    muted: hex(0xA69D90, 1.0),
    mic: hex(0x7D72DE, 1.0),
    sys: hex(0xCFC8BB, 1.0),
    rec: hex(0xE8886F, 1.0),
    halo: hex(0xE8886F, 0.2),
    ok: hex(0x7FBE8C, 1.0),
    warn: hex(0xD9B24E, 1.0),
    hover: hex(0xFFFFFF, 0.1),
    warn_bg: hex(0x3A3120, 1.0),
    shadow: hex(0x000000, 0.6),
    shadow_inset: 6.0,
};

static MIC: AtomicU32 = AtomicU32::new(0);
static SYS: AtomicU32 = AtomicU32::new(0);
/// Unix ms of the last system frame above [`AUDIBLE_PEAK`].
static SYS_HEARD_MS: AtomicU64 = AtomicU64::new(0);
/// The app that was in front the last time a Yogurt page had focus, which a click brings back.
static FRONT_PID: AtomicI32 = AtomicI32::new(0);
static EVENTS: OnceLock<Box<dyn Fn(Event) + Send + Sync>> = OnceLock::new();

thread_local! {
    static BUBBLE: OnceCell<Bubble> = const { OnceCell::new() };
}

/// Shows the bubble for a recording that started at `started_at_ms` (Unix ms), after the mock's wait, at `corner` (the pill's bottom-right in screen points) or the default spot.
/// Showing it again while it is up only updates the start.
pub fn show(started_at_ms: u64, corner: Option<(f64, f64)>) {
    DispatchQueue::main().exec_async(move || with_bubble(|b| b.show(started_at_ms, corner)));
}

pub fn hide() {
    DispatchQueue::main().exec_async(|| with_bubble(|b| b.hide()));
}

/// The recording's pause (Unix ms it started, or `None`) and mic mute, which the bubble draws and its buttons toggle.
pub fn status(paused_since_ms: Option<u64>, muted: bool) {
    DispatchQueue::main().exec_async(move || with_bubble(|b| b.set_status(paused_since_ms, muted)));
}

/// What the hover card shows: the title, the last transcript lines, and for Pro the recap.
pub fn card(card: crate::bubble::Card) {
    DispatchQueue::main().exec_async(move || with_bubble(|b| b.set_card(card)));
}

/// The recording stopped: settle the bars, say Saved, then leave.
pub fn saved() {
    DispatchQueue::main().exec_async(|| with_bubble(|b| b.saved()));
}

/// Feeds one frame's peak amplitude (0..1) for the mic or the system channel.
pub fn level(mic: bool, peak: f32) {
    let bits = level_from_peak(peak).to_bits();
    if mic {
        MIC.fetch_max(bits, Ordering::Relaxed);
    } else {
        SYS.fetch_max(bits, Ordering::Relaxed);
        if peak > AUDIBLE_PEAK {
            SYS_HEARD_MS.store(now_ms(), Ordering::Relaxed);
        }
    }
}

/// A paused recording is silent on purpose, so its frames hold off the honey warning.
pub fn paused_frame() {
    SYS_HEARD_MS.store(now_ms(), Ordering::Relaxed);
}

/// Notes the frontmost app, which a click on the pill brings back. Call it when a Yogurt page gains focus.
pub fn remember_front() {
    DispatchQueue::main().exec_async(|| {
        let Some(app) = NSWorkspace::sharedWorkspace().frontmostApplication() else {
            return;
        };
        let pid = app.processIdentifier();
        if pid != std::process::id() as i32 {
            FRONT_PID.store(pid, Ordering::Relaxed);
        }
    });
}

/// Opens `url` in the default browser through LaunchServices, in this process.
pub fn open_url(url: &str) {
    let url = url.to_string();
    DispatchQueue::main().exec_async(move || {
        if let Some(u) = NSURL::URLWithString(&NSString::from_str(&url)) {
            NSWorkspace::sharedWorkspace().openURL(&u);
        }
    });
}

/// Where the panel's clicks, closes and drags go. The first call wins.
pub fn on_event(f: impl Fn(Event) + Send + Sync + 'static) {
    let _ = EVENTS.set(Box::new(f));
}

fn emit(event: Event) {
    if let Some(f) = EVENTS.get() {
        f(event);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

fn with_bubble(f: impl FnOnce(&Bubble)) {
    let mtm = MainThreadMarker::new().expect("the bubble runs on the main thread");
    BUBBLE.with(|b| f(b.get_or_init(|| Bubble::new(mtm))));
}

fn after(delay: Duration, f: impl FnOnce(&Bubble) + Send + 'static) {
    let when = DispatchTime::NOW.time(delay.as_nanos() as i64);
    let _ = DispatchQueue::main().after(when, move || with_bubble(f));
}

fn reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

fn ease_pop() -> Retained<CAMediaTimingFunction> {
    CAMediaTimingFunction::functionWithControlPoints(0.0, 0.0, 0.2, 1.0)
}

/// The mock's `--slide`, which unfolds the buttons.
fn ease_slide() -> Retained<CAMediaTimingFunction> {
    CAMediaTimingFunction::functionWithControlPoints(0.2, 0.7, 0.2, 1.0)
}

/// Sets `key` on `layer` without its implicit animation and, unless `secs` is 0, animates it there from `from`.
fn animate(layer: &CALayer, key: &str, from: f64, to: f64, secs: f64) {
    let key = NSString::from_str(key);
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    unsafe { layer.setValue_forKeyPath(Some(&NSNumber::new_f64(to)), &key) };
    CATransaction::commit();
    layer.removeAnimationForKey(&key);
    if secs > 0.0 {
        let a = CABasicAnimation::animationWithKeyPath(Some(&key));
        unsafe {
            a.setFromValue(Some(&NSNumber::new_f64(from)));
            a.setToValue(Some(&NSNumber::new_f64(to)));
        }
        a.setDuration(secs);
        a.setTimingFunction(Some(&ease_pop()));
        layer.addAnimation_forKey(&a, Some(&key));
    }
}

/// A Core Foundation object as the Objective-C object it is toll-free bridged to.
fn cf_object(path: &CGPath) -> &AnyObject {
    unsafe { &*(path as *const CGPath).cast::<AnyObject>() }
}

fn attributed(text: &str, font: &NSFont, c: Rgba, kern: f64) -> Retained<NSAttributedString> {
    let ns_color = NSColor::colorWithSRGBRed_green_blue_alpha(c.0, c.1, c.2, c.3);
    let kern = NSNumber::new_f64(kern);
    let keys = unsafe {
        [
            NSFontAttributeName,
            NSForegroundColorAttributeName,
            NSKernAttributeName,
        ]
    };
    let values: [&AnyObject; 3] = [font, &ns_color, &kern];
    let attrs = NSDictionary::from_slices(&keys, &values);
    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&attrs),
        )
    }
}

/// A 24-unit icon path from the mock, scaled to `size` points with y flipped for a y-up layer.
fn icon(size: f64, strokes: &[&[(f64, f64)]]) -> objc2_core_foundation::CFRetained<CGMutablePath> {
    let path = CGMutablePath::new();
    let k = size / 24.0;
    for stroke in strokes {
        for (i, &(x, y)) in stroke.iter().enumerate() {
            let (x, y) = (x * k, size - y * k);
            unsafe {
                if i == 0 {
                    CGMutablePath::move_to_point(Some(&path), std::ptr::null(), x, y);
                } else {
                    CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), x, y);
                }
            }
        }
    }
    path
}

/// The mock's mic, `M5 11a7 7 0 0 0 14 0M12 18v3` over a 6 by 11 capsule, with its slash when `off`.
fn mic_path(size: f64, off: bool) -> objc2_core_foundation::CFRetained<CGMutablePath> {
    let path = CGMutablePath::new();
    let k = size / 24.0;
    let y = |v: f64| size - v * k;
    let none = std::ptr::null();
    unsafe {
        // From the left of the cup round its bottom to the right, which is counterclockwise in a y-up layer.
        CGMutablePath::add_arc(
            Some(&path),
            none,
            12.0 * k,
            y(11.0),
            7.0 * k,
            PI,
            2.0 * PI,
            false,
        );
        CGMutablePath::move_to_point(Some(&path), none, 12.0 * k, y(18.0));
        CGMutablePath::add_line_to_point(Some(&path), none, 12.0 * k, y(21.0));
        CGMutablePath::add_rounded_rect(
            Some(&path),
            none,
            CGRect::new(
                CGPoint::new(9.0 * k, y(14.0)),
                CGSize::new(6.0 * k, 11.0 * k),
            ),
            3.0 * k,
            3.0 * k,
        );
        if off {
            CGMutablePath::move_to_point(Some(&path), none, 4.0 * k, y(4.0));
            CGMutablePath::add_line_to_point(Some(&path), none, 20.0 * k, y(20.0));
        }
    }
    path
}

fn pause_path(size: f64, paused: bool) -> objc2_core_foundation::CFRetained<CGMutablePath> {
    if paused {
        icon(
            size,
            &[&[(8.0, 5.0), (8.0, 19.0), (19.0, 12.0), (8.0, 5.0)]],
        )
    } else {
        icon(
            size,
            &[&[(9.0, 5.0), (9.0, 19.0)], &[(15.0, 5.0), (15.0, 19.0)]],
        )
    }
}

/// `width` is in the icon's 24-unit viewBox, as the mock's SVG stroke is.
fn stroke_layer(parent: &CALayer, size: f64, width: f64) -> Retained<CAShapeLayer> {
    let l = CAShapeLayer::new();
    l.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(size, size)));
    l.setFillColor(None);
    l.setLineWidth(width * size / 24.0);
    l.setLineCap(unsafe { objc2_quartz_core::kCALineCapRound });
    l.setLineJoin(unsafe { objc2_quartz_core::kCALineJoinRound });
    parent.addSublayer(&l);
    l
}

define_class!(
    /// The panel's content view, which takes clicks, hover and drags without activating Yogurt.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "YogurtMeetingBubbleView"]
    struct BubbleView;

    impl BubbleView {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            with_bubble(|b| b.down());
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, _event: &NSEvent) {
            with_bubble(|b| b.dragged());
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            with_bubble(|b| b.up());
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, _event: &NSEvent) {
            with_bubble(|b| b.track());
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            with_bubble(|b| b.track());
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            with_bubble(|b| b.track());
        }
    }
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Hidden,
    /// Shown is asked for, after the wait that keeps a quick app switch from flashing the bubble.
    Waiting,
    Shown,
    Leaving,
}

#[derive(Debug, PartialEq)]
enum HideAction {
    Nothing,
    OrderOut,
    Leave,
}

/// Whether the buttons are unfolded, and the silence and mute flags as they were then.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Fold {
    open: bool,
    held: Option<(bool, bool)>,
}

impl Fold {
    fn unfolded(live: (bool, bool)) -> Self {
        Self {
            open: true,
            held: Some(live),
        }
    }

    /// Each flag's slot in the row and whether its glyph shows.
    /// While unfolded the slots hold, so a click never moves a button out from under the pointer, and a glyph shows only while its live value is on too.
    fn flags(self, live: (bool, bool)) -> [(bool, bool); 2] {
        let slots = self.held.unwrap_or(live);
        [(slots.0, slots.0 && live.0), (slots.1, slots.1 && live.1)]
    }
}

/// What a hide does in each phase.
fn hide_action(phase: Phase) -> HideAction {
    match phase {
        // A second hide must not cancel the first one's order-out.
        Phase::Hidden | Phase::Leaving => HideAction::Nothing,
        // A show during a leave cancelled its order-out, so a hide in the wait that follows orders the panel out itself.
        Phase::Waiting => HideAction::OrderOut,
        Phase::Shown => HideAction::Leave,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Target {
    Pill,
    Close,
    Pause,
    Mute,
    /// The padding and divider around the buttons, which hover but do nothing on a click, so a near miss never switches apps.
    Strip,
}

#[derive(Clone, Copy)]
struct Press {
    mouse: CGPoint,
    corner: (f64, f64),
    target: Target,
    moved: bool,
}

struct Bubble {
    mtm: MainThreadMarker,
    panel: Retained<NSPanel>,
    shadow_view: Retained<NSView>,
    shadow: Retained<CALayer>,
    body: Retained<NSVisualEffectView>,
    face_view: Retained<NSView>,
    face: Retained<CALayer>,
    tint: Retained<CALayer>,
    row: Retained<CALayer>,
    halo: Retained<CALayer>,
    dot: Retained<CALayer>,
    mic: Vec<Retained<CALayer>>,
    sys: Vec<Retained<CALayer>>,
    time: Retained<CATextLayer>,
    flag: Retained<CAShapeLayer>,
    /// The paused dot: the mock's two grey 3 by 9 bars.
    glyph: Retained<CALayer>,
    mic_flag: Retained<CAShapeLayer>,
    acts: Retained<CALayer>,
    card_clip: Retained<CALayer>,
    card_top: Retained<CALayer>,
    card_lines: Retained<CALayer>,
    /// Holds both card images, so a slide moves them as one and they never drift apart.
    card_body: Retained<CALayer>,
    /// The warning boxes' icons, laid over the card's image: the triangle and the mic with its slash.
    warn_icons: [Retained<CAShapeLayer>; 2],
    fonts: card::Fonts,
    card: RefCell<crate::bubble::Card>,
    /// The card's height above the row, and the part above its transcript lines.
    card_h: Cell<f64>,
    top_h: Cell<f64>,
    /// The recap age the card was painted with.
    painted_age: RefCell<Option<String>>,
    rise: Cell<f64>,
    pause_bg: Retained<CALayer>,
    mute_bg: Retained<CALayer>,
    pause_icon: Retained<CAShapeLayer>,
    mute_icon: Retained<CAShapeLayer>,
    divider: Retained<CALayer>,
    close: Retained<CALayer>,
    close_x: Retained<CAShapeLayer>,
    toast: Retained<CALayer>,
    toast_text: Retained<CATextLayer>,
    mono: Retained<NSFont>,
    sans: Retained<NSFont>,
    phase: Cell<Phase>,
    /// Bumped on every show, hide and press, so a delayed step from an older one does nothing.
    generation: Cell<u64>,
    started_at_ms: Cell<u64>,
    /// The pill's bottom-right corner on screen, which stays put while its width changes.
    corner: Cell<(f64, f64)>,
    width: Cell<f64>,
    dark: Cell<Option<bool>>,
    silent: Cell<bool>,
    hover: Cell<bool>,
    /// Whether the buttons are unfolded, which follows hover, and the flags they hold still.
    fold: Cell<Fold>,
    /// Nonzero while the unfold slides, so a layout leaves the blur view's animated frame alone.
    sliding: Cell<u64>,
    hot: Cell<Option<Target>>,
    paused_since_ms: Cell<Option<u64>>,
    muted: Cell<bool>,
    press: Cell<Option<Press>>,
    saving: Cell<bool>,
    /// Nonzero while a scale runs, so a layout leaves the blur view's animated frame alone.
    scaling: Cell<u64>,
    ticking: Cell<bool>,
    ticks: Cell<u32>,
    mic_level: Cell<f32>,
    sys_level: Cell<f32>,
    seed: Cell<u32>,
    clock_text: RefCell<String>,
}

impl Bubble {
    fn new(mtm: MainThreadMarker) -> Self {
        let frame = CGRect::new(CGPoint::ZERO, CGSize::new(200.0, PILL_H + 2.0 * MARGIN));
        let panel = NSPanel::initWithContentRect_styleMask_backing_defer(
            NSPanel::alloc(mtm),
            frame,
            NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            NSBackingStoreType::Buffered,
            false,
        );
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        panel.setLevel(NSFloatingWindowLevel);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary,
        );
        // Keeps the bubble out of a screen share, which macOS honors for most captures but does not promise.
        // A debug build can opt out with `YOGURT_BUBBLE_CAPTURABLE`, which screenshots of the bubble itself need.
        if !(cfg!(debug_assertions) && std::env::var_os("YOGURT_BUBBLE_CAPTURABLE").is_some()) {
            panel.setSharingType(NSWindowSharingType::None);
        }
        panel.setHidesOnDeactivate(false);
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(true);
        unsafe { panel.setReleasedWhenClosed(false) };

        let view: Retained<BubbleView> =
            unsafe { msg_send![BubbleView::alloc(mtm), initWithFrame: frame] };
        view.setWantsLayer(true);
        let area = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                frame,
                NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(&view),
                None,
            )
        };
        view.addTrackingArea(&area);
        panel.setContentView(Some(&view));

        let (shadow_view, shadow) = hosting_view(mtm, frame);
        shadow.setShadowOpacity(1.0);
        shadow.setShadowRadius(14.0);
        shadow.setShadowOffset(CGSize::new(0.0, -12.0));
        view.addSubview(&shadow_view);

        let body = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
        body.setMaterial(NSVisualEffectMaterial::Popover);
        body.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        body.setState(NSVisualEffectState::Active);
        body.setWantsLayer(true);
        let body_layer = body.layer().expect("layer-backed view");
        body_layer.setCornerRadius(PILL_H / 2.0);
        body_layer.setMasksToBounds(true);
        view.addSubview(&body);

        let (face_view, face) = hosting_view(mtm, frame);
        view.addSubview(&face_view);

        let tint = CALayer::new();
        tint.setCornerRadius(PILL_H / 2.0);
        tint.setBorderWidth(1.0);
        face.addSublayer(&tint);
        // The dot, bars, timer and flag, laid out in the pill's own coordinates.
        let row = CALayer::new();
        face.addSublayer(&row);

        let halo = CALayer::new();
        halo.setBounds(CGRect::new(
            CGPoint::ZERO,
            CGSize::new(DOT + 2.0 * HALO, DOT + 2.0 * HALO),
        ));
        halo.setCornerRadius(DOT / 2.0 + HALO);
        row.addSublayer(&halo);
        let dot = CALayer::new();
        dot.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(DOT, DOT)));
        dot.setCornerRadius(DOT / 2.0);
        dot.setPosition(CGPoint::new(DOT / 2.0 + HALO, DOT / 2.0 + HALO));
        halo.addSublayer(&dot);

        let half = |anchor_y: f64| {
            (0..BARS)
                .map(|_| {
                    let l = CALayer::new();
                    l.setAnchorPoint(CGPoint::new(0.5, anchor_y));
                    row.addSublayer(&l);
                    l
                })
                .collect::<Vec<_>>()
        };
        // The mic grows up from just above the center line and system audio down from just below it.
        let mic = half(0.0);
        let sys = half(1.0);

        let time = CATextLayer::new();
        row.addSublayer(&time);

        let flag = stroke_layer(&row, FLAG, 2.0);
        // The mock's warning triangle, `M12 4 3 20h18ZM12 10v5M12 17.6v.2`.
        flag.setPath(Some(&icon(
            FLAG,
            &[
                &[(12.0, 4.0), (3.0, 20.0), (21.0, 20.0), (12.0, 4.0)],
                &[(12.0, 10.0), (12.0, 15.0)],
                &[(12.0, 17.6), (12.0, 17.8)],
            ],
        )));

        let glyph = CALayer::new();
        glyph.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(DOT, 9.0)));
        for x in [0.0, 5.0] {
            let bar = CALayer::new();
            bar.setFrame(CGRect::new(CGPoint::new(x, 0.0), CGSize::new(3.0, 9.0)));
            bar.setCornerRadius(1.0);
            glyph.addSublayer(&bar);
        }
        row.addSublayer(&glyph);
        let mic_flag = stroke_layer(&row, FLAG, 2.0);
        mic_flag.setPath(Some(&mic_path(FLAG, true)));

        let card_clip = CALayer::new();
        card_clip.setMasksToBounds(true);
        card_clip.setOpacity(0.0);
        face.addSublayer(&card_clip);
        let card_top = CALayer::new();
        let card_lines = CALayer::new();
        let card_body = CALayer::new();
        card_clip.addSublayer(&card_body);
        card_body.addSublayer(&card_top);
        card_body.addSublayer(&card_lines);
        let warn_icons = [
            stroke_layer(&card_top, FLAG, 2.0),
            stroke_layer(&card_top, FLAG, 2.0),
        ];
        warn_icons[0].setPath(Some(&icon(
            FLAG,
            &[
                &[(12.0, 4.0), (3.0, 20.0), (21.0, 20.0), (12.0, 4.0)],
                &[(12.0, 10.0), (12.0, 15.0)],
                &[(12.0, 17.6), (12.0, 17.8)],
            ],
        )));
        warn_icons[1].setPath(Some(&mic_path(FLAG, true)));

        let acts = CALayer::new();
        acts.setOpacity(0.0);
        face.addSublayer(&acts);
        let button = |x: f64| {
            let bg = CALayer::new();
            bg.setFrame(CGRect::new(
                CGPoint::new(x, BTN_Y),
                CGSize::new(BTN_W, BTN_H),
            ));
            bg.setCornerRadius(9.0);
            acts.addSublayer(&bg);
            let icon = stroke_layer(&bg, BTN_ICON, 2.0);
            icon.setPosition(CGPoint::new(BTN_W / 2.0, BTN_H / 2.0));
            (bg, icon)
        };
        let (pause_bg, pause_icon) = button(BTN_X[0]);
        let (mute_bg, mute_icon) = button(BTN_X[1]);
        let divider = CALayer::new();
        divider.setFrame(CGRect::new(
            CGPoint::new(ACTS_W - 1.0, (PILL_H - 16.0) / 2.0),
            CGSize::new(1.0, 16.0),
        ));
        acts.addSublayer(&divider);

        let close = CALayer::new();
        close.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(CLOSE, CLOSE)));
        close.setCornerRadius(CLOSE / 2.0);
        close.setBorderWidth(1.0);
        close.setShadowOpacity(1.0);
        close.setShadowRadius(14.0);
        close.setShadowOffset(CGSize::new(0.0, -12.0));
        close.setOpacity(0.0);
        face.addSublayer(&close);
        let close_x = stroke_layer(&close, 10.0, 2.6);
        close_x.setPath(Some(&icon(
            10.0,
            &[&[(6.0, 6.0), (18.0, 18.0)], &[(18.0, 6.0), (6.0, 18.0)]],
        )));
        close_x.setPosition(CGPoint::new(CLOSE / 2.0, CLOSE / 2.0));

        let toast = CALayer::new();
        toast.setBorderWidth(1.0);
        toast.setShadowOpacity(1.0);
        toast.setShadowRadius(14.0);
        toast.setShadowOffset(CGSize::new(0.0, -12.0));
        toast.setOpacity(0.0);
        face.addSublayer(&toast);
        let toast_text = CATextLayer::new();
        toast.addSublayer(&toast_text);

        static MONO: &[u8] = include_bytes!("../assets/jetbrains-mono-latin-500-normal.woff");
        Self {
            mtm,
            panel,
            shadow_view,
            shadow,
            body,
            face_view,
            face,
            tint,
            row,
            halo,
            dot,
            mic,
            sys,
            time,
            flag,
            glyph,
            mic_flag,
            acts,
            card_clip,
            card_top,
            card_lines,
            card_body,
            warn_icons,
            fonts: card::Fonts::new(bundled_font(MONO, 10.5, "JetBrains Mono")),
            card: RefCell::new(Default::default()),
            card_h: Cell::new(0.0),
            top_h: Cell::new(0.0),
            painted_age: RefCell::new(None),
            rise: Cell::new(0.0),
            pause_bg,
            mute_bg,
            pause_icon,
            mute_icon,
            divider,
            close,
            close_x,
            toast,
            toast_text,
            mono: bundled_font(MONO, 12.0, "JetBrains Mono"),
            sans: manrope(12.0),
            phase: Cell::new(Phase::Hidden),
            generation: Cell::new(0),
            started_at_ms: Cell::new(0),
            corner: Cell::new((0.0, 0.0)),
            width: Cell::new(0.0),
            dark: Cell::new(None),
            silent: Cell::new(false),
            hover: Cell::new(false),
            fold: Cell::new(Fold::default()),
            sliding: Cell::new(0),
            hot: Cell::new(None),
            paused_since_ms: Cell::new(None),
            muted: Cell::new(false),
            press: Cell::new(None),
            saving: Cell::new(false),
            scaling: Cell::new(0),
            ticking: Cell::new(false),
            ticks: Cell::new(0),
            mic_level: Cell::new(0.0),
            sys_level: Cell::new(0.0),
            seed: Cell::new(0x9e37_79b9),
            clock_text: RefCell::new(String::new()),
        }
    }

    fn bump(&self) -> u64 {
        let g = self.generation.get() + 1;
        self.generation.set(g);
        g
    }

    fn palette(&self) -> &'static Palette {
        if self.dark.get().unwrap_or(false) {
            &DARK
        } else {
            &LIGHT
        }
    }

    fn show(&self, started_at_ms: u64, corner: Option<(f64, f64)>) {
        // A new recording starts its silence count over, and leaving yogurt again does not.
        if self.started_at_ms.replace(started_at_ms) != started_at_ms {
            SYS_HEARD_MS.store(now_ms(), Ordering::Relaxed);
        }
        if self.phase.get() == Phase::Shown && self.saving.replace(false) {
            // A new recording started during the Saved hold: cancel its hide and carry on.
            self.bump();
            self.toast.setOpacity(0.0);
            self.pulse();
            return;
        }
        if matches!(self.phase.get(), Phase::Waiting | Phase::Shown) {
            return;
        }
        let generation = self.bump();
        self.phase.set(Phase::Waiting);
        self.saving.set(false);
        self.corner.set(self.place_corner(corner));
        after(WAIT, move |b| {
            if b.generation.get() == generation && b.phase.get() == Phase::Waiting {
                b.appear();
            }
        });
    }

    fn appear(&self) {
        self.phase.set(Phase::Shown);
        self.hover.set(false);
        // A bubble that left while unfolded comes back folded, with its flags read fresh.
        self.fold.set(Fold::default());
        self.hot.set(None);
        self.acts.setOpacity(0.0);
        self.card_clip.setOpacity(0.0);
        self.icons();
        self.press.set(None);
        MIC.store(0, Ordering::Relaxed);
        SYS.store(0, Ordering::Relaxed);
        self.mic_level.set(0.0);
        self.sys_level.set(0.0);
        self.toast.setOpacity(0.0);
        self.close.setOpacity(0.0);
        self.refresh(true);
        self.repaint();
        self.pulse();
        let still = reduce_motion();
        self.panel.setAlphaValue(if still { 1.0 } else { 0.0 });
        self.panel.orderFrontRegardless();
        if !still {
            self.fade(1.0, APPEAR_FADE);
        }
        self.scale(START_SCALE, 1.0, if still { 0.0 } else { APPEAR });
        if !self.ticking.replace(true) {
            self.tick();
        }
    }

    fn hide(&self) {
        match hide_action(self.phase.get()) {
            HideAction::Nothing => {}
            HideAction::OrderOut => {
                self.bump();
                self.phase.set(Phase::Hidden);
                self.panel.orderOut(None);
            }
            HideAction::Leave => {
                let generation = self.bump();
                self.phase.set(Phase::Leaving);
                let secs = if reduce_motion() { 0.0 } else { LEAVE };
                self.fade(0.0, secs);
                self.scale(1.0, START_SCALE, secs);
                after(Duration::from_secs_f64(secs), move |b| {
                    if b.generation.get() == generation {
                        b.panel.orderOut(None);
                        b.phase.set(Phase::Hidden);
                    }
                });
            }
        }
    }

    fn saved(&self) {
        if self.phase.get() != Phase::Shown {
            return self.hide();
        }
        let generation = self.bump();
        self.saving.set(true);
        self.unfold(false);
        self.halo.removeAllAnimations();
        let still = reduce_motion();
        self.layout_toast();
        animate(
            &self.toast,
            "opacity",
            0.0,
            1.0,
            if still { 0.0 } else { 0.16 },
        );
        animate(
            &self.toast,
            "transform.translation.y",
            -3.0,
            0.0,
            if still { 0.0 } else { 0.26 },
        );
        after(SAVED_HOLD, move |b| {
            if b.generation.get() == generation {
                b.hide();
            }
        });
    }

    /// The window's alpha, which AppKit owns, animated from wherever it is now.
    fn fade(&self, to: f64, secs: f64) {
        NSAnimationContext::beginGrouping();
        let context = NSAnimationContext::currentContext();
        context.setDuration(secs);
        context.setTimingFunction(Some(&ease_pop()));
        self.panel.animator().setAlphaValue(to);
        NSAnimationContext::endGrouping();
    }

    /// Scales everything about the pill's bottom-right corner: our layers by transform, and the blur view, which AppKit owns, by its frame.
    /// The pill is a capsule at every size, so a frame change draws the same shape a scale would.
    fn scale(&self, from: f64, to: f64, secs: f64) {
        for layer in [&self.face, &self.shadow] {
            animate(layer, "transform.scale", from, to, secs);
        }
        let size = self.pill_rect().size;
        let (w, h, right) = (size.width, size.height, self.right());
        let rect = |s: f64| {
            CGRect::new(
                CGPoint::new(right - w * s, MARGIN),
                CGSize::new(w * s, h * s),
            )
        };
        self.body.setFrame(rect(from));
        self.body
            .layer()
            .expect("layer-backed view")
            .setCornerRadius(PILL_H * from.min(to) / 2.0);
        NSAnimationContext::beginGrouping();
        let context = NSAnimationContext::currentContext();
        context.setDuration(secs);
        context.setTimingFunction(Some(&ease_pop()));
        self.body.animator().setFrame(rect(to));
        NSAnimationContext::endGrouping();
        let generation = self.scaling.get() + 1;
        self.scaling.set(generation);
        after(Duration::from_secs_f64(secs), move |b| {
            if b.scaling.get() != generation {
                return;
            }
            b.scaling.set(0);
            if to >= 1.0 {
                // The width may have changed meanwhile, and a smaller radius would show square corners on the grown capsule.
                b.body.setFrame(b.pill_rect());
                b.body
                    .layer()
                    .expect("layer-backed view")
                    .setCornerRadius(PILL_H / 2.0);
            }
        });
    }

    /// Picks the saved corner if it is still on a screen, or the default spot on the screen under the mouse.
    fn place_corner(&self, saved: Option<(f64, f64)>) -> (f64, f64) {
        let screens = NSScreen::screens(self.mtm);
        // Room for the unfolded buttons, so hover never pushes them off the screen.
        let size = self.open_size();
        if let Some(c) = saved {
            for s in screens.iter() {
                let v = s.visibleFrame();
                if contains(v, c.0 - 1.0, c.1 + 1.0) {
                    return clamp_corner(c, size, rect4(v));
                }
            }
        }
        let mouse = NSEvent::mouseLocation();
        let screen = screens
            .iter()
            .find(|s| contains(s.frame(), mouse.x, mouse.y))
            .or_else(|| NSScreen::mainScreen(self.mtm));
        let Some(screen) = screen else {
            return (INSET + size.0, INSET);
        };
        let v = screen.visibleFrame();
        (v.origin.x + v.size.width - INSET, v.origin.y + INSET)
    }

    /// Re-reads the appearance, the silence and the clock, and lays the pill out again when one changed or `force` asks.
    fn refresh(&self, force: bool) {
        let app = NSApplication::sharedApplication(self.mtm);
        let names =
            NSArray::from_slice(unsafe { &[NSAppearanceNameAqua, NSAppearanceNameDarkAqua] });
        let dark = app
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(&names)
            .is_some_and(|n| &*n == unsafe { NSAppearanceNameDarkAqua });
        let silent = !self.saving.get()
            && now_ms().saturating_sub(SYS_HEARD_MS.load(Ordering::Relaxed))
                >= SILENT_AFTER.as_millis() as u64;
        // The timer stops with the recording, and holds where a pause began.
        let text = if self.saving.get() {
            self.clock_text.borrow().clone()
        } else if let Some(paused) = self.paused_since_ms.get() {
            clock(Duration::from_millis(
                paused.saturating_sub(self.started_at_ms.get()),
            ))
        } else {
            clock(Duration::from_millis(
                now_ms().saturating_sub(self.started_at_ms.get()),
            ))
        };
        let flipped = self.dark.replace(Some(dark)) != Some(dark);
        let silence = self.silent.replace(silent) != silent;
        let changed = silence || *self.clock_text.borrow() != text;
        if flipped || changed || force {
            *self.clock_text.borrow_mut() = text;
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            self.layout();
            CATransaction::commit();
        }
        // Restarting the pulse on every layout would cut each 1.4 s beat short at the next clock tick.
        if flipped && !force {
            self.pulse();
        }
        // The card is an image, painted in the old palette's colors and with the old warnings.
        if self.fold.get().open
            && !force
            && (flipped || silence || *self.painted_age.borrow() != self.age())
        {
            self.repaint();
        }
    }

    fn layout(&self) {
        let p = self.palette();
        let appearance = if self.dark.get() == Some(true) {
            unsafe { NSAppearanceNameDarkAqua }
        } else {
            unsafe { NSAppearanceNameAqua }
        };
        self.body
            .setAppearance(NSAppearance::appearanceNamed(appearance).as_deref());
        let scale = self.panel.backingScaleFactor();

        let ink = if self.paused_since_ms.get().is_some() {
            p.muted
        } else {
            p.ink
        };
        let text = attributed(&self.clock_text.borrow(), &self.mono, ink, -0.24);
        unsafe { self.time.setString(Some(&text)) };
        self.time.setContentsScale(scale);
        let text_size = self.time.preferredFrameSize();
        let silent = self.silent.get();

        let paused = self.paused_since_ms.get().is_some();
        let muted = self.muted.get();
        let mut x = PAD_L;
        let mid = PILL_H / 2.0;
        self.halo.setHidden(paused);
        self.glyph.setHidden(!paused);
        self.halo.setPosition(CGPoint::new(x + DOT / 2.0, mid));
        self.glyph.setPosition(CGPoint::new(x + DOT / 2.0, mid));
        for bar in unsafe { self.glyph.sublayers() }.into_iter().flatten() {
            bar.setBackgroundColor(Some(&color(p.muted)));
        }
        x += DOT + GAP;
        // The mock greys both halves at 55% while paused, and the mic's at 50% while muted.
        let (mic_c, mic_o) = match (paused, muted) {
            (true, _) => (p.muted, 0.55),
            (false, true) => (p.muted, 0.5),
            _ => (p.mic, 1.0),
        };
        let (sys_c, sys_o) = match (paused, silent) {
            (true, _) => (p.muted, 0.55),
            (false, true) => (p.warn, 1.0),
            _ => (p.sys, 1.0),
        };
        for i in 0..BARS {
            let bx = x + i as f64 * (BAR_W + BAR_GAP) + BAR_W / 2.0;
            self.mic[i].setPosition(CGPoint::new(bx, mid + 0.5));
            self.sys[i].setPosition(CGPoint::new(bx, mid - 0.5));
            self.mic[i].setBackgroundColor(Some(&color(mic_c)));
            self.mic[i].setOpacity(mic_o);
            self.sys[i].setBackgroundColor(Some(&color(sys_c)));
            self.sys[i].setOpacity(sys_o);
        }
        x += BARS_W + GAP;
        self.time.setFrame(CGRect::new(
            CGPoint::new(x, ((PILL_H - text_size.height) / 2.0).round()),
            text_size,
        ));
        x += text_size.width;
        let [sys_flag, mic_flag] = self.fold.get().flags(self.flags());
        for (flag, (slot, shown), c) in [
            (&self.flag, sys_flag, p.warn),
            (&self.mic_flag, mic_flag, p.rec),
        ] {
            flag.setHidden(!shown);
            if slot {
                x += GAP;
                flag.setPosition(CGPoint::new(x + FLAG / 2.0, mid));
                flag.setStrokeColor(Some(&color(c)));
                x += FLAG;
            }
        }
        let w = (x + PAD_R).ceil();
        self.width.set(w);

        let right = self.right();
        let (cw, ch) = (right + MARGIN, PILL_H + 2.0 * MARGIN + self.card_h.get());
        let content = CGRect::new(CGPoint::ZERO, CGSize::new(cw, ch));
        let corner = CGPoint::new(right, MARGIN);
        for (view, layer) in [
            (&self.face_view, &self.face),
            (&self.shadow_view, &self.shadow),
        ] {
            view.setFrame(content);
            layer.setBounds(content);
            layer.setAnchorPoint(CGPoint::new(corner.x / cw, corner.y / ch));
            layer.setPosition(corner);
        }
        self.row.setFrame(CGRect::new(
            CGPoint::new(right - w, MARGIN),
            CGSize::new(w, PILL_H),
        ));
        self.acts.setFrame(CGRect::new(
            CGPoint::new(right - w - ACTS_W, MARGIN),
            CGSize::new(ACTS_W, PILL_H),
        ));
        if self.sliding.get() == 0 {
            self.pill_frames(0.0);
        }
        self.tint.setBackgroundColor(Some(&color(p.bg)));
        let edge = p.edge;
        self.tint.setBorderColor(Some(&color(edge)));
        self.shadow
            .setShadowColor(Some(&color((p.shadow.0, p.shadow.1, p.shadow.2, 1.0))));
        self.shadow.setShadowOpacity(p.shadow.3 as f32);

        self.halo.setBackgroundColor(Some(&color(p.halo)));
        self.dot.setBackgroundColor(Some(&color(p.rec)));

        self.close.setPosition(CGPoint::new(
            corner.x + CLOSE_OUT - CLOSE / 2.0,
            corner.y - CLOSE_OUT + CLOSE / 2.0,
        ));
        self.close.setBackgroundColor(Some(&color(p.bg)));
        self.close.setBorderColor(Some(&color(p.edge)));
        self.close
            .setShadowColor(Some(&color((p.shadow.0, p.shadow.1, p.shadow.2, 1.0))));
        self.close.setShadowOpacity(p.shadow.3 as f32);
        self.close.setShadowPath(Some(&*unsafe {
            CGPath::with_ellipse_in_rect(
                inset(
                    CGRect::new(CGPoint::ZERO, CGSize::new(CLOSE, CLOSE)),
                    p.shadow_inset / 2.0,
                ),
                std::ptr::null(),
            )
        }));
        self.paint_buttons();
        for l in [
            &self.close_x,
            &self.flag,
            &self.mic_flag,
            &self.pause_icon,
            &self.mute_icon,
        ] {
            l.setContentsScale(scale);
        }
        self.layout_toast();
        self.bars();
        self.place();
    }

    fn layout_toast(&self) {
        let p = self.palette();
        let text = attributed("Saved", &self.sans, p.ok, 0.0);
        unsafe { self.toast_text.setString(Some(&text)) };
        self.toast_text
            .setContentsScale(self.panel.backingScaleFactor());
        let t = self.toast_text.preferredFrameSize();
        let size = CGSize::new((t.width + 22.0).ceil(), (t.height + 12.0).ceil());
        self.toast_text.setFrame(CGRect::new(
            CGPoint::new(11.0, ((size.height - t.height) / 2.0).round()),
            t,
        ));
        let right = self.right();
        self.toast.setFrame(CGRect::new(
            CGPoint::new(right - size.width, MARGIN + PILL_H + 8.0),
            size,
        ));
        self.toast.setCornerRadius(size.height / 2.0);
        self.toast.setBackgroundColor(Some(&color(p.bg)));
        self.toast.setBorderColor(Some(&color(p.edge)));
        self.toast
            .setShadowColor(Some(&color((p.shadow.0, p.shadow.1, p.shadow.2, 1.0))));
        self.toast.setShadowOpacity(p.shadow.3 as f32);
    }

    /// The mock's `recpulse`, or none under reduced motion.
    fn pulse(&self) {
        self.halo.removeAllAnimations();
        if reduce_motion() || self.saving.get() || self.paused_since_ms.get().is_some() {
            return;
        }
        for (key, low) in [("opacity", 0.55), ("transform.scale", 1.15)] {
            let a = CABasicAnimation::animationWithKeyPath(Some(&NSString::from_str(key)));
            unsafe {
                a.setFromValue(Some(&NSNumber::new_f64(1.0)));
                a.setToValue(Some(&NSNumber::new_f64(low)));
            }
            a.setDuration(0.7);
            a.setAutoreverses(true);
            a.setRepeatCount(f32::INFINITY);
            a.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(unsafe {
                objc2_quartz_core::kCAMediaTimingFunctionEaseInEaseOut
            })));
            self.halo
                .addAnimation_forKey(&a, Some(&NSString::from_str(key)));
        }
    }

    fn place(&self) {
        // A card that grew since the last drag or show must still fit on its screen, though not while it is open under the pointer.
        let corner = self.corner.get();
        let screens = NSScreen::screens(self.mtm);
        if let Some(s) = screens
            .iter()
            .filter(|_| !self.fold.get().open)
            .find(|s| contains(s.frame(), corner.0 - 1.0, corner.1 + 1.0))
        {
            self.corner.set(clamp_corner(
                corner,
                self.open_size(),
                rect4(s.visibleFrame()),
            ));
        }
        let (right, bottom) = self.corner.get();
        self.panel.setFrame_display(
            CGRect::new(
                CGPoint::new(right - self.right(), bottom - MARGIN),
                CGSize::new(
                    self.right() + MARGIN,
                    PILL_H + 2.0 * MARGIN + self.card_h.get(),
                ),
            ),
            true,
        );
    }

    fn bars(&self) {
        let (m, s) = (self.mic_level.get(), self.sys_level.get());
        let still = reduce_motion();
        for i in 0..BARS {
            let jitter = |b: &Self| if still { 1.0 } else { b.jitter() };
            for (layer, level) in [(&self.mic[i], m), (&self.sys[i], s)] {
                let h = half_height(level, i, jitter(self));
                layer.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(BAR_W, h)));
                layer.setCornerRadius((h / 2.0).min(BAR_W / 2.0));
            }
        }
    }

    fn tick(&self) {
        if !matches!(self.phase.get(), Phase::Shown | Phase::Leaving) {
            self.ticking.set(false);
            return;
        }
        let n = self.ticks.get().wrapping_add(1);
        self.ticks.set(n);
        let (mic, sys) = (
            f32::from_bits(MIC.swap(0, Ordering::Relaxed)),
            f32::from_bits(SYS.swap(0, Ordering::Relaxed)),
        );
        let (mic, sys) = if self.saving.get() {
            (0.0, 0.0)
        } else {
            (mic, sys)
        };
        let still = reduce_motion();
        if still {
            if n.is_multiple_of(STEP_TICKS) {
                self.mic_level.set(mic);
                self.sys_level.set(sys);
            }
        } else {
            self.mic_level.set(smooth(self.mic_level.get(), mic));
            self.sys_level.set(smooth(self.sys_level.get(), sys));
        }
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        if !still || n.is_multiple_of(STEP_TICKS) {
            self.bars();
        }
        CATransaction::commit();
        self.refresh(false);
        // AppKit can miss the exit when the bubble appears under a resting pointer, so a hover or an open card rechecks where the pointer is.
        if self.hover.get() || self.fold.get().open {
            self.track();
        }
        after(TICK, |b| b.tick());
    }

    /// xorshift32, enough to keep the bars from moving in lockstep.
    fn jitter(&self) -> f32 {
        let mut x = self.seed.get();
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed.set(x);
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    /// The mouse in the content view's coordinates.
    fn mouse(&self) -> CGPoint {
        let m = NSEvent::mouseLocation();
        let o = self.panel.frame().origin;
        CGPoint::new(m.x - o.x, m.y - o.y)
    }

    /// The pill's right edge in the panel, which hover, pause and mute never move.
    fn right(&self) -> f64 {
        MARGIN + self.room() + self.width.get()
    }

    /// The panel's room left of the row, for the buttons and for the card, which is wider than the row.
    fn room(&self) -> f64 {
        ACTS_W.max(card::CARD_W - self.width.get())
    }

    /// The bubble's width and height while unfolded: the card over the row and its buttons.
    fn open_size(&self) -> (f64, f64) {
        (
            card::CARD_W.max(self.width.get() + ACTS_W),
            PILL_H + self.card_h.get(),
        )
    }

    /// The bubble: the pill at rest, and the card over the row while unfolded.
    fn pill_rect(&self) -> CGRect {
        let (w, h) = if self.fold.get().open {
            self.open_size()
        } else {
            (self.width.get(), PILL_H)
        };
        CGRect::new(CGPoint::new(self.right() - w, MARGIN), CGSize::new(w, h))
    }

    fn close_rect(&self) -> CGRect {
        CGRect::new(
            CGPoint::new(self.right() + CLOSE_OUT - CLOSE, MARGIN - CLOSE_OUT),
            CGSize::new(CLOSE, CLOSE),
        )
    }

    /// The unfolded strip left of the row.
    fn strip_rect(&self) -> CGRect {
        CGRect::new(
            CGPoint::new(self.right() - self.width.get() - ACTS_W, MARGIN),
            CGSize::new(ACTS_W, PILL_H),
        )
    }

    fn button_rect(&self, target: Target) -> CGRect {
        let i = usize::from(target == Target::Mute);
        CGRect::new(
            CGPoint::new(self.strip_rect().origin.x + BTN_X[i], MARGIN + BTN_Y),
            CGSize::new(BTN_W, BTN_H),
        )
    }

    /// What the pointer at `m` is on. The close button and the buttons count only while hover shows them.
    fn target(&self, m: CGPoint) -> Option<Target> {
        if self.hover.get() && rect_contains(self.close_rect(), m) {
            return Some(Target::Close);
        }
        if self.fold.get().open && rect_contains(self.strip_rect(), m) {
            let buttons = [Target::Pause, Target::Mute];
            let on = buttons
                .into_iter()
                .find(|&t| rect_contains(self.button_rect(t), m));
            // The recording has ended during the Saved hold, so there is nothing left to pause or mute.
            return Some(on.filter(|_| !self.saving.get()).unwrap_or(Target::Strip));
        }
        rect_contains(self.pill_rect(), m).then_some(Target::Pill)
    }

    /// Hover shows the close button and unfolds pause and mute, as the mock's `.is-hover` does.
    fn track(&self) {
        if self.phase.get() != Phase::Shown {
            return;
        }
        let target = self.target(self.mouse());
        let hover = target.is_some();
        let still = reduce_motion();
        // The front app resets the cursor on its own moves, so set it on every one of ours.
        if hover {
            NSCursor::pointingHandCursor().set();
        }
        if self.hover.replace(hover) != hover {
            if !hover {
                NSCursor::arrowCursor().set();
            }
            animate(
                &self.close,
                "opacity",
                f64::from(unsafe { self.close.presentationLayer() }.map_or(0.0, |l| l.opacity())),
                if hover { 1.0 } else { 0.0 },
                if still { 0.0 } else { HOVER_FADE },
            );
        }
        // Runs every time, so a fold that fell out of step with the hover catches up; the Saved hold keeps it shut.
        self.unfold(hover && !self.saving.get());
        if self.hot.replace(target) != target {
            CATransaction::begin();
            CATransaction::setDisableActions(true);
            self.paint_buttons();
            CATransaction::commit();
        }
    }

    /// Unfolds or folds the buttons: the pill grows or shrinks to the left over the mock's 220 ms slide while the buttons fade.
    fn unfold(&self, open: bool) {
        if self.fold.get().open == open {
            return;
        }
        if open {
            // Painted while still folded, so the corner clamps once to the card's real height before it opens.
            self.repaint();
            // That clamp can move the pill out from under the pointer, and the exit it delivers has already ended the hover.
            if !self.hover.get() {
                return;
            }
        }
        self.fold.set(if open {
            Fold::unfolded(self.flags())
        } else {
            Fold::default()
        });

        let secs = if reduce_motion() { 0.0 } else { OPEN };
        for layer in [&self.acts, &self.card_clip] {
            animate(
                layer,
                "opacity",
                f64::from(unsafe { layer.presentationLayer() }.map_or(0.0, |l| l.opacity())),
                if open { 1.0 } else { 0.0 },
                if secs > 0.0 { HOVER_FADE } else { 0.0 },
            );
        }
        let generation = self.sliding.get() + 1;
        self.sliding.set(generation);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.layout();
        CATransaction::commit();
        CATransaction::begin();
        CATransaction::setAnimationDuration(secs);
        CATransaction::setAnimationTimingFunction(Some(&ease_slide()));
        CATransaction::setDisableActions(secs == 0.0);
        self.pill_frames(secs);
        CATransaction::commit();
        after(Duration::from_secs_f64(secs), move |b| {
            if b.sliding.get() == generation {
                b.sliding.set(0);
                // A line or warning that arrived during the slide changed the card's height.
                CATransaction::begin();
                CATransaction::setDisableActions(true);
                b.layout();
                CATransaction::commit();
            }
        });
    }

    /// Whether the silence and mute flags show beside the timer now.
    fn flags(&self) -> (bool, bool) {
        let paused = self.paused_since_ms.get().is_some();
        (self.silent.get() && !paused, self.muted.get())
    }

    /// The tint, its shadow and the blur view at the pill's current width, animated by the open transaction.
    fn pill_frames(&self, secs: f64) {
        let p = self.palette();
        let pill = self.pill_rect();
        self.tint.setFrame(pill);
        let shadow_rect = inset(pill, p.shadow_inset);
        let radius = PILL_H / 2.0 - p.shadow_inset;
        let path =
            unsafe { CGPath::with_rounded_rect(shadow_rect, radius, radius, std::ptr::null()) };
        // A shadow path takes no implicit animation, so without this the shadow jumps to the open size ahead of the bubble.
        if secs > 0.0 {
            let key = NSString::from_str("shadowPath");
            let from = unsafe { self.shadow.presentationLayer() }.and_then(|l| l.shadowPath());
            let a = CABasicAnimation::animationWithKeyPath(Some(&key));
            unsafe {
                a.setFromValue(from.as_deref().map(cf_object));
                a.setToValue(Some(cf_object(&path)));
            }
            a.setDuration(secs);
            a.setTimingFunction(Some(&ease_slide()));
            self.shadow.addAnimation_forKey(&a, Some(&key));
        }
        self.shadow.setShadowPath(Some(&*path));
        // The card clips to the part of the bubble above the row, and its content hangs from the top as the mock's `.up` does.
        let clip = CGRect::new(
            CGPoint::new(pill.origin.x, MARGIN + PILL_H),
            CGSize::new(pill.size.width, pill.size.height - PILL_H),
        );
        self.card_clip.setFrame(clip);
        let (top_h, card_h) = (self.top_h.get(), self.card_h.get());
        self.card_body.setFrame(CGRect::new(
            CGPoint::new(clip.size.width - card::CARD_W, clip.size.height - card_h),
            CGSize::new(card::CARD_W, card_h),
        ));
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.card_top.setFrame(CGRect::new(
            CGPoint::new(0.0, card_h - top_h),
            CGSize::new(card::CARD_W, top_h),
        ));
        self.card_lines.setFrame(CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(card::CARD_W, card_h - top_h),
        ));
        CATransaction::commit();
        if self.scaling.get() == 0 {
            NSAnimationContext::beginGrouping();
            let context = NSAnimationContext::currentContext();
            context.setDuration(secs);
            context.setTimingFunction(Some(&ease_slide()));
            self.body.animator().setFrame(pill);
            NSAnimationContext::endGrouping();
        }
    }

    /// The pause and mute icons for the current state, rebuilt only when it changes.
    fn icons(&self) {
        self.pause_icon.setPath(Some(&pause_path(
            BTN_ICON,
            self.paused_since_ms.get().is_some(),
        )));
        self.mute_icon
            .setPath(Some(&mic_path(BTN_ICON, self.muted.get())));
    }

    /// Button colors and hover backgrounds, and the close button's color.
    fn paint_buttons(&self) {
        let p = self.palette();
        let hot = self.hot.get();
        let muted = self.muted.get();
        self.pause_icon.setStrokeColor(Some(&color(p.ink)));
        self.mute_icon
            .setStrokeColor(Some(&color(if muted { p.rec } else { p.ink })));
        for (bg, t) in [
            (&self.pause_bg, Target::Pause),
            (&self.mute_bg, Target::Mute),
        ] {
            let c = if hot == Some(t) {
                p.hover
            } else {
                (0.0, 0.0, 0.0, 0.0)
            };
            bg.setBackgroundColor(Some(&color(c)));
        }
        self.divider.setBackgroundColor(Some(&color(p.edge)));
        self.close_x
            .setStrokeColor(Some(&color(if hot == Some(Target::Close) {
                p.ink
            } else {
                p.muted
            })));
    }

    fn down(&self) {
        if self.phase.get() != Phase::Shown {
            return;
        }
        let Some(target) = self.target(self.mouse()) else {
            return;
        };
        self.press.set(Some(Press {
            mouse: NSEvent::mouseLocation(),
            corner: self.corner.get(),
            target,
            moved: false,
        }));
    }

    fn dragged(&self) {
        let Some(mut press) = self.press.get() else {
            return;
        };
        if press.target == Target::Close {
            return;
        }
        let m = NSEvent::mouseLocation();
        let (dx, dy) = (m.x - press.mouse.x, m.y - press.mouse.y);
        if !press.moved && dx.hypot(dy) < DRAG_SLOP {
            return;
        }
        press.moved = true;
        self.press.set(Some(press));
        let target = (press.corner.0 + dx, press.corner.1 + dy);
        let screens = NSScreen::screens(self.mtm);
        let screen = screens
            .iter()
            .find(|s| contains(s.frame(), m.x, m.y))
            .or_else(|| NSScreen::mainScreen(self.mtm));
        let corner = match screen {
            Some(s) => clamp_corner(target, self.open_size(), rect4(s.visibleFrame())),
            None => target,
        };
        self.corner.set(corner);
        self.place();
    }

    fn up(&self) {
        let Some(press) = self.press.take() else {
            return;
        };
        if press.moved {
            let (right, bottom) = self.corner.get();
            return emit(Event::Moved { right, bottom });
        }
        // A click counts only where it started.
        if self.target(self.mouse()) != Some(press.target) {
            return;
        }
        match press.target {
            Target::Close => {
                self.hide();
                emit(Event::Dismiss);
            }
            Target::Pause => {
                // The click's moment goes to the server too, so the frozen timer reads the same on both.
                let at = self.paused_since_ms.get().is_none().then(now_ms);
                self.set_status(at, self.muted.get());
                emit(Event::Pause(at));
            }
            Target::Mute => {
                let mute = !self.muted.get();
                self.set_status(self.paused_since_ms.get(), mute);
                emit(Event::Mute(mute));
            }
            Target::Pill => self.bring_back(),
            Target::Strip => {}
        }
    }

    fn bring_back(&self) {
        let still = reduce_motion();
        self.scale(1.0, PRESS_SCALE, if still { 0.0 } else { PRESS });
        let pid = FRONT_PID.load(Ordering::Relaxed);
        let raised = pid != 0
            && NSRunningApplication::runningApplicationWithProcessIdentifier(pid).is_some_and(
                |app| app.activateWithOptions(NSApplicationActivationOptions::empty()),
            );
        emit(Event::Return { raised });
        // Yogurt's page reports focus and the server hides the bubble. When the page is not the browser's front tab, it springs back instead.
        after(Duration::from_millis(110), |b| {
            if b.phase.get() == Phase::Shown {
                b.scale(PRESS_SCALE, 1.0, if reduce_motion() { 0.0 } else { APPEAR });
            }
        });
    }

    fn set_card(&self, card: crate::bubble::Card) {
        let added = card.added > self.card.borrow().added && card.title == self.card.borrow().title;
        if *self.card.borrow() == card {
            return;
        }
        *self.card.borrow_mut() = card;
        if self.phase.get() != Phase::Shown || !self.fold.get().open {
            return;
        }
        self.repaint();
        // The mock's new line: the older lines move up and the new one rises in from below.
        if added && self.fold.get().open && !reduce_motion() {
            animate(
                &self.card_lines,
                "transform.translation.y",
                -self.rise.get(),
                0.0,
                0.26,
            );
        }
    }

    /// The recap's age as the card would read it now.
    fn age(&self) -> Option<String> {
        let at = self.card.borrow().pro.as_ref()?.updated_at_ms?;
        Some(crate::bubble::ago(Duration::from_millis(
            now_ms().saturating_sub(at),
        )))
    }

    /// Draws the card for its contents and the live warnings, and lays the bubble out around its new height.
    fn repaint(&self) {
        *self.painted_age.borrow_mut() = self.age();
        let (silent, muted) = self.flags();
        let warns: Vec<card::Warn> = [(silent, card::Warn::Silent), (muted, card::Warn::Muted)]
            .into_iter()
            .filter_map(|(on, w)| on.then_some(w))
            .collect();
        let p = self.palette();
        let painted = card::paint(&self.card.borrow(), &warns, p, &self.fonts);
        let scale = self.panel.backingScaleFactor();
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        unsafe {
            self.card_top.setContents(Some(&painted.top));
            self.card_lines
                .setContents(painted.lines.as_deref().map(|i| &**i as &AnyObject));
        }
        for l in [&self.card_top, &self.card_lines] {
            l.setContentsScale(scale);
        }
        for (icon, w) in self
            .warn_icons
            .iter()
            .zip([card::Warn::Silent, card::Warn::Muted])
        {
            let at = painted.icons.iter().find(|(kind, _)| *kind == w);
            icon.setHidden(at.is_none());
            if let Some((_, c)) = at {
                // The image is laid out top-down, and the layer counts up from its bottom.
                icon.setPosition(CGPoint::new(c.x, painted.top_h - c.y));
                icon.setStrokeColor(Some(&color(p.warn)));
                icon.setContentsScale(scale);
            }
        }
        self.top_h.set(painted.top_h);
        self.rise.set(painted.rise);
        self.card_h.set(painted.top_h + painted.lines_h);
        self.layout();
        CATransaction::commit();
    }

    fn set_status(&self, paused_since_ms: Option<u64>, muted: bool) {
        let paused_changed = self.paused_since_ms.replace(paused_since_ms) != paused_since_ms;
        let muted_changed = self.muted.replace(muted) != muted;
        if !(paused_changed || muted_changed) {
            return;
        }
        self.icons();
        if self.phase.get() == Phase::Hidden {
            return;
        }
        self.refresh(true);
        if self.fold.get().open {
            self.repaint();
        }
        if paused_changed {
            self.pulse();
        }
    }
}

fn contains(r: CGRect, x: f64, y: f64) -> bool {
    x >= r.origin.x
        && x <= r.origin.x + r.size.width
        && y >= r.origin.y
        && y <= r.origin.y + r.size.height
}

fn rect_contains(r: CGRect, p: CGPoint) -> bool {
    contains(r, p.x, p.y)
}

fn rect4(r: CGRect) -> (f64, f64, f64, f64) {
    (r.origin.x, r.origin.y, r.size.width, r.size.height)
}

fn inset(r: CGRect, by: f64) -> CGRect {
    CGRect::new(
        CGPoint::new(r.origin.x + by, r.origin.y + by),
        CGSize::new(r.size.width - 2.0 * by, r.size.height - 2.0 * by),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfolded_flags_hold_their_slots_and_a_fresh_fold_reads_them_live() {
        let unfolded = Fold::unfolded((true, false));
        // Pausing clears the silence flag and muting adds the mic flag: the slots stay, the silence glyph hides, the mic glyph waits for the fold.
        assert_eq!(
            unfolded.flags((false, true)),
            [(true, false), (false, false)]
        );
        // What appear() starts from, so a bubble that left while unfolded comes back with its flags as they are now.
        assert_eq!(
            Fold::default().flags((false, true)),
            [(false, false), (true, true)]
        );
    }

    #[test]
    fn a_hide_in_the_wait_orders_the_panel_out_and_a_repeat_hide_does_nothing() {
        assert_eq!(hide_action(Phase::Waiting), HideAction::OrderOut);
        assert_eq!(hide_action(Phase::Shown), HideAction::Leave);
        assert_eq!(hide_action(Phase::Leaving), HideAction::Nothing);
        assert_eq!(hide_action(Phase::Hidden), HideAction::Nothing);
    }
}
