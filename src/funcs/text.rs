//! Text functions — UTF-16 semantics (Excel counts code units, so `LEN`
//! of an emoji is 2) plus the `TEXT` format-code engine (numeric subset +
//! date/time codes) and the General fallback.

use super::{arg_count, arg_count_exact, int_arg, text_arg, Ctx, R};
use crate::ast::Expr;
use crate::date;
use crate::error::{ExcelError, FormulaError};
use crate::value::{coerce_text, number_to_text, Value};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Excel's cell text cap — `REPT` overflows past it.
const MAX_TEXT: usize = 32_767;

const MONTHS_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTHS_LONG: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `CONCATENATE(...)` — joins coerced text (`&` in function form; omitted
/// slots join as empty).
pub(crate) fn concatenate(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, usize::MAX)?;
    let mut out = String::new();
    for a in args {
        if crate::ast::is_omitted_arg(a) {
            continue;
        }
        let v = super::eval1(ctx, a)?;
        out.push_str(&coerce_text(&v).map_err(FormulaError::Eval)?);
    }
    Ok(Value::Text(out))
}

/// `LEFT(t, [n=1])`
pub(crate) fn left(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, 2)?;
    let s = text_arg(ctx, args, 0)?;
    let n = opt_int(ctx, args, 1, 1.0)?;
    if n < 0.0 {
        return Err(ExcelError::Value.into());
    }
    Ok(Value::Text(take_utf16(&s, n as usize)))
}

/// `RIGHT(t, [n=1])`
pub(crate) fn right(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 1, 2)?;
    let s = text_arg(ctx, args, 0)?;
    let n = opt_int(ctx, args, 1, 1.0)?;
    if n < 0.0 {
        return Err(ExcelError::Value.into());
    }
    let units: Vec<u16> = s.encode_utf16().collect();
    let start = units.len().saturating_sub(n as usize);
    let tail = units.get(start..).unwrap_or(&[]);
    Ok(Value::Text(String::from_utf16_lossy(tail)))
}

/// `MID(t, start, len)` — 1-based UTF-16 start; `start < 1` or
/// `len < 0` → `#VALUE!`.
pub(crate) fn mid(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 3)?;
    let s = text_arg(ctx, args, 0)?;
    let start = int_arg(ctx, args, 1)?;
    let len = int_arg(ctx, args, 2)?;
    if start < 1.0 || len < 0.0 {
        return Err(ExcelError::Value.into());
    }
    let units: Vec<u16> = s.encode_utf16().collect();
    let start = (start as usize - 1).min(units.len());
    let end = start.saturating_add(len as usize).min(units.len());
    let window = units.get(start..end).unwrap_or(&[]);
    Ok(Value::Text(String::from_utf16_lossy(window)))
}

/// `LEN(t)` — UTF-16 code units, like Excel.
pub(crate) fn len(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let s = text_arg(ctx, args, 0)?;
    Ok(Value::Number(s.encode_utf16().count() as f64))
}

/// `LOWER(t)`
pub(crate) fn lower(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    Ok(Value::Text(text_arg(ctx, args, 0)?.to_lowercase()))
}

/// `UPPER(t)`
pub(crate) fn upper(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    Ok(Value::Text(text_arg(ctx, args, 0)?.to_uppercase()))
}

/// `PROPER(t)` — capitalizes each letter that follows a non-letter
/// (`PROPER("o'neil 2nd")` → `"O'Neil 2Nd"`), lowercases the rest.
pub(crate) fn proper(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let s = text_arg(ctx, args, 0)?;
    let mut out = String::with_capacity(s.len());
    let mut prev_is_letter = false;
    for c in s.chars() {
        if c.is_alphabetic() {
            if prev_is_letter {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            prev_is_letter = true;
        } else {
            out.push(c);
            prev_is_letter = false;
        }
    }
    Ok(Value::Text(out))
}

/// `TRIM(t)` — collapses runs of *spaces* (U+0020 only, like Excel) and
/// strips the ends; other whitespace survives.
pub(crate) fn trim(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 1)?;
    let s = text_arg(ctx, args, 0)?;
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        if c == ' ' {
            pending_space = !out.is_empty();
        } else {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(c);
        }
    }
    Ok(Value::Text(out))
}

/// `SUBSTITUTE(t, old, new, [instance])` — case-sensitive; empty `old` is
/// a no-op; `instance < 1` → `#VALUE!`; an `instance` beyond the
/// occurrence count leaves the text unchanged.
pub(crate) fn substitute(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 3, 4)?;
    let s = text_arg(ctx, args, 0)?;
    let old = text_arg(ctx, args, 1)?;
    let new = text_arg(ctx, args, 2)?;
    if old.is_empty() {
        return Ok(Value::Text(s));
    }
    let occurrences: Vec<usize> = s.match_indices(old.as_str()).map(|(i, _)| i).collect();
    if args.len() > 3 && !crate::ast::is_omitted_arg(super::arg(args, 3)?) {
        let inst = int_arg(ctx, args, 3)?;
        if inst < 1.0 {
            return Err(ExcelError::Value.into());
        }
        let inst = inst as usize;
        if inst > occurrences.len() {
            return Ok(Value::Text(s));
        }
        let Some(&pos) = occurrences.get(inst - 1) else {
            return Ok(Value::Text(s));
        };
        let head = s.get(..pos).ok_or(ExcelError::Value)?;
        let tail = s.get(pos + old.len()..).ok_or(ExcelError::Value)?;
        let mut out = String::with_capacity(s.len());
        out.push_str(head);
        out.push_str(&new);
        out.push_str(tail);
        Ok(Value::Text(out))
    } else {
        Ok(Value::Text(s.replace(old.as_str(), &new)))
    }
}

/// `FIND(find, within, [start=1])` — case-sensitive, **no** wildcards;
/// miss → `#VALUE!`.
pub(crate) fn find(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 2, 3)?;
    let needle = text_arg(ctx, args, 0)?;
    let hay = text_arg(ctx, args, 1)?;
    let start = opt_int(ctx, args, 2, 1.0)?;
    position_of(&needle, &hay, start, false)
}

/// `SEARCH(find, within, [start=1])` — case-insensitive **with**
/// wildcards (`?` `*`, `~` escape); miss → `#VALUE!`.
pub(crate) fn search(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count(args, 2, 3)?;
    let needle = text_arg(ctx, args, 0)?;
    let hay = text_arg(ctx, args, 1)?;
    let start = opt_int(ctx, args, 2, 1.0)?;
    position_of(&needle, &hay, start, true)
}

/// Shared `FIND`/`SEARCH` position scan over UTF-16 units.
fn position_of(needle: &str, hay: &str, start: f64, wildcard: bool) -> R {
    if start < 1.0 {
        return Err(ExcelError::Value.into());
    }
    let units: Vec<u16> = hay.encode_utf16().collect();
    let from = (start as usize - 1).min(units.len());
    if needle.is_empty() {
        // Excel: FIND("", s, n) → n (when n ≤ len + 1).
        if start as usize > units.len() + 1 {
            return Err(ExcelError::Value.into());
        }
        return Ok(Value::Number(start));
    }
    let n_units: Vec<u16> = needle.encode_utf16().collect();
    if !wildcard && n_units.len() > units.len().saturating_sub(from) {
        return Err(ExcelError::Value.into());
    }
    if wildcard {
        // Wildcard patterns match variable-length windows: for each start,
        // does the pattern match SOME prefix of the remaining text?
        for i in from..=units.len() {
            let rest = units
                .get(i..)
                .map_or(alloc::string::String::new(), String::from_utf16_lossy);
            if super::wildcard_prefix_match(needle, &rest) {
                return Ok(Value::Number(i as f64 + 1.0));
            }
        }
    } else {
        let mut i = from;
        while i + n_units.len() <= units.len() {
            let window = units
                .get(i..i + n_units.len())
                .map_or(alloc::string::String::new(), String::from_utf16_lossy);
            if window == needle {
                return Ok(Value::Number(i as f64 + 1.0));
            }
            i += 1;
        }
    }
    Err(ExcelError::Value.into())
}

/// `REPT(t, n)` — `n` truncated; negative → `#VALUE!`; past Excel's
/// 32767-character cell cap → `#VALUE!`.
pub(crate) fn rept(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 2)?;
    let s = text_arg(ctx, args, 0)?;
    let n = int_arg(ctx, args, 1)?;
    if n < 0.0 {
        return Err(ExcelError::Value.into());
    }
    let unit = s.encode_utf16().count();
    let total = unit.saturating_mul(n as usize);
    if total > MAX_TEXT {
        return Err(ExcelError::Value.into());
    }
    let mut out = String::with_capacity(s.len().saturating_mul(n as usize));
    for _ in 0..n as usize {
        out.push_str(&s);
    }
    Ok(Value::Text(out))
}

/// `TEXT(value, format)` — numeric subset (`0`, `0.00`, `#,##0`,
/// `#,##0.00`, `0%`, `0.00%`, `0.00E+00`, `@`, `General`) plus date/time
/// codes (`yyyy`, `mm`, `dd`, `h:mm:ss` family, `mmm`/`mmmm` month names,
/// `ddd`/`dddd` weekday names, `AM/PM`). Unrecognized formats fall back to
/// General, as documented. Text values pass through unchanged.
pub(crate) fn text(ctx: &mut Ctx<'_>, args: &[Expr]) -> R {
    arg_count_exact(args, 2)?;
    let v = super::eval_arg(ctx, args, 0)?;
    let fmt = text_arg(ctx, args, 1)?;
    match &v {
        Value::Text(s) => Ok(Value::Text(s.clone())),
        Value::Empty => Ok(Value::Text(String::new())),
        other => {
            let n = crate::value::coerce_number(other).map_err(FormulaError::Eval)?;
            Ok(Value::Text(format_value(n, &fmt)?))
        }
    }
}

fn opt_int(ctx: &mut Ctx<'_>, args: &[Expr], i: usize, default: f64) -> Result<f64, FormulaError> {
    match args.get(i) {
        None => Ok(default),
        Some(e) if crate::ast::is_omitted_arg(e) => Ok(default),
        Some(_) => int_arg(ctx, args, i),
    }
}

/// UTF-16-aware prefix (Excel's string indexing).
fn take_utf16(s: &str, n: usize) -> String {
    let units: Vec<u16> = s.encode_utf16().collect();
    let end = n.min(units.len());
    let head = units.get(..end).unwrap_or(&[]);
    String::from_utf16_lossy(head)
}

// ------------------------------------------------------------- TEXT engine

/// Routes date-vs-number: any `y`/`d`/`h`/`s` code (or an `mm` run, which
/// is minutes' double duty) is a date format; everything else goes through
/// the numeric engine. `0.00E+00` stays numeric.
fn format_value(n: f64, fmt: &str) -> Result<String, ExcelError> {
    let f = fmt.to_ascii_lowercase();
    let has_date_code = f.chars().any(|c| matches!(c, 'y' | 'd' | 'h' | 's'))
        || (f.contains("mm") && !f.contains("e+"));
    if has_date_code && !f.contains("e+") {
        Ok(format_date(n, &f)?)
    } else {
        Ok(format_number_fmt(n, &f))
    }
}

/// The numeric format subset.
fn format_number_fmt(n: f64, fmt: &str) -> String {
    if fmt.contains('@') || fmt.eq_ignore_ascii_case("general") || fmt.is_empty() {
        return number_to_text(n);
    }
    if let Some(epos) = fmt.find("e+") {
        let mantissa_fmt = &fmt[..epos];
        let decimals = count_decimals(mantissa_fmt);
        let exp_digits = fmt[epos..].chars().filter(|c| *c == '0').count().max(1);
        return sci(n, decimals, exp_digits);
    }
    let percents = fmt.matches('%').count();
    let core = fmt.replace('%', "");
    let grouping = core.contains(',');
    let decimals = count_decimals(&core);
    let scaled = n * libm::pow(100.0, percents as f64);
    let factor = libm::pow(10.0, decimals as f64);
    let rounded = super::math::round_half_away(scaled * factor) / factor;
    let mut out = fixed(rounded, decimals, grouping);
    for _ in 0..percents {
        out.push('%');
    }
    out
}

fn count_decimals(fmt: &str) -> usize {
    match fmt.find('.') {
        Some(dot) => fmt[dot + 1..]
            .chars()
            .filter(|c| *c == '0' || *c == '#')
            .count(),
        None => 0,
    }
}

/// Fixed-point with optional thousands grouping.
fn fixed(n: f64, decimals: usize, grouping: bool) -> String {
    let neg = n < 0.0;
    let a = n.abs();
    let sci = format!("{:.*}", decimals, a);
    let (int_part, frac_part) = match sci.split_once('.') {
        Some((i, fr)) => (i, Some(fr)),
        None => (sci.as_str(), None),
    };
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    if grouping {
        let digits = int_part.as_bytes();
        for (i, b) in digits.iter().enumerate() {
            if i > 0 && (digits.len() - i) % 3 == 0 {
                out.push(',');
            }
            out.push(*b as char);
        }
    } else {
        out.push_str(int_part);
    }
    if let Some(fr) = frac_part {
        out.push('.');
        out.push_str(fr);
    }
    out
}

/// `0.00E+00` style scientific.
fn sci(n: f64, decimals: usize, exp_digits: usize) -> String {
    if n == 0.0 {
        let zero_mantissa = if decimals == 0 {
            "0".to_string()
        } else {
            format!("0.{}", "0".repeat(decimals))
        };
        return format!("{}E+{:0>width$}", zero_mantissa, 0, width = exp_digits);
    }
    let a = n.abs();
    let mut exp = libm::floor(libm::log10(a)) as i32;
    let mut mantissa = a / libm::pow(10.0, exp as f64);
    // Renormalize after formatting rounds up (9.9995 → "10.00").
    let mut m = format!("{:.*}", decimals, mantissa);
    if m.starts_with("10") {
        exp += 1;
        mantissa = 1.0;
        m = format!("{:.*}", decimals, mantissa);
    }
    let sign = if exp < 0 { '-' } else { '+' };
    format!(
        "{}{}E{}{:0>width$}",
        if n < 0.0 { "-" } else { "" },
        m,
        sign,
        exp.abs(),
        width = exp_digits
    )
}

/// Date/time codes over the serial's calendar. Out-of-range serials are
/// `#NUM!`, like Excel.
fn format_date(n: f64, fmt: &str) -> Result<String, ExcelError> {
    let serial = libm::trunc(n) as i64;
    let frac = n - libm::trunc(n);
    // Time-only formats (`h:mm`) are legal for fractional serials — the
    // calendar is only consulted (and range-checked) when a y/m/d code is
    // actually formatted.
    let has_date_part = fmt.chars().any(|c| matches!(c, 'y' | 'd'));
    let (y, mo, d) = if has_date_part {
        date::serial_to_ymd(serial)?
    } else {
        (1900, 1, 1)
    };
    let (h24, mi, s) = date::fraction_to_hms(frac);
    let has_ampm = fmt.contains("am/pm");
    // AM/PM switches the hour to 12-hour form (suffix decided on h24).
    let h: u32 = if has_ampm {
        let h12 = h24 % 12;
        if h12 == 0 {
            12
        } else {
            h12
        }
    } else {
        h24
    };
    let mut out = String::new();
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    let mut last_was_hour = false;
    while let Some(&c) = chars.get(i) {
        let mut run = 1;
        while chars.get(i + run) == Some(&c) {
            run += 1;
        }
        match c {
            'y' => {
                if run >= 3 {
                    date::pad2(&mut out, y / 100);
                    date::pad2(&mut out, y % 100);
                } else {
                    date::pad2(&mut out, y % 100);
                }
                last_was_hour = false;
            }
            'm' => {
                if run >= 3 {
                    let idx = (mo - 1).clamp(0, 11) as usize;
                    let name = MONTHS_SHORT.get(idx).and_then(|_| MONTHS_LONG.get(idx));
                    if let Some(long_name) = name {
                        out.push_str(if run >= 4 {
                            long_name
                        } else {
                            MONTHS_SHORT.get(idx).copied().unwrap_or("")
                        });
                    }
                    last_was_hour = false;
                } else if last_was_hour {
                    date::pad2(&mut out, i64::from(mi));
                    last_was_hour = false;
                } else if run >= 2 {
                    date::pad2(&mut out, mo);
                } else {
                    date::append_int(&mut out, mo);
                }
            }
            'd' => {
                if run >= 4 {
                    out.push_str(date::weekday_name(serial, true));
                } else if run == 3 {
                    out.push_str(date::weekday_name(serial, false));
                } else if run == 2 {
                    date::pad2(&mut out, d);
                } else {
                    date::append_int(&mut out, d);
                }
                last_was_hour = false;
            }
            'h' => {
                if run >= 2 {
                    date::pad2(&mut out, i64::from(h));
                } else {
                    date::append_int(&mut out, i64::from(h));
                }
                last_was_hour = true;
            }
            's' => {
                if run >= 2 {
                    date::pad2(&mut out, i64::from(s));
                } else {
                    date::append_int(&mut out, i64::from(s));
                }
                last_was_hour = false;
            }
            'a' if fmt
                .chars()
                .skip(i)
                .take(5)
                .collect::<String>()
                .eq_ignore_ascii_case("am/pm") =>
            {
                out.push_str(if h24 >= 12 { "PM" } else { "AM" });
                i += 4; // plus the run's own +1 below = 5 chars
            }
            _ => {
                // Literals (separators like ':', '-', ' ') do NOT reset the
                // hour context — `h:mm` needs it across the colon.
                out.push(c);
            }
        }
        i += run;
    }
    Ok(out)
}
