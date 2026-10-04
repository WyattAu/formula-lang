//! Known-answer tests for the logical family — lazy IF, AND/OR semantics,
//! and the error-catching pair.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

mod common;

use common::*;
use formula_lang::{ExcelError, Value};

#[test]
fn if_basic() {
    assert_eq!(num("IF(1,10,20)"), 10.0);
    assert_eq!(num("IF(0,10,20)"), 20.0);
    assert_eq!(text("IF(1<2,\"yes\",\"no\")"), "yes");
}

#[test]
fn if_conditions_coerce() {
    assert_eq!(num("IF(\"true\",1,2)"), 1.0);
    assert_eq!(num("IF(\"FALSE\",1,2)"), 2.0);
    assert_eq!(num("IF(3,1,2)"), 1.0);
    assert_eq!(num("IF(0.0,1,2)"), 2.0);
    assert_eq!(num("IF(A1,1,2)"), 2.0); // empty → FALSE
    assert_err("IF(\"maybe\",1,2)", ExcelError::Value);
    assert_err("IF(1/0,1,2)", ExcelError::DivZero);
}

#[test]
fn if_branch_omissions() {
    // Missing else → FALSE (Excel).
    assert_eq!(ev("IF(FALSE,1)"), Ok(Value::Boolean(false)));
    assert_eq!(ev("IF(TRUE,5)"), Ok(Value::Number(5.0)));
    // Empty then slot → 0.
    assert_eq!(num("IF(TRUE,,5)"), 0.0);
    assert_eq!(num("IF(FALSE,,5)"), 5.0);
    // Empty else slot → 0 (not FALSE!).
    assert_eq!(num("IF(FALSE,1,)"), 0.0);
}

#[test]
fn if_is_lazy() {
    // Errors in the untaken branch never surface.
    assert_eq!(num("IF(TRUE,1,1/0)"), 1.0);
    assert_eq!(num("IF(FALSE,1/0,2)"), 2.0);
    assert_eq!(num("IF(TRUE,1,FOO())"), 1.0); // even unknown functions
}

#[test]
fn if_nested() {
    // A1 = 10 on the fixture sheet.
    assert_eq!(text_over("IF(A1>10,\"big\",\"small\")"), "small");
    assert_eq!(text_over("IF(A1>=10,\"big\",\"small\")"), "big");
    assert_eq!(text_over("IF(A1<=10,\"low\",\"high\")"), "low");
}

#[test]
fn and_or() {
    assert_eq!(ev("AND(TRUE,TRUE)"), Ok(Value::Boolean(true)));
    assert_eq!(ev("AND(TRUE,FALSE)"), Ok(Value::Boolean(false)));
    assert_eq!(ev("OR(FALSE,TRUE)"), Ok(Value::Boolean(true)));
    assert_eq!(ev("OR(FALSE,0)"), Ok(Value::Boolean(false)));
    // Numbers coerce: nonzero is TRUE.
    assert_eq!(ev("AND(1,2,3)"), Ok(Value::Boolean(true)));
    assert_eq!(ev("OR(0,0.0)"), Ok(Value::Boolean(false)));
    // Text "TRUE"/"FALSE" coerces in direct args.
    assert_eq!(ev("AND(\"TRUE\",1)"), Ok(Value::Boolean(true)));
    assert_err("AND(\"yes\")", ExcelError::Value);
    // Errors always propagate (Excel does NOT short-circuit AND/OR).
    assert_err("AND(FALSE,1/0)", ExcelError::DivZero);
    assert_err("OR(TRUE,1/0)", ExcelError::DivZero);
    // No logical values at all → #VALUE!.
    assert_err("AND(Z1:Z9)", ExcelError::Value);
}

#[test]
fn and_or_over_ranges() {
    let s = sheet(); // B3 = TRUE
                     // B3 is TRUE, B1 is text (skipped in ranges), B2 is 2.5 → TRUE.
    assert_eq!(ev_over("AND(B2:B3)", &s), Ok(Value::Boolean(true)));
    assert_eq!(ev_over("OR(B1:B2)", &s), Ok(Value::Boolean(true)));
    // All-empty range: no logical values → #VALUE!.
    assert_err("OR(B4:B4)", ExcelError::Value);
}

#[test]
fn not() {
    assert_eq!(ev("NOT(TRUE)"), Ok(Value::Boolean(false)));
    assert_eq!(ev("NOT(0)"), Ok(Value::Boolean(true)));
    assert_eq!(ev("NOT(\"false\")"), Ok(Value::Boolean(true)));
    assert_err("NOT(\"x\")", ExcelError::Value);
    assert_err("NOT()", ExcelError::Value);
    assert_err("NOT(1,2)", ExcelError::Value);
}

#[test]
fn iferror_catches_everything() {
    assert_eq!(num("IFERROR(1/0,42)"), 42.0);
    assert_eq!(text("IFERROR(#N/A,\"caught\")"), "caught");
    assert_eq!(num("IFERROR(7,42)"), 7.0); // no error → value
                                           // An empty cell is NOT an error — IFERROR passes it through.
    assert_eq!(ev("IFERROR(A1,\"x\")"), Ok(Value::Empty));
    // Fallback itself can fail.
    assert_err("IFERROR(1/0,2/0)", ExcelError::DivZero);
    // Unknown functions are NOT catchable (hard typed error).
    assert!(matches!(
        ev("IFERROR(FOO(),1)"),
        Err(formula_lang::FormulaError::UnknownFunction(_))
    ));
}

#[test]
fn ifna_catches_only_na() {
    assert_eq!(num("IFNA(#N/A,5)"), 5.0);
    assert_eq!(text("IFNA(#N/A,\"na\")"), "na");
    // Non-NA errors pass through.
    assert_err("IFNA(1/0,5)", ExcelError::DivZero);
    assert_err("IFNA(#REF!,5)", ExcelError::Ref);
    assert_eq!(num("IFNA(3,5)"), 3.0);
}

#[test]
fn true_false_functions() {
    assert_eq!(ev("TRUE()"), Ok(Value::Boolean(true)));
    assert_eq!(ev("FALSE()"), Ok(Value::Boolean(false)));
    assert_err("TRUE(1)", ExcelError::Value);
    // Bare literals too.
    assert_eq!(ev("TRUE"), Ok(Value::Boolean(true)));
    assert_eq!(num("IF(TRUE(),1,2)"), 1.0);
}

#[test]
fn boolean_results_are_bool_not_number() {
    assert_eq!(ev("1=1"), Ok(Value::Boolean(true)));
    assert_eq!(ev("AND(1)"), Ok(Value::Boolean(true)));
    // Coercing back into arithmetic works.
    assert_eq!(num("(1=1)*10"), 10.0);
    assert_eq!(num("(1=2)+5"), 5.0);
}
