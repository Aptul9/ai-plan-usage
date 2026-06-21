//! Pace-aware color logic for usage indicators.
//!
//! See `docs/superpowers/specs/2026-05-28-dynamic-color-schedule-design.md`.

use chrono::{DateTime, Utc};

pub const HEX_GREEN: u32 = 0x22a06b;
pub const HEX_AMBER: u32 = 0xd97706;
pub const HEX_RED: u32 = 0xdc2626;
pub const HEX_GRAY: u32 = 0x888888;

/// Window total length in seconds.
pub const SESSION_5H_SECS: i64 = 5 * 3600;
pub const WEEKLY_7D_SECS: i64 = 168 * 3600;

#[derive(Debug, Clone, Copy)]
pub struct ColorParams {
    pub a_start: f64,
    pub a_end: f64,
    pub r_start: f64,
    pub r_end: f64,
    /// Optional absolute red floor. If `used_pct >= red_floor`, color is red
    /// regardless of pace/delta. Used on the session window to catch
    /// near-depletion late in the window.
    pub red_floor: Option<f64>,
}

pub const PARAMS_SESSION: ColorParams = ColorParams {
    a_start: 25.0,
    a_end: 3.0,
    r_start: 30.0,
    r_end: 5.0,
    red_floor: Some(95.0),
};

pub const PARAMS_WEEKLY: ColorParams = ColorParams {
    a_start: 20.0,
    a_end: 2.0,
    r_start: 28.0,
    r_end: 3.0,
    red_floor: None,
};

/// Linear interpolation between `start` (pace=0) and `end` (pace=100).
pub fn threshold(pace: f64, start: f64, end: f64) -> f64 {
    start + (end - start) * (pace / 100.0)
}

/// Static fallback used when `resets_at` is missing.
pub fn color_for_pct_static(used_pct: Option<f64>) -> u32 {
    let Some(pct) = used_pct else {
        return HEX_GRAY;
    };
    if pct >= 95.0 {
        HEX_RED
    } else if pct >= 80.0 {
        HEX_AMBER
    } else {
        HEX_GREEN
    }
}

/// Compute the indicator color for a given usage block.
///
/// `now_secs` and `resets_at_secs` are absolute UNIX timestamps in seconds.
/// `total_window_secs` is the full window length (e.g. 5h, 7d).
pub fn color_for_window(
    used_pct: Option<f64>,
    resets_at_secs: Option<i64>,
    now_secs: i64,
    total_window_secs: i64,
    params: ColorParams,
) -> u32 {
    let Some(used) = used_pct else {
        return HEX_GRAY;
    };
    // Absolute red floor applies even before pace/delta (e.g. session 95%).
    if let Some(floor) = params.red_floor {
        if used >= floor {
            return HEX_RED;
        }
    }
    let Some(resets) = resets_at_secs else {
        return color_for_pct_static(Some(used));
    };
    if total_window_secs <= 0 {
        return color_for_pct_static(Some(used));
    }

    let remaining = (resets - now_secs).max(0).min(total_window_secs);
    let elapsed = total_window_secs - remaining;
    let pace = (elapsed as f64 / total_window_secs as f64) * 100.0;
    let delta = used - pace;

    let amber_thr = threshold(pace, params.a_start, params.a_end);
    let red_thr = threshold(pace, params.r_start, params.r_end);

    if delta >= red_thr {
        HEX_RED
    } else if delta >= amber_thr {
        HEX_AMBER
    } else {
        HEX_GREEN
    }
}

/// Convenience: parse an ISO 8601 string into a UNIX timestamp.
pub fn parse_iso_secs(iso: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|d| d.with_timezone(&Utc).timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_endpoints() {
        assert!((threshold(0.0, 25.0, 3.0) - 25.0).abs() < 1e-9);
        assert!((threshold(100.0, 25.0, 3.0) - 3.0).abs() < 1e-9);
        assert!((threshold(50.0, 25.0, 3.0) - 14.0).abs() < 1e-9);
    }

    fn run(used: f64, pace_pct: f64, params: ColorParams, total: i64) -> u32 {
        let now = 1_700_000_000_i64;
        let elapsed = (pace_pct / 100.0 * total as f64) as i64;
        let resets = now + (total - elapsed);
        color_for_window(Some(used), Some(resets), now, total, params)
    }

    #[test]
    fn session_walkthrough() {
        let p = PARAMS_SESSION;
        let t = SESSION_5H_SECS;
        // Hour 1 (pace 20), used 25 -> green
        assert_eq!(run(25.0, 20.0, p, t), HEX_GREEN);
        // Hour 1, used 50 (delta +30) -> red
        assert_eq!(run(50.0, 20.0, p, t), HEX_RED);
        // Hour 4 (pace 80), used 90 (delta +10) -> red
        assert_eq!(run(90.0, 80.0, p, t), HEX_RED);
        // Hour 4.5 (pace 90), used 80 (delta -10) -> green
        assert_eq!(run(80.0, 90.0, p, t), HEX_GREEN);
        // Hour 4 (pace 80), used 88 (delta +8, amber_thr 7.4, red_thr 10) -> amber
        assert_eq!(run(88.0, 80.0, p, t), HEX_AMBER);
        // Hour 4.5, used 95 (hit floor) -> red
        assert_eq!(run(95.0, 90.0, p, t), HEX_RED);
        // Hour 4.5, used 98 -> red (floor fires)
        assert_eq!(run(98.0, 90.0, p, t), HEX_RED);
    }

    #[test]
    fn session_red_floor_applies_early_window() {
        // Even at pace 10, used 95 -> red (floor wins).
        let p = PARAMS_SESSION;
        let t = SESSION_5H_SECS;
        assert_eq!(run(95.0, 10.0, p, t), HEX_RED);
        assert_eq!(run(99.0, 10.0, p, t), HEX_RED);
    }

    #[test]
    fn weekly_no_red_floor() {
        // Weekly: used 99 at day 1 (pace 14) -> delta +85, way above red_thr -> red by delta.
        // Used 95 at day 6 (pace 86) -> delta +9, above red_thr 6.08 -> red by delta (no abs floor).
        // Confirms no abs floor on weekly (color comes from delta, not floor).
        let p = PARAMS_WEEKLY;
        let t = WEEKLY_7D_SECS;
        assert_eq!(run(95.0, 86.0, p, t), HEX_RED);
    }

    #[test]
    fn weekly_walkthrough() {
        let p = PARAMS_WEEKLY;
        let t = WEEKLY_7D_SECS;
        // Day 1 (pace 14), used 30 (delta +16) -> amber_thr 17.48 -> green
        assert_eq!(run(30.0, 14.0, p, t), HEX_GREEN);
        // Day 1, used 45 (delta +31) -> red_thr 24.5 -> red
        assert_eq!(run(45.0, 14.0, p, t), HEX_RED);
        // Day 4 (pace 57), used 70 (delta +13) -> amber_thr 9.74, red_thr 13.75 -> amber
        assert_eq!(run(70.0, 57.0, p, t), HEX_AMBER);
        // Day 6 (pace 86), used 92 (delta +6) -> amber_thr 4.53, red_thr 6.08 -> amber
        assert_eq!(run(92.0, 86.0, p, t), HEX_AMBER);
        // Day 6, used 96 (delta +10) -> red_thr 6.08 -> red
        assert_eq!(run(96.0, 86.0, p, t), HEX_RED);
    }

    #[test]
    fn missing_resets_at_falls_back_to_static() {
        assert_eq!(
            color_for_window(Some(96.0), None, 0, SESSION_5H_SECS, PARAMS_SESSION),
            HEX_RED
        );
        assert_eq!(
            color_for_window(Some(85.0), None, 0, SESSION_5H_SECS, PARAMS_SESSION),
            HEX_AMBER
        );
        assert_eq!(
            color_for_window(Some(40.0), None, 0, SESSION_5H_SECS, PARAMS_SESSION),
            HEX_GREEN
        );
    }

    #[test]
    fn missing_used_pct_returns_gray() {
        assert_eq!(
            color_for_window(None, Some(0), 0, SESSION_5H_SECS, PARAMS_SESSION),
            HEX_GRAY
        );
    }

    #[test]
    fn resets_in_past_clamps_pace_to_100() {
        let now = 1_000_000;
        let resets_past = now - 10_000;
        // used 80 with pace=100 -> delta=-20 -> green
        assert_eq!(
            color_for_window(
                Some(80.0),
                Some(resets_past),
                now,
                SESSION_5H_SECS,
                PARAMS_SESSION
            ),
            HEX_GREEN
        );
        // used 110 with pace=100 -> delta=10 >= red_end=5 -> red
        assert_eq!(
            color_for_window(
                Some(110.0),
                Some(resets_past),
                now,
                SESSION_5H_SECS,
                PARAMS_SESSION
            ),
            HEX_RED
        );
    }

    #[test]
    fn resets_far_future_clamps_pace_to_0() {
        let now = 1_000_000;
        let resets_future = now + SESSION_5H_SECS * 5;
        // used 40 with pace=0 -> delta=40 >= red_start=30 -> red
        assert_eq!(
            color_for_window(
                Some(40.0),
                Some(resets_future),
                now,
                SESSION_5H_SECS,
                PARAMS_SESSION
            ),
            HEX_RED
        );
        // used 20 -> delta=20 < amber_start=25 -> green
        assert_eq!(
            color_for_window(
                Some(20.0),
                Some(resets_future),
                now,
                SESSION_5H_SECS,
                PARAMS_SESSION
            ),
            HEX_GREEN
        );
    }

    #[test]
    fn parse_iso_secs_round_trip() {
        // 2026-01-01T00:00:00Z = 1_767_225_600
        let iso = "2026-01-01T00:00:00+00:00";
        let secs = parse_iso_secs(iso).expect("parse");
        assert_eq!(secs, 1_767_225_600);
        assert!(parse_iso_secs("not-a-date").is_none());
    }
}
