//! Date formatting for the documents list, without a timezone database: callers pass the UTC
//! offset to use.

use crate::library::Timestamp;

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A calendar date and time of day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: i64,
    /// 1-12
    pub month: u32,
    /// 1-31
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

impl DateTime {
    /// The local date and time of `ts` at `offset` seconds east of UTC.
    pub fn from_timestamp(ts: Timestamp, offset: i64) -> Self {
        let t = ts + offset;
        let days = t.div_euclid(86_400);
        let secs = t.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (secs / 3600) as u32,
            minute: ((secs % 3600) / 60) as u32,
        }
    }
}

/// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (year, month, day).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Formats a "Modified" date the way the documents list shows it: `2:19 PM Sep 22` for dates in
/// the current year, `Sep 22, 2024` for older ones.
pub fn format_modified(ts: Timestamp, now: Timestamp, offset: i64) -> String {
    let d = DateTime::from_timestamp(ts, offset);
    let n = DateTime::from_timestamp(now, offset);
    let month = MONTHS[(d.month - 1) as usize];
    if d.year != n.year {
        return format!("{month} {}, {}", d.day, d.year);
    }
    let (h12, ampm) = match d.hour {
        0 => (12, "AM"),
        1..=11 => (d.hour, "AM"),
        12 => (12, "PM"),
        h => (h - 12, "PM"),
    };
    format!("{h12}:{:02} {ampm} {month} {}", d.minute, d.day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2026-09-22 is day 20718.
        assert_eq!(civil_from_days(20_718), (2026, 9, 22));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn modified_format() {
        // 2026-09-22 14:19 UTC.
        let ts = 20_718 * 86_400 + 14 * 3600 + 19 * 60;
        let now = ts + 86_400;
        assert_eq!(format_modified(ts, now, 0), "2:19 PM Sep 22");
        assert_eq!(format_modified(ts, now, 2 * 3600), "4:19 PM Sep 22");
        assert_eq!(format_modified(ts - 14 * 3600, now, 0), "12:19 AM Sep 22");
        assert_eq!(format_modified(ts - 2 * 3600, now, 0), "12:19 PM Sep 22");
        assert_eq!(format_modified(ts - 400 * 86_400, now, 0), "Aug 18, 2025");
    }
}
