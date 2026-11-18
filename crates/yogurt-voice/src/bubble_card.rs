//! The hover card's contents, drawn with AppKit's text layout into images: everything above the transcript, and one per transcript line, which the panel moves on its own.
//! Sizes and colors are the mock's `.upin`, `.pk-head`, `.warn`, `.pk-lab`, `.recap` and `.ln`.

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::AnyThread;
use objc2_app_kit::{
    NSAttributedStringNSExtendedStringDrawing, NSBezierPath, NSColor, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSImage, NSKernAttributeName, NSLineBreakMode,
    NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSStringDrawingOptions,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{
    NSAttributedString, NSDictionary, NSMutableAttributedString, NSNumber, NSString,
};

use super::Palette;
use crate::bubble::Card;
use crate::panel::{bundled_font, Rgba};

pub(super) const CARD_W: f64 = 300.0;
pub(super) const SIDE: f64 = 13.0;
pub(super) const INNER: f64 = CARD_W - 2.0 * SIDE;
/// The mock's margin above each block, and below the last one.
const STEP: f64 = 10.0;
pub(super) const LAST: f64 = 2.0;
const BODY_LINE: f64 = 12.5 * 1.45;
pub(super) const WINDOW_H: f64 = 8.0 * BODY_LINE;
/// How far down from the window's top edge the older words fade in, once they run past it.
pub(super) const FADE: f64 = 18.0;
pub(super) const LINE_GAP: f64 = 4.0;

/// Which warning a box carries, for the icon layer laid over it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Warn {
    Silent,
    Muted,
}

pub(super) struct Painted {
    pub top: Retained<NSImage>,
    pub top_h: f64,
    /// The transcript, oldest first: each line's key, its image, and its full height.
    /// A line taller than the window keeps only its bottom [`WINDOW_H`] in the image, which is all the window can show.
    pub lines: Vec<(u64, Retained<NSImage>, f64)>,
    /// Each warning's icon center, from the top-left of the top image.
    pub icons: Vec<(Warn, CGPoint)>,
}

pub(super) struct Fonts {
    title: Retained<NSFont>,
    body: Retained<NSFont>,
    bold: Retained<NSFont>,
    warn: Retained<NSFont>,
    label: Retained<NSFont>,
}

impl Fonts {
    pub(super) fn new(mono_label: Retained<NSFont>) -> Self {
        static W500: &[u8] = include_bytes!("../assets/manrope-latin-500-normal.woff");
        static W700: &[u8] = include_bytes!("../assets/manrope-latin-700-normal.woff");
        static W800: &[u8] = include_bytes!("../assets/manrope-latin-800-normal.woff");
        Self {
            title: bundled_font(W800, 13.5, "Manrope 800"),
            body: bundled_font(W500, 12.5, "Manrope 500"),
            bold: bundled_font(W700, 12.5, "Manrope 700"),
            warn: bundled_font(W500, 12.0, "Manrope 500"),
            label: mono_label,
        }
    }
}

fn ns(c: Rgba) -> Retained<NSColor> {
    NSColor::colorWithSRGBRed_green_blue_alpha(c.0, c.1, c.2, c.3)
}

/// A run of text with a fixed line height, the way CSS's `line-height` sets it.
fn run(
    text: &str,
    font: &NSFont,
    c: Rgba,
    kern: f64,
    line: f64,
) -> Retained<NSMutableAttributedString> {
    let style = NSMutableParagraphStyle::new();
    style.setMinimumLineHeight(line);
    style.setMaximumLineHeight(line);
    style.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
    let color = ns(c);
    let kern = NSNumber::new_f64(kern);
    let keys = unsafe {
        [
            NSFontAttributeName,
            NSForegroundColorAttributeName,
            NSKernAttributeName,
            NSParagraphStyleAttributeName,
        ]
    };
    let values: [&AnyObject; 4] = [font, &color, &kern, &style];
    let attrs = NSDictionary::from_slices(&keys, &values);
    unsafe {
        NSMutableAttributedString::initWithString_attributes(
            NSMutableAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&attrs),
        )
    }
}

fn options() -> NSStringDrawingOptions {
    NSStringDrawingOptions::UsesLineFragmentOrigin
        | NSStringDrawingOptions::TruncatesLastVisibleLine
}

fn height(text: &NSAttributedString, width: f64) -> f64 {
    text.boundingRectWithSize_options_context(CGSize::new(width, 1e6), options(), None)
        .size
        .height
        .ceil()
}

/// The height `text` takes in `width`, at most `max_lines` lines of `line` points.
fn measure(text: &NSAttributedString, width: f64, line: f64, max_lines: usize) -> f64 {
    height(text, width).min(line * max_lines as f64).max(line)
}

enum Ink {
    Text(Retained<NSMutableAttributedString>, CGRect),
    Rounded(CGRect, f64, Rgba),
}

/// Draws `inks`, laid out top-down, into an image `width` by `height`.
fn image(inks: Vec<Ink>, width: f64, height: f64) -> Retained<NSImage> {
    let draw = RcBlock::new(move |_rect: CGRect| -> Bool {
        for ink in &inks {
            match ink {
                Ink::Text(text, r) => text.drawWithRect_options_context(*r, options(), None),
                Ink::Rounded(r, radius, c) => {
                    ns(*c).setFill();
                    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(*r, *radius, *radius)
                        .fill();
                }
            }
        }
        Bool::YES
    });
    NSImage::imageWithSize_flipped_drawingHandler(CGSize::new(width, height), true, &draw)
}

/// Lays out the card for its current contents and the live silence and mute warnings.
pub(super) fn paint(card: &Card, warns: &[Warn], p: &Palette, f: &Fonts) -> Painted {
    let mut inks = Vec::new();
    let mut icons = Vec::new();
    let mut y = STEP;

    // `.pk-head`: the title, one line.
    let head_line = 13.5 * 1.45;
    let room = INNER;
    let title = run(&card.title, &f.title, p.ink, 0.0, head_line);
    let title_w = title
        .boundingRectWithSize_options_context(CGSize::new(1e4, head_line), options(), None)
        .size
        .width
        .ceil()
        .min(room);
    inks.push(Ink::Text(
        title,
        CGRect::new(CGPoint::new(SIDE, y), CGSize::new(title_w, head_line)),
    ));
    y += head_line + STEP;

    // `.warn`: a honey box with its icon, 8 pt in, and its text 7 pt after the icon.
    for &w in warns {
        let text = match w {
            Warn::Silent => "No system audio for 40 s. Check the output device.",
            Warn::Muted => "Your mic is muted. The other side is still transcribed.",
        };
        let line = 12.0 * 1.45;
        let t = run(text, &f.warn, p.ink, 0.0, line);
        let tw = INNER - 8.0 - 14.0 - 7.0 - 8.0;
        let h = measure(&t, tw, line, 4);
        inks.push(Ink::Rounded(
            CGRect::new(CGPoint::new(SIDE, y), CGSize::new(INNER, h + 12.0)),
            9.0,
            p.warn_bg,
        ));
        icons.push((w, CGPoint::new(SIDE + 8.0 + 7.0, y + 6.0 + 1.0 + 7.0)));
        inks.push(Ink::Text(
            t,
            CGRect::new(
                CGPoint::new(SIDE + 8.0 + 14.0 + 7.0, y + 6.0),
                CGSize::new(tw, h),
            ),
        ));
        y += h + 12.0 + STEP;
    }

    // `.pk-lab` and `.recap`: Pro's two newest bullets under a mono label with the update's age.
    let label_line = 10.5 * 1.45;
    let label = |text: &str| {
        run(
            &text.to_uppercase(),
            &f.label,
            p.muted,
            10.5 * 0.05,
            label_line,
        )
    };

    inks.push(Ink::Text(
        label("Transcript"),
        CGRect::new(CGPoint::new(SIDE, y), CGSize::new(INNER, label_line)),
    ));
    y += label_line + 4.0;
    let top = image(inks, CARD_W, y);

    // `.ln`: "Me" in the mic's color or "Them" muted, then the words, faded while the line is unfinished as the page's dock draws partials.
    let line_image = |t: Retained<NSMutableAttributedString>, h: f64| {
        let shown = h.min(WINDOW_H);
        let at = CGRect::new(CGPoint::new(0.0, shown - h), CGSize::new(INNER, h));
        image(vec![Ink::Text(t, at)], INNER, shown)
    };
    let lines = if card.lines.is_empty() {
        let t = run(
            "Waiting for audio\u{2026}",
            &f.body,
            p.muted,
            0.0,
            BODY_LINE,
        );
        vec![(0, line_image(t, BODY_LINE), BODY_LINE)]
    } else {
        // Key 0 is the waiting line's.
        let first = card.first_key().max(1);
        card.lines
            .iter()
            .zip(first..)
            .map(|(line, key)| {
                let (who, c) = if line.me {
                    ("Me", p.mic)
                } else {
                    ("Them", p.muted)
                };
                let words = if line.done {
                    p.ink
                } else {
                    (p.ink.0, p.ink.1, p.ink.2, p.ink.3 * 0.7)
                };
                let t = run(who, &f.bold, c, 0.0, BODY_LINE);
                t.appendAttributedString(&run(
                    &format!("\u{2006}\u{2006}{}", line.text),
                    &f.body,
                    words,
                    0.0,
                    BODY_LINE,
                ));
                let h = height(&t, INNER).max(BODY_LINE);
                (key, line_image(t, h), h)
            })
            .collect()
    };
    Painted {
        top,
        top_h: y,
        lines,
        icons,
    }
}
