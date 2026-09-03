//! Campaign-window advisory. ADVISORY ONLY: zcode-axi never claims to know
//! quota; it just reports where "now" falls relative to Wesley's stated free
//! usage window: 12:00–22:00 America/Sao_Paulo, Sep 3–20 2026.
//!
//! America/Sao_Paulo has had no DST since 2019, so a fixed UTC-3 offset is
//! correct for the whole window.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

pub const OFFSET_SECONDS: i64 = -3 * 3600;
pub const WINDOW_START_DAY: CivilDate = CivilDate::new(2026, 9, 3);
pub const WINDOW_END_DAY: CivilDate = CivilDate::new(2026, 9, 20);
pub const WINDOW_START_HOUR: u32 = 12;
pub const WINDOW_END_HOUR: u32 = 22;

/// Civil (Gregorian) date. No chrono dependency: days-from-civil math.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct CivilDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl CivilDate {
    pub const fn new(year: i32, month: u32, day: u32) -> Self {
        Self { year, month, day }
    }
}

/// A civil date + local time-of-day in America/Sao_Paulo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct LocalMoment {
    pub date: CivilDate,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl LocalMoment {
    /// Convert a UTC unix timestamp to local civil time via fixed offset.
    pub fn from_unix(unix: i64) -> Self {
        let local = unix + OFFSET_SECONDS;
        let days = local.div_euclid(86_400);
        let secs = local.rem_euclid(86_400);
        let (y, m, d) = civil_from_days(days);
        Self {
            date: CivilDate::new(y, m, d),
            hour: (secs / 3600) as u32,
            minute: ((secs % 3600) / 60) as u32,
            second: (secs % 60) as u32,
        }
    }

    /// Convert local civil time back to a UTC unix timestamp.
    pub fn to_unix(&self) -> i64 {
        days_from_civil(self.date.year, self.date.month, self.date.day) * 86_400
            + i64::from(self.hour) * 3600
            + i64::from(self.minute) * 60
            + i64::from(self.second)
            - OFFSET_SECONDS
    }
}

/// Howard Hinnant's `civil_from_days` / `days_from_civil` algorithms.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((y + i64::from(m <= 2)) as i32, m, d)
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = if m > 2 {
        i64::from(m) - 3
    } else {
        i64::from(m) + 9
    };
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// UTC civil date for a days-since-epoch value (used for stable timestamp
/// rendering elsewhere in the crate).
pub fn civil_from_days_pub(days: i64) -> (i32, u32, u32) {
    civil_from_days(days)
}

/// Outcome of comparing "now" to the advisory window.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WindowAdvice {
    pub in_window: bool,
    /// e.g. "Sep 3–20 2026, 12:00–22:00 America/Sao_Paulo (UTC-3)".
    pub window: String,
    /// Human explanation of the current position relative to the window.
    pub detail: String,
    /// UTC unix seconds when the next window opens, if computable.
    pub next_window_unix: Option<i64>,
}

/// Evaluate the advisory at UTC timestamp `unix_now`.
pub fn advise(unix_now: i64) -> WindowAdvice {
    let now = LocalMoment::from_unix(unix_now);
    let window = format!(
        "Sep {}–{} 2026, 12:00–22:00 America/Sao_Paulo (UTC-3)",
        WINDOW_START_DAY.day, WINDOW_END_DAY.day
    );
    let in_date_range = (WINDOW_START_DAY..=WINDOW_END_DAY).contains(&now.date);
    let in_hour_range = (WINDOW_START_HOUR..WINDOW_END_HOUR).contains(&now.hour);
    let in_window = in_date_range && in_hour_range;

    // Next opening moment: today 12:00 if before it (and within campaign),
    // else the following day's 12:00 while the campaign lasts.
    let next_open_today = LocalMoment {
        date: now.date,
        hour: WINDOW_START_HOUR,
        minute: 0,
        second: 0,
    };
    let tomorrow_day = days_from_civil(now.date.year, now.date.month, now.date.day) + 1;
    let (ty, tm, td) = civil_from_days(tomorrow_day);
    let next_open_tomorrow = LocalMoment {
        date: CivilDate::new(ty, tm, td),
        hour: WINDOW_START_HOUR,
        minute: 0,
        second: 0,
    };
    let next_window_unix = if in_window {
        None
    } else if !in_date_range {
        if now.date < WINDOW_START_DAY {
            Some(
                LocalMoment {
                    date: WINDOW_START_DAY,
                    hour: WINDOW_START_HOUR,
                    minute: 0,
                    second: 0,
                }
                .to_unix(),
            )
        } else {
            None // campaign over
        }
    } else if now.hour < WINDOW_START_HOUR {
        Some(next_open_today.to_unix())
    } else {
        Some(next_open_tomorrow.to_unix())
    };

    let detail = if in_window {
        format!(
            "now is inside the daily free window (local {now_local})",
            now_local = format_local(now)
        )
    } else if !in_date_range && now.date < WINDOW_START_DAY {
        format!(
            "campaign has not started yet (local date {d}); window opens Sep 3 2026 12:00 local",
            d = format_date(now.date)
        )
    } else if !in_date_range {
        format!(
            "campaign window ended Sep 20 2026 (local date {d})",
            d = format_date(now.date)
        )
    } else if now.hour < WINDOW_START_HOUR {
        format!(
            "before today's window (local {t}); opens 12:00",
            t = format_local(now)
        )
    } else {
        format!(
            "after today's window (local {t}); reopens tomorrow 12:00 if within campaign dates",
            t = format_local(now)
        )
    };

    WindowAdvice {
        in_window,
        window,
        detail,
        next_window_unix,
    }
}

/// Advisory evaluated at the current clock.
pub fn advise_now() -> WindowAdvice {
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    advise(unix)
}

fn format_date(d: CivilDate) -> String {
    format!("{:04}-{:02}-{:02}", d.year, d.month, d.day)
}

fn format_local(m: LocalMoment) -> String {
    format!(
        "{} {:02}:{:02}:{:02}",
        format_date(m.date),
        m.hour,
        m.minute,
        m.second
    )
}
