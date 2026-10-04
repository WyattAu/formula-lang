//! Public date API — Excel serial conversions, Lotus leap bug, Hinnant
//! round-trips.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

use formula_lang::date;
use formula_lang::ExcelError;

#[test]
fn epoch_anchors() {
    assert_eq!(date::ymd_to_serial(1900, 1, 1), 1);
    assert_eq!(date::ymd_to_serial(1900, 1, 2), 2);
    assert_eq!(date::ymd_to_serial(1900, 2, 28), 59);
    assert_eq!(date::ymd_to_serial(1900, 3, 1), 61); // the bug's seam
    assert_eq!(date::ymd_to_serial(1970, 1, 1), 25_569);
    assert_eq!(date::ymd_to_serial(2000, 1, 1), 36_526);
    assert_eq!(date::ymd_to_serial(2026, 10, 4), 46_299);
    assert_eq!(date::ymd_to_serial(9999, 12, 31), 2_958_465);
}

#[test]
fn serial_anchors_and_the_phantom_leap_day() {
    assert_eq!(date::serial_to_ymd(1), Ok((1900, 1, 1)));
    assert_eq!(date::serial_to_ymd(59), Ok((1900, 2, 28)));
    assert_eq!(date::serial_to_ymd(60), Ok((1900, 2, 29))); // the bug
    assert_eq!(date::serial_to_ymd(61), Ok((1900, 3, 1)));
    assert_eq!(date::serial_to_ymd(25_569), Ok((1970, 1, 1)));
    assert_eq!(date::serial_to_ymd(2_958_465), Ok((9999, 12, 31)));
    // Out of range.
    assert_eq!(date::serial_to_ymd(0), Err(ExcelError::Num));
    assert_eq!(date::serial_to_ymd(-1), Err(ExcelError::Num));
    assert_eq!(date::serial_to_ymd(2_958_466), Err(ExcelError::Num));
}

#[test]
fn roundtrip_all_anchor_days() {
    for serial in [
        1i64, 2, 59, 60, 61, 62, 100, 25_569, 45_123, 36_526, 2_958_465,
    ] {
        let (y, m, d) = date::serial_to_ymd(serial).unwrap();
        assert_eq!(date::ymd_to_serial(y, m, d), serial, "serial {serial}");
    }
}

#[test]
fn hinnant_roundtrip_sweep() {
    // Every month boundary over 400 years.
    let mut days = date::days_from_civil(1900, 1, 1);
    let end = date::days_from_civil(2300, 1, 1);
    while days <= end {
        let (y, m, d) = date::civil_from_days(days);
        assert_eq!(date::days_from_civil(y, m, d), days, "{days}");
        days += 17; // prime step hits varied month positions
    }
}

#[test]
fn leap_years_follow_gregorian() {
    assert_eq!(
        date::ymd_to_serial(2000, 2, 29) - date::ymd_to_serial(2000, 2, 28),
        1
    );
    assert_eq!(
        date::ymd_to_serial(1900, 3, 1) - date::ymd_to_serial(1900, 2, 28),
        2
    ); // 1900 not leap (post-bug alignment)
    assert_eq!(
        date::ymd_to_serial(2024, 2, 29) - date::ymd_to_serial(2024, 2, 28),
        1
    );
    assert_eq!(
        date::ymd_to_serial(2023, 3, 1) - date::ymd_to_serial(2023, 2, 28),
        1
    );
}

#[test]
fn weekday_alignment() {
    // Known weekdays: 2026-10-04 = Sunday, 1970-01-01 = Thursday.
    assert_eq!(
        date::weekday_name(date::ymd_to_serial(2026, 10, 4), true),
        "Sunday"
    );
    assert_eq!(
        date::weekday_name(date::ymd_to_serial(1970, 1, 1), false),
        "Thu"
    );
    assert_eq!(
        date::weekday_name(date::ymd_to_serial(2024, 2, 29), true),
        "Thursday"
    );
    assert_eq!(date::weekday_name(45_123, false), "Sun"); // 2023-07-16
}

#[test]
fn fraction_to_hms() {
    assert_eq!(date::fraction_to_hms(0.0), (0, 0, 0));
    assert_eq!(date::fraction_to_hms(0.25), (6, 0, 0));
    assert_eq!(date::fraction_to_hms(0.5), (12, 0, 0));
    assert_eq!(date::fraction_to_hms(0.75), (18, 0, 0));
    assert_eq!(date::fraction_to_hms(1.0 / 24.0), (1, 0, 0));
    assert_eq!(date::fraction_to_hms(1.0 / 24.0 / 60.0), (0, 1, 0));
    // Rounding to the nearest second; 86400 rolls to zero.
    assert_eq!(date::fraction_to_hms(86_399.0 / 86_400.0), (23, 59, 59));
    assert_eq!(date::fraction_to_hms(0.999999), (0, 0, 0)); // 86399.91 rounds up → 86400 → 0
    assert_eq!(date::fraction_to_hms(-0.5), (0, 0, 0)); // guarded
}

#[test]
fn unix_epoch_constant() {
    assert_eq!(date::UNIX_EPOCH_SERIAL, 25_569);
    // std's clock path lands on the same serial arithmetic.
    let now_days = 20_000i64; // arbitrary unix days
    assert_eq!(
        date::serial_to_ymd(now_days + date::UNIX_EPOCH_SERIAL).unwrap(),
        date::civil_from_days(now_days)
    );
}
