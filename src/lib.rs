//! Spreadsheet formula language — tokenizer, Pratt parser, AST, evaluator
//! core. Excel-compatible function semantics.
//!
//! `formula-lang` is the estate's **L0 leaf** for spreadsheet work: a pure,
//! total formula engine with no I/O and no cell storage. Host engines
//! implement the [`CellResolver`] trait (a sheet, a sparse map, a replay
//! fixture); the crate turns formula text into typed results.
//!
//! ```
//! use formula_lang::{parse, evaluate, MapResolver, Value};
//!
//! // A tiny sheet: A1=2, B1=3, C1 text.
//! let mut sheet = MapResolver::new();
//! sheet.set_num(1, 1, 2.0);
//! sheet.set_num(2, 1, 3.0);
//! sheet.set_text(3, 1, "unit");
//!
//! let e = parse("=SUM(A1:B1) * 10 & \" \" & C1").unwrap();
//! assert_eq!(
//!     evaluate(&e, &sheet).unwrap(),
//!     Value::Text("50 unit".to_string())
//! );
//! ```
//!
//! # The pipeline
//!
//! 1. [`tokenize`] — `&str` in, zero-copy [`Token`]s out (`&[u8]`-backed
//!    slices of the input; the one allocation is the token vector).
//! 2. [`parse`] — a Pratt parser over the Excel precedence ladder
//!    (comparison < concat < add < mul < **right-assoc** power < unary <
//!    postfix `%`), producing the [`Expr`] AST.
//! 3. [`evaluate`] — a depth-capped, allocation-light evaluator over a
//!    [`CellResolver`]; [`evaluate_with_clock`] pins "now" for the
//!    volatile functions.
//! 4. [`Display for Expr`](Expr#impl-Display) / [`to_formula`] —
//!    precedence-correct serialization (round-trip property-tested).
//!
//! # Excel compatibility, stated precisely
//!
//! - **Precedence & the unary quirk**: `-2^2` is `(-2)^2` = `4` — prefix
//!   unary binds *tighter* than power, as Excel does (and unlike school
//!   math). `2^-3` still parses; `^` is right-associative (`2^3^2` =
//!   `512`).
//! - **Coercions**: arithmetic coerces bool→1/0, numeric text→number,
//!   empty→0; comparisons rank Number < Text < Boolean and never
//!   cross-coerce (`"1" = 1` is FALSE); text compares case-insensitively;
//!   empty substitutes the other side's zero.
//! - **Errors**: the seven `#…!` values propagate as data;
//!   `IFERROR`/`IFNA`/`ISERROR`/`ISNA` catch them. Two documented
//!   divergences: *unknown function names* are a hard
//!   [`FormulaError::UnknownFunction`] rather than `#NAME?` (typed over
//!   forgiving), and a bare range used as a scalar (`=A1:B2`) is
//!   `#VALUE!` rather than a spill (scalar core).
//! - **Rounding**: `ROUND` is half-away-from-zero; `INT` floors (−∞);
//!   `TRUNC`/`ROUNDDOWN` truncate; `MOD` takes the divisor's sign;
//!   `CEILING`/`FLOOR` carry Excel's significance-sign rules.
//! - **Text**: UTF-16 code-unit indexing (`LEN("𐍈")` = 2); `TRIM` only
//!   ever touches U+0020; `SEARCH` honors wildcards, `FIND` does not;
//!   cell text caps at 32767 units (`REPT` overflows to `#VALUE!`).
//! - **Dates**: the Lotus leap bug is preserved — serial 60 is the
//!   phantom 1900-02-29; 1970-01-01 is serial 25569.
//! - **Volatile functions**: `TODAY`, `NOW`, `OFFSET` — reported by
//!   [`is_volatile`], clocked by [`evaluate_with_clock`].
//!
//! # The omitted-argument marker
//!
//! The public AST has no "omitted" variant, so empty argument slots
//! (`IF(A1,,5)`, `SUM(1,)`) are stored as
//! [`Expr::Error`]`(`[`ExcelError::Null`]`)` and detected with
//! [`is_omitted_arg`]. One documented trade-off: a literal `#NULL!` in a
//! direct argument position is treated as omitted. Outside argument
//! positions the marker is inert.
//!
//! # Safety & totality
//!
//! `#![no_std]` + `alloc`; `unsafe_code = deny`; `unwrap`/`expect`/
//! `panic`/indexing deny-linted on the lib target. Parsing and evaluation
//! are **total**: hostile input (fuzz-verified) yields typed errors —
//! including [`FormulaError::RecursionLimit`] past [`MAX_DEPTH`] nesting —
//! never a panic, never a stack overflow.
//!
//! # Allocation discipline
//!
//! The numeric scalar path (operators, coercions, all arithmetic and
//! comparison functions) allocates nothing — coercion happens through
//! `f64` locals, and results are returned unboxed. The allocating edges
//! are exactly where ownership forces them: text results (owned
//! `String`s), aggregate operand vectors, range materialization (the
//! resolver's policy — see [`CellResolver::get_range`]), and the
//! parse-path's token vector and AST nodes.

#![no_std]
#![warn(missing_docs)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod ast;
pub mod date;
pub mod display;
pub mod error;
pub mod eval;
pub mod funcs;
pub mod lexer;
pub mod parser;
pub mod resolver;
pub mod token;
pub mod value;

pub use crate::ast::{
    col_name, col_number, is_omitted_arg, normalize_range, BinaryOp, CellRef, Expr, UnaryOp,
    MAX_COL, MAX_ROW,
};
pub use crate::display::{to_formula, to_formula as to_string};
pub use crate::error::{ExcelError, FormulaError};
pub use crate::eval::{evaluate, evaluate_with_clock, is_volatile};
pub use crate::funcs::builtin_functions;
pub use crate::lexer::tokenize;
pub use crate::parser::parse;
pub use crate::parser::MAX_DEPTH;
pub use crate::resolver::{CellResolver, EmptyResolver, MapResolver};
pub use crate::token::{Token, TokenKind};
pub use crate::value::Value;

/// Crate version, for host diagnostics.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;
    use crate::alloc::string::ToString;

    #[test]
    fn readme_example() {
        let mut sheet = MapResolver::new();
        sheet.set_num(1, 1, 2.0);
        sheet.set_num(2, 1, 3.0);
        sheet.set_text(3, 1, "unit");
        let e = parse("=SUM(A1:B1) * 10 & \" \" & C1").unwrap();
        assert_eq!(
            evaluate(&e, &sheet).unwrap(),
            Value::Text("50 unit".to_string())
        );
    }

    #[test]
    fn excel_quirks_hold() {
        let empty = EmptyResolver;
        let ev = |s: &str| evaluate(&parse(s).unwrap(), &empty).unwrap();
        // Unary binds tighter than ^ (Excel quirk).
        assert_eq!(ev("-2^2"), Value::Number(4.0));
        // Right-associative power.
        assert_eq!(ev("2^3^2"), Value::Number(512.0));
        // Postfix percent binds tightest.
        assert_eq!(ev("50%*2"), Value::Number(1.0));
        // Cross-type comparison is ranked, not coerced.
        assert_eq!(ev("1=\"1\""), Value::Boolean(false));
        // Text equality is case-insensitive.
        assert_eq!(ev("\"a\"=\"A\""), Value::Boolean(true));
        // Half-away-from-zero rounding.
        assert_eq!(ev("ROUND(2.5,0)"), Value::Number(3.0));
        assert_eq!(ev("ROUND(-2.5,0)"), Value::Number(-3.0));
        // MOD takes the divisor's sign.
        assert_eq!(ev("MOD(-3,2)"), Value::Number(1.0));
        // The Lotus leap bug: serial 60 is the phantom 1900-02-29.
        assert_eq!(
            ev("TEXT(60,\"yyyy-mm-dd\")"),
            Value::Text("1900-02-29".to_string())
        );
    }
}
