//! What the meeting bubble shows, apart from AppKit: the two-channel bar math, the timer text, and when a channel counts as silent.

use std::time::Duration;

#[cfg(target_os = "macos")]
pub use crate::bubble_panel::{
    card, hide, level, on_event, open_url, paused_frame, remember_front, saved, show, status,
};

/// Bars on each side of the center line, as in the mock's pill.
pub const BARS: usize = 7;

/// A bar half's height in points at full level, above its 1.5 pt floor.
pub const AMP: f64 = 9.0;

/// System audio with no peak above [`AUDIBLE_PEAK`] for this long turns its bars honey.
pub const SILENT_AFTER: Duration = Duration::from_secs(40);

/// The peak below which a frame counts as silence, for the honey bars and the meeting's silence auto-stop.
pub const AUDIBLE_PEAK: f32 = 0.02;

/// Transcript lines the card keeps, newest last.
pub const CARD_LINES: usize = 3;

/// What the hover card shows above the pill.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Card {
    pub title: String,
    /// At most [`CARD_LINES`], newest last.
    pub lines: Vec<Line>,
    /// How many lines [`merge`] has added, so the panel can tell a new line from an edit to the last one.
    pub added: u64,
    /// `None` without Yogurt Pro, which leaves the recap out and the edge plain.
    pub pro: Option<ProCard>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The mic's line rather than the other side's.
    pub me: bool,
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProCard {
    /// The live recap's two newest bullets, oldest first. Empty while the recap is off or has none yet.
    pub bullets: Vec<String>,
    /// Unix ms of the recap's last update.
    pub updated_at_ms: Option<u64>,
}

impl Card {
    /// Adds one transcript event as the page's dock does: a partial replaces its channel's unfinished line, anything else is a new line.
    pub fn merge(&mut self, me: bool, text: &str, done: bool) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let line = Line {
            me,
            text: text.to_string(),
            done,
        };
        match self.lines.iter().rposition(|l| l.me == me) {
            Some(i) if !self.lines[i].done => self.lines[i] = line,
            _ => {
                self.lines.push(line);
                self.added += 1;
                let extra = self.lines.len().saturating_sub(CARD_LINES);
                self.lines.drain(..extra);
            }
        }
    }
}

/// The recap label's age, as the mock writes it.
pub fn ago(elapsed: Duration) -> String {
    match elapsed.as_secs() / 60 {
        0 => "Just now".to_string(),
        m if m < 60 => format!("{m} min ago"),
        m => format!("{} h ago", m / 60),
    }
}

/// What the bubble's panel reports back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// The pill was clicked. `raised` says whether the app that last showed a yogurt page came forward.
    Return { raised: bool },
    /// The close button hid the bubble until a Yogurt page has focus again.
    Dismiss,
    /// The pause button asks for the recording to be paused from this Unix ms, or resumed (`None`).
    Pause(Option<u64>),
    /// The mute button asks for the mic to be muted (`true`) or unmuted.
    Mute(bool),
    /// A drag ended with the pill's bottom-right corner here, in screen points.
    Moved { right: f64, bottom: f64 },
}

/// Bar `i` of [`BARS`]'s share of the level, tallest in the middle.
pub fn bar_shape(i: usize) -> f32 {
    0.35 + 0.65 * (std::f32::consts::PI * (i as f32 + 0.5) / BARS as f32).sin()
}

/// One bar half's height in points: 1.5 at rest, up to 10.5, with `jitter` (0..1) so the bars do not move in lockstep.
pub fn half_height(level: f32, i: usize, jitter: f32) -> f64 {
    1.5 + f64::from(level * bar_shape(i) * (0.7 + jitter * 0.3)) * AMP
}

/// Keeps a pill of `size` (width, height) whose bottom-right corner is `corner` inside `frame` (x, y, width, height), 6 points from its edges as the mock's drag is.
pub fn clamp_corner(
    corner: (f64, f64),
    size: (f64, f64),
    frame: (f64, f64, f64, f64),
) -> (f64, f64) {
    const EDGE: f64 = 6.0;
    let (x, y, w, h) = frame;
    let right = corner.0.min(x + w - EDGE).max(x + size.0 + EDGE);
    let bottom = corner.1.max(y + EDGE).min(y + h - size.1 - EDGE);
    (right, bottom)
}

/// The timer's text: whole minutes, then seconds, as the recording pill shows them.
pub fn clock(elapsed: Duration) -> String {
    let s = elapsed.as_secs();
    format!("{:02}:{:02}", s / 60, s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_middle_bar_is_tallest_and_the_ends_match() {
        assert!((bar_shape(3) - 1.0).abs() < 1e-6);
        assert!((bar_shape(0) - bar_shape(6)).abs() < 1e-6);
        assert!(bar_shape(0) < bar_shape(1));
    }

    #[test]
    fn halves_rest_at_one_and_a_half_points_and_peak_at_ten_and_a_half() {
        assert_eq!(half_height(0.0, 3, 1.0), 1.5);
        assert!((half_height(1.0, 3, 1.0) - 10.5).abs() < 1e-6);
    }

    #[test]
    fn a_corner_is_kept_six_points_inside_the_screen() {
        let screen = (0.0, 0.0, 1000.0, 800.0);
        let size = (120.0, 38.0);
        assert_eq!(clamp_corner((500.0, 300.0), size, screen), (500.0, 300.0));
        assert_eq!(clamp_corner((1200.0, -50.0), size, screen), (994.0, 6.0));
        assert_eq!(clamp_corner((10.0, 900.0), size, screen), (126.0, 756.0));
    }

    #[test]
    fn a_partial_replaces_its_channels_open_line_and_the_card_keeps_three() {
        let mut c = Card::default();
        c.merge(false, "If we cut the", false);
        c.merge(false, "If we cut the map tutorial", true);
        assert_eq!(c.lines.len(), 1);
        assert_eq!(c.added, 1);
        c.merge(true, "Two screens", false);
        c.merge(false, "Yes, but", false);
        c.merge(true, "Two screens works.", true);
        assert_eq!(
            c.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            [
                "If we cut the map tutorial",
                "Two screens works.",
                "Yes, but"
            ],
        );
        c.merge(true, "Good.", true);
        assert_eq!(c.lines.len(), CARD_LINES);
        assert_eq!(c.lines[0].text, "Two screens works.");
        assert_eq!(c.added, 4);
        c.merge(true, "   ", true);
        assert_eq!(c.added, 4, "blank text adds nothing");
        c.merge(true, "Good.", true);
        assert_eq!(c.added, 5, "the same words said twice show twice");
    }

    #[test]
    fn the_recap_age_reads_as_the_mock_does() {
        assert_eq!(ago(Duration::from_secs(20)), "Just now");
        assert_eq!(ago(Duration::from_secs(61)), "1 min ago");
        assert_eq!(ago(Duration::from_secs(2 * 3600 + 5)), "2 h ago");
    }

    #[test]
    fn the_clock_counts_minutes_past_an_hour() {
        assert_eq!(clock(Duration::from_secs(0)), "00:00");
        assert_eq!(clock(Duration::from_secs(24 * 60 + 13)), "24:13");
        assert_eq!(clock(Duration::from_secs(75 * 60)), "75:00");
    }
}
