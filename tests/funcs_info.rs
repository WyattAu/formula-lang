//! Known-answer tests for the 7 information functions.

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
fn isblank() {
    assert_eq!(ev("ISBLANK(A1)"), Ok(Value::Boolean(true))); // empty sheet
    assert!(bool_over("NOT(ISBLANK(A1))")); // A1 = 10 on the fixture sheet
    assert!(bool_over("ISBLANK(B4)")); // B4 left empty
                                       // Empty STRING is text, not blank (Excel).
    assert!(bool_over(r#"NOT(ISBLANK(""))"#));
    assert!(bool_over("NOT(ISBLANK(0))"));
    // Errors are not blank.
    assert!(bool_over("NOT(ISBLANK(1/0))"));
    assert!(bool_over("NOT(ISBLANK(E1))")); // error cell
}

#[test]
fn isnumber_istext_islogical() {
    assert!(bool_over("ISNUMBER(A1)"));
    assert!(bool_over("NOT(ISNUMBER(B1))")); // text
    assert!(bool_over("NOT(ISNUMBER(B3))")); // bool
    assert!(bool_over(r#"NOT(ISNUMBER("5"))"#)); // text literal is not a number
                                                 // Errors are not numbers (both channels).
    assert!(bool_over("NOT(ISNUMBER(1/0))"));
    assert!(bool_over("NOT(ISNUMBER(E1))"));

    assert!(bool_over("ISTEXT(B1)"));
    assert!(bool_over("NOT(ISTEXT(A1))"));
    assert!(bool_over(r#"ISTEXT("")"#));

    assert!(bool_over("ISLOGICAL(B3)"));
    assert!(bool_over("NOT(ISLOGICAL(1))"));
    assert!(bool_over("ISLOGICAL(TRUE)"));
    assert!(bool_over(r#"NOT(ISLOGICAL("TRUE"))"#));
}

#[test]
fn iserror_catches_both_channels() {
    assert!(bool_over("ISERROR(E1)")); // stored error
    assert!(bool_over("ISERROR(1/0)")); // evaluation error
    assert!(bool_over("ISERROR(#N/A)"));
    assert!(bool_over("NOT(ISERROR(1))"));
    assert!(bool_over(r#"NOT(ISERROR("x"))"#));
    assert!(bool_over("NOT(ISERROR(A1))"));
    assert!(bool_over("NOT(ISERROR(B4))")); // empty is not an error
}

#[test]
fn isna_is_selective() {
    assert!(bool_over("ISNA(#N/A)"));
    // E1 is #REF!, not #N/A.
    assert!(bool_over("NOT(ISNA(E1))"));
    assert!(bool_over("NOT(ISNA(1/0))"));
    // VLOOKUP miss → #N/A → observable.
    assert!(bool_over("ISNA(VLOOKUP(99,A1:C4,2,FALSE))"));
    assert!(bool_over("NOT(ISNA(VLOOKUP(10,A1:C4,2,FALSE)))"));
}

#[test]
fn isref_is_syntactic() {
    assert!(bool_over("ISREF(A1)"));
    assert!(bool_over("ISREF(A1:B2)"));
    assert!(bool_over("ISREF($X$99)"));
    assert!(bool_over("NOT(ISREF(1))"));
    assert!(bool_over(r#"NOT(ISREF("A1"))"#));
    assert!(bool_over("NOT(ISREF(SUM(A1:A2)))"));
    // The resolver never sees the argument — an unfetched ref is still a ref.
    assert_eq!(ev("ISREF(XFD1048576)"), Ok(Value::Boolean(true)));
}

#[test]
fn predicates_observe_errors() {
    assert_eq!(ev("ISNUMBER(#N/A)"), Ok(Value::Boolean(false)));
    assert_eq!(ev("ISNA(#N/A)"), Ok(Value::Boolean(true)));
    assert_eq!(ev("ISERROR(#REF!)"), Ok(Value::Boolean(true)));
}

#[test]
fn wrong_arity_is_value_error() {
    for f in [
        "ISBLANK()",
        "ISNUMBER(1,2)",
        "ISTEXT()",
        "ISLOGICAL(1,2)",
        "ISERROR()",
        "ISNA(1,2)",
        "ISREF()",
    ] {
        assert_err(f, ExcelError::Value);
    }
}
