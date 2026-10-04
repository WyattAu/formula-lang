//! Known-answer tests for the 18 arithmetic functions, including Excel's
//! rounding/significance corner cases.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

mod common;

use common::*;
use formula_lang::ExcelError;

#[test]
fn sum() {
    assert_eq!(num("SUM(1,2,3)"), 6.0);
    assert_eq!(num("SUM(1)"), 1.0);
    assert_eq!(num("SUM(-1,-2)"), -3.0);
    assert_eq!(num("SUM(1,,3)"), 4.0); // omitted slot skipped
    assert_eq!(num("SUM(\"3\",TRUE)"), 4.0); // direct scalars coerce
    assert_err("SUM(\"x\")", ExcelError::Value);
    assert_err("SUM()", ExcelError::Value); // zero args (Excel requires ≥1)
}

#[test]
fn sum_over_ranges() {
    let s = sheet();
    // A1:A4 = 10..40.
    assert_eq!(num_over(&s, "SUM(A1:A4)"), 100.0);
    assert_eq!(num_over(&s, "SUM(A1:B4)"), 102.5); // B2=2.5; text/bool/empty in range ignored
    assert_eq!(num_over(&s, "SUM(A1:A2,A3:A4)"), 100.0); // multi-range args
    assert_eq!(num_over(&s, "SUM(B1:B4)"), 2.5);
    assert_eq!(num_over(&s, "SUM(A1)"), 10.0);
    // Single-cell ref of text content is skipped like any reference.
    assert_eq!(num_over(&s, "SUM(B1)"), 0.0);
}

#[test]
fn product() {
    assert_eq!(num("PRODUCT(2,3,4)"), 24.0);
    assert_eq!(num("PRODUCT(5)"), 5.0);
    assert_eq!(num("PRODUCT(2,,3)"), 6.0);
    // Product over an all-empty range is 0 (Excel).
    let s = MapResolver::new();
    assert_eq!(num_over(&s, "PRODUCT(Z1:Z9)"), 0.0);
    assert_err("PRODUCT()", ExcelError::Value);
}

#[test]
fn abs_sign() {
    assert_eq!(num("ABS(-3.5)"), 3.5);
    assert_eq!(num("ABS(3.5)"), 3.5);
    assert_eq!(num("ABS(0)"), 0.0);
    assert_eq!(num("SIGN(-9)"), -1.0);
    assert_eq!(num("SIGN(0)"), 0.0);
    assert_eq!(num("SIGN(0.001)"), 1.0);
    assert_err("ABS()", ExcelError::Value);
    assert_err("ABS(1,2)", ExcelError::Value);
}

#[test]
fn sqrt_power_exp_logs() {
    assert_eq!(num("SQRT(16)"), 4.0);
    assert_eq!(num("SQRT(2)"), 2.0f64.sqrt());
    assert_err("SQRT(-1)", ExcelError::Num);

    assert_eq!(num("POWER(2,10)"), 1024.0);
    assert!((num("POWER(9,0.5)") - 3.0).abs() < 1e-12);
    assert_err("POWER(0,0)", ExcelError::Num);
    assert_err("POWER(0,-2)", ExcelError::DivZero);
    assert_err("POWER(-1,0.5)", ExcelError::Num); // NaN → #NUM!

    assert!((num("EXP(1)") - core::f64::consts::E).abs() < 1e-12);
    assert_eq!(num("EXP(0)"), 1.0);
    assert_err("EXP(1000)", ExcelError::Num); // inf → #NUM!

    assert!((num("LN(2.718281828459045)") - 1.0).abs() < 1e-12);
    assert_err("LN(0)", ExcelError::Num);
    assert_err("LN(-1)", ExcelError::Num);

    // Excel's LOG defaults to base 10, not e.
    assert_eq!(num("LOG(100)"), 2.0);
    assert!((num("LOG(8,2)") - 3.0).abs() < 1e-12);
    assert!((num("LOG10(1000)") - 3.0).abs() < 1e-12);
    assert_err("LOG(-5)", ExcelError::Num);
    assert_err("LOG(5,1)", ExcelError::DivZero);
}

#[test]
fn mod_takes_divisor_sign() {
    assert_eq!(num("MOD(3,2)"), 1.0);
    assert_eq!(num("MOD(-3,2)"), 1.0);
    assert_eq!(num("MOD(3,-2)"), -1.0);
    assert_eq!(num("MOD(-3,-2)"), -1.0);
    assert_eq!(num("MOD(5,5)"), 0.0);
    assert_err("MOD(5,0)", ExcelError::DivZero);
}

#[test]
fn int_trunc_floor_toward_minus_inf() {
    assert_eq!(num("INT(8.9)"), 8.0);
    assert_eq!(num("INT(-8.9)"), -9.0); // INT floors
    assert_eq!(num("TRUNC(-8.9)"), -8.0); // TRUNC truncates toward zero
    assert_eq!(num("TRUNC(8.9)"), 8.0);
    assert_eq!(num("TRUNC(3.14159,3)"), 3.141);
    assert_eq!(num("TRUNC(-3.14159,3)"), -3.141);
    assert_eq!(num("TRUNC(1234.5678,-2)"), 1200.0); // negative digits
    assert_eq!(num("INT(3)"), 3.0);
}

#[test]
fn round_is_half_away_from_zero() {
    assert_eq!(num("ROUND(2.5,0)"), 3.0);
    assert_eq!(num("ROUND(-2.5,0)"), -3.0); // not banker's rounding
    assert_eq!(num("ROUND(1.4,0)"), 1.0);
    assert_eq!(num("ROUND(1.5,0)"), 2.0);
    assert_eq!(num("ROUND(2.345,2)"), 2.35);
    assert_eq!(num("ROUND(1234.5678,-2)"), 1200.0);
    assert_eq!(num("ROUND(1250,-2)"), 1300.0);
    assert_eq!(num("ROUND(3.14159)"), 3.0); // digits omitted = 0
    assert_eq!(num("ROUND(1.98,)"), 2.0); // omitted slot = 0 digits
}

#[test]
fn roundup_rounddown() {
    assert_eq!(num("ROUNDUP(3.2,0)"), 4.0);
    assert_eq!(num("ROUNDUP(-3.2,0)"), -4.0); // away from zero
    assert_eq!(num("ROUNDUP(3.14159,3)"), 3.142);
    assert_eq!(num("ROUNDDOWN(3.9,0)"), 3.0);
    assert_eq!(num("ROUNDDOWN(-3.9,0)"), -3.0); // toward zero
    assert_eq!(num("ROUNDDOWN(3.99999,3)"), 3.999);
    assert_eq!(num("ROUNDUP(0,0)"), 0.0);
}

#[test]
fn ceiling_rules() {
    assert_eq!(num("CEILING(2.5,1)"), 3.0);
    assert_eq!(num("CEILING(2.5,2)"), 4.0);
    assert_eq!(num("CEILING(-2.5,2)"), -2.0); // toward +∞ scaled by sig
    assert_eq!(num("CEILING(-2.5,-2)"), -4.0); // both negative: away
    assert_eq!(num("CEILING(0,3)"), 0.0);
    assert_err("CEILING(2.5,0)", ExcelError::DivZero);
    assert_err("CEILING(2.5,-2)", ExcelError::Num); // positive n, negative sig
}

#[test]
fn floor_rules() {
    assert_eq!(num("FLOOR(2.5,1)"), 2.0);
    assert_eq!(num("FLOOR(2.5,2)"), 2.0);
    assert_eq!(num("FLOOR(-2.5,-2)"), -2.0);
    assert_eq!(num("FLOOR(0,3)"), 0.0);
    assert_err("FLOOR(2.5,0)", ExcelError::DivZero);
    // FLOOR is stricter than CEILING on mixed signs.
    assert_err("FLOOR(-2.5,2)", ExcelError::Num);
    assert_err("FLOOR(2.5,-2)", ExcelError::Num);
}

#[test]
fn date_functions_use_the_clock() {
    assert_eq!(num("TODAY()"), 45_123.0); // floor of 45123.75
    assert_eq!(num("NOW()"), 45_123.75);
    // TODAY is a real date: 45123 = 2023-07-16 (a Sunday).
    assert_eq!(text("TEXT(TODAY(),\"yyyy-mm-dd\")"), "2023-07-16");
}

#[test]
fn functions_are_case_insensitive() {
    assert_eq!(num("sum(1,2)"), 3.0);
    assert_eq!(num("Sum(1,2)"), 3.0);
    assert_eq!(num("SUM(1,2)"), 3.0);
}

#[test]
fn builtin_table_is_complete() {
    let table = formula_lang::builtin_functions();
    assert_eq!(table.len(), 63);
    // Every name dispatches (probe one per family with valid args).
    for probe in [
        "SUM(1)",
        "PRODUCT(1)",
        "ABS(1)",
        "SIGN(1)",
        "SQRT(1)",
        "POWER(1,1)",
        "EXP(0)",
        "LN(1)",
        "LOG(1)",
        "LOG10(1)",
        "MOD(1,1)",
        "INT(1)",
        "TRUNC(1)",
        "ROUND(1)",
        "ROUNDUP(1)",
        "ROUNDDOWN(1)",
        "CEILING(1,1)",
        "FLOOR(1,1)",
        "AVERAGE(1)",
        "COUNT(1)",
        "COUNTA(1)",
        "COUNTBLANK(A1)",
        "MAX(1)",
        "MIN(1)",
        "MEDIAN(1)",
        "STDEV(1,2)",
        "VAR(1,2)",
        "IF(1,1)",
        "AND(1)",
        "OR(1)",
        "NOT(1)",
        "IFERROR(1,2)",
        "IFNA(1,2)",
        "TRUE()",
        "FALSE()",
        "CONCATENATE(\"a\")",
        "LEFT(\"a\")",
        "RIGHT(\"a\")",
        "MID(\"a\",1,1)",
        "LEN(\"a\")",
        "LOWER(\"a\")",
        "UPPER(\"a\")",
        "PROPER(\"a\")",
        "TRIM(\"a\")",
        "SUBSTITUTE(\"a\",\"a\",\"b\")",
        "FIND(\"a\",\"a\")",
        "SEARCH(\"a\",\"a\")",
        "REPT(\"a\",1)",
        "TEXT(1,\"0\")",
        "IFERROR(VLOOKUP(1,A1:B2,1),0)",
        "IFERROR(HLOOKUP(1,A1:B2,1),0)",
        "INDEX(A1:B2,1,1)",
        "IFERROR(MATCH(1,A1:A2),0)",
        "OFFSET(A1,0,0)",
        "TODAY()",
        "NOW()",
        "ISBLANK(A1)",
        "ISNUMBER(1)",
        "ISTEXT(\"a\")",
        "ISLOGICAL(1)",
        "ISERROR(1)",
        "ISNA(1)",
        "ISREF(A1)",
    ] {
        let parsed = formula_lang::parse(probe).unwrap();
        let result = formula_lang::evaluate_with_clock(&parsed, &MapResolver::new(), NOW);
        assert!(result.is_ok(), "{probe} failed: {result:?}");
    }
    // The unknown one fails typed.
    assert!(matches!(
        formula_lang::evaluate_with_clock(
            &formula_lang::parse("NOTAFUNC(1)").unwrap(),
            &MapResolver::new(),
            NOW,
        ),
        Err(formula_lang::FormulaError::UnknownFunction(name)) if name == "NOTAFUNC"
    ));
}

// Re-export the sheet helper for this file's use sites above.
use common::sheet;
use formula_lang::{MapResolver, Value};

fn num_over(s: &MapResolver, f: &str) -> f64 {
    match ev_over(f, s) {
        Ok(Value::Number(n)) => n,
        other => panic!("{f}: expected number, got {other:?}"),
    }
}
