//! The ranges a stats page covers and the buckets its chart draws.

use chrono::{Datelike, Days, NaiveDate};
use serde::{Deserialize, Serialize};

/// Days in the 30-day range.
pub const CHART_DAYS: u64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Range {
    #[serde(rename = "today")]
    Today,
    #[serde(rename = "7d")]
    Week,
    #[default]
    #[serde(rename = "30d")]
    Month,
    #[serde(rename = "all")]
    All,
}

impl Range {
    /// The range's first local day, or `None` for all time.
    pub fn start(self, today: NaiveDate) -> Option<NaiveDate> {
        let days = match self {
            Range::Today => 1,
            Range::Week => 7,
            Range::Month => CHART_DAYS,
            Range::All => return None,
        };
        Some(today - Days::new(days - 1))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    Hour,
    Day,
    Week,
    Month,
}

pub fn monday(d: NaiveDate) -> NaiveDate {
    d - Days::new(d.weekday().num_days_from_monday() as u64)
}

/// The local hour `00` to `23`, the day or the week's Monday as `YYYY-MM-DD`, or the month as `YYYY-MM`.
pub fn key(unit: Unit, d: NaiveDate, hour: u32) -> String {
    match unit {
        Unit::Hour => format!("{hour:02}"),
        Unit::Day => d.to_string(),
        Unit::Week => monday(d).to_string(),
        Unit::Month => d.format("%Y-%m").to_string(),
    }
}

/// A chart's unit and its bucket keys, oldest first: today by the hour, 7 and 30 days by the day, and all time by the week, or by the day for its first two weeks and by the month past a year.
/// `start` is the range's first day. All time has none and starts at `first`, the first day with anything to count.
pub fn axis(
    start: Option<NaiveDate>,
    today: NaiveDate,
    first: Option<NaiveDate>,
) -> (Unit, Vec<String>) {
    let from = start.or(first).unwrap_or(today).min(today);
    let unit = match start {
        Some(d) if d == today => Unit::Hour,
        Some(_) => Unit::Day,
        // Two weeks of history would be one or two weekly bars.
        None if (today - from).num_days() < 14 => Unit::Day,
        None if (today - from).num_days() <= 366 => Unit::Week,
        None => Unit::Month,
    };
    let mut keys: Vec<String> = match unit {
        Unit::Hour => (0..24).map(|h| key(unit, today, h)).collect(),
        Unit::Week => monday(from)
            .iter_weeks()
            .take_while(|d| *d <= today)
            .map(|d| key(unit, d, 0))
            .collect(),
        Unit::Day | Unit::Month => from
            .iter_days()
            .take_while(|d| *d <= today)
            .map(|d| key(unit, d, 0))
            .collect(),
    };
    keys.dedup();
    (unit, keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    #[test]
    fn the_axis_follows_the_range() {
        let today = day("2026-11-13");
        let (unit, keys) = axis(Range::Today.start(today), today, None);
        assert_eq!(
            (unit, keys.len(), keys[23].as_str()),
            (Unit::Hour, 24, "23")
        );

        let (unit, keys) = axis(Range::Week.start(today), today, None);
        assert_eq!(unit, Unit::Day);
        assert_eq!(
            (keys[0].as_str(), keys[6].as_str(), keys.len()),
            ("2026-11-07", "2026-11-13", 7)
        );

        assert_eq!(axis(Range::Month.start(today), today, None).1.len(), 30);

        let (unit, keys) = axis(None, today, Some(day("2026-10-28")));
        assert_eq!(unit, Unit::Week);
        assert_eq!(keys, ["2026-10-26", "2026-11-02", "2026-11-09"]);

        let (unit, keys) = axis(None, today, Some(day("2025-09-30")));
        assert_eq!(unit, Unit::Month);
        assert_eq!(
            (keys[0].as_str(), keys.last().unwrap().as_str(), keys.len()),
            ("2025-09", "2026-11", 15)
        );

        let (unit, keys) = axis(None, today, Some(day("2026-11-10")));
        assert_eq!(
            (unit, keys.len()),
            (Unit::Day, 4),
            "a short history charts its days"
        );
        assert_eq!(
            axis(None, today, None),
            (Unit::Day, vec!["2026-11-13".to_string()])
        );
    }
}
