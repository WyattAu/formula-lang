//! Statistical functions — Excel's ignore-text-in-references aggregation.

use super::{arg_count, collect_numbers, Ctx, R};
use crate::ast::Expr;
use crate::error::{ExcelError, FormulaError};
use crate::value::Value;

/// `AVERAGE(...)` — arithmetic mean of the numeric set; empty set →
/// `#DIV/0!` (Excel).
pub(crate) fn average(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let ns = collect_numbers(ctx, args)?;
    if ns.is_empty() {
        return Err(ExcelError::DivZero.into());
    }
    let n = ns.iter().sum::<f64>();
    Ok(Value::Number(n / ns.len() as f64))
}

/// `COUNT(...)` — how many numeric values the operands hold. References
/// contribute their `Number` cells; direct scalars count 1 when they
/// coerce to a number (`COUNT("3")` = 1, `COUNT("x")` = 0, `COUNT(TRUE)`
/// = 1).
pub(crate) fn count(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let mut n = 0f64;
    for a in args {
        if crate::ast::is_omitted_arg(a) {
            continue;
        }
        if crate::eval::is_ref_expr(a) {
            // Excel: COUNT over references tallies Number cells only —
            // text, logicals, empties, and even error cells are ignored.
            if let Some(vals) = crate::eval::cell_values(ctx, a)? {
                n += vals
                    .iter()
                    .filter(|v| matches!(v, Value::Number(_)))
                    .count() as f64;
            }
            continue;
        }
        // Direct scalars count when they coerce; anything else is simply
        // not a number (COUNT never errors on type).
        match super::eval1(ctx, a) {
            Ok(Value::Number(_)) | Ok(Value::Boolean(_)) => n += 1.0,
            Ok(Value::Text(t)) => {
                if crate::value::coerce_number(&Value::Text(t)).is_ok() {
                    n += 1.0;
                }
            }
            Ok(_) => {}
            Err(FormulaError::Eval(_)) => {} // errors are not numbers
            Err(e) => return Err(e),
        }
    }
    Ok(Value::Number(n))
}

/// `COUNTA(...)` — how many non-empty values the operands hold. Direct
/// scalars always count (they are present by construction); reference
/// cells count unless `Empty`.
pub(crate) fn counta(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let mut n = 0f64;
    for a in args {
        if crate::ast::is_omitted_arg(a) {
            continue;
        }
        if crate::eval::is_ref_expr(a) {
            if let Some(vals) = crate::eval::cell_values(ctx, a)? {
                n += vals.iter().filter(|v| !matches!(v, Value::Empty)).count() as f64;
                continue;
            }
        }
        match super::eval1(ctx, a)? {
            Value::Error(e) => return Err(FormulaError::Eval(e)),
            _ => n += 1.0,
        }
    }
    Ok(Value::Number(n))
}

/// `COUNTBLANK(...)` — empties across the operands. Reference cells count
/// when `Empty`; a direct scalar never counts (it is present).
pub(crate) fn countblank(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let mut n = 0f64;
    for a in args {
        if crate::ast::is_omitted_arg(a) {
            continue;
        }
        if let Some(vals) = crate::eval::cell_values(ctx, a)? {
            n += vals.iter().filter(|v| matches!(v, Value::Empty)).count() as f64;
        }
    }
    Ok(Value::Number(n))
}

/// `MAX(...)` — largest of the numeric set; empty set → 0 (Excel).
pub(crate) fn max(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let ns = collect_numbers(ctx, args)?;
    Ok(Value::Number(
        ns.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    ))
}

/// `MIN(...)` — smallest of the numeric set; empty set → 0 (Excel).
pub(crate) fn min(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let ns = collect_numbers(ctx, args)?;
    Ok(Value::Number(
        ns.iter().copied().fold(f64::INFINITY, f64::min),
    ))
}

/// `MEDIAN(...)` — middle of the sorted set (mean of the two middles for
/// even counts); empty set → `#NUM!`.
pub(crate) fn median(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let mut ns = collect_numbers(ctx, args)?;
    if ns.is_empty() {
        return Err(ExcelError::Num.into());
    }
    ns.sort_by(f64::total_cmp);
    let mid = ns.len() / 2;
    // Odd count: the middle; even: mean of the two middles. `ns` is
    // non-empty here (checked above), so `mid` is in bounds.
    let m = match (ns.get(mid), ns.get(mid.wrapping_sub(1))) {
        (Some(&hi), Some(&lo)) if ns.len() % 2 == 0 => (lo + hi) / 2.0,
        (Some(&hi), _) => hi,
        _ => return Err(ExcelError::Num.into()),
    };
    Ok(Value::Number(m))
}

/// `STDEV(...)` — sample standard deviation (n−1 denominator); fewer than
/// two values → `#DIV/0!`.
pub(crate) fn stdev(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    let v = sample_variance(ctx, args)?;
    Ok(Value::Number(libm::sqrt(v)))
}

/// `VAR(...)` — sample variance (n−1 denominator); fewer than two values
/// → `#DIV/0!`.
pub(crate) fn var_(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    let v = sample_variance(ctx, args)?;
    Ok(Value::Number(v))
}

fn sample_variance(ctx: &mut Ctx<'_>, args: &[Expr]) -> Result<f64, FormulaError> {
    arg_count(args, 1, usize::MAX)?;
    let ns = collect_numbers(ctx, args)?;
    if ns.len() < 2 {
        return Err(ExcelError::DivZero.into());
    }
    let mean = ns.iter().sum::<f64>() / ns.len() as f64;
    let sq: f64 = ns.iter().map(|x| (x - mean) * (x - mean)).sum();
    Ok(sq / (ns.len() - 1) as f64)
}
