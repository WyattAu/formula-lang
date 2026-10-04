//! Shared fixtures for the integration tests.
// Test harness: assertions legitimately panic; the lib target remains
// lint-clean. Each test binary links this module and uses a subset of the
// helpers, hence the dead_code lift.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

use formula_lang::{evaluate_with_clock, parse, FormulaError, MapResolver, Value};

pub const NOW: f64 = 45_123.75; // fixed clock for deterministic tests

/// Parse + evaluate over an empty sheet.
pub fn ev(input: &str) -> Result<Value, FormulaError> {
    let e = parse(input)?;
    evaluate_with_clock(&e, &formula_lang::EmptyResolver, NOW)
}

/// Parse + evaluate over a prepared sheet.
pub fn ev_over(input: &str, sheet: &MapResolver) -> Result<Value, FormulaError> {
    let e = parse(input)?;
    evaluate_with_clock(&e, sheet, NOW)
}

/// Number-valued formula convenience.
pub fn num(input: &str) -> f64 {
    match ev(input) {
        Ok(Value::Number(n)) => n,
        other => panic!("{input}: expected number, got {other:?}"),
    }
}

/// Text-valued formula convenience.
pub fn text(input: &str) -> String {
    match ev(input) {
        Ok(Value::Text(s)) => s,
        other => panic!("{input}: expected text, got {other:?}"),
    }
}

/// Bool-valued formula convenience.
pub fn boolean(input: &str) -> bool {
    match ev(input) {
        Ok(Value::Boolean(b)) => b,
        other => panic!("{input}: expected bool, got {other:?}"),
    }
}

/// Formula must evaluate to exactly this Excel error.
#[track_caller]
pub fn assert_err(input: &str, expected: formula_lang::ExcelError) {
    match ev(input) {
        Err(FormulaError::Eval(e)) if e == expected => {}
        other => panic!("{input}: expected {expected}, got {other:?}"),
    }
}

/// Formula must evaluate to exactly this Excel error over `sheet`.
#[track_caller]
pub fn assert_err_over(input: &str, sheet: &MapResolver, expected: formula_lang::ExcelError) {
    match ev_over(input, sheet) {
        Err(FormulaError::Eval(e)) if e == expected => {}
        other => panic!("{input}: expected {expected}, got {other:?}"),
    }
}

/// Formula must fail to parse (any parse/tokenize/limit failure).
#[track_caller]
pub fn assert_parse_err(input: &str) -> FormulaError {
    match parse(input) {
        Err(e) => e,
        Ok(e) => panic!("{input}: expected parse error, got {e:?}"),
    }
}

/// Number result over the standard sheet.
#[track_caller]
pub fn num_over(f: &str) -> f64 {
    match ev_over(f, &sheet()) {
        Ok(Value::Number(n)) => n,
        other => panic!("{f}: expected number, got {other:?}"),
    }
}

/// Text result over the standard sheet.
#[track_caller]
pub fn text_over(f: &str) -> String {
    match ev_over(f, &sheet()) {
        Ok(Value::Text(t)) => t,
        other => panic!("{f}: expected text, got {other:?}"),
    }
}

/// Bool result over the standard sheet.
#[track_caller]
pub fn bool_over(f: &str) -> bool {
    match ev_over(f, &sheet()) {
        Ok(Value::Boolean(b)) => b,
        other => panic!("{f}: expected bool, got {other:?}"),
    }
}

/// A standard test sheet used by the function suites.
pub fn sheet() -> MapResolver {
    let mut s = MapResolver::new();
    // A1:A4 numbers (col 1, rows 1..=4)
    s.set_num(1, 1, 10.0);
    s.set_num(1, 2, 20.0);
    s.set_num(1, 3, 30.0);
    s.set_num(1, 4, 40.0);
    // B1:B4 mixed
    s.set_text(2, 1, "alpha");
    s.set_num(2, 2, 2.5);
    s.set(2, 3, Value::Boolean(true));
    // B4 left empty
    // C1:C4 numbers for lookups
    s.set_num(3, 1, 1.0);
    s.set_num(3, 2, 2.0);
    s.set_num(3, 3, 3.0);
    s.set_num(3, 4, 4.0);
    // D1:D4 names
    s.set_text(4, 1, "apple");
    s.set_text(4, 2, "banana");
    s.set_text(4, 3, "cherry");
    s.set_text(4, 4, "date");
    // E1 holds an error value
    s.set(5, 1, Value::Error(formula_lang::ExcelError::Ref));
    s
}
