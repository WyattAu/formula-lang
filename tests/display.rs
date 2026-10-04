//! Display / serialization — precedence-correct rendering of every node
//! kind, plus the omitted-argument convention.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

use formula_lang::{parse, to_formula, Expr};

fn rt(s: &str) -> String {
    let e = parse(s).unwrap();
    let out = to_formula(&e);
    // Every rendered form re-parses to the identical AST.
    assert_eq!(parse(&out).unwrap(), e, "{s} -> {out}");
    out
}

#[test]
fn literals_render() {
    assert_eq!(rt("42"), "42");
    assert_eq!(rt("3.5"), "3.5");
    assert_eq!(rt("\"hi\""), "\"hi\"");
    assert_eq!(rt("\"a\"\"b\""), "\"a\"\"b\"");
    assert_eq!(rt("TRUE"), "TRUE");
    assert_eq!(rt("false"), "FALSE");
    assert_eq!(rt("#N/A"), "#N/A");
    assert_eq!(rt("#div/0!"), "#DIV/0!");
}

#[test]
fn refs_and_ranges_render() {
    assert_eq!(rt("a1"), "A1"); // normalized upper
    assert_eq!(rt("$A$1"), "$A$1");
    assert_eq!(rt("b$2"), "B$2");
    assert_eq!(rt("$xfd1048576"), "$XFD1048576");
    assert_eq!(rt("a1:b2"), "A1:B2");
    assert_eq!(rt("$a$1:$b$10"), "$A$1:$B$10");
    assert_eq!(rt("b2:a1"), "B2:A1"); // as written, not normalized
}

#[test]
fn functions_render() {
    assert_eq!(rt("sum(1,2,3)"), "SUM(1,2,3)");
    assert_eq!(rt("if(a1>1,b1,c1)"), "IF(A1>1,B1,C1)");
    assert_eq!(rt("today()"), "TODAY()");
    assert_eq!(rt("F()"), "F()");
    assert_eq!(rt("F(1,)"), "F(1,)"); // omitted slot
    assert_eq!(rt("F(,,)"), "F(,,)");
    // A bare `#NULL!` outside an argument slot renders as itself.
    assert_eq!(rt("#NULL!"), "#NULL!");
}

#[test]
fn precedence_parens_are_minimal_and_correct() {
    assert_eq!(rt("1+2*3"), "1+2*3");
    assert_eq!(rt("(1+2)*3"), "(1+2)*3");
    // Right child of a left-assoc parent only wraps at equal precedence —
    // `1&2+3` is unambiguous as-is.
    assert_eq!(rt("1&2+3"), "1&2+3");
    assert_eq!(rt("1-(2-3)"), "1-(2-3)"); // equal-bp right child wraps
    assert_eq!(rt("1=(2&3)"), "1=2&3");
    assert_eq!(rt("2^3^2"), "2^3^2"); // right-assoc: no parens
    assert_eq!(rt("(2^3)^2"), "(2^3)^2"); // left-nested: parens
    assert_eq!(rt("1<(2<3)"), "1<(2<3)");
    assert_eq!(rt("-2^2"), "-2^2"); // Excel quirk preserved
    assert_eq!(rt("-(2^2)"), "-(2^2)");
    assert_eq!(rt("-(1+2)"), "-(1+2)");
    assert_eq!(rt("-(-1)"), "--1"); // unary chains render paren-free
    assert_eq!(rt("50%*2"), "50%*2");
}

#[test]
fn unary_rendering() {
    // Chains render paren-free and re-parse identically.
    assert_eq!(rt("--1"), "--1");
    assert_eq!(rt("+-+-1"), "+-+-1");
    assert_eq!(rt("50%%"), "50%%");
    assert_eq!(rt("-50%"), "-50%");
    assert_eq!(rt("2^-3"), "2^-3");
    // Mixed binding gets exactly the parens it needs.
    assert_eq!(rt("-(1&2)"), "-(1&2)");
    assert_eq!(rt("(1+2)%"), "(1+2)%");
    // Deep chains stay linear in size (no quadratic parens); the parsed
    // chain is 400 minuses + the leading expression token.
    let deep = rt(&format!("{}1", "-".repeat(400)));
    assert_eq!(deep.len(), 401);
}

#[test]
fn roundtrip_display_of_display() {
    // The rendered form is a fixed point.
    for s in ["1+2*3", "(1+2)*3", "-(1&2)", "SUM(1,,3)", "$A$1:B$2"] {
        let once = rt(s);
        let twice = rt(&once);
        assert_eq!(once, twice);
    }
}

#[test]
fn deep_ast_rendering_is_iterative() {
    // 50k-deep tree cannot overflow the Display work stack.
    let mut e = Expr::Number(0.0);
    for _ in 0..50_000 {
        e = Expr::Binary {
            op: formula_lang::BinaryOp::Add,
            left: Box::new(e),
            right: Box::new(Expr::Number(1.0)),
        };
    }
    let out = to_formula(&e);
    assert_eq!(out.len(), 100_001); // "0" + "+1" × 50_000
    assert!(out.starts_with("0+1+1"));
}

#[test]
fn nonfinite_numbers_render_deterministically() {
    // inf renders as 1E+999 (re-lexes to inf); NaN as #NUM! (lossy, documented).
    let inf = Expr::Number(f64::INFINITY);
    assert_eq!(to_formula(&inf), "1E+999");
    assert_eq!(parse("1E+999").unwrap(), inf);
    let nan = Expr::Number(f64::NAN);
    assert_eq!(to_formula(&nan), "#NUM!");
}

#[test]
fn clone_and_eq_survive_drop_stress() {
    // Clone a mid-size tree, compare, drop both — exercise the iterative
    // Drop without recursion.
    let e = parse("IF(SUM(A1:B9)>10,MAX(1,2,3)&\"x\",-2^2)").unwrap();
    let c = e.clone();
    assert_eq!(e, c);
    drop(c);
    drop(e);
}
