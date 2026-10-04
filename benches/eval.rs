//! Evaluation and parsing benchmarks.
// Bench harness: fixed literal formulas unwrap legally; the lib target
// remains lint-clean.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![allow(missing_docs)]
//!
//! The headline number this crate promises: the numeric scalar eval path
//! allocates nothing. `eval_scalar` and `eval_arith` are that path;
//! `eval_lookup` exercises range materialization through the resolver
//! (the documented allocating edge); `parse` and `parse_deep` pin the
//! Pratt parser's throughput.

use criterion::{criterion_group, criterion_main, Criterion};
use formula_lang::{evaluate, parse, Expr, MapResolver};

fn scalar_expr() -> Expr {
    parse("1 + 2 * 3 - 4 / 2").unwrap()
}

fn arith_expr() -> Expr {
    parse("ROUND(SQRT(POWER(3,2)) + MOD(10, 3) + ABS(-5) * SIGN(-2), 3)").unwrap()
}

fn lookup_expr() -> Expr {
    parse("VLOOKUP(42, A1:C1000, 3, FALSE)").unwrap()
}

fn lookup_sheet() -> MapResolver {
    let mut s = MapResolver::new();
    for r in 1u32..=1000 {
        s.set_num(1, r, f64::from(r));
        s.set_text(2, r, "row");
        s.set_num(3, r, f64::from(r) * 10.0);
    }
    s
}

fn bench_eval(c: &mut Criterion) {
    let sheet = lookup_sheet();
    c.bench_function("eval_scalar", |b| {
        let e = scalar_expr();
        b.iter(|| evaluate(core::hint::black_box(&e), &sheet).unwrap())
    });
    c.bench_function("eval_functions", |b| {
        let e = arith_expr();
        b.iter(|| evaluate(core::hint::black_box(&e), &sheet).unwrap())
    });
    c.bench_function("eval_lookup_1000", |b| {
        let e = lookup_expr();
        b.iter(|| evaluate(core::hint::black_box(&e), &sheet).unwrap())
    });
    c.bench_function("eval_concat", |b| {
        let e = parse("LEFT(\"hello\", 2) & UPPER(\"world\") & LEN(\"abc\")").unwrap();
        b.iter(|| evaluate(core::hint::black_box(&e), &sheet).unwrap())
    });
}

fn bench_parse(c: &mut Criterion) {
    c.bench_function("parse_scalar", |b| {
        b.iter(|| parse(core::hint::black_box("1 + 2 * 3 - 4 / 2")).unwrap())
    });
    c.bench_function("parse_deep", |b| {
        b.iter(|| {
            parse(core::hint::black_box(
                "=SUM(A1:B2)+IF(1<2,SUM(1,2,3),MAX(4,5))+6%-$C$3",
            ))
            .unwrap()
        })
    });
}

criterion_group!(benches, bench_eval, bench_parse);
criterion_main!(benches);
