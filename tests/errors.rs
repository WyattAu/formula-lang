//! Error taxonomy surface — every variant, its display, and the
//! value-vs-error channel contract.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

mod common;

use common::*;
use formula_lang::{ExcelError, FormulaError, Value};

#[test]
fn excel_error_literals_and_display() {
    let pairs = [
        (ExcelError::DivZero, "#DIV/0!"),
        (ExcelError::NA, "#N/A"),
        (ExcelError::Name, "#NAME?"),
        (ExcelError::Null, "#NULL!"),
        (ExcelError::Num, "#NUM!"),
        (ExcelError::Ref, "#REF!"),
        (ExcelError::Value, "#VALUE!"),
    ];
    for (e, lit) in pairs {
        assert_eq!(e.literal(), lit);
        assert_eq!(e.to_string(), lit);
        assert_eq!(ExcelError::from_literal(lit), Some(e));
    }
    assert_eq!(ExcelError::from_literal("#nope"), None);
    assert_eq!(ExcelError::from_literal("#NA"), None);
}

#[test]
fn formula_error_display_and_helpers() {
    assert_eq!(
        FormulaError::Tokenize { pos: 3, char: '@' }.to_string(),
        "tokenize error at byte 3: unexpected '@'"
    );
    let pe = FormulaError::Parse {
        pos: 1,
        expected: "')'",
        got: ",".into(),
    };
    assert!(pe.to_string().contains("expected ')'"));
    assert!(pe.to_string().contains("got ,"));
    assert_eq!(
        FormulaError::Eval(ExcelError::DivZero).to_string(),
        "evaluation error: #DIV/0!"
    );
    assert_eq!(
        FormulaError::UnknownFunction("NOPE".into()).to_string(),
        "unknown function: NOPE"
    );
    assert_eq!(
        FormulaError::RecursionLimit.to_string(),
        "recursion limit exceeded"
    );
    assert_eq!(
        FormulaError::InvalidRange.to_string(),
        "invalid range argument"
    );
    // excel_error() accessor
    assert_eq!(
        FormulaError::Eval(ExcelError::Num).excel_error(),
        Some(ExcelError::Num)
    );
    assert_eq!(FormulaError::RecursionLimit.excel_error(), None);
    // From<ExcelError>
    assert_eq!(
        FormulaError::from(ExcelError::NA),
        FormulaError::Eval(ExcelError::NA)
    );
}

#[test]
fn tokenize_errors_carry_positions() {
    assert!(matches!(
        formula_lang::parse("1 ? 2"),
        Err(FormulaError::Tokenize { pos: 2, char: '?' })
    ));
    assert!(matches!(
        formula_lang::parse("\"open"),
        Err(FormulaError::Tokenize { pos: 0, char: '"' })
    ));
    assert!(matches!(
        formula_lang::parse("#wat"),
        Err(FormulaError::Tokenize { pos: 0, char: '#' })
    ));
    assert!(matches!(
        formula_lang::parse("$A"),
        Err(FormulaError::Tokenize { pos: 0, char: '$' })
    ));
}

#[test]
fn parse_errors_carry_context() {
    let e = formula_lang::parse("SUM(1 2)").unwrap_err();
    match e {
        FormulaError::Parse { pos, expected, got } => {
            assert_eq!(got, "2");
            assert!(pos >= 6);
            assert!(!expected.is_empty());
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn eval_errors_are_iferror_catchable_but_hard_at_top() {
    // Hard at top:
    assert!(matches!(
        ev("1/0"),
        Err(FormulaError::Eval(ExcelError::DivZero))
    ));
    // Catchable inside:
    assert_eq!(num("IFERROR(1/0,-1)"), -1.0);
    // Nested catches: inner wins.
    assert_eq!(num("IFERROR(IFERROR(1/0,#REF!),0)"), 0.0);
    // Non-NA passes through IFNA.
    assert!(matches!(
        ev("IFNA(#REF!,0)"),
        Err(FormulaError::Eval(ExcelError::Ref))
    ));
}

#[test]
fn stored_errors_are_data_until_consumed() {
    let s = sheet(); // E1 = #REF!
                     // Passthrough: data channel.
    assert_eq!(ev_over("E1", &s), Ok(Value::Error(ExcelError::Ref)));
    assert_eq!(ev_over("E1", &s), ev_over("E1", &s));
    // Consumption: hard channel.
    assert!(matches!(
        ev_over("-E1", &s),
        Err(FormulaError::Eval(ExcelError::Ref))
    ));
    assert!(matches!(
        ev_over("E1&\"x\"", &s),
        Err(FormulaError::Eval(ExcelError::Ref))
    ));
    assert!(matches!(
        ev_over("E1=1", &s),
        Err(FormulaError::Eval(ExcelError::Ref))
    ));
    // Even ISNUMBER sees the data channel.
    assert_eq!(ev_over("ISNUMBER(E1)", &s), Ok(Value::Boolean(false)));
}

#[test]
fn unknown_function_names_are_normalized_upper() {
    match ev("notafunc(1)") {
        Err(FormulaError::UnknownFunction(n)) => assert_eq!(n, "NOTAFUNC"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn invalid_range_channel() {
    // Non-reference where a range is required.
    assert_eq!(ev("OFFSET(5,0,0)"), Err(FormulaError::InvalidRange));
    assert_eq!(ev("VLOOKUP(1,5,1)"), Err(FormulaError::InvalidRange));
    assert_eq!(ev("INDEX(42,1)"), Err(FormulaError::InvalidRange));
}

#[test]
fn error_display_roundtrip_in_formulas() {
    // Error literals serialize back and re-parse.
    for lit in [
        "#DIV/0!", "#N/A", "#NAME?", "#NULL!", "#NUM!", "#REF!", "#VALUE!",
    ] {
        let e = formula_lang::parse(lit).unwrap();
        assert_eq!(formula_lang::to_formula(&e), lit);
    }
}

#[test]
fn recursion_and_range_are_not_excel_errors() {
    // These two are structural, not worksheet values.
    assert!(matches!(
        ev(&format!("1{}", "+1".repeat(10_000))),
        Err(FormulaError::RecursionLimit) // 10k left-chain exceeds MAX_DEPTH at eval
    ));
    assert_eq!(
        FormulaError::RecursionLimit.excel_error(),
        None,
        "recursion limit is not an Excel error"
    );
    assert_eq!(FormulaError::InvalidRange.excel_error(), None);
}

use common::sheet;
