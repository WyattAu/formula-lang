//! The coercion matrix — every operand-type pairing for arithmetic,
//! comparison, and concatenation, pinned against Excel behavior.

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
fn arithmetic_matrix_number() {
    assert_eq!(num("5+5"), 10.0);
    assert_eq!(num("5+TRUE"), 6.0);
    assert_eq!(num("5+FALSE"), 5.0);
    assert_eq!(num("5+\"2\""), 7.0);
    assert_eq!(num("5+A1"), 5.0); // empty → 0
    assert_err("5+\"x\"", ExcelError::Value);
    assert_err("5+#N/A", ExcelError::NA);
}

#[test]
fn arithmetic_matrix_boolean() {
    assert_eq!(num("TRUE+TRUE"), 2.0);
    assert_eq!(num("TRUE*\"3\""), 3.0);
    assert_eq!(num("TRUE-A1"), 1.0);
    assert_eq!(num("FALSE^0.5"), 0.0);
    assert_err("TRUE+\"x\"", ExcelError::Value);
}

#[test]
fn arithmetic_matrix_text() {
    assert_eq!(num("\"2\"+\"3\""), 5.0);
    assert_eq!(num("\" 2 \"+1"), 3.0);
    assert_eq!(num("\"1e2\"+0"), 100.0); // scientific text parses
    assert_eq!(num("\".5\"*4"), 2.0);
    // Whitespace-trimmed parse; no locale separators, no booleans.
    assert_err("\"1,5\"+0", ExcelError::Value);
    assert_err("\"TRUE\"*1", ExcelError::Value);
    assert_err("\"\"+1", ExcelError::Value);
}

#[test]
fn arithmetic_matrix_empty_via_unary() {
    // Unary operators reach the same coercion.
    assert_eq!(num("-A1"), -0.0);
    assert_eq!(num("+A1"), 0.0);
    assert_eq!(num("-TRUE"), -1.0);
    assert_eq!(num("-\"2.5\""), -2.5);
    assert_err("-\"x\"", ExcelError::Value);
    assert_eq!(num("A1%"), 0.0);
    assert_eq!(num("TRUE%"), 0.01);
}

#[test]
fn comparison_matrix_cross_type() {
    // Rank: Number < Text < Boolean. Never coerces across.
    assert!(boolean("1<\"\"")); // any number < any text
    assert!(boolean("\"\"<FALSE")); // any text < any boolean
    assert!(boolean("999999<\"a\""));
    assert!(boolean("\"zzz\"<TRUE"));
    assert!(boolean("FALSE<TRUE"));
    assert!(!boolean("\"1\"=1"));
    assert!(!boolean("TRUE=1"));
    assert!(!boolean("TRUE=\"TRUE\""));
    assert!(boolean("1<>\"1\""));
}

#[test]
fn comparison_matrix_same_type() {
    assert!(boolean("\"A\"<\"B\""));
    assert!(boolean("\"b\"<\"C\"")); // case-insensitive: b == B < C
    assert!(boolean("\"abc\"<\"abd\""));
    assert!(boolean("\"abc\"<\"zzz\""));
    assert!(boolean("2>1"));
    assert!(boolean("2>=2"));
    assert!(boolean("TRUE>FALSE"));
}

#[test]
fn comparison_matrix_empty() {
    // Empty substitutes the other side's zero-value.
    assert!(boolean("A1=0"));
    assert!(boolean("A1=\"\""));
    assert!(boolean("A1=FALSE"));
    assert!(boolean("A1<1"));
    assert!(boolean("A1>=0"));
    assert!(boolean("NOT(A1>A2)")); // 0 > 0 false
    assert!(boolean("A1<>1"));
    // Empty vs empty: equal.
    assert!(boolean("A1=A2"));
    // Empty on the right too.
    assert!(boolean("0=A1"));
    assert!(boolean("FALSE<=A1"));
}

#[test]
fn comparison_errors_dominate() {
    assert_err("#N/A<1", ExcelError::NA);
    assert_err("1<=#N/A", ExcelError::NA);
    assert_err("\"a\">=#REF!", ExcelError::Ref);
    // Stored error cell: compare consumes → hard channel.
    let s = sheet();
    assert!(matches!(
        ev_over("E1>0", &s),
        Err(formula_lang::FormulaError::Eval(ExcelError::Ref))
    ));
}

#[test]
fn concat_matrix() {
    assert_eq!(text("\"a\"&\"b\""), "ab");
    assert_eq!(text("\"a\"&1"), "a1");
    assert_eq!(text("\"a\"&TRUE"), "aTRUE");
    assert_eq!(text("\"a\"&FALSE"), "aFALSE");
    assert_eq!(text("\"a\"&A1"), "a");
    assert_eq!(text("\"x\"&A1&\"y\""), "xy");
    // Number rendering through concat = General format.
    assert_eq!(text("1.0&\"\""), "1");
    assert_eq!(text("(1/3)&\"\""), "0.333333333333333");
    assert_eq!(text("(2^60)&\"\""), "1.15292150460685E+18");
    assert_eq!(text("(0.00001)&\"\""), "1E-05"); // below 1e-4: scientific
    assert_eq!(text("(0.000001)&\"\""), "1E-06");
    assert_eq!(text("(1e11)&\"\""), "1E+11");
    assert_eq!(text("(1e10)&\"\""), "10000000000");
    assert_err("\"a\"&1/0", ExcelError::DivZero);
    assert_err("\"a\"&#N/A", ExcelError::NA);
}

#[test]
fn bool_coercion_matrix() {
    assert_eq!(ev("IF(1,1,0)"), Ok(Value::Number(1.0)));
    assert_eq!(ev("IF(0.0,1,0)"), Ok(Value::Number(0.0)));
    assert_eq!(ev("IF(-1,1,0)"), Ok(Value::Number(1.0))); // nonzero → true
    assert_eq!(ev("IF(\"true\",1,0)"), Ok(Value::Number(1.0)));
    assert_eq!(ev("IF(\"False\",1,0)"), Ok(Value::Number(0.0)));
    assert_eq!(ev("IF(A1,1,0)"), Ok(Value::Number(0.0))); // empty → false
    assert_err("IF(\"0\",1,0)", ExcelError::Value); // numeric text is NOT bool text
    assert_err("IF(\" \",1,0)", ExcelError::Value);
}

#[test]
fn coercion_via_functions() {
    // SUM-style aggregates coerce direct scalars but skip range text/bools.
    assert_eq!(num_over("SUM(TRUE,\"2\")"), 3.0);
    assert_eq!(num_over("SUM(B1:B3)"), 2.5); // range text/bool skipped
    assert_eq!(num_over("AVERAGE(TRUE,3)"), 2.0);
    assert_eq!(num_over("MAX(\"5\",4)"), 5.0);
    // Comparison functions over mixed types.
    assert_eq!(text_over("CONCATENATE(1,\"-\",TRUE)"), "1-TRUE");
}

use common::sheet;

fn num_over(f: &str) -> f64 {
    match ev_over(f, &sheet()) {
        Ok(Value::Number(n)) => n,
        other => panic!("{f}: {other:?}"),
    }
}

fn text_over(f: &str) -> String {
    match ev_over(f, &sheet()) {
        Ok(Value::Text(t)) => t,
        other => panic!("{f}: {other:?}"),
    }
}
