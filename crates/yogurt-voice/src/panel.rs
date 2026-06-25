//! The pill: a borderless, non-activating `NSPanel` at the bottom center of the screen under the mouse.
//! [`show`] and [`level`] are safe from any thread. Everything else runs on the main thread, which `run_app` owns.

use std::cell::{Cell, OnceCell};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSAppearance, NSAppearanceCustomization,
    NSAppearanceNameDarkAqua, NSAutoresizingMaskOptions, NSBackingStoreType, NSColor, NSEvent,
    NSFloatingWindowLevel, NSFont, NSFontDescriptor, NSFontWeightSemibold, NSPanel, NSScreen,
    NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
    NSVisualEffectView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFType, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{CGColor, CGMutablePath, CGPath};
use objc2_foundation::{NSArray, NSData, NSNumber, NSString};
use objc2_quartz_core::{
    kCAMediaTimingFunctionEaseInEaseOut, kCAMediaTimingFunctionEaseOut, CAAnimationGroup,
    CACurrentMediaTime, CAKeyframeAnimation, CALayer, CAMediaTiming, CAMediaTimingFunction,
    CAShapeLayer, CATextLayer, CATransaction,
};

use crate::pill::{bar_height, level_from_peak, smooth, PillState, ERROR_FOR, PASTED_FOR};

const HEIGHT: f64 = 36.0;
const MIN_WIDTH: f64 = 132.0;
const PAD: f64 = 16.0;
const GAP: f64 = 9.0;
const BAR_W: f64 = 3.0;
const BAR_GAP: f64 = 3.0;
const BARS: usize = 11;
const BARS_W: f64 = BARS as f64 * BAR_W + (BARS - 1) as f64 * BAR_GAP;
/// Room around the pill for its shadow, which the panel draws itself.
const MARGIN: f64 = 40.0;
/// Above the bottom of the screen's visible frame, which keeps the pill above the Dock.
const LIFT: f64 = 24.0;
const FADE: f64 = 0.16;
const TICK: Duration = Duration::from_nanos(1_000_000_000 / 30);

type Rgba = (f64, f64, f64, f64);
const fn hex(rgb: u32, a: f64) -> Rgba {
    (
        ((rgb >> 16) & 0xff) as f64 / 255.0,
        ((rgb >> 8) & 0xff) as f64 / 255.0,
        (rgb & 0xff) as f64 / 255.0,
        a,
    )
}
const BLUEBERRY: Rgba = hex(0x7D72DE, 1.0);
const SHIMMER_SLOW: Rgba = hex(0xA69D90, 1.0);
const STRAWBERRY: Rgba = hex(0xE8886F, 1.0);
const HALO: Rgba = hex(0xE8886F, 0.18);
const MATCHA: Rgba = hex(0x7FBE8C, 1.0);
const ERROR_TEXT: Rgba = hex(0xF6D9D1, 1.0);
const TINT: Rgba = hex(0x1E1B18, 0.84);
const ERROR_TINT: Rgba = hex(0x4E221C, 0.9);
const EDGE: Rgba = hex(0xFFFFFF, 0.09);
const CLEAR: Rgba = (0.0, 0.0, 0.0, 0.0);

/// The loudest frame since the last tick, as `f32` bits, which order like `u32` for non-negative values.
static LEVEL: AtomicU32 = AtomicU32::new(0);

thread_local! {
    static PILL: OnceCell<Pill> = const { OnceCell::new() };
}

/// Shows the pill in `state` for clip number `clip`, or hides it. Pasted and Error hide on their own.
/// A state for an older clip than the last one shown is dropped, so a slow transcription cannot touch a newer clip's pill.
/// The check runs on the main thread, where the states land in order.
pub fn show(clip: u64, state: PillState) {
    DispatchQueue::main().exec_async(move || {
        with_pill(|p| {
            if clip >= p.clip.get() {
                p.clip.set(clip);
                p.set(state);
            }
        })
    });
}

/// Feeds one mic frame's peak amplitude (0..1) to the bars, which keep the loudest frame between two 30 Hz ticks.
pub fn level(peak: f32) {
    LEVEL.fetch_max(level_from_peak(peak).to_bits(), Ordering::Relaxed);
}

fn with_pill(f: impl FnOnce(&Pill)) {
    let mtm = MainThreadMarker::new().expect("the pill runs on the main thread");
    PILL.with(|p| f(p.get_or_init(|| Pill::new(mtm))));
}

fn after(delay: Duration, f: impl FnOnce(&Pill) + Send + 'static) {
    let when = DispatchTime::NOW.time(delay.as_nanos() as i64);
    let _ = DispatchQueue::main().after(when, move || with_pill(f));
}

fn color(c: Rgba) -> objc2_core_foundation::CFRetained<CGColor> {
    CGColor::new_srgb(c.0, c.1, c.2, c.3)
}

/// A view that hosts a layer of ours, so AppKit leaves that layer's properties alone.
fn hosting_view(mtm: MainThreadMarker, frame: CGRect) -> (Retained<NSView>, Retained<CALayer>) {
    let layer = CALayer::new();
    let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
    view.setLayer(Some(&layer));
    view.setWantsLayer(true);
    (view, layer)
}

fn layer(parent: &CALayer, size: f64, radius: f64) -> Retained<CALayer> {
    let l = CALayer::new();
    l.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(size, size)));
    l.setCornerRadius(radius);
    parent.addSublayer(&l);
    l
}

struct Pill {
    mtm: MainThreadMarker,
    panel: Retained<NSPanel>,
    shadow: Retained<CALayer>,
    body: Retained<NSVisualEffectView>,
    face_view: Retained<NSView>,
    face: Retained<CALayer>,
    halo: Retained<CALayer>,
    mark: Retained<CALayer>,
    bars: Vec<Retained<CALayer>>,
    check: Retained<CAShapeLayer>,
    /// Drawn by Core Animation, without the font smoothing AppKit gives light text on dark, as the mock's `antialiased` text has none.
    label: Retained<CATextLayer>,
    state: Cell<PillState>,
    clip: Cell<u64>,
    /// Bumped on every state change, so a delayed hide or tick from an older state does nothing.
    generation: Cell<u64>,
    ticking: Cell<bool>,
    smoothed: Cell<f32>,
    seed: Cell<u32>,
    /// Center x and bottom y of the pill on the screen it appeared on.
    anchor: Cell<(f64, f64)>,
}

impl Pill {
    fn new(mtm: MainThreadMarker) -> Self {
        let frame = CGRect::new(
            CGPoint::ZERO,
            CGSize::new(MIN_WIDTH + 2.0 * MARGIN, HEIGHT + 2.0 * MARGIN),
        );
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
        panel.setIgnoresMouseEvents(true);
        panel.setHidesOnDeactivate(false);
        panel.setFloatingPanel(true);
        panel.setBecomesKeyOnlyIfNeeded(true);
        unsafe { panel.setReleasedWhenClosed(false) };

        let content = NSView::initWithFrame(NSView::alloc(mtm), frame);
        content.setWantsLayer(true);
        panel.setContentView(Some(&content));

        let (shadow_view, shadow) = hosting_view(mtm, frame);
        shadow.setShadowColor(Some(&color(hex(0, 1.0))));
        shadow.setShadowOpacity(0.5);
        shadow.setShadowRadius(14.0);
        shadow.setShadowOffset(CGSize::new(0.0, -12.0));
        shadow_view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        content.addSubview(&shadow_view);

        let body = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), frame);
        body.setMaterial(NSVisualEffectMaterial::HUDWindow);
        body.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
        body.setState(NSVisualEffectState::Active);
        body.setAppearance(
            NSAppearance::appearanceNamed(unsafe { NSAppearanceNameDarkAqua }).as_deref(),
        );
        body.setWantsLayer(true);
        let body_layer = body.layer().expect("layer-backed view");
        body_layer.setCornerRadius(HEIGHT / 2.0);
        body_layer.setMasksToBounds(true);
        content.addSubview(&body);

        let (face_view, face) = hosting_view(mtm, frame);
        face.setCornerRadius(HEIGHT / 2.0);
        face.setBorderWidth(1.0);
        face.setBorderColor(Some(&color(EDGE)));
        body.addSubview(&face_view);

        let halo = layer(&face, 13.0, 6.5);
        let mark = layer(&halo, 7.0, 3.5);
        let bars = (0..BARS)
            .map(|_| {
                let b = layer(&face, BAR_W, BAR_W / 2.0);
                b.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(BAR_W, 4.0)));
                b
            })
            .collect();

        let check = CAShapeLayer::new();
        check.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(16.0, 16.0)));
        let path = CGMutablePath::new();
        // The mock's 16 pt check, with y flipped for a y-up layer.
        unsafe {
            CGMutablePath::move_to_point(Some(&path), std::ptr::null(), 3.5, 7.5);
            CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), 6.5, 4.5);
            CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), 12.5, 11.5);
        }
        check.setPath(Some(&path));
        check.setFillColor(None);
        check.setStrokeColor(Some(&color(MATCHA)));
        check.setLineWidth(2.0);
        check.setLineCap(unsafe { objc2_quartz_core::kCALineCapRound });
        check.setLineJoin(unsafe { objc2_quartz_core::kCALineJoinRound });
        face.addSublayer(&check);

        let label = CATextLayer::new();
        let font = manrope(12.5);
        // NSFont is toll-free bridged to CTFont, which is what the layer takes.
        unsafe { label.setFont(Some(&*Retained::as_ptr(&font).cast::<CFType>())) };
        label.setFontSize(12.5);
        face.addSublayer(&label);

        Self {
            mtm,
            panel,
            shadow,
            body,
            face_view,
            face,
            halo,
            mark,
            bars,
            check,
            label,
            state: Cell::new(PillState::Hidden),
            clip: Cell::new(0),
            generation: Cell::new(0),
            ticking: Cell::new(false),
            smoothed: Cell::new(0.0),
            seed: Cell::new(0x9e37_79b9),
            anchor: Cell::new((0.0, 0.0)),
        }
    }

    fn set(&self, state: PillState) {
        let hide_after = match state {
            PillState::Pasted => Some(PASTED_FOR),
            PillState::Error(_) => Some(ERROR_FOR),
            _ => None,
        };
        // Pasted and Error restart their timer on a repeat, the rest have nothing to redo.
        if state == self.state.get() && hide_after.is_none() {
            return;
        }
        let was = self.state.replace(state);
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        if state == PillState::Hidden {
            if was != PillState::Hidden {
                self.fade(0.0);
                after(Duration::from_secs_f64(FADE), move |p| {
                    if p.generation.get() == generation {
                        p.panel.orderOut(None);
                        p.stop_shimmer();
                    }
                });
            }
            return;
        }
        if state.is_recording() && !was.is_recording() {
            LEVEL.store(0, Ordering::Relaxed);
            self.smoothed.set(0.0);
        }

        CATransaction::begin();
        CATransaction::setDisableActions(true);
        self.layout(state);
        CATransaction::commit();

        if was == PillState::Hidden || !self.panel.isVisible() {
            self.anchor.set(anchor(self.mtm));
            self.place();
            self.panel.setAlphaValue(0.0);
            self.panel.orderFrontRegardless();
            self.fade(1.0);
        } else {
            self.place();
        }

        if state.is_recording() && !self.ticking.replace(true) {
            self.tick();
        }
        if let Some(delay) = hide_after {
            after(delay, move |p| {
                if p.generation.get() == generation {
                    p.set(PillState::Hidden);
                }
            });
        }
    }

    fn width(&self) -> f64 {
        self.face_view.frame().size.width
    }

    fn layout(&self, state: PillState) {
        let error = matches!(state, PillState::Error(_));
        let busy = matches!(state, PillState::Transcribing | PillState::CleaningUp);
        let text = match state {
            PillState::Pasted => "Pasted",
            PillState::Error(t) => t,
            _ => "",
        };
        unsafe { self.label.setString(Some(&NSString::from_str(text))) };
        let text_size = self.label.preferredFrameSize();
        let text_w = text_size.width;

        let mark_w = match state {
            PillState::Recording | PillState::Error(_) => 7.0,
            PillState::HandsFree => 8.0,
            _ => 0.0,
        };
        let content_w = match state {
            PillState::Recording | PillState::HandsFree => mark_w + GAP + BARS_W,
            PillState::Transcribing | PillState::CleaningUp => BARS_W,
            PillState::Pasted => 16.0 + GAP + text_w,
            PillState::Error(_) => mark_w + GAP + text_w,
            PillState::Hidden => 0.0,
        };
        let width = (content_w + 2.0 * PAD).max(MIN_WIDTH).ceil();
        let size = CGSize::new(width, HEIGHT);
        let mid = HEIGHT / 2.0;
        let mut x = (width - content_w) / 2.0;

        let pill = CGRect::new(CGPoint::new(MARGIN, MARGIN), size);
        self.body.setFrame(pill);
        self.face_view.setFrame(CGRect::new(CGPoint::ZERO, size));
        self.face
            .setBackgroundColor(Some(&color(if error { ERROR_TINT } else { TINT })));
        let inset = CGRect::new(
            CGPoint::new(MARGIN + 8.0, MARGIN + 8.0),
            CGSize::new(width - 16.0, HEIGHT - 16.0),
        );
        let shadow_path = unsafe { CGPath::with_rounded_rect(inset, 10.0, 10.0, std::ptr::null()) };
        self.shadow.setShadowPath(Some(&shadow_path));

        self.halo.setHidden(mark_w == 0.0);
        if mark_w > 0.0 {
            let square = state == PillState::HandsFree;
            let (core_r, halo_r) = if square { (2.0, 5.0) } else { (3.5, 6.5) };
            let halo_size = mark_w + 6.0;
            self.halo.setBounds(CGRect::new(
                CGPoint::ZERO,
                CGSize::new(halo_size, halo_size),
            ));
            self.halo.setCornerRadius(halo_r);
            self.halo.setPosition(CGPoint::new(x + mark_w / 2.0, mid));
            self.halo
                .setBackgroundColor(Some(&color(if error { CLEAR } else { HALO })));
            self.mark
                .setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(mark_w, mark_w)));
            self.mark.setCornerRadius(core_r);
            self.mark
                .setPosition(CGPoint::new(halo_size / 2.0, halo_size / 2.0));
            self.mark.setBackgroundColor(Some(&color(STRAWBERRY)));
            x += mark_w + GAP;
        }

        let show_bars = state.is_recording() || busy;
        let bar_color = color(if state == PillState::CleaningUp {
            SHIMMER_SLOW
        } else {
            BLUEBERRY
        });
        self.stop_shimmer();
        for (i, bar) in self.bars.iter().enumerate() {
            bar.setHidden(!show_bars);
            bar.setBackgroundColor(Some(&bar_color));
            bar.setPosition(CGPoint::new(
                x + i as f64 * (BAR_W + BAR_GAP) + BAR_W / 2.0,
                mid,
            ));
            if state.is_recording() {
                let h = bar_height(self.smoothed.get(), i, 1.0);
                bar.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(BAR_W, h)));
            }
        }
        if busy {
            self.start_shimmer(if state == PillState::CleaningUp {
                1.4
            } else {
                0.9
            });
        }

        self.check.setHidden(state != PillState::Pasted);
        if state == PillState::Pasted {
            self.check.setPosition(CGPoint::new(x + 8.0, mid));
            x += 16.0 + GAP;
        }

        self.label.setHidden(text.is_empty());
        if !text.is_empty() {
            self.label
                .setForegroundColor(Some(&color(if error { ERROR_TEXT } else { MATCHA })));
            self.label.setFrame(CGRect::new(
                // 2 pt above the line box's center puts the glyphs where the mock's `line-height: 1` does, and a whole point keeps them sharp at 1x.
                CGPoint::new(x, ((HEIGHT - text_size.height) / 2.0 + 2.0).round()),
                text_size,
            ));
        }
    }

    fn place(&self) {
        let (cx, bottom) = self.anchor.get();
        let w = self.width();
        let frame = CGRect::new(
            CGPoint::new((cx - w / 2.0).round() - MARGIN, bottom - MARGIN),
            CGSize::new(w + 2.0 * MARGIN, HEIGHT + 2.0 * MARGIN),
        );
        self.panel.setFrame_display(frame, true);
        let scale = self.panel.backingScaleFactor();
        self.check.setContentsScale(scale);
        self.label.setContentsScale(scale);
    }

    /// Animates the window's alpha, which AppKit owns, from wherever it is now.
    fn fade(&self, to: f64) {
        NSAnimationContext::beginGrouping();
        let context = NSAnimationContext::currentContext();
        context.setDuration(FADE);
        context.setTimingFunction(Some(&CAMediaTimingFunction::functionWithName(unsafe {
            kCAMediaTimingFunctionEaseOut
        })));
        self.panel.animator().setAlphaValue(to);
        NSAnimationContext::endGrouping();
    }

    /// The mock's `shimmer` keyframes: each bar rises to 11 pt and back, 60 ms after its left neighbor.
    fn start_shimmer(&self, period: f64) {
        let ease =
            CAMediaTimingFunction::functionWithName(unsafe { kCAMediaTimingFunctionEaseInEaseOut });
        let nums = |v: &[f64]| {
            NSArray::from_retained_slice(
                &v.iter().map(|&n| NSNumber::new_f64(n)).collect::<Vec<_>>(),
            )
        };
        let key_times = nums(&[0.0, 0.4, 1.0]);
        let timing = NSArray::from_retained_slice(&[ease.clone(), ease]);
        let now = CACurrentMediaTime();
        for (i, bar) in self.bars.iter().enumerate() {
            bar.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(BAR_W, 4.0)));
            let frames = |key: &str, values: &[f64]| {
                let a = CAKeyframeAnimation::animationWithKeyPath(Some(&NSString::from_str(key)));
                let values: Vec<Retained<AnyObject>> = values
                    .iter()
                    .map(|&n| {
                        Retained::into_super(Retained::into_super(Retained::into_super(
                            NSNumber::new_f64(n),
                        )))
                    })
                    .collect();
                unsafe { a.setValues(Some(&NSArray::from_retained_slice(&values))) };
                a.setKeyTimes(Some(&key_times));
                a.setTimingFunctions(Some(&timing));
                // An animation in a group otherwise runs for the default 0.25 s of each period.
                a.setDuration(period);
                a
            };
            let height = frames("bounds.size.height", &[4.0, 11.0, 4.0]);
            let opacity = frames("opacity", &[0.55, 1.0, 0.55]);
            let group = CAAnimationGroup::new();
            group.setAnimations(Some(&NSArray::from_retained_slice(&[
                Retained::into_super(Retained::into_super(height)),
                Retained::into_super(Retained::into_super(opacity)),
            ])));
            group.setDuration(period);
            group.setRepeatCount(f32::INFINITY);
            group.setBeginTime(now + i as f64 * 0.06);
            bar.addAnimation_forKey(&group, Some(&NSString::from_str("shimmer")));
        }
    }

    fn stop_shimmer(&self) {
        for bar in &self.bars {
            bar.removeAllAnimations();
        }
    }

    fn tick(&self) {
        if !self.state.get().is_recording() {
            self.ticking.set(false);
            return;
        }
        let target = f32::from_bits(LEVEL.swap(0, Ordering::Relaxed));
        let level = smooth(self.smoothed.get(), target);
        self.smoothed.set(level);
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        for (i, bar) in self.bars.iter().enumerate() {
            let h = bar_height(level, i, self.jitter());
            bar.setBounds(CGRect::new(CGPoint::ZERO, CGSize::new(BAR_W, h)));
        }
        CATransaction::commit();
        after(TICK, |p| p.tick());
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
}

#[link(name = "CoreText", kind = "framework")]
extern "C" {
    fn CTFontManagerCreateFontDescriptorFromData(data: &NSData) -> *mut NSFontDescriptor;
    fn CTFontCreateWithFontDescriptor(
        descriptor: &NSFontDescriptor,
        size: f64,
        matrix: *const c_void,
    ) -> *mut NSFont;
}

/// Manrope SemiBold, the app's own face, as in the mocks. Falls back to the system font.
fn manrope(size: f64) -> Retained<NSFont> {
    static WOFF: &[u8] = include_bytes!("../assets/manrope-latin-600-normal.woff");
    // CTFontDescriptor and CTFont are toll-free bridged to NSFontDescriptor and NSFont, and both functions return +1.
    let font = unsafe {
        Retained::from_raw(CTFontManagerCreateFontDescriptorFromData(
            &NSData::with_bytes(WOFF),
        ))
        .and_then(|d| {
            Retained::from_raw(CTFontCreateWithFontDescriptor(&d, size, std::ptr::null()))
        })
    };
    font.unwrap_or_else(|| {
        tracing::warn!(
            "could not load the bundled Manrope; the pill falls back to the system font"
        );
        NSFont::systemFontOfSize_weight(size, unsafe { NSFontWeightSemibold })
    })
}

fn anchor(mtm: MainThreadMarker) -> (f64, f64) {
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| {
            let f = s.frame();
            mouse.x >= f.origin.x
                && mouse.x < f.origin.x + f.size.width
                && mouse.y >= f.origin.y
                && mouse.y < f.origin.y + f.size.height
        })
        .or_else(|| NSScreen::mainScreen(mtm));
    let Some(screen) = screen else {
        return (0.0, LIFT);
    };
    let v = screen.visibleFrame();
    (v.origin.x + v.size.width / 2.0, v.origin.y + LIFT)
}
