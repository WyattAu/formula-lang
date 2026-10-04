//! Logical functions — lazy `IF`, non-lazy `AND`/`OR` (Excel propagates
//! their errors even when short-circuiting would "work"), and the
//! error-catching family.

use super::{arg, arg_count, arg_count_exact, bool_arg, collect_bools, eval1, Ctx, R};
use crate::ast::{is_omitted_arg, Expr};
use crate::error::FormulaError;
use crate::value::Value;

/// `IF(cond, then, [else])` — evaluates **only** the taken branch.
/// Omitted then/else slots yield 0 (`IF(A1,,5)` → 0 when taken); a
/// missing third argument yields FALSE (`IF(FALSE,1)` → FALSE).
pub(crate) fn if_(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 2, 3)?;
    let cond = bool_arg(ctx, args, 0)?;
    if cond {
        match args.get(1) {
            Some(e) if !is_omitted_arg(e) => eval1(ctx, e),
            _ => Ok(Value::Number(0.0)),
        }
    } else {
        match args.get(2) {
            Some(e) if !is_omitted_arg(e) => eval1(ctx, e),
            Some(_) => Ok(Value::Number(0.0)), // `IF(c,1,)` → 0
            None => Ok(Value::Boolean(false)), // `IF(c,1)` → FALSE
        }
    }
}

/// `AND(...)` — all logicals true; evaluates every argument (errors
/// propagate, Excel-style).
pub(crate) fn and(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let bs = collect_bools(ctx, args)?;
    Ok(Value::Boolean(bs.iter().all(|b| *b)))
}

/// `OR(...)` — any logical true; evaluates every argument.
pub(crate) fn or(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let bs = collect_bools(ctx, args)?;
    Ok(Value::Boolean(bs.iter().any(|b| *b)))
}

/// `NOT(x)`
pub(crate) fn not(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    Ok(Value::Boolean(!bool_arg(ctx, args, 0)?))
}

/// `IFERROR(value, fallback)` — catches **any** evaluation error or error
/// value in `value` and yields `fallback` instead. This is the function
/// that makes the two error channels observable as one.
pub(crate) fn iferror(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 2)?;
    catch(ctx, args, None)
}

/// `IFNA(value, fallback)` — catches only `#N/A`.
pub(crate) fn ifna(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 2)?;
    catch(ctx, args, Some(crate::error::ExcelError::NA))
}

fn catch(ctx: &mut Ctx<'_>, args: &[Expr], only: Option<crate::error::ExcelError>) -> R {
    match eval1(ctx, arg(args, 0)?) {
        Err(FormulaError::Eval(e)) => {
            if only.is_none_or(|only| e == only) {
                eval1(ctx, arg(args, 1)?)
            } else {
                Err(FormulaError::Eval(e))
            }
        }
        Ok(Value::Error(e)) => {
            if only.is_none_or(|only| e == only) {
                eval1(ctx, arg(args, 1)?)
            } else {
                Ok(Value::Error(e))
            }
        }
        Ok(v) => Ok(v),
        Err(e) => Err(e),
    }
}

/// `TRUE()` — the zero-arg function form of the `TRUE` literal.
pub(crate) fn true_(args: &[Expr]) -> R {
    arg_count_exact(args, 0)?;
    Ok(Value::Boolean(true))
}

/// `FALSE()` — the zero-arg function form of the `FALSE` literal.
pub(crate) fn false_(args: &[Expr]) -> R {
    arg_count_exact(args, 0)?;
    Ok(Value::Boolean(false))
}
