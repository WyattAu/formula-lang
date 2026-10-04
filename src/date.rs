//! Excel serial dates — the calendar math behind `TODAY`, `NOW`, and the
//! date formats of `TEXT`.
//!
//! Excel's serial epoch is 1900-01-01 = 1, **with the famous Lotus leap
//! bug**: serial 60 is the nonexistent 1900-02-29, so dates before
//! 1900-03-01 are shifted by one. `serial_to_ymd` reproduces the bug
//! exactly (including mapping 60 to the phantom February 29th);
//! `ymd_to_serial` inverts it. The conversion core is Howard Hinnant's
//! `days_from_civil`/`civil_from_days` — pure integer math, `no_std`,
//! total over the legal serial window 1..=2958465 (1900-01-01 ..=
//! 9999-12-31).

use crate::error::ExcelError;
use alloc::string::String;

/// Excel serial of the Unix epoch: 1970-01-01 = 25569.
pub const UNIX_EPOCH_SERIAL: i64 = 25_569;

/// Serial of the phantom 1900-02-29 (the Lotus bug).
const PHANTOM_LEAP_SERIAL: i64 = 60;

/// Serial of 1900-03-01 — the first serial after the bug's shift.
const MARCH_1_1900_SERIAL: i64 = 61;

/// Howard Hinnant's `days_from_civil` — days since 1970-01-01 for a
/// proleptic Gregorian civil date.
#[must_use]
pub const fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m + 9) % 12; // [0, 11]: Mar = 0
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Howard Hinnant's `civil_from_days` — the inverse.
#[must_use]
pub const fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Excel serial (1-based, Lotus-bug-compatible) → `(y, m, d)`.
/// Returns [`ExcelError::Num`] outside the legal window
/// `1..=2958465` (`1900-01-01` ..= `9999-12-31`).
pub fn serial_to_ymd(serial: i64) -> Result<(i64, i64, i64), ExcelError> {
    if !(1..=2_958_465).contains(&serial) {
        return Err(ExcelError::Num);
    }
    if serial == PHANTOM_LEAP_SERIAL {
        return Ok((1900, 2, 29)); // the bug, faithfully
    }
    let unix_days = if serial >= MARCH_1_1900_SERIAL {
        serial - UNIX_EPOCH_SERIAL
    } else {
        // Before the phantom day, serials are one behind the true count.
        serial - UNIX_EPOCH_SERIAL + 1
    };
    Ok(civil_from_days(unix_days))
}

/// `(y, m, d)` → Excel serial. Inverse of [`serial_to_ymd`] (the phantom
/// date maps to 60, like Excel).
/// `(y, m, d)` → Excel serial. Inverse of [`serial_to_ymd`] — including
/// the phantom `1900-02-29`, which maps to 60 exactly as Excel's
/// `DATE(1900,2,29)` does.
#[must_use]
pub fn ymd_to_serial(y: i64, m: i64, d: i64) -> i64 {
    // The phantom day before the general math (it falls through to the
    // 1900-03-01 serial otherwise).
    if (y, m, d) == (1900, 2, 29) {
        return 60;
    }
    let unix_days = days_from_civil(y, m, d);
    let serial = unix_days + UNIX_EPOCH_SERIAL;
    if serial < MARCH_1_1900_SERIAL {
        serial - 1
    } else {
        serial
    }
}

/// Fractional part of a serial → `(hour, minute, second)` with
/// round-to-nearest-second and an 86400 rollover guard.
#[must_use]
pub fn fraction_to_hms(frac: f64) -> (u32, u32, u32) {
    let f = if frac.is_finite() && frac >= 0.0 {
        frac % 1.0
    } else {
        0.0
    };
    let secs = libm::round(f * 86_400.0) as i64;
    let secs = if secs >= 86_400 { 0 } else { secs };
    (
        (secs / 3600) as u32,
        ((secs % 3600) / 60) as u32,
        (secs % 60) as u32,
    )
}

/// Weekday name (English) for a serial. Excel treats serial 1
/// (1900-01-01) as a Sunday in Lotus-land — the bug keeps weekday
/// alignment consistent for all dates ≥ 1900-03-01 with the proleptic
/// Gregorian truth (`2026-10-04` → `"Sunday"`).
#[must_use]
pub fn weekday_name(serial: i64, long: bool) -> &'static str {
    const SHORT: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const LONG: [&str; 7] = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    // 1970-01-01 (serial 25569) was a Thursday; rem_euclid keeps idx in
    // 0..7, so `get` always hits.
    let idx = (serial - 25_569 + 3).rem_euclid(7) as usize;
    let names: [&str; 7] = if long { LONG } else { SHORT };
    names.get(idx).copied().unwrap_or("")
}

/// Renders `(y, m, d)` zero-padded per width.
pub(crate) fn pad2(out: &mut String, n: i64) {
    if n < 10 {
        out.push('0');
    }
    append_int(out, n);
}

pub(crate) fn append_int(out: &mut String, n: i64) {
    let _ = core::fmt::Write::write_fmt(out, format_args!("{n}"));
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    #[test]
    fn civil_roundtrip() {
        for &(y, m, d) in &[
            (1900i64, 1i64, 1i64),
            (1970, 1, 1),
            (2000, 2, 29),
            (2026, 10, 4),
            (9999, 12, 31),
        ] {
            let days = days_from_civil(y, m, d);
            assert_eq!(civil_from_days(days), (y, m, d));
        }
    }

    #[test]
    fn serial_anchors() {
        assert_eq!(ymd_to_serial(1900, 1, 1), 1);
        assert_eq!(ymd_to_serial(1900, 2, 28), 59);
        assert_eq!(ymd_to_serial(1900, 3, 1), 61);
        assert_eq!(ymd_to_serial(1970, 1, 1), 25_569);
        assert_eq!(ymd_to_serial(9999, 12, 31), 2_958_465);
        assert_eq!(serial_to_ymd(60).unwrap(), (1900, 2, 29)); // the bug
        assert_eq!(serial_to_ymd(59).unwrap(), (1900, 2, 28));
        assert_eq!(serial_to_ymd(1).unwrap(), (1900, 1, 1));
        assert_eq!(serial_to_ymd(25_569).unwrap(), (1970, 1, 1));
        assert_eq!(serial_to_ymd(0), Err(ExcelError::Num));
        assert_eq!(serial_to_ymd(2_958_466), Err(ExcelError::Num));
    }

    #[test]
    fn weekday_anchor() {
        // 1970-01-01 was a Thursday.
        assert_eq!(weekday_name(25_569, false), "Thu");
        // 2026-10-04 is a Sunday.
        assert_eq!(weekday_name(ymd_to_serial(2026, 10, 4), true), "Sunday");
    }
}
