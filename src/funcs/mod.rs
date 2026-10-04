//! The built-in function library — 63 functions, Excel-compatible
//! semantics, one flat dispatch.
//!
//! Functions see arguments as AST slices (they decide what to evaluate and
//! when — `IF` is lazy, `AND` is not), share the [`Ctx`] (resolver, clock,
//! recursion budget), and return the same `Result<Value, FormulaError>`
//! channel as [`evaluate`](crate::evaluate). Argument-count violations and
//! type failures are Excel error *values* (`#VALUE!`) — catchable with
//! `IFERROR`, as in Excel.

pub(crate) mod info;
pub(crate) mod logical;
pub(crate) mod lookup;
pub(crate) mod math;
pub(crate) mod stat;
pub(crate) mod text;

use crate::ast::Expr;
use crate::error::{ExcelError, FormulaError};
use crate::resolver::CellResolver;
use crate::value::Value;
use alloc::string::String;
use alloc::vec::Vec;

/// Evaluator context threaded through every function.
pub(crate) struct Ctx<'a> {
    /// The host's cell store.
    pub resolver: &'a dyn CellResolver,
    /// "Now" as an Excel serial — the volatile functions' only clock.
    pub now: f64,
    /// Live recursion budget (checked by [`crate::eval::eval`]).
    pub depth: usize,
}

pub(crate) type R = Result<Value, FormulaError>;

/// The full built-in table, upper case. Exposed for hosts that want to
/// validate or autocomplete function names.
///
/// ```
/// use formula_lang::builtin_functions;
///
/// assert!(builtin_functions().contains(&"VLOOKUP"));
/// assert_eq!(builtin_functions().len(), 63);
/// ```
#[must_use]
pub fn builtin_functions() -> &'static [&'static str] {
    &[
        // arithmetic (18)
        "SUM",
        "PRODUCT",
        "ABS",
        "SIGN",
        "SQRT",
        "POWER",
        "EXP",
        "LN",
        "LOG",
        "LOG10",
        "MOD",
        "INT",
        "TRUNC",
        "ROUND",
        "ROUNDUP",
        "ROUNDDOWN",
        "CEILING",
        "FLOOR",
        // statistical (9)
        "AVERAGE",
        "COUNT",
        "COUNTA",
        "COUNTBLANK",
        "MAX",
        "MIN",
        "MEDIAN",
        "STDEV",
        "VAR",
        // logical (6 + 2 zero-arg constants)
        "IF",
        "AND",
        "OR",
        "NOT",
        "IFERROR",
        "IFNA",
        "TRUE",
        "FALSE",
        // text (15)
        "CONCATENATE",
        "LEFT",
        "RIGHT",
        "MID",
        "LEN",
        "LOWER",
        "UPPER",
        "PROPER",
        "TRIM",
        "SUBSTITUTE",
        "FIND",
        "SEARCH",
        "REPT",
        "TEXT",
        // lookup (5)
        "VLOOKUP",
        "HLOOKUP",
        "INDEX",
        "MATCH",
        "OFFSET",
        // date (2, volatile)
        "TODAY",
        "NOW",
        // information (7)
        "ISBLANK",
        "ISNUMBER",
        "ISTEXT",
        "ISLOGICAL",
        "ISERROR",
        "ISNA",
        "ISREF",
    ]
}

/// Dispatches a function call. Unknown names are the one hard,
/// non-`IFERROR`-able failure: [`FormulaError::UnknownFunction`].
pub(crate) fn dispatch(ctx: &mut Ctx<'_>, name: &str, args: &[Expr]) -> R {
    // The parser normalizes names to upper case; user-built ASTs get the
    // same treatment here (Excel is case-insensitive).
    let upper = if name.bytes().all(|b| b.is_ascii_uppercase()) {
        None
    } else {
        Some(name.to_ascii_uppercase())
    };
    let name: &str = upper.as_deref().unwrap_or(name);
    match name {
        // arithmetic
        "SUM" => math::sum(ctx, args),
        "PRODUCT" => math::product(ctx, args),
        "ABS" => math::abs(ctx, args),
        "SIGN" => math::sign(ctx, args),
        "SQRT" => math::sqrt(ctx, args),
        "POWER" => math::power(ctx, args),
        "EXP" => math::exp(ctx, args),
        "LN" => math::ln(ctx, args),
        "LOG" => math::log(ctx, args),
        "LOG10" => math::log10(ctx, args),
        "MOD" => math::mod_(ctx, args),
        "INT" => math::int(ctx, args),
        "TRUNC" => math::trunc(ctx, args),
        "ROUND" => math::round(ctx, args),
        "ROUNDUP" => math::roundup(ctx, args),
        "ROUNDDOWN" => math::rounddown(ctx, args),
        "CEILING" => math::ceiling(ctx, args),
        "FLOOR" => math::floor(ctx, args),
        // statistical
        "AVERAGE" => stat::average(ctx, args),
        "COUNT" => stat::count(ctx, args),
        "COUNTA" => stat::counta(ctx, args),
        "COUNTBLANK" => stat::countblank(ctx, args),
        "MAX" => stat::max(ctx, args),
        "MIN" => stat::min(ctx, args),
        "MEDIAN" => stat::median(ctx, args),
        "STDEV" => stat::stdev(ctx, args),
        "VAR" => stat::var_(ctx, args),
        // logical
        "IF" => logical::if_(ctx, args),
        "AND" => logical::and(ctx, args),
        "OR" => logical::or(ctx, args),
        "NOT" => logical::not(ctx, args),
        "IFERROR" => logical::iferror(ctx, args),
        "IFNA" => logical::ifna(ctx, args),
        "TRUE" => logical::true_(args),
        "FALSE" => logical::false_(args),
        // text
        "CONCATENATE" => text::concatenate(ctx, args),
        "LEFT" => text::left(ctx, args),
        "RIGHT" => text::right(ctx, args),
        "MID" => text::mid(ctx, args),
        "LEN" => text::len(ctx, args),
        "LOWER" => text::lower(ctx, args),
        "UPPER" => text::upper(ctx, args),
        "PROPER" => text::proper(ctx, args),
        "TRIM" => text::trim(ctx, args),
        "SUBSTITUTE" => text::substitute(ctx, args),
        "FIND" => text::find(ctx, args),
        "SEARCH" => text::search(ctx, args),
        "REPT" => text::rept(ctx, args),
        "TEXT" => text::text(ctx, args),
        // lookup
        "VLOOKUP" => lookup::vlookup(ctx, args),
        "HLOOKUP" => lookup::hlookup(ctx, args),
        "INDEX" => lookup::index(ctx, args),
        "MATCH" => lookup::match_(ctx, args),
        "OFFSET" => lookup::offset(ctx, args),
        // date — volatile; ctx.now is the only clock they see
        "TODAY" => Ok(Value::Number(libm::floor(ctx.now))),
        "NOW" => Ok(Value::Number(ctx.now)),
        // information
        "ISBLANK" => info::isblank(ctx, args),
        "ISNUMBER" => info::isnumber(ctx, args),
        "ISTEXT" => info::istext(ctx, args),
        "ISLOGICAL" => info::is_logical(ctx, args),
        "ISERROR" => info::iserror(ctx, args),
        "ISNA" => info::isna(ctx, args),
        "ISREF" => info::isref(args),
        _ => Err(FormulaError::UnknownFunction(String::from(name))),
    }
}

// ---------------------------------------------------------------- helpers

/// Argument-count check → `#VALUE!` when violated. Excel rejects these at
/// entry; the pure evaluator reports them at runtime as catchable errors.
pub(crate) fn arg_count(args: &[Expr], min: usize, max: usize) -> Result<(), FormulaError> {
    if args.len() < min || args.len() > max {
        Err(ExcelError::Value.into())
    } else {
        Ok(())
    }
}

/// Exactly-`n`-arguments convenience wrapper.
pub(crate) fn arg_count_exact(args: &[Expr], n: usize) -> Result<(), FormulaError> {
    arg_count(args, n, n)
}

/// Checked argument access — the deny-lint-safe replacement for `args[i]`.
pub(crate) fn arg(args: &[Expr], i: usize) -> Result<&Expr, FormulaError> {
    args.get(i).ok_or_else(|| ExcelError::Value.into())
}

/// Evaluates one argument slot.
pub(crate) fn eval1(ctx: &mut Ctx<'_>, e: &Expr) -> R {
    crate::eval::eval(ctx, e)
}

/// Evaluates argument `i`, treating an omitted slot as `#VALUE!` (callers
/// that give omitted slots meaning handle them before this).
pub(crate) fn eval_arg(ctx: &mut Ctx<'_>, args: &[Expr], i: usize) -> R {
    let Some(e) = args.get(i) else {
        return Err(ExcelError::Value.into());
    };
    if crate::ast::is_omitted_arg(e) {
        return Err(ExcelError::Value.into());
    }
    eval1(ctx, e)
}

/// Evaluates an argument and coerces to `f64`.
pub(crate) fn num_arg(ctx: &mut Ctx<'_>, args: &[Expr], i: usize) -> Result<f64, FormulaError> {
    let v = eval_arg(ctx, args, i)?;
    crate::value::coerce_number(&v).map_err(FormulaError::Eval)
}

/// Evaluates an argument, coerces to `f64`, and truncates toward zero —
/// the coercion every index/count parameter uses.
pub(crate) fn int_arg(ctx: &mut Ctx<'_>, args: &[Expr], i: usize) -> Result<f64, FormulaError> {
    Ok(libm::trunc(num_arg(ctx, args, i)?))
}

/// Evaluates an argument and coerces to `String` (text semantics).
pub(crate) fn text_arg(ctx: &mut Ctx<'_>, args: &[Expr], i: usize) -> Result<String, FormulaError> {
    let v = eval_arg(ctx, args, i)?;
    crate::value::coerce_text(&v).map_err(FormulaError::Eval)
}

/// Evaluates an argument and coerces to `bool`.
pub(crate) fn bool_arg(ctx: &mut Ctx<'_>, args: &[Expr], i: usize) -> Result<bool, FormulaError> {
    let v = eval_arg(ctx, args, i)?;
    crate::value::coerce_bool(&v).map_err(FormulaError::Eval)
}

/// Collects the numeric operand set of an aggregate (`SUM`, `MAX`,
/// `MEDIAN`, …) under Excel's two-track rule:
///
/// - **inside references** (`A1`, `A1:B9`): only `Number` cells count —
///   text, booleans, and empties are skipped; error cells propagate;
/// - **direct scalar arguments**: coerced (`SUM("3")` = 3,
///   `SUM(TRUE)` = 1, `SUM("x")` → `#VALUE!`).
///
/// Omitted slots are skipped entirely.
pub(crate) fn collect_numbers(ctx: &mut Ctx<'_>, args: &[Expr]) -> Result<Vec<f64>, FormulaError> {
    let mut out = Vec::new();
    for a in args {
        if crate::ast::is_omitted_arg(a) {
            continue;
        }
        if crate::eval::is_ref_expr(a) {
            if let Some(vals) = crate::eval::cell_values(ctx, a)? {
                for v in vals {
                    match v {
                        Value::Number(n) => out.push(n),
                        Value::Error(e) => return Err(FormulaError::Eval(e)),
                        Value::Text(_) | Value::Boolean(_) | Value::Empty => {}
                    }
                }
                continue;
            }
        }
        let v = eval1(ctx, a)?;
        match v {
            Value::Number(n) => out.push(n),
            Value::Text(_) | Value::Boolean(_) => {
                out.push(crate::value::coerce_number(&v).map_err(FormulaError::Eval)?);
            }
            Value::Empty => {}
            Value::Error(e) => return Err(FormulaError::Eval(e)),
        }
    }
    Ok(out)
}

/// Collects logical operand values for `AND`/`OR` under Excel's rule:
/// inside references only numbers (`≠ 0`) and booleans count; direct
/// scalars coerce through [`crate::value::coerce_bool`]. Empties and range
/// text are skipped; errors propagate. An empty logical set is `#VALUE!`.
pub(crate) fn collect_bools(ctx: &mut Ctx<'_>, args: &[Expr]) -> Result<Vec<bool>, FormulaError> {
    let mut out = Vec::new();
    for a in args {
        if crate::ast::is_omitted_arg(a) {
            continue;
        }
        if crate::eval::is_ref_expr(a) {
            if let Some(vals) = crate::eval::cell_values(ctx, a)? {
                for v in vals {
                    match v {
                        Value::Boolean(b) => out.push(b),
                        Value::Number(n) => out.push(n != 0.0),
                        Value::Error(e) => return Err(FormulaError::Eval(e)),
                        Value::Text(_) | Value::Empty => {}
                    }
                }
                continue;
            }
        }
        let v = eval1(ctx, a)?;
        match v {
            Value::Error(e) => return Err(FormulaError::Eval(e)),
            Value::Empty => {}
            other => out.push(crate::value::coerce_bool(&other).map_err(FormulaError::Eval)?),
        }
    }
    if out.is_empty() {
        return Err(ExcelError::Value.into());
    }
    Ok(out)
}

/// Wildcard match where the pattern needs to consume only a *prefix* of
/// the text — the inner loop of wildcard `SEARCH`.
pub(crate) fn wildcard_prefix_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    prefix_at(&p, 0, &t, 0)
}

fn prefix_at(p: &[char], pi: usize, t: &[char], ti: usize) -> bool {
    let Some(&pc) = p.get(pi) else {
        return true; // pattern exhausted = prefix matched
    };
    let p_rest = p.get(pi + 1);
    match pc {
        '*' => {
            let mut next = pi + 1;
            while p.get(next) == Some(&'*') {
                next += 1;
            }
            (ti..=t.len()).any(|k| prefix_at(p, next, t, k))
        }
        '~' if matches!(p_rest, Some('*') | Some('?') | Some('~')) => match (t.get(ti), p_rest) {
            (Some(&tc), Some(&esc)) if tc == esc => prefix_at(p, pi + 2, t, ti + 1),
            _ => false,
        },
        '?' => match t.get(ti) {
            Some(_) => prefix_at(p, pi + 1, t, ti + 1),
            None => false,
        },
        c => match t.get(ti) {
            Some(&tc) if tc == c => prefix_at(p, pi + 1, t, ti + 1),
            _ => false,
        },
    }
}

/// Excel wildcard match for `SEARCH`/`MATCH`: `?` is one character, `*` is
/// any run (including empty), `~` escapes the next character, matching is
/// case-insensitive. A pattern with no wildcards is an ordinary
/// case-insensitive equality.
pub(crate) fn wildcard_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    wildcard_at(&p, 0, &t, 0)
}

fn wildcard_at(p: &[char], pi: usize, t: &[char], ti: usize) -> bool {
    let (Some(&pc), p_rest) = (p.get(pi), p.get(pi + 1)) else {
        return ti == t.len();
    };
    match pc {
        '*' => {
            // Collapse consecutive `*`; try every suffix length.
            let mut next = pi + 1;
            while p.get(next) == Some(&'*') {
                next += 1;
            }
            if next == p.len() {
                return true;
            }
            (ti..=t.len()).any(|k| wildcard_at(p, next, t, k))
        }
        // `~` escapes the next wildcard character.
        '~' if matches!(p_rest, Some('*') | Some('?') | Some('~')) => match (t.get(ti), p_rest) {
            (Some(&tc), Some(&esc)) if tc == esc => wildcard_at(p, pi + 2, t, ti + 1),
            _ => false,
        },
        '?' => match t.get(ti) {
            Some(_) => wildcard_at(p, pi + 1, t, ti + 1),
            None => false,
        },
        c => match t.get(ti) {
            Some(&tc) if tc == c => wildcard_at(p, pi + 1, t, ti + 1),
            _ => false,
        },
    }
}
