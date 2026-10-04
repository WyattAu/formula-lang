//! Runtime values and Excel's coercion rules.
//!
//! The four rules that make spreadsheet arithmetic feel like spreadsheet
//! arithmetic, all here:
//!
//! 1. **Arithmetic coerces.** Booleans are 1/0, numeric text parses,
//!    empty is 0, everything else is `#VALUE!`.
//! 2. **Comparisons rank types.** Number < Text < Boolean; `"1" = 1` is
//!    FALSE (no cross-type coercion); text compares case-insensitively;
//!    empty coerces to the other side's zero value.
//! 3. **Concatenation stringifies** through an Excel-approximating
//!    General format (15 significant digits, `E+NN` exponents).
//! 4. **Errors dominate.** Any error operand propagates before coercion
//!    is attempted.

use crate::error::ExcelError;
use alloc::format;
use alloc::string::{String, ToString};

/// A runtime value — the evaluator's currency.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// A 64-bit float. Non-finite values never escape evaluation: any
    /// operation producing inf/NaN folds to
    /// [`ExcelError::Num`](ExcelError::Num).
    Number(f64),
    /// Text (owned; this is the only allocating variant).
    Text(String),
    /// A logical value.
    Boolean(bool),
    /// An error **stored as data** — a cell that holds `#REF!`, or a range
    /// element. Operators convert this to the hard
    /// [`FormulaError::Eval`](crate::FormulaError::Eval) channel on first
    /// use; `ISERROR` and friends inspect it directly.
    Error(ExcelError),
    /// An empty cell.
    Empty,
}

impl Value {
    /// Excel-flavored type name: `"number"`, `"text"`, `"logical"`,
    /// `"error"`, `"empty"`.
    #[must_use]
    pub const fn typename(&self) -> &'static str {
        match self {
            Self::Number(_) => "number",
            Self::Text(_) => "text",
            Self::Boolean(_) => "logical",
            Self::Error(_) => "error",
            Self::Empty => "empty",
        }
    }

    /// True when this is [`Value::Error`].
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }
}

/// Coerces a value to a number under Excel arithmetic rules.
///
/// `Number` → itself · `Boolean` → 1/0 · `Text` → trimmed `f64` parse
/// (never boolean text — `="TRUE"+0` is `#VALUE!`) · `Empty` → 0 ·
/// `Error` → itself. Failures are [`ExcelError::Value`].
pub(crate) fn coerce_number(v: &Value) -> Result<f64, ExcelError> {
    match v {
        Value::Number(n) => Ok(*n),
        Value::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Text(s) => match s.trim().parse::<f64>() {
            Ok(n) => Ok(n),
            Err(_) => Err(ExcelError::Value),
        },
        Value::Empty => Ok(0.0),
        Value::Error(e) => Err(*e),
    }
}

/// Coerces a value to text under concatenation rules: numbers render in
/// General format, `TRUE`/`FALSE` upper-case, empty → `""`, error →
/// propagates.
pub(crate) fn coerce_text(v: &Value) -> Result<String, ExcelError> {
    match v {
        Value::Number(n) => Ok(number_to_text(*n)),
        Value::Text(s) => Ok(s.clone()),
        Value::Boolean(b) => Ok(if *b { "TRUE" } else { "FALSE" }.to_string()),
        Value::Empty => Ok(String::new()),
        Value::Error(e) => Err(*e),
    }
}

/// Coerces a value to a logical under `IF`/`AND`/`OR` rules: `Number` →
/// `n != 0` · `Text` → `"true"`/`"false"` case-insensitive, else `#VALUE!`
/// · `Empty` → `FALSE` · `Error` → propagates.
pub(crate) fn coerce_bool(v: &Value) -> Result<bool, ExcelError> {
    match v {
        Value::Boolean(b) => Ok(*b),
        Value::Number(n) => Ok(*n != 0.0),
        Value::Text(s) => {
            if s.eq_ignore_ascii_case("TRUE") {
                Ok(true)
            } else if s.eq_ignore_ascii_case("FALSE") {
                Ok(false)
            } else {
                Err(ExcelError::Value)
            }
        }
        Value::Empty => Ok(false),
        Value::Error(e) => Err(*e),
    }
}

/// The comparison type rank: Number < Text < Boolean. Errors never reach
/// here (propagated first).
fn type_rank(v: &Value) -> u8 {
    match v {
        Value::Number(_) => 0,
        Value::Text(_) => 1,
        Value::Boolean(_) => 2,
        // Empty is substituted away before ranking.
        Value::Empty | Value::Error(_) => u8::MAX,
    }
}

/// Excel's empty-cell substitution: comparing against an empty yields the
/// other side's zero value (`A1 = 0`, `A1 = ""`, `A1 = FALSE` all TRUE for
/// an empty A1).
fn empty_substitute(other: &Value) -> Value {
    match other {
        Value::Number(_) => Value::Number(0.0),
        Value::Text(_) => Value::Text(String::new()),
        Value::Boolean(_) => Value::Boolean(false),
        _ => Value::Number(0.0),
    }
}

/// Excel comparison: returns `Ordering` with the type ladder
/// Number < Text < Boolean, case-insensitive text order, and empty-cell
/// substitution. Errors must be propagated by the caller.
pub(crate) fn compare(a: &Value, b: &Value) -> Result<core::cmp::Ordering, ExcelError> {
    use core::cmp::Ordering;
    match (a, b) {
        (Value::Empty, Value::Empty) => Ok(Ordering::Equal),
        (Value::Empty, _) => compare(&empty_substitute(b), b),
        (_, Value::Empty) => compare(a, &empty_substitute(a)),
        (Value::Error(e), _) | (_, Value::Error(e)) => Err(*e),
        (Value::Number(x), Value::Number(y)) => Ok(x.partial_cmp(y).unwrap_or(Ordering::Equal)),
        (Value::Text(x), Value::Text(y)) => Ok(compare_text(x, y)),
        (Value::Boolean(x), Value::Boolean(y)) => Ok(x.cmp(y)),
        // Cross-type: no coercion — rank decides.
        _ => Ok(type_rank(a).cmp(&type_rank(b))),
    }
}

/// Case-insensitive text ordering. ASCII folds exactly; non-ASCII folds by
/// simple `char::to_lowercase` (locale-collation exactness is out of scope
/// for the core and documented).
fn compare_text(x: &str, y: &str) -> core::cmp::Ordering {
    let mut xi = x.chars().flat_map(char::to_lowercase);
    let mut yi = y.chars().flat_map(char::to_lowercase);
    loop {
        match (xi.next(), yi.next()) {
            (None, None) => return core::cmp::Ordering::Equal,
            (None, Some(_)) => return core::cmp::Ordering::Less,
            (Some(_), None) => return core::cmp::Ordering::Greater,
            (Some(cx), Some(cy)) => match cx.cmp(&cy) {
                core::cmp::Ordering::Equal => continue,
                ord => return ord,
            },
        }
    }
}

/// Renders a number in an Excel-approximating **General** format:
/// 15 significant digits, trailing zeros stripped, exponent notation
/// (`1E+20`, `1.5E-07`) outside `[1e-4, 1e11)`. This is what `&`, `TEXT`
/// General, and `CONCATENATE` use.
///
/// Deterministic and std-free — no platform float formatting beyond
/// `core::fmt`'s shortest round-trip digits.
#[must_use]
pub fn number_to_text(n: f64) -> String {
    if n.is_nan() {
        return ExcelError::Num.literal().to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "1E+999" } else { "-1E+999" }.to_string();
    }
    if n == 0.0 {
        return "0".to_string();
    }
    let a = n.abs();
    if (1e-4..1e11).contains(&a) {
        // Decimal, 15 significant digits, trailing zeros stripped.
        let sci = format!("{:.14e}", a); // d.ddddddddddddde±x
        let (mantissa, exp) = split_sci(&sci);
        let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
        let exp: i32 = exp.parse().unwrap_or(0);
        let mut out = String::new();
        if n < 0.0 {
            out.push('-');
        }
        if exp >= 0 {
            let ip = exp as usize + 1;
            let ip = ip.min(digits.len().max(1));
            out.push_str(&digits[..ip]);
            let frac = digits[ip..].trim_end_matches('0');
            if !frac.is_empty() {
                out.push('.');
                out.push_str(frac);
            }
        } else {
            out.push_str("0.");
            for _ in 0..(-exp - 1) {
                out.push('0');
            }
            out.push_str(digits.trim_end_matches('0'));
        }
        out
    } else {
        // Exponent notation: 15 significant digits, `E±NN` (two-digit min).
        let neg = n < 0.0;
        let sci = format!("{:.14e}", a); // d.ddddddddddddddd±x — 15 sig
        let (mantissa, exp) = split_sci(&sci);
        let exp: i32 = exp.parse().unwrap_or(0);
        let mut out = String::new();
        if neg {
            out.push('-');
        }
        out.push_str(mantissa.trim_end_matches('0').trim_end_matches('.'));
        out.push('E');
        if exp < 0 {
            out.push('-');
        } else {
            out.push('+');
        }
        let ea = exp.abs();
        if ea < 10 {
            out.push('0');
        }
        let _ = core::fmt::Write::write_fmt(&mut out, format_args!("{ea}"));
        out
    }
}

/// Splits `core::fmt`'s `{:e}`/`{:.14e}` output (`"3.00000000000000e-1"`)
/// into mantissa and exponent-number strings.
fn split_sci(sci: &str) -> (&str, &str) {
    match sci.split_once('e') {
        Some((m, e)) => (m, e),
        None => (sci, "0"),
    }
}

#[cfg(test)]
mod tests {
    // Test harness: assertions legitimately panic; the lib target's
    // non-test paths remain lint-clean.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;

    #[test]
    fn general_format_decimal() {
        assert_eq!(number_to_text(0.0), "0");
        assert_eq!(number_to_text(1.0), "1");
        assert_eq!(number_to_text(-1.5), "-1.5");
        // 15 significant digits — the Excel display rule.
        assert_eq!(number_to_text(0.1 + 0.2), "0.3");
        assert_eq!(number_to_text(1.0 / 3.0), "0.333333333333333");
        assert_eq!(number_to_text(123456789.125), "123456789.125");
        assert_eq!(number_to_text(0.0001), "0.0001");
        assert_eq!(number_to_text(-0.0001), "-0.0001");
        assert_eq!(number_to_text(1.5e10), "15000000000");
    }

    #[test]
    fn general_format_exponent() {
        assert_eq!(number_to_text(1e11), "1E+11");
        assert_eq!(number_to_text(1e20), "1E+20");
        assert_eq!(number_to_text(1.5e-5), "1.5E-05");
        assert_eq!(number_to_text(1e-7), "1E-07");
        assert_eq!(number_to_text(-2.5e13), "-2.5E+13");
        assert_eq!(number_to_text(6.02e23), "6.02E+23");
        assert_eq!(number_to_text(f64::INFINITY), "1E+999");
        assert_eq!(number_to_text(f64::NEG_INFINITY), "-1E+999");
        assert_eq!(number_to_text(f64::NAN), "#NUM!");
    }

    #[test]
    fn coercions() {
        assert_eq!(coerce_number(&Value::Boolean(true)), Ok(1.0));
        assert_eq!(coerce_number(&Value::Empty), Ok(0.0));
        assert_eq!(coerce_number(&Value::Text(" 3.5 ".into())), Ok(3.5));
        assert_eq!(
            coerce_number(&Value::Text("TRUE".into())),
            Err(ExcelError::Value)
        );
        assert_eq!(
            coerce_number(&Value::Text("x".into())),
            Err(ExcelError::Value)
        );
        assert_eq!(
            coerce_number(&Value::Error(ExcelError::NA)),
            Err(ExcelError::NA)
        );
        assert_eq!(coerce_bool(&Value::Number(2.0)), Ok(true));
        assert_eq!(coerce_bool(&Value::Text("false".into())), Ok(false));
        assert_eq!(
            coerce_bool(&Value::Text("yes".into())),
            Err(ExcelError::Value)
        );
        assert_eq!(coerce_text(&Value::Empty), Ok(String::new()));
        assert_eq!(coerce_text(&Value::Boolean(false)), Ok("FALSE".into()));
    }

    #[test]
    fn comparison_ladder() {
        use core::cmp::Ordering::*;
        assert_eq!(
            compare(&Value::Number(1.0), &Value::Text("a".into())),
            Ok(Less)
        );
        assert_eq!(
            compare(&Value::Text("z".into()), &Value::Boolean(false)),
            Ok(Less)
        );
        assert_eq!(
            compare(&Value::Boolean(false), &Value::Boolean(true)),
            Ok(Less)
        );
        // Cross-type equality is FALSE (never coerced).
        assert_eq!(
            compare(&Value::Number(1.0), &Value::Text("1".into())),
            Ok(Less)
        );
        // Case-insensitive text.
        assert_eq!(
            compare(&Value::Text("ABC".into()), &Value::Text("abc".into())),
            Ok(Equal)
        );
        // Empty substitutes.
        assert_eq!(compare(&Value::Empty, &Value::Number(0.0)), Ok(Equal));
        assert_eq!(compare(&Value::Empty, &Value::Text("".into())), Ok(Equal));
        assert_eq!(compare(&Value::Empty, &Value::Boolean(false)), Ok(Equal));
        assert_eq!(compare(&Value::Empty, &Value::Number(1.0)), Ok(Less));
        // Errors propagate.
        assert_eq!(
            compare(&Value::Error(ExcelError::Ref), &Value::Number(1.0)),
            Err(ExcelError::Ref)
        );
    }
}
