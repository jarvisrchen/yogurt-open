//! What the pill shows, and the mic-level math behind its bars.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PillState {
    Hidden,
    Recording,
    /// Recording after a tap, so the next press finishes the clip.
    HandsFree,
    Transcribing,
    CleaningUp,
    /// Shows for [`PASTED_FOR`], then hides.
    Pasted,
    /// One line such as "Mic unavailable", shown for [`ERROR_FOR`], then hides.
    Error(&'static str),
}

impl PillState {
    pub fn is_recording(self) -> bool {
        matches!(self, Self::Recording | Self::HandsFree)
    }
}

pub const PASTED_FOR: std::time::Duration = std::time::Duration::from_millis(600);
pub const ERROR_FOR: std::time::Duration = std::time::Duration::from_secs(2);

/// Each bar's share of the level, tallest in the middle.
pub const BAR_SHAPE: [f32; 11] = [0.35, 0.55, 0.75, 0.9, 1.0, 0.95, 1.0, 0.9, 0.75, 0.55, 0.35];

/// dBFS that maps to an empty and a full bar. Speech peaks sit around -30 to -10 dBFS.
const FLOOR_DB: f32 = -50.0;
const CEIL_DB: f32 = -10.0;

/// Maps a frame's peak amplitude (0..1) to a bar level (0..1) on a dB scale, so quiet speech still moves the bars.
pub fn level_from_peak(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() - FLOOR_DB) / (CEIL_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

/// One 30 Hz step toward `target`: fast attack so the bars jump with speech, slow release so they ease down.
pub fn smooth(current: f32, target: f32) -> f32 {
    let k = if target > current { 0.6 } else { 0.15 };
    current + (target - current) * k
}

/// A bar's height in points: 4 at rest, up to 20, with `jitter` (0..1) so the bars do not move in lockstep.
pub fn bar_height(level: f32, bar: usize, jitter: f32) -> f64 {
    f64::from(4.0 + level * 16.0 * BAR_SHAPE[bar % BAR_SHAPE.len()] * (0.7 + jitter * 0.3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_maps_peaks_on_a_db_scale() {
        assert_eq!(level_from_peak(0.0), 0.0);
        assert_eq!(level_from_peak(0.001), 0.0);
        assert!(
            (level_from_peak(0.0316) - 0.5).abs() < 0.01,
            "-30 dBFS is half"
        );
        assert_eq!(level_from_peak(1.0), 1.0);
    }

    #[test]
    fn smoothing_attacks_fast_and_releases_slowly() {
        assert!((smooth(0.0, 1.0) - 0.6).abs() < 1e-6);
        assert!((smooth(1.0, 0.0) - 0.85).abs() < 1e-6);
    }

    #[test]
    fn bars_rest_at_4_and_peak_at_20() {
        assert_eq!(bar_height(0.0, 5, 1.0), 4.0);
        assert_eq!(bar_height(1.0, 4, 1.0), 20.0);
        assert!(bar_height(1.0, 0, 0.0) < bar_height(1.0, 4, 0.0));
    }
}
