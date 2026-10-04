//! Known-answer tests for the 9 statistical functions.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

mod common;

use common::*;
use formula_lang::{ExcelError, MapResolver, Value};

/// Evaluate over the standard fixture sheet.
fn n_over(f: &str) -> f64 {
    num_over(f)
}

/// Evaluate over an explicitly given resolver.
fn n_on(f: &str, s: &MapResolver) -> f64 {
    match ev_over(f, s) {
        Ok(Value::Number(n)) => n,
        other => panic!("{f}: expected number, got {other:?}"),
    }
}

#[test]
fn average() {
    assert_eq!(num("AVERAGE(1,2,3)"), 2.0);
    assert_eq!(num("AVERAGE(2)"), 2.0);
    assert_eq!(num("AVERAGE(1,2,\"3\")"), 2.0); // direct text coerces
    assert_eq!(num("AVERAGE(TRUE,3)"), 2.0); // direct bool coerces
    assert_err("AVERAGE(\"x\")", ExcelError::Value);
}

#[test]
fn average_over_ranges() {
    assert_eq!(n_over("AVERAGE(A1:A4)"), 25.0);
    // Text/bool/empty cells in ranges are excluded from count and sum.
    assert_eq!(n_over("AVERAGE(B1:B4)"), 2.5);
    // All-empty range → #DIV/0!.
    assert_err("AVERAGE(Z1:Z9)", ExcelError::DivZero);
}

#[test]
fn count_family() {
    // A1:A4 numbers, B: text/number/bool/empty, C numbers.
    assert_eq!(n_over("COUNT(A1:B4)"), 5.0); // A1..A4 + B2
    assert_eq!(n_over("COUNTA(A1:B4)"), 7.0); // all but B4
    assert_eq!(n_over("COUNTBLANK(A1:B4)"), 1.0); // B4
    assert_eq!(n_over("COUNT(\"3\",TRUE,\"x\")"), 2.0); // "x" is not a number: skipped, not an error
    assert_eq!(n_over("COUNTA(1,\"x\",A1:B4)"), 9.0); // 2 direct + 7 non-empty cells
    assert_eq!(n_over("COUNT(Z1:Z9)"), 0.0);
    assert_eq!(n_over("COUNTA(Z1:Z9)"), 0.0);
    assert_eq!(n_over("COUNTBLANK(Z1:Z9)"), 9.0);
    // COUNTA counts error cells as present (not blank, not propagated).
    assert_eq!(n_over("COUNTA(E1)"), 1.0);
}

#[test]
fn max_min() {
    assert_eq!(num("MAX(1,9,3)"), 9.0);
    assert_eq!(num("MIN(4,-2,3)"), -2.0);
    assert_eq!(num("MAX(7)"), 7.0);
    // Zero args is an argument-count error (#VALUE!).
    assert_err("MAX()", ExcelError::Value);
    assert_err("MIN()", ExcelError::Value);
    assert_eq!(n_over("MAX(A1:A4)"), 40.0);
    assert_eq!(n_over("MIN(A1:A4)"), 10.0);
    // Range text/bool skipped.
    assert_eq!(n_over("MAX(B1:B4)"), 2.5);
}

#[test]
fn median() {
    assert_eq!(num("MEDIAN(3,1,2)"), 2.0);
    assert_eq!(num("MEDIAN(4,1,2,3)"), 2.5); // even count → mean of middles
    assert_eq!(num("MEDIAN(5)"), 5.0);
    assert_eq!(num("MEDIAN(1,,3)"), 2.0);
    assert_err("MEDIAN()", ExcelError::Value); // zero args, not empty set
    assert_eq!(n_over("MEDIAN(A1:A4)"), 25.0);
}

#[test]
fn stdev_var_sample() {
    // Sample (n-1) statistics: {2,4,4,4,5,5,7,9} → var 32/7, stdev √(32/7).
    assert!((num("VAR(2,4,4,4,5,5,7,9)") - 32.0 / 7.0).abs() < 1e-12);
    assert!((num("STDEV(2,4,4,4,5,5,7,9)") - (32.0f64 / 7.0).sqrt()).abs() < 1e-12);
    assert!((num("VAR(1,2)") - 0.5).abs() < 1e-12);
    assert!((num("STDEV(1,2)") - core::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
    // Fewer than two values → #DIV/0!.
    assert_err("VAR(5)", ExcelError::DivZero);
    assert_err("STDEV(5)", ExcelError::DivZero);
    assert_err("VAR()", ExcelError::Value); // zero args
                                            // Ranges feed the same pipeline.
    assert!((n_over("VAR(A1:A4)") - 166.66666666666666).abs() < 1e-9);
    assert!((n_over("STDEV(A1:A4)") - 12.909944487358056).abs() < 1e-9);
}

#[test]
fn error_cells_propagate_through_aggregates() {
    // E1 holds #REF! — consuming it is the hard channel.
    assert_err_over("SUM(E1:E1)", &sheet(), ExcelError::Ref);
    assert_err_over("AVERAGE(E1:E1)", &sheet(), ExcelError::Ref);
    assert_err_over("MAX(E1,1)", &sheet(), ExcelError::Ref);
    // COUNT ignores even error cells inside references (Excel).
    assert_eq!(n_over("COUNT(E1:E1)"), 0.0);
    assert_eq!(n_over("COUNTA(E1:E1)"), 1.0);
}

#[test]
fn empty_sheet_aggregates() {
    let s = MapResolver::new();
    assert_eq!(n_on("SUM(A1:A9)", &s), 0.0);
    assert_eq!(n_on("COUNT(A1:A9)", &s), 0.0);
    assert_eq!(n_on("COUNTA(A1:A9)", &s), 0.0);
    assert_eq!(n_on("COUNTBLANK(A1:A9)", &s), 9.0);
}
