//! The evaluator — [`Expr`] + [`CellResolver`] → [`Value`].
//!
//! # Contract highlights
//!
//! - **Stack-safe.** Recursion (nested parens, unary chains, right-nested
//!   `^`, nested calls) is capped at [`MAX_DEPTH`] with
//!   [`FormulaError::RecursionLimit`] — never a stack overflow, fuzz-
//!   verified against hostile nesting.
//! - **Two error channels.** Excel error *values* produced by evaluation
//!   (arithmetic, coercion, lookups) surface as
//!   `Err(FormulaError::Eval(_))` at the top and are catchable inside the
//!   formula by `IFERROR`/`IFNA`/`ISERROR`/`ISNA`. Error values *stored in
//!   cells* stay data (`Value::Error`) when simply fetched (`=A1`) — the
//!   shape `ISERROR(A1)` needs to observe — and convert to the hard
//!   channel the moment an operator or aggregate consumes them.
//! - **No allocation on the numeric scalar path.** Operators coerce
//!   through `f64` locals; the only allocating nodes are text results
//!   (inherently owned `String`s), function-argument vectorization, and
//!   range materialization (the resolver's policy).
//! - **Lazy branches.** `IF` evaluates only the taken arm;
//!   `IF(TRUE, 1, 1/0)` is `1`, as in Excel. `AND`/`OR` evaluate all
//!   arguments (Excel propagates their errors even when short-circuiting
//!   would "work").

use crate::ast::{normalize_range, Expr};
use crate::error::{ExcelError, FormulaError};
use crate::funcs::{dispatch, Ctx};
use crate::parser::MAX_DEPTH;
use crate::resolver::CellResolver;
use crate::value::Value;
use alloc::vec;
use alloc::vec::Vec;

/// Evaluates an expression against a resolver. Under the `std` feature the
/// volatile functions (`TODAY`, `NOW`) read the system clock; use
/// [`evaluate_with_clock`] for determinism (tests, replay, `no_std`).
///
/// ```
/// use formula_lang::{parse, evaluate, EmptyResolver, Value};
///
/// let e = parse("1 + 2 * 3").unwrap();
/// assert_eq!(evaluate(&e, &EmptyResolver).unwrap(), Value::Number(7.0));
/// ```
///
/// # Errors
/// [`FormulaError::Eval`] for Excel error results,
/// [`FormulaError::UnknownFunction`] for names outside the built-in
/// table, [`FormulaError::RecursionLimit`] past [`MAX_DEPTH`] nesting,
/// [`FormulaError::InvalidRange`] for structurally impossible range
/// arguments.
pub fn evaluate(expr: &Expr, resolver: &dyn CellResolver) -> Result<Value, FormulaError> {
    evaluate_with_clock(expr, resolver, system_now_serial())
}

/// Like [`evaluate`] with an explicit "now" — an Excel date serial
/// (`2026-01-01 12:00` ≈ `46023.5`). The only clock the volatile functions
/// (`TODAY`, `NOW`) see.
///
/// # Errors
/// As [`evaluate`].
pub fn evaluate_with_clock(
    expr: &Expr,
    resolver: &dyn CellResolver,
    now_serial: f64,
) -> Result<Value, FormulaError> {
    let now = if now_serial.is_finite() && now_serial >= 0.0 {
        now_serial
    } else {
        0.0
    };
    let mut ctx = Ctx {
        resolver,
        now,
        depth: 0,
    };
    eval(&mut ctx, expr)
}

/// True when the expression contains a **volatile** function — `TODAY`,
/// `NOW`, or `OFFSET` — meaning its value can change between evaluations
/// even with identical inputs. Iterative (explicit stack): safe on
/// arbitrarily deep ASTs.
///
/// ```
/// use formula_lang::parse;
/// use formula_lang::is_volatile;
///
/// assert!(is_volatile(&parse("TODAY()+1").unwrap()));
/// assert!(!is_volatile(&parse("SUM(A1:A9)").unwrap()));
/// ```
#[must_use]
pub fn is_volatile(expr: &Expr) -> bool {
    let mut stack: Vec<&Expr> = vec![expr];
    while let Some(e) = stack.pop() {
        match e {
            Expr::Function { name, args } => {
                if matches!(name.as_str(), "TODAY" | "NOW" | "OFFSET") {
                    return true;
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
    false
}

#[cfg(feature = "std")]
fn system_now_serial() -> f64 {
    use ::std::time::{SystemTime, UNIX_EPOCH};
    let Ok(d) = SystemTime::now().duration_since(UNIX_EPOCH) else {
        return 0.0;
    };
    let days = d.as_secs_f64() / 86_400.0;
    crate::date::UNIX_EPOCH_SERIAL as f64 + days
}

#[cfg(not(feature = "std"))]
fn system_now_serial() -> f64 {
    // no_std: no clock exists. `evaluate` stays callable (deterministic
    // NOW=0); real clocking goes through `evaluate_with_clock`.
    0.0
}

/// The recursive core. `ctx.depth` is the live recursion budget.
pub(crate) fn eval(ctx: &mut Ctx<'_>, e: &Expr) -> Result<Value, FormulaError> {
    ctx.depth += 1;
    if ctx.depth > MAX_DEPTH {
        return Err(FormulaError::RecursionLimit);
    }
    let out = eval_inner(ctx, e);
    ctx.depth -= 1;
    out
}

fn eval_inner(ctx: &mut Ctx<'_>, e: &Expr) -> Result<Value, FormulaError> {
    match e {
        Expr::Number(n) => {
            if n.is_finite() {
                Ok(Value::Number(*n))
            } else {
                // `1e999` lexes to inf; Excel shows #NUM!.
                Err(ExcelError::Num.into())
            }
        }
        Expr::Text(s) => Ok(Value::Text(s.clone())),
        Expr::Boolean(b) => Ok(Value::Boolean(*b)),
        Expr::Error(err) => Err(FormulaError::Eval(*err)),
        Expr::CellRef { col, row, .. } => {
            // Pure data passthrough — a cell holding an error yields the
            // error *value* (that is what ISERROR observes). Operators
            // convert it to the hard channel on use.
            Ok(ctx.resolver.get(*col, *row).unwrap_or(Value::Empty))
        }
        // A bare range used as a scalar: Excel would spill/intersect; the
        // scalar core reports #VALUE! (documented).
        Expr::Range { .. } => Err(ExcelError::Value.into()),
        Expr::Unary { op, expr } => eval_unary(ctx, *op, expr),
        Expr::Binary { op, left, right } => eval_binary(ctx, *op, left, right),
        Expr::Function { name, args } => dispatch(ctx, name, args),
    }
}

fn eval_unary(
    ctx: &mut Ctx<'_>,
    op: crate::ast::UnaryOp,
    expr: &Expr,
) -> Result<Value, FormulaError> {
    use crate::ast::UnaryOp;
    let v = eval(ctx, expr)?;
    match op {
        UnaryOp::Percent => {
            let n = checked_num(v)?;
            fin(n / 100.0)
        }
        UnaryOp::Neg => {
            let n = checked_num(v)?;
            fin(-n)
        }
        UnaryOp::Pos => {
            // Excel: unary + still requires a number (+"a" → #VALUE!).
            let n = checked_num(v)?;
            fin(n)
        }
    }
}

fn eval_binary(
    ctx: &mut Ctx<'_>,
    op: crate::ast::BinaryOp,
    left: &Expr,
    right: &Expr,
) -> Result<Value, FormulaError> {
    use crate::ast::BinaryOp;
    use crate::value::{coerce_text, compare};
    match op {
        BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
            let l = eval(ctx, left)?;
            let r = eval(ctx, right)?;
            let ord = compare(&l, &r).map_err(FormulaError::Eval)?;
            let b = match op {
                BinaryOp::Eq => ord.is_eq(),
                BinaryOp::Ne => !ord.is_eq(),
                BinaryOp::Lt => ord.is_lt(),
                BinaryOp::Gt => ord.is_gt(),
                BinaryOp::Le => !ord.is_gt(),
                BinaryOp::Ge => !ord.is_lt(),
                _ => unreachable!("comparison arm"),
            };
            Ok(Value::Boolean(b))
        }
        BinaryOp::Concat => {
            let l = eval(ctx, left)?;
            let r = eval(ctx, right)?;
            let mut ls = coerce_text(&l).map_err(FormulaError::Eval)?;
            let rs = coerce_text(&r).map_err(FormulaError::Eval)?;
            ls.push_str(&rs);
            Ok(Value::Text(ls))
        }
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Pow => {
            let l = checked_num(eval(ctx, left)?)?;
            let r = checked_num(eval(ctx, right)?)?;
            match op {
                BinaryOp::Add => fin(l + r),
                BinaryOp::Sub => fin(l - r),
                BinaryOp::Mul => fin(l * r),
                BinaryOp::Div => {
                    if r == 0.0 {
                        Err(ExcelError::DivZero.into())
                    } else {
                        fin(l / r)
                    }
                }
                BinaryOp::Pow => {
                    // Excel corner cases before powf:
                    // 0^0 → #NUM!, 0^negative → #DIV/0!, NaN results → #NUM!.
                    if l == 0.0 && r == 0.0 {
                        Err(ExcelError::Num.into())
                    } else if l == 0.0 && r < 0.0 {
                        Err(ExcelError::DivZero.into())
                    } else {
                        fin(libm::pow(l, r))
                    }
                }
                _ => unreachable!("arithmetic arm"),
            }
        }
    }
}

/// Converts an operand value to a number, promoting stored error values to
/// the hard channel and coercion failures to `#VALUE!`.
fn checked_num(v: Value) -> Result<f64, FormulaError> {
    crate::value::coerce_number(&v).map_err(FormulaError::Eval)
}

/// Wraps an arithmetic result: non-finite folds to `#NUM!` (Excel never
/// surfaces inf/NaN).
fn fin(n: f64) -> Result<Value, FormulaError> {
    if n.is_finite() {
        Ok(Value::Number(n))
    } else {
        Err(ExcelError::Num.into())
    }
}

/// Fetches the values of a range/cell argument, row-major. `Ok(None)` for
/// non-reference arguments (caller decides whether that is an error).
pub(crate) fn cell_values(ctx: &Ctx<'_>, e: &Expr) -> Result<Option<Vec<Value>>, FormulaError> {
    match e {
        Expr::CellRef { col, row, .. } => Ok(Some(vec![ctx
            .resolver
            .get(*col, *row)
            .unwrap_or(Value::Empty)])),
        Expr::Range { start, end } => {
            let (c0, r0, c1, r1) = normalize_range(start, end);
            Ok(Some(ctx.resolver.get_range((c0, r0), (c1, r1))))
        }
        _ => Ok(None),
    }
}

/// Extracts a range's normalized corner coordinates from an AST argument.
/// `CellRef` becomes a 1×1 range; anything else is
/// [`FormulaError::InvalidRange`].
pub(crate) fn range_bounds(e: &Expr) -> Result<(u32, u32, u32, u32), FormulaError> {
    match e {
        Expr::CellRef { col, row, .. } => Ok((*col, *row, *col, *row)),
        Expr::Range { start, end } => {
            let (c0, r0, c1, r1) = normalize_range(start, end);
            if c0 == 0 || c1 == 0 || r0 == 0 || r1 == 0 {
                Err(FormulaError::InvalidRange)
            } else {
                Ok((c0, r0, c1, r1))
            }
        }
        _ => Err(FormulaError::InvalidRange),
    }
}

/// True when the AST node is a cell or range reference.
pub(crate) fn is_ref_expr(e: &Expr) -> bool {
    matches!(e, Expr::CellRef { .. } | Expr::Range { .. })
}
