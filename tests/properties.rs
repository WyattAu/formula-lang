//! Property tests — totality, serialization round-trips, algebraic laws,
//! and the depth cap. 500 cases per property (fleet convention).

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

use formula_lang::{
    evaluate_with_clock, is_omitted_arg, parse, to_formula, BinaryOp, ExcelError, Expr,
    FormulaError, MapResolver, UnaryOp, Value, MAX_DEPTH,
};
use proptest::prelude::*;

pub(crate) const NOW: f64 = 45_123.75;

/// True when the AST has exactly one lossy render case: a call whose
/// arguments are a *single* omitted slot (`F([omitted])` renders as
/// `F()`, which re-parses to zero args — the grammar cannot express one
/// empty slot). Round-trip properties assume this away; see the docs on
/// `Display for Expr` / [`formula_lang::is_omitted_arg`].
fn render_faithful(e: &Expr) -> bool {
    let mut stack = vec![e];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Function { args, .. } => {
                if args.len() == 1 && is_omitted_arg(&args[0]) {
                    return false;
                }
                stack.extend(args.iter());
            }
            Expr::Binary { left, right, .. } => {
                stack.push(left);
                stack.push(right);
            }
            Expr::Unary { expr, .. } => stack.push(expr),
            _ => {}
        }
    }
    true
}

/// Finite float leaf.
fn number() -> impl Strategy<Value = Expr> {
    (any::<i64>()).prop_map(|i| {
        let f = (i % 1_000_000) as f64 / 8.0;
        Expr::Number(f)
    })
}

fn leaf() -> impl Strategy<Value = Expr> {
    prop_oneof![
        4 => number(),
        2 => "[^\"]{0,8}".prop_map(Expr::Text),
        1 => any::<bool>().prop_map(Expr::Boolean),
        3 => (1u32..=20u32, 1u32..=20u32).prop_map(|(c, r)| Expr::CellRef { col: c, row: r, col_abs: false, row_abs: false }),
        1 => proptest::sample::select(vec![
            ExcelError::DivZero, ExcelError::NA, ExcelError::Name,
            ExcelError::Null, ExcelError::Num, ExcelError::Ref, ExcelError::Value,
        ]).prop_map(Expr::Error),
    ]
}

fn binary_ops() -> Vec<BinaryOp> {
    vec![
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Pow,
        BinaryOp::Concat,
        BinaryOp::Eq,
        BinaryOp::Ne,
        BinaryOp::Lt,
        BinaryOp::Gt,
        BinaryOp::Le,
        BinaryOp::Ge,
    ]
}

/// Names from the built-in table (arity-safe shapes below).
fn func_names() -> Vec<&'static str> {
    vec![
        "SUM",
        "MAX",
        "MIN",
        "COUNT",
        "COUNTA",
        "CONCATENATE",
        "ABS",
        "NOT",
        "IF",
        "ROUND",
        "FOO", // unknown on purpose: exercises the UnknownFunction channel
    ]
}

fn expr_strategy(depth: u32) -> impl Strategy<Value = Expr> {
    leaf().prop_recursive(
        depth,
        256,
        3,
        |inner| {
            prop_oneof![
                4 => (proptest::sample::select(binary_ops()), inner.clone(), inner.clone())
                    .prop_map(|(op, l, r)| Expr::Binary { op, left: Box::new(l), right: Box::new(r) }),
                2 => (proptest::sample::select(vec![UnaryOp::Neg, UnaryOp::Pos, UnaryOp::Percent]), inner.clone())
                    .prop_map(|(op, e)| Expr::Unary { op, expr: Box::new(e) }),
                2 => (proptest::sample::select(func_names()), proptest::collection::vec(inner.clone(), 0..4))
                    .prop_map(|(name, args)| Expr::Function { name: name.into(), args }),
                1 => (inner.clone(), inner.clone())
                    .prop_map(|(a, b)| {
                        // Ranges need CellRef corners.
                        let start = match a { Expr::CellRef { col, row, .. } => formula_lang::CellRef::new(col, row), _ => formula_lang::CellRef::new(1, 1) };
                        let end = match b { Expr::CellRef { col, row, .. } => formula_lang::CellRef::new(col, row), _ => formula_lang::CellRef::new(2, 2) };
                        Expr::Range { start, end }
                    }),
            ]
        },
    )
}

/// A small random sheet: cells in a 10×10 grid with mixed value types.
fn arb_sheet() -> impl Strategy<Value = MapResolver> {
    proptest::collection::vec(
        (
            1u32..=10,
            1u32..=10,
            proptest::sample::select(vec![
                Value::Number(1.5),
                Value::Number(0.0),
                Value::Number(-3.0),
                Value::Text("x".into()),
                Value::Text("".into()),
                Value::Boolean(true),
                Value::Boolean(false),
                Value::Error(ExcelError::NA),
                Value::Empty,
            ]),
        ),
        0..40,
    )
    .prop_map(|cells| {
        let mut s = MapResolver::new();
        for (c, r, v) in cells {
            s.set(c, r, v);
        }
        s
    })
}

proptest! {
    #![proptest_config(proptest::test_runner::Config::with_cases(500))]

    /// Totality: arbitrary strings never panic the tokenizer or the parser.
    #[test]
    fn parse_is_total_on_arbitrary_input(s in "\\PC{0,120}") {
        let _ = parse(&s);
    }

    /// Serialization round-trip: whenever a formula parses, its rendered
    /// form re-parses to the identical AST (the Display contract).
    #[test]
    fn display_roundtrips_through_parse(s in "[0-9+\\-*/^&=<>%,() A-Za-z:.\"#,]{0,80}") {
        if let Ok(e) = parse(&s) {
            prop_assume!(render_faithful(&e));
            let out = to_formula(&e);
            match parse(&out) {
                Ok(e2) => {
                    let msg = format!("input {s:?} rendered {out:?}");
                    prop_assert_eq!(e, e2, "{}", msg);
                }
                Err(err) => {
                    let msg = format!("input {s:?} rendered {out:?} did not re-parse: {err:?}");
                    prop_assert!(false, "{}", msg);
                }
            }
        }
    }

    /// Display is idempotent: render(parse(render(x))) == render(x).
    #[test]
    fn display_is_idempotent(s in "[0-9+\\-*/^&=<>%,() A-Za-z:.\"#,]{0,60}") {
        if let Ok(e) = parse(&s) {
            prop_assume!(render_faithful(&e));
            let once = to_formula(&e);
            let twice = to_formula(&parse(&once).unwrap());
            prop_assert_eq!(once, twice);
        }
    }

    /// Evaluation totality: arbitrary ASTs over arbitrary sheets produce
    /// Ok or a typed error, never a panic.
    #[test]
    fn eval_is_total_on_generated_asts(
        e in expr_strategy(6),
        sheet in arb_sheet(),
    ) {
        let _ = evaluate_with_clock(&e, &sheet, NOW);
    }

    /// Eval round-trip: an AST and its re-parsed serialization evaluate
    /// identically over the same resolver.
    #[test]
    fn eval_roundtrips_through_display(
        e in expr_strategy(5),
        sheet in arb_sheet(),
    ) {
        prop_assume!(render_faithful(&e));
        let out = to_formula(&e);
        let reparsed = parse(&out).unwrap();
        let a = evaluate_with_clock(&e, &sheet, NOW);
        let b = evaluate_with_clock(&reparsed, &sheet, NOW);
        let msg = format!("expr {out}");
        prop_assert_eq!(a, b, "{}", msg);
    }

    /// Addition and multiplication commute (IEEE floats: exact for these
    /// magnitudes).
    #[test]
    fn add_mul_commute(a in -1e6f64..1e6, b in -1e6f64..1e6) {
        let ev = |s: String| match evaluate_with_clock(&parse(&s).unwrap(), &MapResolver::new(), NOW) {
            Ok(Value::Number(n)) => n,
            other => panic!("{s} -> {other:?}"),
        };
        prop_assert_eq!(ev(format!("({a})+({b})")), ev(format!("({b})+({a})")));
        prop_assert_eq!(ev(format!("({a})*({b})")), ev(format!("({b})*({a})")));
    }

    /// Identity and annihilation: x+0 == x, x*1 == x, x*0 == 0.
    #[test]
    fn arithmetic_identities(x in -1e9f64..1e9) {
        let ev = |s: String| match evaluate_with_clock(&parse(&s).unwrap(), &MapResolver::new(), NOW) {
            Ok(Value::Number(n)) => n,
            other => panic!("{s} -> {other:?}"),
        };
        prop_assert_eq!(ev(format!("({x})+0")), x);
        prop_assert_eq!(ev(format!("({x})*1")), x);
        prop_assert_eq!(ev(format!("({x})*0")), 0.0);
    }

    /// Double negation over booleans; comparison antisymmetry.
    #[test]
    fn logical_laws(a in -100.0f64..100.0, b in -100.0f64..100.0) {
        let evb = |s: String| match evaluate_with_clock(&parse(&s).unwrap(), &MapResolver::new(), NOW) {
            Ok(Value::Boolean(v)) => v,
            other => panic!("{s} -> {other:?}"),
        };
        prop_assert_eq!(evb(format!("NOT(NOT(({a})<({b})))")), evb(format!("({a})<({b})")));
        prop_assert_eq!(evb(format!("({a})<({b})")), evb(format!("({b})>({a})")));
        prop_assert_eq!(evb(format!("({a})=({b})")), evb(format!("({b})=({a})")));
    }

    /// Concatenation associates (string semantics, no float edge cases).
    #[test]
    fn concat_associates(a in "[ab]{0,4}", b in "[cd]{0,4}", c in "[ef]{0,4}") {
        let evt = |s: String| match evaluate_with_clock(&parse(&s).unwrap(), &MapResolver::new(), NOW) {
            Ok(Value::Text(t)) => t,
            other => panic!("{s} -> {other:?}"),
        };
        let (a, b, c) = (format!("\"{a}\""), format!("\"{b}\""), format!("\"{c}\""));
        prop_assert_eq!(
            evt(format!("({a}&{b})&{c}")),
            evt(format!("{a}&({b}&{c})"))
        );
    }

    /// Omitted-argument slots survive a full parse→render→parse cycle.
    #[test]
    fn omitted_slots_roundtrip(n in 0usize..5, k in 0usize..3) {
        let mut f = String::from("F(");
        for i in 0..n + k {
            if i > 0 {
                f.push(',');
            }
            if i % 2 == 0 {
                f.push('1');
            }
        }
        f.push(')');
        let e = parse(&f).unwrap();
        if let Expr::Function { args, .. } = &e {
            prop_assert_eq!(args.len(), n + k);
            if n + k >= 2 {
                prop_assert!(args.iter().any(is_omitted_arg));
            }
        }
        let out = to_formula(&e);
        let e2 = parse(&out).unwrap();
        prop_assert_eq!(e, e2);
    }
}

/// Not in the proptest! block: deterministic depth-cap checks.
#[test]
fn depth_cap_is_typed() {
    let deep = {
        let mut e = Expr::Number(0.0);
        for _ in 0..(MAX_DEPTH + 10) {
            e = Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(e),
                right: Box::new(Expr::Number(1.0)),
            };
        }
        e
    };
    assert_eq!(
        evaluate_with_clock(&deep, &MapResolver::new(), NOW),
        Err(FormulaError::RecursionLimit)
    );
}

#[test]
fn formula_error_is_matchable_and_typed() {
    // Exhaustive match compiles: every variant is nameable and Eq.
    let errs = [
        FormulaError::Tokenize { pos: 0, char: '?' },
        FormulaError::Parse {
            pos: 1,
            expected: "x",
            got: "y".into(),
        },
        FormulaError::Eval(ExcelError::NA),
        FormulaError::UnknownFunction("X".into()),
        FormulaError::RecursionLimit,
        FormulaError::InvalidRange,
    ];
    for e in errs {
        let _ = match e {
            FormulaError::Tokenize { .. } => 1,
            FormulaError::Parse { .. } => 2,
            FormulaError::Eval(_) => 3,
            FormulaError::UnknownFunction(_) => 4,
            FormulaError::RecursionLimit => 5,
            FormulaError::InvalidRange => 6,
        };
    }
}

/// Regression (fuzz-found): unary chains used to render with nested
/// parens, inflating re-parse depth past [`MAX_DEPTH`]. The rendered form
/// of a maximal unary chain must re-parse.
#[test]
fn regression_unary_chain_roundtrip_at_depth() {
    let src = format!("{}1{}", "-".repeat(300), "&\"x\"".repeat(0));
    let e = parse(&src).unwrap();
    let out = to_formula(&e);
    assert_eq!(parse(&out).unwrap(), e);
    // Rendered size stays linear in chain length.
    assert_eq!(out.len(), 301);
}

/// Regression (fuzz-found): the iterative Drop leaked the dummy `Box`
/// placeholders left in emptied shells. A stringy deep tree must free all
/// of its heap — assertable only by not crashing under a leak detector,
/// so this test exists to run under valgrind/ASan in CI.
#[test]
fn regression_drop_stringy_deep_tree_without_leaks() {
    let mut e = Expr::Text("root".into());
    for i in 0..10_000 {
        e = Expr::Binary {
            op: BinaryOp::Concat,
            left: Box::new(e),
            right: Box::new(Expr::Text(format!("n{i}"))),
        };
    }
    drop(e);
}
