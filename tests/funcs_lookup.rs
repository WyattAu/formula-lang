//! Known-answer tests for VLOOKUP / HLOOKUP / INDEX / MATCH / OFFSET.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

mod common;

use common::*;
use formula_lang::{ExcelError, MapResolver, Value};

/// A small product table at A1:C4 (row-major):
/// | 10 | "apple"  | 1.5 |
/// | 20 | "banana" | 2.5 |
/// | 30 | "cherry" | 3.5 |
/// | 40 | "date"   | 4.5 |
fn products() -> MapResolver {
    let mut s = MapResolver::new();
    let rows = [
        (10.0, "apple", 1.5),
        (20.0, "banana", 2.5),
        (30.0, "cherry", 3.5),
        (40.0, "date", 4.5),
    ];
    for (i, (code, name, price)) in rows.iter().enumerate() {
        let r = i as u32 + 1;
        s.set_num(1, r, *code);
        s.set_text(2, r, name);
        s.set_num(3, r, *price);
    }
    s
}

fn t_over(f: &str) -> String {
    match ev_over(f, &products()) {
        Ok(Value::Text(t)) => t,
        other => panic!("{f}: expected text, got {other:?}"),
    }
}

fn n_over(f: &str) -> f64 {
    match ev_over(f, &products()) {
        Ok(Value::Number(n)) => n,
        other => panic!("{f}: expected number, got {other:?}"),
    }
}

#[test]
fn vlookup_exact() {
    assert_eq!(t_over("VLOOKUP(20,A1:C4,2,FALSE)"), "banana");
    assert_eq!(n_over("VLOOKUP(20,A1:C4,3,FALSE)"), 2.5);
    assert_eq!(t_over("VLOOKUP(40,A1:C4,2,0)"), "date"); // 0 = exact
                                                         // Text keys only work against a text-keyed first column (B:C here).
    assert_eq!(n_over("VLOOKUP(\"CHERRY\",B1:C4,2,FALSE)"), 3.5);
    // Miss → #N/A.
    assert_err("VLOOKUP(25,A1:C4,2,FALSE)", ExcelError::NA);
    // Column index past the table width → #REF!.
    assert_err("VLOOKUP(20,A1:C4,4,FALSE)", ExcelError::Ref);
    // Column index < 1 → #VALUE!.
    assert_err("VLOOKUP(20,A1:C4,0,FALSE)", ExcelError::Value);
}

#[test]
fn vlookup_wildcard_exact() {
    assert_eq!(n_over("VLOOKUP(\"ban*\",B1:C4,2,FALSE)"), 2.5);
    assert_eq!(n_over("VLOOKUP(\"*erry\",B1:C4,2,FALSE)"), 3.5);
    assert_err("VLOOKUP(\"x*\",B1:C4,2,FALSE)", ExcelError::NA);
    assert_eq!(t_over("VLOOKUP(\"ban*\",B1:C4,1,FALSE)"), "banana"); // result col 1 = key
}

#[test]
fn vlookup_approximate_defaults_to_true() {
    // Sorted ascending: the largest key ≤ lookup wins.
    assert_eq!(t_over("VLOOKUP(25,A1:C4,2)"), "banana"); // default TRUE
    assert_eq!(t_over("VLOOKUP(25,A1:C4,2,TRUE)"), "banana");
    assert_eq!(t_over("VLOOKUP(20,A1:C4,2,TRUE)"), "banana"); // exact hop
    assert_eq!(n_over("VLOOKUP(999,A1:C4,3,TRUE)"), 4.5); // last row
                                                          // Below the first key → #N/A.
    assert_err("VLOOKUP(5,A1:C4,2,TRUE)", ExcelError::NA);
}

#[test]
fn hlookup() {
    // Transposed table at A1:D2:
    // row 1: 10 20 30 40 (keys)
    // row 2: names
    let mut s = MapResolver::new();
    for (i, code) in [10.0, 20.0, 30.0, 40.0].iter().enumerate() {
        s.set_num(i as u32 + 1, 1, *code);
    }
    for (i, name) in ["apple", "banana", "cherry", "date"].iter().enumerate() {
        s.set_text(i as u32 + 1, 2, name);
    }
    let got = |f: &str| match ev_over(f, &s) {
        Ok(Value::Text(t)) => t,
        other => panic!("{f}: {other:?}"),
    };
    assert_eq!(got("HLOOKUP(30,A1:D2,2,FALSE)"), "cherry");
    assert_eq!(got("HLOOKUP(35,A1:D2,2,TRUE)"), "cherry");
    assert_err("HLOOKUP(99,A1:D2,2,FALSE)", ExcelError::NA);
    assert_err("HLOOKUP(10,A1:D2,3,FALSE)", ExcelError::Ref); // row past bottom
}

#[test]
fn index_shapes() {
    // Rectangular: INDEX(range, row, col).
    assert_eq!(n_over("INDEX(A1:C4,2,1)"), 20.0);
    assert_eq!(t_over("INDEX(A1:C4,2,2)"), "banana");
    assert_eq!(n_over("INDEX(A1:C4,4,3)"), 4.5);
    // Single column: INDEX(range, n).
    assert_eq!(n_over("INDEX(A1:A4,3)"), 30.0);
    // Single row: the index addresses columns.
    assert_eq!(t_over("INDEX(A1:C1,2)"), "apple"); // B1 = "apple"
                                                   // Out of range → #REF!; zero position → #VALUE!.
    assert_err("INDEX(A1:C4,5,1)", ExcelError::Ref);
    assert_err("INDEX(A1:C4,1,4)", ExcelError::Ref);
    assert_err("INDEX(A1:C4,0,1)", ExcelError::Value);
    // Empty target reads as 0 (Excel reference semantics).
    let mut s = MapResolver::new();
    s.set_num(1, 1, 1.0);
    assert_eq!(
        match ev_over("INDEX(A1:B2,2,2)", &s) {
            Ok(Value::Number(n)) => n,
            other => panic!("{other:?}"),
        },
        0.0
    );
}

#[test]
fn match_types() {
    // Vector of keys at A1:A4.
    assert_eq!(n_over("MATCH(30,A1:A4,0)"), 3.0); // exact
    assert_eq!(n_over("MATCH(25,A1:A4)"), 2.0); // ascending approximate (default)
    assert_eq!(n_over("MATCH(25,A1:A4,1)"), 2.0);
    assert_eq!(n_over("MATCH(10,A1:A4,1)"), 1.0);
    assert_eq!(n_over("MATCH(999,A1:A4,1)"), 4.0);
    assert_err("MATCH(5,A1:A4,1)", ExcelError::NA);
    // Descending vector: E1:E4 = 4,3,2,1 (custom sheet).
    let mut desc = MapResolver::new();
    for (i, v) in [4.0, 3.0, 2.0, 1.0].iter().enumerate() {
        desc.set_num(5, i as u32 + 1, *v); // column E
    }
    assert_eq!(
        match ev_over("MATCH(2.5,E1:E4,-1)", &desc) {
            Ok(Value::Number(n)) => n,
            other => panic!("{other:?}"),
        },
        2.0
    );
    assert_eq!(
        match ev_over("MATCH(4,E1:E4,-1)", &desc) {
            Ok(Value::Number(n)) => n,
            other => panic!("{other:?}"),
        },
        1.0
    );
    // Wildcards in exact text match.
    assert_eq!(n_over("MATCH(\"ban*\",B1:B4,0)"), 2.0); // B1:B4 are names
    assert_err("MATCH(30,A1:B4,0)", ExcelError::NA); // 2-D vector → #N/A
}

#[test]
fn offset_shifts_through_resolver() {
    assert_eq!(n_over("OFFSET(A1,0,0)"), 10.0);
    assert_eq!(n_over("OFFSET(A1,1,0)"), 20.0); // down one row
    assert_eq!(n_over("OFFSET(A1,0,2)"), 1.5); // right two cols
    assert_eq!(t_over("OFFSET(B1,2,0)"), "cherry");
    // From a range: base is the top-left corner.
    assert_eq!(n_over("OFFSET(A1:B4,3,2)"), 4.5);
    // Empty target reads as 0.
    let mut s = MapResolver::new();
    s.set_num(1, 1, 9.0);
    assert_eq!(
        match ev_over("OFFSET(A1,1,1)", &s) {
            Ok(Value::Number(n)) => n,
            other => panic!("{other:?}"),
        },
        0.0
    );
    // Off-grid → #REF!.
    assert_err("OFFSET(A1,-1,0)", ExcelError::Ref);
    assert_err("OFFSET(A1,0,-1)", ExcelError::Ref);
    assert_err("OFFSET(A1,1048576,0)", ExcelError::Ref);
    // Multi-cell result → #VALUE! (scalar core).
    assert_err("OFFSET(A1,0,0,2,2)", ExcelError::Value);
    assert_err("OFFSET(A1,0,0,0,1)", ExcelError::Value);
    // Non-reference first arg → InvalidRange.
    assert!(matches!(
        ev_over("OFFSET(5,0,0)", &products()),
        Err(formula_lang::FormulaError::InvalidRange)
    ));
}

#[test]
fn lookup_over_fixture_sheet() {
    // C1:C4 = 1..4, D1:D4 = fruit names.
    assert_eq!(text_over("VLOOKUP(3,C1:D4,2,FALSE)"), "cherry");
    assert_eq!(text_over("INDEX(D1:D4,4)"), "date");
    assert_eq!(num_over("MATCH(\"date\",D1:D4,0)"), 4.0);
}

#[test]
fn lookup_errors_in_table_propagate() {
    // E1 holds #REF! on the fixture sheet.
    assert_err_over("VLOOKUP(1,E1:E1,1,FALSE)", &sheet(), ExcelError::Ref);
    assert_err_over("MATCH(1,E1:E1,0)", &sheet(), ExcelError::Ref);
}

use common::sheet;
