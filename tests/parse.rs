//! Parser — grammar, precedence at every level, associativity, refs and
//! ranges, function calls with omitted slots, and every error path.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

use formula_lang::{
    parse, to_formula, BinaryOp as B, CellRef, ExcelError, Expr, FormulaError, UnaryOp as U,
};

fn bin(op: B, l: Expr, r: Expr) -> Expr {
    Expr::Binary {
        op,
        left: Box::new(l),
        right: Box::new(r),
    }
}

fn n(x: f64) -> Expr {
    Expr::Number(x)
}

fn cell(col: u32, row: u32) -> Expr {
    Expr::CellRef {
        col,
        row,
        col_abs: false,
        row_abs: false,
    }
}

#[test]
fn literals() {
    assert_eq!(parse("42"), Ok(n(42.0)));
    assert_eq!(parse("3.5"), Ok(n(3.5)));
    assert_eq!(parse(".25"), Ok(n(0.25)));
    assert_eq!(parse("1e2"), Ok(n(100.0)));
    assert_eq!(parse("\"hi\""), Ok(Expr::Text("hi".into())));
    assert_eq!(parse("\"a\"\"b\""), Ok(Expr::Text("a\"b".into())));
    assert_eq!(parse("TRUE"), Ok(Expr::Boolean(true)));
    assert_eq!(parse("false"), Ok(Expr::Boolean(false)));
    assert_eq!(parse("=1+1"), parse("1+1")); // optional leading `=`
}

#[test]
fn error_literals() {
    for (lit, e) in [
        ("#DIV/0!", ExcelError::DivZero),
        ("#N/A", ExcelError::NA),
        ("#NAME?", ExcelError::Name),
        ("#NULL!", ExcelError::Null),
        ("#NUM!", ExcelError::Num),
        ("#REF!", ExcelError::Ref),
        ("#VALUE!", ExcelError::Value),
    ] {
        assert_eq!(parse(lit), Ok(Expr::Error(e)), "{lit}");
    }
}

#[test]
fn precedence_full_ladder() {
    // comparison < concat < add < mul < pow
    let e = parse("1=2&3+4*5^6").unwrap();
    let expected = bin(
        B::Eq,
        n(1.0),
        bin(
            B::Concat,
            n(2.0),
            bin(
                B::Add,
                n(3.0),
                bin(B::Mul, n(4.0), bin(B::Pow, n(5.0), n(6.0))),
            ),
        ),
    );
    assert_eq!(e, expected);
}

#[test]
fn precedence_comparison_chain_is_left() {
    assert_eq!(
        parse("1<2<3"),
        Ok(bin(B::Lt, bin(B::Lt, n(1.0), n(2.0)), n(3.0)))
    );
}

#[test]
fn precedence_concat_is_looser_than_add() {
    assert_eq!(
        parse("1+2&3+4"),
        Ok(bin(
            B::Concat,
            bin(B::Add, n(1.0), n(2.0)),
            bin(B::Add, n(3.0), n(4.0))
        ))
    );
}

#[test]
fn power_is_right_associative() {
    assert_eq!(
        parse("2^3^2"),
        Ok(bin(B::Pow, n(2.0), bin(B::Pow, n(3.0), n(2.0))))
    );
}

#[test]
fn unary_binds_tighter_than_power_excel_quirk() {
    // -2^2 = (-2)^2 in Excel.
    assert_eq!(
        parse("-2^2"),
        Ok(bin(
            B::Pow,
            Expr::Unary {
                op: U::Neg,
                expr: Box::new(n(2.0))
            },
            n(2.0)
        ))
    );
    // Nested unary: --1 and +-1 parse.
    assert_eq!(
        parse("--1"),
        Ok(Expr::Unary {
            op: U::Neg,
            expr: Box::new(Expr::Unary {
                op: U::Neg,
                expr: Box::new(n(1.0))
            })
        })
    );
    // Prefix-of-prefix conservatively parenthesizes; both forms re-parse
    // to the identical AST.
    assert_eq!(parse("+-1").unwrap().to_string(), "+-1");
}

#[test]
fn power_accepts_unary_rhs() {
    assert_eq!(
        parse("2^-2"),
        Ok(bin(
            B::Pow,
            n(2.0),
            Expr::Unary {
                op: U::Neg,
                expr: Box::new(n(2.0))
            }
        ))
    );
}

#[test]
fn postfix_percent_binds_tightest() {
    assert_eq!(
        parse("50%"),
        Ok(Expr::Unary {
            op: U::Percent,
            expr: Box::new(n(50.0))
        })
    );
    assert_eq!(
        parse("50%%"),
        Ok(Expr::Unary {
            op: U::Percent,
            expr: Box::new(Expr::Unary {
                op: U::Percent,
                expr: Box::new(n(50.0))
            })
        })
    );
    // Tighter than unary minus on the left: -50% = -(50%).
    assert_eq!(
        parse("-50%"),
        Ok(Expr::Unary {
            op: U::Neg,
            expr: Box::new(Expr::Unary {
                op: U::Percent,
                expr: Box::new(n(50.0))
            })
        })
    );
    // And than binary ops on the right: 2*3% = 2*(3%).
    assert_eq!(
        parse("2*3%"),
        Ok(bin(
            B::Mul,
            n(2.0),
            Expr::Unary {
                op: U::Percent,
                expr: Box::new(n(3.0))
            }
        ))
    );
}

#[test]
fn parens_override() {
    assert_eq!(
        parse("(1+2)*3"),
        Ok(bin(B::Mul, bin(B::Add, n(1.0), n(2.0)), n(3.0)))
    );
    // `(A1)` is a cell ref, not a call.
    assert_eq!(parse("(A1)"), Ok(cell(1, 1)));
}

#[test]
fn cell_refs_and_anchors() {
    assert_eq!(parse("A1"), Ok(cell(1, 1)));
    assert_eq!(parse("a1"), Ok(cell(1, 1))); // case-insensitive
    assert_eq!(parse("XFD1048576"), Ok(cell(16_384, 1_048_576)));
    assert_eq!(parse("AB1048576"), Ok(cell(28, 1_048_576)));
    assert_eq!(
        parse("$A$1"),
        Ok(Expr::CellRef {
            col: 1,
            row: 1,
            col_abs: true,
            row_abs: true
        })
    );
    assert_eq!(
        parse("B$2"),
        Ok(Expr::CellRef {
            col: 2,
            row: 2,
            col_abs: false,
            row_abs: true
        })
    );
    assert_eq!(
        parse("$C3"),
        Ok(Expr::CellRef {
            col: 3,
            row: 3,
            col_abs: true,
            row_abs: false
        })
    );
}

#[test]
fn out_of_grid_refs_are_parse_errors() {
    assert!(matches!(
        parse("XFE1"),
        Err(FormulaError::Parse {
            expected: "cell reference within A1:XFD1048576",
            ..
        })
    ));
    assert!(matches!(parse("A1048577"), Err(FormulaError::Parse { .. })));
    assert!(matches!(
        parse("$A$1048577"),
        Err(FormulaError::Parse { .. })
    ));
    // Four letters is a name, not a ref — and names are not in the grammar.
    assert!(matches!(parse("ABCD1"), Err(FormulaError::Parse { .. })));
}

#[test]
fn ranges() {
    assert_eq!(
        parse("A1:B10"),
        Ok(Expr::Range {
            start: CellRef::new(1, 1),
            end: CellRef::new(2, 10),
        })
    );
    assert_eq!(
        parse("$A$1:$B$10"),
        Ok(Expr::Range {
            start: CellRef {
                col: 1,
                row: 1,
                col_abs: true,
                row_abs: true
            },
            end: CellRef {
                col: 2,
                row: 10,
                col_abs: true,
                row_abs: true
            },
        })
    );
    // Reversed corners parse (eval normalizes).
    assert_eq!(
        parse("B2:A1"),
        Ok(Expr::Range {
            start: CellRef::new(2, 2),
            end: CellRef::new(1, 1),
        })
    );
}

#[test]
fn range_errors() {
    assert!(matches!(
        parse("A1:"),
        Err(FormulaError::Parse {
            expected: "cell reference after ':'",
            ..
        })
    ));
    assert!(matches!(
        parse("A1:5"),
        Err(FormulaError::Parse {
            expected: "cell reference after ':'",
            ..
        })
    ));
    assert!(matches!(
        parse("A1:B2:C3"),
        Err(FormulaError::Parse {
            expected: "end of range (single ':' only)",
            ..
        })
    ));
}

#[test]
fn function_calls() {
    assert_eq!(
        parse("SUM(1,2,3)"),
        Ok(Expr::Function {
            name: "SUM".into(),
            args: vec![n(1.0), n(2.0), n(3.0)]
        })
    );
    // Case-normalized.
    assert_eq!(
        parse("sum(1)"),
        Ok(Expr::Function {
            name: "SUM".into(),
            args: vec![n(1.0)]
        })
    );
    // Zero args.
    assert_eq!(
        parse("TODAY()"),
        Ok(Expr::Function {
            name: "TODAY".into(),
            args: vec![]
        })
    );
    // Ref-shaped function names lex as idents; the `(` wins.
    assert_eq!(
        parse("LOG10(100)").unwrap(),
        Expr::Function {
            name: "LOG10".into(),
            args: vec![n(100.0)]
        }
    );
    // TRUE()/FALSE() are function calls, distinct from the literals.
    assert_eq!(
        parse("TRUE()"),
        Ok(Expr::Function {
            name: "TRUE".into(),
            args: vec![]
        })
    );
}

#[test]
fn omitted_argument_slots() {
    // Empty slot in the middle.
    let e = parse("IF(A1,,5)").unwrap();
    let Expr::Function { args, .. } = &e else {
        panic!()
    };
    assert_eq!(args.len(), 3);
    assert!(formula_lang::is_omitted_arg(&args[1]));
    // Trailing empty slot.
    let e = parse("SUM(1,)").unwrap();
    let Expr::Function { args, .. } = &e else {
        panic!()
    };
    assert_eq!(args.len(), 2);
    assert!(formula_lang::is_omitted_arg(&args[1]));
    // Leading and doubled empties.
    let e = parse("F(,,)").unwrap();
    let Expr::Function { args, .. } = &e else {
        panic!()
    };
    assert_eq!(args.len(), 3);
    assert!(args.iter().all(formula_lang::is_omitted_arg));
}

#[test]
fn nested_calls_and_exprs_as_args() {
    assert_eq!(
        parse("IF(A1>10, SUM(B1:B3), 0)").unwrap(),
        Expr::Function {
            name: "IF".into(),
            args: vec![
                bin(B::Gt, cell(1, 1), n(10.0)),
                Expr::Function {
                    name: "SUM".into(),
                    args: vec![Expr::Range {
                        start: CellRef::new(2, 1),
                        end: CellRef::new(2, 3),
                    }],
                },
                n(0.0),
            ],
        }
    );
}

#[test]
fn trailing_garbage_rejected() {
    assert!(matches!(
        parse("1 2"),
        Err(FormulaError::Parse { expected: "end of formula", got, .. }) if got == "2"
    ));
    assert!(matches!(parse("SUM(1 2)"), Err(FormulaError::Parse { .. })));
    assert!(matches!(
        parse("(1+2"),
        Err(FormulaError::Parse {
            expected: "')'",
            ..
        })
    ));
    assert!(matches!(parse("SUM(1,2"), Err(FormulaError::Parse { .. })));
    assert!(
        matches!(parse(""), Err(FormulaError::Parse { expected: "expression", got, .. }) if got == "end of formula")
    );
    assert!(matches!(parse("+"), Err(FormulaError::Parse { .. })));
    assert!(matches!(parse("1+"), Err(FormulaError::Parse { .. })));
}

#[test]
fn bare_names_are_not_in_the_grammar() {
    // `=foo` in Excel would be #NAME?; this grammar has no name production,
    // so it is a typed parse error (documented divergence).
    assert!(matches!(
        parse("foo"),
        Err(FormulaError::Parse {
            expected: "cell reference, TRUE/FALSE, or function call",
            ..
        })
    ));
    assert!(matches!(parse("_private"), Err(FormulaError::Parse { .. })));
}

#[test]
fn whitespace_tolerance() {
    assert_eq!(parse(" 1 + 2 "), parse("1+2"));
    assert_eq!(parse("SUM( 1 , 2 )"), parse("SUM(1,2)"));
    assert_eq!(parse("A1 : B2"), parse("A1:B2"));
    assert_eq!(parse("\t1<\n2"), parse("1<2"));
}

#[test]
fn recursion_limit_on_deep_parens() {
    let deep = format!("{}1{}", "(".repeat(700), ")".repeat(700));
    assert_eq!(parse(&deep), Err(FormulaError::RecursionLimit));
    let deep = format!("{}1", "-".repeat(700));
    assert_eq!(parse(&deep), Err(FormulaError::RecursionLimit));
    // Deep right-nested power.
    let deep = "2".to_string() + &"^2".repeat(700);
    assert_eq!(parse(&deep), Err(FormulaError::RecursionLimit));
}

#[test]
fn left_chains_iterate_without_recursion() {
    // 100k-term additive chain must parse (evaluation depth-caps later).
    let f = "1+".repeat(100_000) + "1";
    let e = parse(&f).unwrap();
    let mut depth = 0;
    let mut cur = &e;
    while let Expr::Binary { left, .. } = cur {
        depth += 1;
        cur = left;
    }
    assert_eq!(depth, 100_000);
}

#[test]
fn display_roundtrip_precedence() {
    // What Display emits re-parses to the identical AST.
    for s in [
        "1=2&3+4*5^6",
        "-2^2",
        "2^-3",
        "2^3^2",
        "50%%*2+1",
        "-(-1)",
        "(1+2)*3",
        "1<(2<3)",
        "SUM(1,,3)",
        "IF(A1>=2,--1,--2)",
        "\"a\"\"b\"&\"c\"",
        "#DIV/0!+1",
        "$A$1:B$2",
    ] {
        let e = parse(s).unwrap();
        let out = to_formula(&e);
        assert_eq!(parse(&out).unwrap(), e, "{s} -> {out}");
    }
}
