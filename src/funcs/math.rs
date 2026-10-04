//! Arithmetic functions — Excel rounding/sign semantics included.
//!
//! Excel's rounding family is famously non-obvious and pinned here:
//! `ROUND` is half-away-from-zero (not banker's), `INT` floors toward
//! −∞ while `TRUNC`/`ROUNDDOWN` go toward zero, `CEILING`/`FLOOR` carry
//! the significance-sign rules (mixed signs → `#NUM!` except
//! `CEILING(negative, positive)`), and `MOD` takes the divisor's sign.

use super::{arg_count, arg_count_exact, collect_numbers, int_arg, num_arg, Ctx, R};
use crate::ast::Expr;
use crate::error::{ExcelError, FormulaError};
use crate::value::Value;

/// `SUM(...)` — adds numbers, skipping text/booleans/empties *inside
/// references* and coercing direct scalar arguments (`SUM(TRUE)` = 1,
/// `SUM("3")` = 3, `SUM(A1)` = 0 when A1 holds text).
pub(crate) fn sum(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let ns = collect_numbers(ctx, args)?;
    Ok(Value::Number(ns.iter().sum()))
}

/// `PRODUCT(...)` — multiplies; an empty operand set multiplies to 0
/// (Excel: `PRODUCT(A1:A2)` over empty cells is 0).
pub(crate) fn product(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let ns = collect_numbers(ctx, args)?;
    if ns.is_empty() {
        return Ok(Value::Number(0.0));
    }
    Ok(Value::Number(ns.iter().product()))
}

/// `ABS(n)`
pub(crate) fn abs(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    Ok(Value::Number(num_arg(ctx, args, 0)?.abs()))
}

/// `SIGN(n)` — −1 / 0 / 1.
pub(crate) fn sign(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let n = num_arg(ctx, args, 0)?;
    Ok(Value::Number(if n > 0.0 {
        1.0
    } else if n < 0.0 {
        -1.0
    } else {
        0.0
    }))
}

/// `SQRT(n)` — `#NUM!` for negative input.
pub(crate) fn sqrt(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let n = num_arg(ctx, args, 0)?;
    if n < 0.0 {
        return Err(ExcelError::Num.into());
    }
    Ok(Value::Number(libm::sqrt(n)))
}

/// `POWER(b, e)` — the `^` operator's function twin, same corner cases
/// (`0^0` → `#NUM!`, `0^negative` → `#DIV/0!`).
pub(crate) fn power(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 2)?;
    let b = num_arg(ctx, args, 0)?;
    let e = num_arg(ctx, args, 1)?;
    if b == 0.0 && e == 0.0 {
        return Err(ExcelError::Num.into());
    }
    if b == 0.0 && e < 0.0 {
        return Err(ExcelError::DivZero.into());
    }
    let r = libm::pow(b, e);
    if r.is_finite() {
        Ok(Value::Number(r))
    } else {
        Err(ExcelError::Num.into())
    }
}

/// `EXP(n)`
pub(crate) fn exp(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let r = libm::exp(num_arg(ctx, args, 0)?);
    if r.is_finite() {
        Ok(Value::Number(r))
    } else {
        Err(ExcelError::Num.into())
    }
}

/// `LN(n)` — natural log; `n ≤ 0` → `#NUM!`.
pub(crate) fn ln(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let n = num_arg(ctx, args, 0)?;
    if n <= 0.0 {
        return Err(ExcelError::Num.into());
    }
    Ok(Value::Number(libm::log(n)))
}

/// `LOG(n, [base=10])` — Excel's default base is 10 (not e!); `n ≤ 0` or
/// `base ≤ 0` or `base == 1` → `#NUM!`/`#DIV/0!`.
pub(crate) fn log(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, 2)?;
    let n = num_arg(ctx, args, 0)?;
    let base = if args.len() > 1 {
        num_arg(ctx, args, 1)?
    } else {
        10.0
    };
    checked_log(n, base)
}

/// `LOG10(n)`
pub(crate) fn log10(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let n = num_arg(ctx, args, 0)?;
    checked_log(n, 10.0)
}

fn checked_log(n: f64, base: f64) -> R {
    if n <= 0.0 || base <= 0.0 {
        return Err(ExcelError::Num.into());
    }
    if base == 1.0 {
        return Err(ExcelError::DivZero.into());
    }
    Ok(Value::Number(libm::log(n) / libm::log(base)))
}

/// `MOD(n, d)` — Excel semantics: result takes the **divisor's** sign
/// (`MOD(-3, 2)` = 1); `d = 0` → `#DIV/0!`.
pub(crate) fn mod_(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 2)?;
    let n = num_arg(ctx, args, 0)?;
    let d = num_arg(ctx, args, 1)?;
    if d == 0.0 {
        return Err(ExcelError::DivZero.into());
    }
    Ok(Value::Number(n - d * libm::floor(n / d)))
}

/// `INT(n)` — floors toward −∞ (`INT(-1.5)` = −2).
pub(crate) fn int(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    Ok(Value::Number(libm::floor(num_arg(ctx, args, 0)?)))
}

/// `TRUNC(n, [digits=0])` — truncates toward zero.
pub(crate) fn trunc(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, 2)?;
    let n = num_arg(ctx, args, 0)?;
    let d = digits_arg(ctx, args, 1)?;
    Ok(Value::Number(scale(n, d, libm::trunc)))
}

/// `ROUND(n, digits)` — half **away from zero** (`ROUND(2.5,0)` = 3,
/// `ROUND(-2.5,0)` = −3; not banker's rounding).
pub(crate) fn round(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, 2)?;
    let n = num_arg(ctx, args, 0)?;
    let d = digits_arg(ctx, args, 1)?;
    Ok(Value::Number(scale(n, d, round_half_away)))
}

/// `ROUNDUP(n, [digits=0])` — away from zero.
pub(crate) fn roundup(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, 2)?;
    let n = num_arg(ctx, args, 0)?;
    let d = digits_arg(ctx, args, 1)?;
    Ok(Value::Number(scale(n, d, |t| {
        if t >= 0.0 {
            libm::ceil(t)
        } else {
            libm::floor(t)
        }
    })))
}

/// `ROUNDDOWN(n, [digits=0])` — toward zero.
pub(crate) fn rounddown(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, 2)?;
    let n = num_arg(ctx, args, 0)?;
    let d = digits_arg(ctx, args, 1)?;
    Ok(Value::Number(scale(n, d, libm::trunc)))
}

/// `CEILING(n, sig)` — rounds **up** (toward +∞ scaled by significance).
/// `sig = 0` → `#DIV/0!`; `n > 0` with `sig < 0` → `#NUM!`;
/// `CEILING(-2.5, 2)` = −2 (valid unlike FLOOR); `CEILING(-2.5, -2)` = −4.
pub(crate) fn ceiling(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 2)?;
    let n = num_arg(ctx, args, 0)?;
    let s = num_arg(ctx, args, 1)?;
    if s == 0.0 {
        return Err(ExcelError::DivZero.into());
    }
    if n == 0.0 {
        return Ok(Value::Number(0.0));
    }
    if s < 0.0 && n > 0.0 {
        return Err(ExcelError::Num.into());
    }
    Ok(Value::Number(libm::ceil(n / s) * s))
}

/// `FLOOR(n, sig)` — rounds **down**. `sig = 0` → `#DIV/0!`; any sign
/// mismatch between `n` and `sig` → `#NUM!` (Excel's FLOOR is stricter
/// than CEILING); `FLOOR(-2.5, -2)` = −2.
pub(crate) fn floor(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 2)?;
    let n = num_arg(ctx, args, 0)?;
    let s = num_arg(ctx, args, 1)?;
    if s == 0.0 {
        return Err(ExcelError::DivZero.into());
    }
    if n == 0.0 {
        return Ok(Value::Number(0.0));
    }
    if n * s < 0.0 {
        return Err(ExcelError::Num.into());
    }
    Ok(Value::Number(libm::floor(n / s) * s))
}

/// The digits parameter: trunc to integer, `|digits| > 15` clamps to 15
/// (beyond f64 significance; Excel behaves as if clamped too).
fn digits_arg(ctx: &mut Ctx<'_>, args: &[Expr], i: usize) -> Result<f64, FormulaError> {
    match args.get(i) {
        None => return Ok(0.0),
        Some(e) if crate::ast::is_omitted_arg(e) => return Ok(0.0),
        Some(_) => {}
    }
    let d = int_arg(ctx, args, i)?;
    Ok(d.clamp(-15.0, 15.0))
}

/// Applies `f` to `n` scaled by `10^digits`, then unscales — the Excel
/// rounding family's shared skeleton.
fn scale(n: f64, digits: f64, f: impl Fn(f64) -> f64) -> f64 {
    if digits == 0.0 {
        return f(n);
    }
    let m = libm::pow(10.0, digits.abs());
    let (m, un) = if digits > 0.0 {
        (m, 1.0 / m)
    } else {
        (1.0 / m, m)
    };
    f(n * m) * un
}

/// Half-away-from-zero rounding of a scalar.
pub(crate) fn round_half_away(t: f64) -> f64 {
    if t >= 0.0 {
        libm::floor(t + 0.5)
    } else {
        libm::ceil(t - 0.5)
    }
}
