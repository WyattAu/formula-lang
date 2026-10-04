//! Evaluation — operators, coercions, comparisons, error channels,
//! depth-capping, and the resolver contract.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

mod common;

use common::*;
use formula_lang::{ExcelError, Expr, FormulaError, MapResolver, Value, MAX_DEPTH};

#[test]
fn arithmetic_operators() {
    assert_eq!(num("1+2"), 3.0);
    assert_eq!(num("5-8"), -3.0);
    assert_eq!(num("6*7"), 42.0);
    assert_eq!(num("7/2"), 3.5);
    assert_eq!(num("2^10"), 1024.0);
    assert_eq!(num("-2^2"), 4.0); // Excel quirk: (-2)^2
    assert_eq!(num("2^-2"), 0.25);
    assert_eq!(num("2^3^2"), 512.0); // right-assoc
    assert_eq!(num("--5"), 5.0);
    assert_eq!(num("+-5"), -5.0);
    assert_eq!(num("50%"), 0.5);
    assert_eq!(num("200%%"), 0.02);
    assert_eq!(num("1+2*3"), 7.0);
    assert_eq!(num("(1+2)*3"), 9.0);
}

#[test]
fn arithmetic_error_corners() {
    assert_err("1/0", ExcelError::DivZero);
    assert_err("0/0", ExcelError::DivZero);
    assert_err("MOD(5,0)", ExcelError::DivZero);
    assert_err("0^0", ExcelError::Num);
    assert_err("0^-1", ExcelError::DivZero);
    // Overflow folds to #NUM!.
    assert_err("10^308*10", ExcelError::Num);
    // Non-finite literals are #NUM! at use.
    assert_err("1e999", ExcelError::Num);
    assert_err("-(1e999)", ExcelError::Num);
}

#[test]
fn concat_operator() {
    assert_eq!(text("\"a\"&\"b\""), "ab");
    assert_eq!(text("1&2"), "12");
    assert_eq!(text("1.5&\"x\""), "1.5x");
    assert_eq!(text("TRUE&\"!\""), "TRUE!");
    assert_eq!(text("\"a\"&A1"), "a"); // empty cell concatenates as ""
                                       // 15-significant-digit General format.
    assert_eq!(text("0.1+0.2&\"\""), "0.3");
    assert_eq!(text("1E+20&\"\""), "1E+20");
}

#[test]
fn comparison_operators_numbers() {
    assert!(boolean("1=1"));
    assert!(!boolean("1=2"));
    assert!(boolean("1<>2"));
    assert!(boolean("2>1"));
    assert!(boolean("1<2"));
    assert!(boolean("1<=1"));
    assert!(boolean("1>=1"));
    // Chains are left-associative: (1<2)<3 → TRUE<3 → bool>num → FALSE.
    assert!(!boolean("1<2<3"));
    assert!(boolean("3>2>1")); // (3>2)>1 → TRUE>1 → FALSE? no: TRUE vs 1 → bool>num → ">" is TRUE.
}

#[test]
fn comparison_text_semantics() {
    // Case-insensitive equality (Excel).
    assert!(boolean("\"abc\"=\"ABC\""));
    assert!(!boolean("\"abc\"<>\"ABC\""));
    assert!(boolean("\"apple\"<\"banana\""));
    // No cross-type coercion: number < text always.
    assert!(boolean("99999<\"a\""));
    assert!(boolean("\"a\"<TRUE"));
    // Booleans above text above numbers.
    assert!(boolean("TRUE>\"zzz\""));
}

#[test]
fn comparison_empty_substitution() {
    // An empty cell substitutes the other side's zero value.
    assert!(boolean("A1=0"));
    assert!(boolean("A1=\"\""));
    assert!(boolean("A1=FALSE"));
    assert!(boolean("0=A1"));
    assert!(!boolean("A1=1"));
}

#[test]
fn arithmetic_coercion_rules() {
    // Booleans coerce.
    assert_eq!(num("TRUE+1"), 2.0);
    assert_eq!(num("FALSE*10"), 0.0);
    // Numeric text coerces.
    assert_eq!(num("\"3\"+1"), 4.0);
    assert_eq!(num("\" 2.5 \"+0"), 2.5);
    // Non-numeric text is #VALUE!.
    assert_err("\"abc\"+1", ExcelError::Value);
    // Boolean TEXT never parses as a number (Excel).
    assert_err("\"TRUE\"+1", ExcelError::Value);
    // Empty is zero.
    assert_eq!(num("A1+5"), 5.0);
    // Unary + still demands a number (Excel).
    assert_err("+\"a\"", ExcelError::Value);
}

#[test]
fn error_literals_propagate() {
    for (lit, e) in [
        ("#DIV/0!", ExcelError::DivZero),
        ("#N/A", ExcelError::NA),
        ("#NAME?", ExcelError::Name),
        ("#NULL!", ExcelError::Null),
        ("#NUM!", ExcelError::Num),
        ("#REF!", ExcelError::Ref),
        ("#VALUE!", ExcelError::Value),
    ] {
        assert_err(lit, e);
        assert_err(&format!("1+{lit}"), e);
        // Documented marker trade-off: a literal `#NULL!` in a direct
        // argument slot is indistinguishable from an omitted slot and is
        // treated as omitted (`SUM(1,#NULL!)` = 1). Every other literal
        // propagates through aggregates.
        if e != ExcelError::Null {
            assert_err(&format!("SUM(1,{lit})"), e);
        }
    }
}

#[test]
fn errors_dominate_coercion() {
    // Evaluation is left-to-right: an error operand propagates before the
    // other side is even evaluated.
    assert_err("#N/A+\"x\"", ExcelError::NA);
    assert_err("#REF!+\"x\"", ExcelError::Ref);
    assert_err("\"x\"+1/0", ExcelError::Value);
}

#[test]
fn cell_data_errors_are_values_until_used() {
    let s = sheet(); // E1 holds #REF!
                     // Pure fetch: the error stays data.
    assert_eq!(ev_over("E1", &s), Ok(Value::Error(ExcelError::Ref)));
    // ISERROR observes it.
    assert!(bool_over("ISERROR(E1)"));
    // The moment an operator consumes it, it becomes the hard channel.
    assert!(matches!(
        ev_over("E1+1", &s),
        Err(FormulaError::Eval(ExcelError::Ref))
    ));
    // IFERROR catches both channels.
    assert_eq!(text_over("IFERROR(E1,\"caught\")"), "caught");
}

#[test]
fn lazy_if_skips_untaken_branch() {
    // Division by zero in the untaken arm never happens.
    assert_eq!(num("IF(TRUE,1,1/0)"), 1.0);
    assert_eq!(num("IF(FALSE,1/0,2)"), 2.0);
}

#[test]
fn bare_range_is_value_error() {
    // Scalar core: a range used as a value is #VALUE! (documented).
    assert_err("A1:B2", ExcelError::Value);
    // ...but works as a function argument.
    assert_eq!(num_over("SUM(A1:A4)"), 100.0);
}

#[test]
fn unknown_function_is_typed() {
    assert_eq!(
        ev("FOO(1)"),
        Err(FormulaError::UnknownFunction("FOO".into()))
    );
    // Case-insensitive dispatch: lowercase user-built names still resolve.
    let e = Expr::Function {
        name: "sum".into(),
        args: vec![Expr::Number(1.0), Expr::Number(2.0)],
    };
    assert_eq!(
        formula_lang::evaluate_with_clock(&e, &formula_lang::EmptyResolver, NOW),
        Ok(Value::Number(3.0))
    );
}

#[test]
fn recursion_limit_is_typed_not_fatal() {
    // Build a 50k-deep left chain: parse iterates fine; evaluation
    // reports RecursionLimit instead of overflowing the stack.
    let mut f = String::from("0");
    let mut e = Expr::Number(0.0);
    for _ in 0..50_000 {
        e = Expr::Binary {
            op: formula_lang::BinaryOp::Add,
            left: Box::new(e),
            right: Box::new(Expr::Number(1.0)),
        };
        f.push_str("+1");
    }
    let _ = f;
    assert_eq!(
        formula_lang::evaluate_with_clock(&e, &formula_lang::EmptyResolver, NOW),
        Err(FormulaError::RecursionLimit)
    );
}

#[test]
fn depth_just_under_limit_evaluates() {
    let mut e = Expr::Number(0.0);
    for _ in 0..(MAX_DEPTH - 2) {
        e = Expr::Binary {
            op: formula_lang::BinaryOp::Add,
            left: Box::new(e),
            right: Box::new(Expr::Number(1.0)),
        };
    }
    assert_eq!(
        formula_lang::evaluate_with_clock(&e, &formula_lang::EmptyResolver, NOW),
        Ok(Value::Number((MAX_DEPTH - 2) as f64))
    );
}

#[test]
fn empty_resolver_and_map_resolver() {
    let empty = formula_lang::EmptyResolver;
    assert_eq!(
        formula_lang::evaluate_with_clock(&formula_lang::parse("A1+1").unwrap(), &empty, NOW),
        Ok(Value::Number(1.0))
    );
    let mut m = MapResolver::new();
    m.set_num(1, 1, 41.0);
    assert_eq!(
        formula_lang::evaluate_with_clock(&formula_lang::parse("A1+1").unwrap(), &m, NOW),
        Ok(Value::Number(42.0))
    );
}

#[test]
fn volatile_clock_control() {
    // TODAY floors the clock; NOW passes it through.
    assert_eq!(num("TODAY()"), NOW.floor());
    assert_eq!(num("NOW()"), NOW);
    // Determinism: evaluate is stable under a fixed clock.
    assert_eq!(num("NOW()"), num("NOW()"));
    // OFFSET is volatile; SUM is not.
    assert!(formula_lang::is_volatile(
        &formula_lang::parse("OFFSET(A1,1,0)").unwrap()
    ));
    assert!(formula_lang::is_volatile(
        &formula_lang::parse("TODAY()").unwrap()
    ));
    assert!(formula_lang::is_volatile(
        &formula_lang::parse("NOW()+1").unwrap()
    ));
    assert!(!formula_lang::is_volatile(
        &formula_lang::parse("SUM(A1:B2)").unwrap()
    ));
    assert!(!formula_lang::is_volatile(
        &formula_lang::parse("1+1").unwrap()
    ));
    // Deep ASTs do not overflow is_volatile (iterative walk).
    let mut e = Expr::Number(0.0);
    for _ in 0..50_000 {
        e = Expr::Binary {
            op: formula_lang::BinaryOp::Add,
            left: Box::new(e),
            right: Box::new(formula_lang::parse("TODAY()").unwrap()),
        };
    }
    assert!(formula_lang::is_volatile(&e));
}

#[test]
fn range_layout_is_row_major() {
    let mut s = MapResolver::new();
    // 2x3 grid: A1..C2 filled 1..=6 row-major.
    let mut v = 1.0;
    for r in 1..=2u32 {
        for c in 1..=3u32 {
            s.set_num(c, r, v);
            v += 1.0;
        }
    }
    // INDEX reads (row, col) over the row-major layout.
    let at = |f: &str| match ev_over(f, &s) {
        Ok(Value::Number(n)) => n,
        other => panic!("{f}: expected number, got {other:?}"),
    };
    assert_eq!(at("INDEX(A1:C2,1,1)"), 1.0);
    assert_eq!(at("INDEX(A1:C2,1,3)"), 3.0);
    assert_eq!(at("INDEX(A1:C2,2,1)"), 4.0);
    assert_eq!(at("INDEX(A1:C2,2,3)"), 6.0);
}
