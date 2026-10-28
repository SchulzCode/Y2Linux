//! The wall-clock forms report.py writes: `datetime.isoformat()` in UTC, the queue's
//! `%Y%m%dT%H%M%SZ`, the saved file's local `%Y%m%d-%H%M%S`, the local UTC offset, and
//! `datetime.fromisoformat(...).timestamp()` for a queued report's age.
// SPDX-License-Identifier: GPL-2.0-only
use std::time::{SystemTime, UNIX_EPOCH};

/// One instant: seconds since the epoch and microseconds (`datetime.now(timezone.utc)`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Instant {
    pub secs: i64,
    pub micros: u32,
}

impl Instant {
    pub fn now() -> Instant {
        let since = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Instant {
            secs: since.as_secs() as i64,
            micros: since.subsec_micros(),
        }
    }

    /// `datetime.fromtimestamp(t, timezone.utc)`: microseconds rounded half to even.
    pub fn from_float(t: f64) -> Instant {
        let mut secs = t.floor();
        let mut micros = ((t - secs) * 1e6).round_ties_even();
        if micros >= 1e6 {
            secs += 1.0;
            micros -= 1e6;
        }
        Instant {
            secs: secs as i64,
            micros: micros as u32,
        }
    }

    pub fn timestamp(&self) -> f64 {
        self.secs as f64 + f64::from(self.micros) / 1e6
    }
}

/// Days since 1970-01-01 to (year, month, day) (proleptic Gregorian).
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// (year, month, day) to days since 1970-01-01.
pub fn days(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub struct Fields {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

pub fn utc_fields(secs: i64) -> Fields {
    let (year, month, day) = civil(secs.div_euclid(86_400));
    let rest = secs.rem_euclid(86_400);
    Fields {
        year,
        month,
        day,
        hour: (rest / 3600) as u32,
        minute: (rest / 60 % 60) as u32,
        second: (rest % 60) as u32,
    }
}

/// The local time of `secs` and its UTC offset in seconds (`localtime_r`, TZ / /etc/localtime).
pub fn local_fields(secs: i64) -> (Fields, i64) {
    let t = secs as libc::time_t;
    // SAFETY: a zeroed tm is a valid output buffer for localtime_r.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call.
    let ok = unsafe { !libc::localtime_r(&t, &mut tm).is_null() };
    if !ok {
        return (utc_fields(secs), 0);
    }
    (
        Fields {
            year: i64::from(tm.tm_year) + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
        },
        tm.tm_gmtoff,
    )
}

/// `now.isoformat()` of an aware UTC datetime.
pub fn isoformat(at: Instant) -> String {
    let f = utc_fields(at.secs);
    let fraction = if at.micros > 0 {
        format!(".{:06}", at.micros)
    } else {
        String::new()
    };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{fraction}+00:00",
        f.year, f.month, f.day, f.hour, f.minute, f.second
    )
}

/// `now.date().isoformat()` (UTC).
pub fn date(at: Instant) -> String {
    let f = utc_fields(at.secs);
    format!("{:04}-{:02}-{:02}", f.year, f.month, f.day)
}

/// `now.strftime('%Y%m%dT%H%M%SZ')` (UTC).
pub fn queue_stamp(at: Instant) -> String {
    let f = utc_fields(at.secs);
    format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        f.year, f.month, f.day, f.hour, f.minute, f.second
    )
}

/// `now.astimezone().strftime('%Y%m%d-%H%M%S')`.
pub fn local_stamp(at: Instant) -> String {
    let (f, _) = local_fields(at.secs);
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        f.year, f.month, f.day, f.hour, f.minute, f.second
    )
}

/// `int(datetime.now().astimezone().utcoffset().total_seconds() // 60)`.
pub fn utc_offset_minutes(at: Instant) -> i64 {
    local_fields(at.secs).1.div_euclid(60)
}

/// `_next_midnight(now)`: 00:05 UTC of the next day.
pub fn next_midnight(now: f64) -> f64 {
    let at = Instant::from_float(now);
    (at.secs.div_euclid(86_400) + 1) as f64 * 86_400.0 + 300.0
}

/// `datetime.fromisoformat(text).timestamp()`; `None` where it raises. A naive time is local.
pub fn fromisoformat(text: &str) -> Option<f64> {
    let b = text.as_bytes();
    let digits = |from: usize, n: usize| -> Option<i64> {
        let part = b.get(from..from + n)?;
        part.iter()
            .all(u8::is_ascii_digit)
            .then(|| std::str::from_utf8(part).ok()?.parse().ok())?
    };
    // The date: YYYY-MM-DD or YYYYMMDD.
    let (year, month, day, mut at) = if b.get(4) == Some(&b'-') {
        if b.get(7) != Some(&b'-') {
            return None;
        }
        (digits(0, 4)?, digits(5, 2)?, digits(8, 2)?, 10)
    } else {
        (digits(0, 4)?, digits(4, 2)?, digits(6, 2)?, 8)
    };
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&month) || day < 1 || day > month_days[month as usize - 1] || year < 1 {
        return None;
    }
    let (mut hour, mut minute, mut second, mut micros) = (0, 0, 0, 0i64);
    let mut offset: Option<i64> = None;
    if at < b.len() {
        if !matches!(b[at], b'T' | b' ') {
            return None;
        }
        at += 1;
        let extended = b.get(at + 2) == Some(&b':');
        hour = digits(at, 2)?;
        at += 2;
        let mut field = |at: &mut usize| -> Option<Option<i64>> {
            if extended {
                if b.get(*at) != Some(&b':') {
                    return Some(None);
                }
                *at += 1;
            } else if !b.get(*at).is_some_and(u8::is_ascii_digit) {
                return Some(None);
            }
            let value = digits(*at, 2)?;
            *at += 2;
            Some(Some(value))
        };
        if let Some(m) = field(&mut at)? {
            minute = m;
            if let Some(s) = field(&mut at)? {
                second = s;
                if matches!(b.get(at), Some(b'.' | b',')) {
                    at += 1;
                    let start = at;
                    while b.get(at).is_some_and(u8::is_ascii_digit) {
                        at += 1;
                    }
                    let n = at - start;
                    if n == 0 {
                        return None;
                    }
                    let take = n.min(6);
                    micros = digits(start, take)? * 10i64.pow((6 - take) as u32);
                }
            }
        }
        if at < b.len() {
            match b[at] {
                b'Z' if at + 1 == b.len() => offset = Some(0),
                b'+' | b'-' => {
                    let sign = if b[at] == b'-' { -1 } else { 1 };
                    at += 1;
                    let oh = digits(at, 2)?;
                    at += 2;
                    if b.get(at) == Some(&b':') {
                        at += 1;
                    }
                    let om = if at < b.len() { digits(at, 2)? } else { 0 };
                    if at < b.len() {
                        at += 2;
                    }
                    let mut os = 0;
                    if b.get(at) == Some(&b':') {
                        os = digits(at + 1, 2)?;
                        at += 3;
                    }
                    if at != b.len() || oh > 23 || om > 59 {
                        return None;
                    }
                    offset = Some(sign * (oh * 3600 + om * 60 + os));
                }
                _ => return None,
            }
        }
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
    }
    let local = days(year, month as u32, day as u32) * 86_400 + hour * 3600 + minute * 60 + second;
    let secs = match offset {
        Some(offset) => local - offset,
        None => {
            // A naive datetime's timestamp() is local time (mktime).
            // SAFETY: a zeroed tm with the fields set is a valid input to mktime.
            let mut tm: libc::tm = unsafe { std::mem::zeroed() };
            let f = utc_fields(local);
            tm.tm_year = (f.year - 1900) as i32;
            tm.tm_mon = f.month as i32 - 1;
            tm.tm_mday = f.day as i32;
            tm.tm_hour = f.hour as i32;
            tm.tm_min = f.minute as i32;
            tm.tm_sec = f.second as i32;
            tm.tm_isdst = -1;
            // SAFETY: tm is valid.
            (unsafe { libc::mktime(&mut tm) }) as i64
        }
    };
    Some(secs as f64 + micros as f64 / 1e6)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms_match_datetime() {
        let at = Instant {
            secs: 1_761_650_096,
            micros: 120_000,
        };
        assert_eq!(isoformat(at), "2025-10-28T11:14:56.120000+00:00");
        assert_eq!(
            isoformat(Instant { secs: 0, micros: 0 }),
            "1970-01-01T00:00:00+00:00"
        );
        assert_eq!(queue_stamp(at), "20251028T111456Z");
        assert_eq!(date(at), "2025-10-28");
        assert_eq!(days(2025, 10, 28) * 86_400 + 40_496, 1_761_650_096);
        assert_eq!(
            fromisoformat("2025-10-28T11:14:56.120000+00:00"),
            Some(1_761_650_096.12)
        );
        assert_eq!(fromisoformat("2025-10-28T13:14:56+02:00"), Some(1_761_650_096.0));
        assert_eq!(fromisoformat("2025-10-28T11:14:56Z"), Some(1_761_650_096.0));
        assert_eq!(fromisoformat("2025-13-28"), None);
        assert_eq!(fromisoformat("nope"), None);
        assert_eq!(next_midnight(1_761_650_096.5), 1_761_696_300.0);
        assert_eq!(
            Instant::from_float(1.0000005),
            Instant { secs: 1, micros: 0 }
        );
    }
}
