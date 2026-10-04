//! Lookup functions — `VLOOKUP`, `HLOOKUP`, `INDEX`, `MATCH`, `OFFSET`
//! over the resolver's row-major range layout.
//!
//! Matching semantics (documented determinism where Excel under-specifies):
//! - **Exact** match: same-type equality only; text is case-insensitive
//!   and honors wildcards (`?` `*`, `~` escape).
//! - **Approximate** (`VLOOKUP`/`HLOOKUP`/`MATCH` type 1): the *last*
//!   same-type value ≤ the lookup scanning forward — equivalent to Excel's
//!   binary search on the sorted data it demands, total on data that
//!   violates the sort. Type −1 mirrors for descending data.
//! - Lookup misses are `#N/A`; an empty result cell reads as `0`
//!   (Excel's reference-into-empty behavior).

use super::{arg, arg_count, bool_arg, eval_arg, int_arg, num_arg, wildcard_match, Ctx, R};
use crate::ast::{is_omitted_arg, Expr};
use crate::error::{ExcelError, FormulaError};
use crate::eval::{cell_values, range_bounds};
use crate::value::Value;
use alloc::vec::Vec;

/// `VLOOKUP(lookup, table, col_index, [range_lookup=TRUE])`
pub(crate) fn vlookup(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 3, 4)?;
    let lookup = eval_arg(ctx, args, 0)?;
    let col_idx = int_arg(ctx, args, 2)?;
    let approx = if args.len() > 3 && !is_omitted_arg(arg(args, 3)?) {
        bool_arg(ctx, args, 3)?
    } else {
        true
    };
    let (bounds, table) = table_of(ctx, arg(args, 1)?)?;
    let (c0, r0, c1, r1) = bounds;
    let width = (c1 - c0 + 1) as usize;
    let height = (r1 - r0 + 1) as usize;
    let col_idx = col_idx as usize;
    if col_idx < 1 {
        return Err(ExcelError::Value.into());
    }
    if col_idx > width {
        return Err(ExcelError::Ref.into());
    }
    let row = find_row(&lookup, &table, width, height, approx)?;
    match row {
        Some(r) => cell_at(&table, width, r, col_idx - 1),
        None => Err(ExcelError::NA.into()),
    }
}

/// `HLOOKUP(lookup, table, row_index, [range_lookup=TRUE])`
pub(crate) fn hlookup(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 3, 4)?;
    let lookup = eval_arg(ctx, args, 0)?;
    let row_idx = int_arg(ctx, args, 2)?;
    let approx = if args.len() > 3 && !is_omitted_arg(arg(args, 3)?) {
        bool_arg(ctx, args, 3)?
    } else {
        true
    };
    let (bounds, table) = table_of(ctx, arg(args, 1)?)?;
    let (c0, r0, c1, r1) = bounds;
    let width = (c1 - c0 + 1) as usize;
    let height = (r1 - r0 + 1) as usize;
    let row_idx = row_idx as usize;
    if row_idx < 1 {
        return Err(ExcelError::Value.into());
    }
    if row_idx > height {
        return Err(ExcelError::Ref.into());
    }
    // First row is the match vector (stride 1 between its cells).
    let row = find_row(&lookup, &table, 1, width, approx)?;
    match row {
        Some(c) => cell_at(&table, width, row_idx - 1, c),
        None => Err(ExcelError::NA.into()),
    }
}

/// `INDEX(range, row, [col])` — 1-based; a single-row range interprets the
/// second argument as a column (Excel's convenience). Position 0 (whole
/// row/column) is outside the scalar core: `#VALUE!`. Off-range → `#REF!`.
pub(crate) fn index(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 2, 3)?;
    let (bounds, table) = table_of(ctx, arg(args, 0)?)?;
    let (c0, r0, c1, r1) = bounds;
    let width = (c1 - c0 + 1) as usize;
    let height = (r1 - r0 + 1) as usize;
    let row = int_arg(ctx, args, 1)? as i64;
    let col = if args.len() > 2 && !is_omitted_arg(arg(args, 2)?) {
        Some(int_arg(ctx, args, 2)? as i64)
    } else {
        None
    };
    let (row, col) = match col {
        Some(c) => (row, c),
        None => {
            if width > 1 && height == 1 {
                (1, row) // single row: the index addresses columns
            } else {
                (row, 1)
            }
        }
    };
    if row < 1 || col < 1 {
        return Err(ExcelError::Value.into());
    }
    if row as usize > height || col as usize > width {
        return Err(ExcelError::Ref.into());
    }
    cell_at(&table, width, row as usize - 1, col as usize - 1)
}

/// `MATCH(lookup, vector, [match_type=1])` — 1-based position in a
/// one-dimensional range; type 1 (default) approximate ascending,
/// 0 exact (wildcards honored for text), −1 approximate descending. A 2-D
/// range is `#N/A` per Excel.
pub(crate) fn match_(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 2, 3)?;
    let lookup = eval_arg(ctx, args, 0)?;
    let kind = if args.len() > 2 && !is_omitted_arg(arg(args, 2)?) {
        num_arg(ctx, args, 2)?
    } else {
        1.0
    };
    let (bounds, table) = table_of(ctx, arg(args, 1)?)?;
    let (c0, r0, c1, r1) = bounds;
    let width = (c1 - c0 + 1) as usize;
    let height = (r1 - r0 + 1) as usize;
    if width != 1 && height != 1 {
        return Err(ExcelError::NA.into());
    }
    let n = width.max(height);
    let mut best: Option<usize> = None;
    if kind == 0.0 {
        for i in 0..n {
            if let Some(v) = table.get(i) {
                if let Value::Error(e) = v {
                    return Err(FormulaError::Eval(*e));
                }
                if lookup_eq(&lookup, v) {
                    best = Some(i);
                    break;
                }
            }
        }
    } else if kind > 0.0 {
        // Ascending: last value ≤ lookup.
        for i in 0..n {
            if let Some(v) = table.get(i) {
                if let Value::Error(e) = v {
                    return Err(FormulaError::Eval(*e));
                }
                if let Some(ord) = lookup_cmp(&lookup, v) {
                    if ord != core::cmp::Ordering::Less {
                        best = Some(i);
                    } else {
                        break;
                    }
                }
            }
        }
    } else {
        // Descending: last value ≥ lookup.
        for i in 0..n {
            if let Some(v) = table.get(i) {
                if let Value::Error(e) = v {
                    return Err(FormulaError::Eval(*e));
                }
                if let Some(ord) = lookup_cmp(&lookup, v) {
                    if ord != core::cmp::Ordering::Greater {
                        best = Some(i);
                    } else {
                        break;
                    }
                }
            }
        }
    }
    match best {
        Some(i) => Ok(Value::Number(i as f64 + 1.0)),
        None => Err(ExcelError::NA.into()),
    }
}

/// `OFFSET(ref, rows, cols, [height], [width])` — resolves a **shifted**
/// cell through the resolver. Multi-cell results are outside the scalar
/// core (`#VALUE!`); off-grid shifts are `#REF!`. Volatile (see
/// [`crate::is_volatile`]).
pub(crate) fn offset(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 3, 5)?;
    let (c0, r0) = match arg(args, 0)? {
        crate::ast::Expr::CellRef { col, row, .. } => (*col, *row),
        crate::ast::Expr::Range { start, .. } => (start.col, start.row),
        _ => return Err(FormulaError::InvalidRange),
    };
    let rows = int_arg(ctx, args, 1)?;
    let cols = int_arg(ctx, args, 2)?;
    let tc = (i64::from(c0) + cols as i64) as u64;
    let tr = (i64::from(r0) + rows as i64) as u64;
    if tc < 1
        || tc > u64::from(crate::ast::MAX_COL)
        || tr < 1
        || tr > u64::from(crate::ast::MAX_ROW)
    {
        return Err(ExcelError::Ref.into());
    }
    let (tc, tr) = (tc as u32, tr as u32);
    let h = if args.len() > 3 && !is_omitted_arg(arg(args, 3)?) {
        int_arg(ctx, args, 3)?
    } else {
        1.0
    };
    let w = if args.len() > 4 && !is_omitted_arg(arg(args, 4)?) {
        int_arg(ctx, args, 4)?
    } else {
        1.0
    };
    if h < 1.0 || w < 1.0 {
        return Err(ExcelError::Value.into());
    }
    if h > 1.0 || w > 1.0 {
        // Multi-cell OFFSET result: the scalar core returns a value, not a
        // range — #VALUE! (documented limitation).
        return Err(ExcelError::Value.into());
    }
    Ok(ctx.resolver.get(tc, tr).unwrap_or(Value::Number(0.0)))
}

// ------------------------------------------------------------------ helpers

/// A materialized table: normalized `(min_col, min_row, max_col, max_row)`
/// corners plus the row-major cell values.
type Table = ((u32, u32, u32, u32), Vec<Value>);

/// Materializes a table argument (range or single cell) with normalized
/// bounds. Anything else is [`FormulaError::InvalidRange`].
fn table_of(ctx: &mut Ctx<'_>, e: &crate::ast::Expr) -> Result<Table, FormulaError> {
    let bounds = range_bounds(e)?;
    let vals = cell_values(ctx, e)?.ok_or(FormulaError::InvalidRange)?;
    Ok((bounds, vals))
}

/// Row-major indexing helper.
fn cell_at(table: &[Value], width: usize, row: usize, col: usize) -> R {
    match table.get(row * width + col) {
        // Excel reads an empty lookup target as 0.
        Some(Value::Empty) | None => Ok(Value::Number(0.0)),
        Some(Value::Error(e)) => Err(FormulaError::Eval(*e)),
        Some(v) => Ok(v.clone()),
    }
}

/// Exact-match row/column scan (first hit wins).
fn find_row(
    lookup: &Value,
    table: &[Value],
    width: usize,
    count: usize,
    approx: bool,
) -> Result<Option<usize>, FormulaError> {
    if !approx {
        for i in 0..count {
            if let Some(v) = table.get(i * width) {
                if let Value::Error(e) = v {
                    return Err(FormulaError::Eval(*e));
                }
                if lookup_eq(lookup, v) {
                    return Ok(Some(i));
                }
            }
        }
        return Ok(None);
    }
    // Approximate: last same-type value ≤ lookup (ascending data).
    let mut best: Option<usize> = None;
    for i in 0..count {
        if let Some(v) = table.get(i * width) {
            if let Value::Error(e) = v {
                return Err(FormulaError::Eval(*e));
            }
            if let Some(ord) = lookup_cmp(lookup, v) {
                if ord != core::cmp::Ordering::Less {
                    best = Some(i);
                } else {
                    break;
                }
            }
        }
    }
    Ok(best)
}

/// Same-type equality for lookup keys: case-insensitive text (with
/// wildcards when the key carries them), exact numbers/booleans.
fn lookup_eq(lookup: &Value, cell: &Value) -> bool {
    match (lookup, cell) {
        (Value::Text(k), Value::Text(t)) => wildcard_match(k, t),
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        _ => false,
    }
}

/// Same-type ordering for approximate lookups; `None` on type mismatch
/// (mismatched rows are skipped, like Excel ignores foreign types in its
/// binary search).
fn lookup_cmp(lookup: &Value, cell: &Value) -> Option<core::cmp::Ordering> {
    use crate::value::compare;
    match (lookup, cell) {
        (Value::Number(_), Value::Number(_))
        | (Value::Boolean(_), Value::Boolean(_))
        | (Value::Text(_), Value::Text(_)) => compare(lookup, cell).ok(),
        _ => None,
    }
}
