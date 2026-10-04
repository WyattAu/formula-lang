//! Information functions — type predicates over values and references.

use super::{arg, arg_count_exact, eval1, Ctx, R};
use crate::ast::Expr;
use crate::error::{ExcelError, FormulaError};
use crate::value::Value;

/// `ISBLANK(x)` — true for an empty cell. Note `ISBLANK("")` is FALSE:
/// an empty *string* is text, not blankness.
pub(crate) fn isblank(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let v = super::eval_arg(ctx, args, 0).unwrap_or(Value::Error(ExcelError::Null));
    Ok(Value::Boolean(matches!(v, Value::Empty)))
}

/// `ISNUMBER(x)` — errors are not numbers (`ISNUMBER(#N/A)` = FALSE).
pub(crate) fn isnumber(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let v = eval_or_error_value(ctx, 0, args);
    Ok(Value::Boolean(matches!(v, Value::Number(_))))
}

/// `ISTEXT(x)`
pub(crate) fn istext(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let v = eval_or_error_value(ctx, 0, args);
    Ok(Value::Boolean(matches!(v, Value::Text(_))))
}

/// `ISLOGICAL(x)`
pub(crate) fn is_logical(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let v = eval_or_error_value(ctx, 0, args);
    Ok(Value::Boolean(matches!(v, Value::Boolean(_))))
}

/// `ISERROR(x)` — true for any evaluation error or stored error value.
pub(crate) fn iserror(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    match eval1(ctx, arg(args, 0)?) {
        Err(FormulaError::Eval(_)) => Ok(Value::Boolean(true)),
        Ok(Value::Error(_)) => Ok(Value::Boolean(true)),
        Ok(_) => Ok(Value::Boolean(false)),
        Err(e) => Err(e),
    }
}

/// `ISNA(x)` — true only for `#N/A`.
pub(crate) fn isna(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let na = ExcelError::NA;
    match eval1(ctx, arg(args, 0)?) {
        Err(FormulaError::Eval(e)) => Ok(Value::Boolean(e == na)),
        Ok(Value::Error(e)) => Ok(Value::Boolean(e == na)),
        Ok(_) => Ok(Value::Boolean(false)),
        Err(e) => Err(e),
    }
}

/// `ISREF(x)` — true when the argument *syntactically* is a cell or range
/// reference (`ISREF(A1)`, `ISREF(A1:B2)`), regardless of the resolver.
pub(crate) fn isref(args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    Ok(Value::Boolean(crate::eval::is_ref_expr(arg(args, 0)?)))
}

/// Evaluates arg `i`, converting evaluation errors to stored error values
/// — the predicates' "look, don't propagate" mode.
fn eval_or_error_value(ctx: &mut Ctx<'_>, i: usize, args: &[Expr]) -> Value {
    match super::eval_arg(ctx, args, i) {
        Ok(v) => v,
        Err(FormulaError::Eval(e)) => Value::Error(e),
        Err(_) => Value::Error(ExcelError::Value),
    }
}
