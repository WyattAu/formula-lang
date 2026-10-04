//! The abstract syntax tree.
//!
//! The enum shape is the crate's public contract — deliberately small, so
//! host engines can build, inspect, and rewrite formulas without pattern-
//! matching ceremony. One non-obvious corner: an **omitted argument**
//! (`IF(A1,,5)`, `SUM(1,)`) is represented as
//! `Expr::Error(ExcelError::Null)`; see [`is_omitted_arg`].
//!
//! Cell references are 1-based (`A1` → col 1, row 1); the legal grid is
//! [`MAX_COL`] × [`MAX_ROW`] — Excel's 16384 × 1048576.

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::mem;

/// The widest legal column (`XFD` = 16384).
pub const MAX_COL: u32 = 16_384;
/// The deepest legal row (1048576).
pub const MAX_ROW: u32 = 1_048_576;

/// A formula expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A numeric literal. Non-finite values (`1e999` lexes to inf) evaluate
    /// to `#NUM!`, Excel-style.
    Number(f64),
    /// A string literal (escapes already resolved — owned).
    Text(String),
    /// `TRUE` / `FALSE`.
    Boolean(bool),
    /// An error literal (`#DIV/0!` …). Evaluates to
    /// [`FormulaError::Eval`](crate::FormulaError::Eval) — except that, as
    /// the canonical omitted-argument marker ([`is_omitted_arg`]), it means
    /// "argument not supplied" when it sits in a function-argument slot.
    Error(crate::error::ExcelError),
    /// A single cell reference. `col`/`row` are 1-based; the `_abs` flags
    /// record `$` anchors and are display-only (evaluation ignores them).
    CellRef {
        /// 1-based column (1 = `A`, 16384 = `XFD`).
        col: u32,
        /// 1-based row.
        row: u32,
        /// Column was `$`-anchored.
        col_abs: bool,
        /// Row was `$`-anchored.
        row_abs: bool,
    },
    /// A rectangular range `start:end`. Coordinates are stored as written
    /// (not normalized); evaluation normalizes via
    /// [`normalize_range`](normalize_range).
    Range {
        /// Top-left as written (may have larger coordinates than `end`).
        start: CellRef,
        /// Bottom-right as written.
        end: CellRef,
    },
    /// A prefix (`-x`, `+x`) or postfix (`x%`) operation.
    Unary {
        /// The operation.
        op: UnaryOp,
        /// The operand.
        expr: Box<Expr>,
    },
    /// An infix operation — arithmetic, concatenation, or comparison.
    Binary {
        /// The operation.
        op: BinaryOp,
        /// Left operand.
        left: Box<Expr>,
        /// Right operand.
        right: Box<Expr>,
    },
    /// A function call. Names are matched case-insensitively at evaluation
    /// (the parser normalizes to upper case); unknown names are
    /// [`FormulaError::UnknownFunction`](crate::FormulaError::UnknownFunction).
    /// Empty slots (`SUM(1,,2)`) hold the omitted-argument marker.
    Function {
        /// Function name, upper case for parsed formulas.
        name: String,
        /// Arguments; an omitted slot is [`Expr::Error`]`(`[`ExcelError::Null`]`)`.
        args: Vec<Expr>,
    },
}

/// A 1-based cell coordinate with `$`-anchor flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRef {
    /// 1-based column.
    pub col: u32,
    /// 1-based row.
    pub row: u32,
    /// Column `$`-anchored.
    pub col_abs: bool,
    /// Row `$`-anchored.
    pub row_abs: bool,
}

impl CellRef {
    /// An unanchored reference (`A1` style).
    #[must_use]
    pub const fn new(col: u32, row: u32) -> Self {
        Self {
            col,
            row,
            col_abs: false,
            row_abs: false,
        }
    }

    /// True when the coordinate is inside the legal grid
    /// (`A1:XFD1048576`).
    #[must_use]
    pub const fn in_grid(&self) -> bool {
        self.col >= 1 && self.col <= MAX_COL && self.row >= 1 && self.row <= MAX_ROW
    }
}

impl fmt::Display for CellRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.col_abs {
            f.write_str("$")?;
        }
        write!(f, "{}", col_name(self.col))?;
        if self.row_abs {
            f.write_str("$")?;
        }
        write!(f, "{}", self.row)
    }
}

/// An infix operator. Arithmetic binds inside concatenation binds inside
/// comparison; `^` is right-associative, everything else left-associative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `/`
    Div,
    /// `^` — right-associative; `-2^2` is `(-2)^2` = 4 (Excel quirk).
    Pow,
    /// `&` — text concatenation.
    Concat,
    /// `=`
    Eq,
    /// `<>`
    Ne,
    /// `<`
    Lt,
    /// `>`
    Gt,
    /// `<=`
    Le,
    /// `>=`
    Ge,
}

impl BinaryOp {
    /// The Pratt binding powers `(left, right)`. `right == left` marks a
    /// right-associative operator; otherwise the operator is
    /// left-associative (`right == left + 1`).
    ///
    /// Precedence, loosest to tightest — comparison (10) < concat (20) <
    /// additive (30) < multiplicative (40) < power (50); prefix unary sits
    /// at 60 and postfix `%` at 70 (see [`UnaryOp`]).
    #[must_use]
    pub const fn binding_power(self) -> (usize, usize) {
        match self {
            Self::Eq | Self::Ne | Self::Lt | Self::Gt | Self::Le | Self::Ge => (10, 11),
            Self::Concat => (20, 21),
            Self::Add | Self::Sub => (30, 31),
            Self::Mul | Self::Div => (40, 41),
            // Right-associative.
            Self::Pow => (50, 50),
        }
    }

    /// The operator's source token (`"+"`, `"<>"`, …).
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::Pow => "^",
            Self::Concat => "&",
            Self::Eq => "=",
            Self::Ne => "<>",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Le => "<=",
            Self::Ge => ">=",
        }
    }
}

/// A prefix or postfix unary operator. Prefix (`-`, `+`) binds tighter than
/// `^` (Excel quirk: `-2^2` = 4); postfix `%` binds tighter still.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    /// `-x` — numeric negation.
    Neg,
    /// `+x` — numeric identity via number coercion (`+"3"` → error,
    /// matching Excel).
    Pos,
    /// `x%` — postfix percent (`50%` → 0.5).
    Percent,
}

impl UnaryOp {
    /// The operator's binding power: prefix 60, postfix 70.
    #[must_use]
    pub const fn binding_power(self) -> usize {
        match self {
            Self::Neg | Self::Pos => 60,
            Self::Percent => 70,
        }
    }

    /// The operator's source token (`"-"`, `"+"`, `"%"`).
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Neg => "-",
            Self::Pos => "+",
            Self::Percent => "%",
        }
    }

    /// True for prefix operators (`Neg`, `Pos`), false for postfix
    /// (`Percent`).
    #[must_use]
    pub const fn is_prefix(self) -> bool {
        !matches!(self, Self::Percent)
    }
}

/// Normalizes a range's corners so `start` is top-left and `end` is
/// bottom-right (`B2:A1` ≡ `A1:B2`, as Excel treats them). Returns
/// `(min_col, min_row, max_col, max_row)`.
#[must_use]
pub fn normalize_range(start: &CellRef, end: &CellRef) -> (u32, u32, u32, u32) {
    (
        start.col.min(end.col),
        start.row.min(end.row),
        start.col.max(end.col),
        start.row.max(end.row),
    )
}

/// True when `expr` is the canonical **omitted-argument marker**:
/// `Expr::Error(ExcelError::Null)`.
///
/// The public `Expr` enum has no dedicated "omitted" variant, so empty
/// argument slots need an in-band encoding. `#NULL!` is the natural pick —
/// the empty-intersection error is already the closest Excel concept — and
/// the trade-off is explicit: a literal `#NULL!` written directly as a
/// function argument (`SUM(#NULL!)`) is indistinguishable from an omitted
/// slot and is treated as omitted. Outside argument slots the marker is
/// inert: `=#NULL!` evaluates to the `#NULL!` error as usual.
#[must_use]
pub fn is_omitted_arg(expr: &Expr) -> bool {
    matches!(expr, Expr::Error(crate::error::ExcelError::Null))
}

/// Renders a 1-based column number as letters (`1` → `"A"`, `28` → `"AB"`,
/// `16384` → `"XFD"`). Values of `0` render as `"?"` (out-of-contract
/// coordinates — see [`CellRef::in_grid`]); values beyond `MAX_COL` wrap
/// arithmetically like a base-26 counter.
#[must_use]
pub fn col_name(col: u32) -> String {
    if col == 0 {
        return "?".to_string();
    }
    let mut rev = Vec::new();
    let mut n = col;
    while n > 0 {
        rev.push(b'A' + ((n - 1) % 26) as u8);
        n = (n - 1) / 26;
    }
    let mut s = String::with_capacity(rev.len());
    while let Some(byte) = rev.pop() {
        s.push(byte as char);
    }
    s
}

/// Parses column letters (`"A"`, `"ab"`, `"XFD"`) to a 1-based column
/// number. Returns `None` for empty input, non-ASCII-alphabetic input, or
/// letters beyond `XFD`.
#[must_use]
pub fn col_number(letters: &str) -> Option<u32> {
    if letters.is_empty() || !letters.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    let mut col: u32 = 0;
    for b in letters.bytes() {
        col = col * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1);
    }
    if col <= MAX_COL {
        Some(col)
    } else {
        None
    }
}

/// Stack-safe teardown: `parse` builds left-deep trees iteratively (a
/// 100k-term sum is a 100k-deep AST), and recursive drop glue would
/// overflow the stack on them. This `Drop` walks the tree with an explicit
/// worklist — O(n), no recursion, no allocation growth.
///
/// The subtle part: an owned `Expr` with children can never be *dropped*
/// inside this function — that would re-enter `Drop` unboundedly. Each
/// node's children are detached into the worklist and replaced with scalar
/// dummies; the emptied shell then drops through the **default glue**
/// (freeing its `Box`es and strings) via the [`trivial_shell`] fast path,
/// which costs one constant stack frame.
impl Drop for Expr {
    fn drop(&mut self) {
        if trivial_shell(self) {
            return;
        }
        let mut stack: Vec<Expr> = Vec::new();
        stack.push(mem::replace(self, Expr::Number(0.0)));
        while let Some(mut node) = stack.pop() {
            // Detach children into the worklist, leaving scalar dummies in
            // the slots.
            match &mut node {
                Expr::Binary { left, right, .. } => {
                    let r = mem::replace(right.as_mut(), Expr::Number(0.0));
                    let l = mem::replace(left.as_mut(), Expr::Number(0.0));
                    stack.push(r);
                    stack.push(l);
                }
                Expr::Unary { expr, .. } => {
                    stack.push(mem::replace(expr.as_mut(), Expr::Number(0.0)));
                }
                Expr::Function { args, .. } => {
                    for a in args.iter_mut() {
                        stack.push(mem::replace(a, Expr::Number(0.0)));
                    }
                }
                _ => {}
            }
            // The shell now contains only scalar dummies — `trivial_shell`
            // makes its own destructor a no-op, and the default glue frees
            // the detached `Box` slots. No leak, no recursion.
            drop(node);
        }
    }
}

/// True when this value's destructor is trivial: either a leaf, or a
/// container whose children have all been replaced with scalar dummies by
/// [`Drop for Expr`](Drop). Detecting shells here lets emptied containers
/// drop through the default glue — freeing their `Box` slots — without
/// re-entering the worklist.
fn trivial_shell(e: &Expr) -> bool {
    match e {
        Expr::Number(_)
        | Expr::Text(_)
        | Expr::Boolean(_)
        | Expr::Error(_)
        | Expr::CellRef { .. }
        | Expr::Range { .. } => true,
        Expr::Binary { left, right, .. } => {
            matches!(**left, Expr::Number(_)) && matches!(**right, Expr::Number(_))
        }
        Expr::Unary { expr, .. } => matches!(**expr, Expr::Number(_)),
        Expr::Function { args, .. } => args.iter().all(|a| matches!(a, Expr::Number(_))),
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::*;
    use crate::error::ExcelError;
    use alloc::string::ToString;

    #[test]
    fn col_name_roundtrip() {
        for col in [1u32, 2, 26, 27, 28, 52, 703, 16_384] {
            assert_eq!(col_number(&col_name(col)), Some(col), "col {col}");
        }
        assert_eq!(col_name(1), "A");
        assert_eq!(col_name(26), "Z");
        assert_eq!(col_name(27), "AA");
        assert_eq!(col_name(28), "AB");
        assert_eq!(col_name(702), "ZZ");
        assert_eq!(col_name(703), "AAA");
        assert_eq!(col_name(16_384), "XFD");
        assert_eq!(col_name(0), "?");
    }

    #[test]
    fn col_number_rejects() {
        assert_eq!(col_number(""), None);
        assert_eq!(col_number("A1"), None);
        assert_eq!(col_number("XFE"), None); // 16385 > MAX_COL
        assert_eq!(col_number("Ä"), None);
        assert_eq!(col_number("ABCD"), None); // shape is caller's concern; 4 letters still numeric
    }

    #[test]
    fn cellref_display_with_anchors() {
        assert_eq!(CellRef::new(1, 1).to_string(), "A1");
        assert_eq!(
            CellRef {
                col: 2,
                row: 3,
                col_abs: true,
                row_abs: false
            }
            .to_string(),
            "$B3"
        );
        assert_eq!(
            CellRef {
                col: 2,
                row: 3,
                col_abs: false,
                row_abs: true
            }
            .to_string(),
            "B$3"
        );
        assert_eq!(
            CellRef {
                col: 16384,
                row: 1_048_576,
                col_abs: true,
                row_abs: true
            }
            .to_string(),
            "$XFD$1048576"
        );
    }

    #[test]
    fn in_grid_bounds() {
        assert!(CellRef::new(1, 1).in_grid());
        assert!(CellRef::new(16_384, 1_048_576).in_grid());
        assert!(!CellRef::new(0, 1).in_grid());
        assert!(!CellRef::new(16_385, 1).in_grid());
        assert!(!CellRef::new(1, 0).in_grid());
        assert!(!CellRef::new(1, 1_048_577).in_grid());
    }

    #[test]
    fn normalize_range_orders_corners() {
        let a = CellRef::new(1, 1);
        let b = CellRef::new(2, 2);
        assert_eq!(normalize_range(&b, &a), (1, 1, 2, 2));
        assert_eq!(normalize_range(&a, &b), (1, 1, 2, 2));
    }

    #[test]
    fn binding_power_ladder() {
        assert!(BinaryOp::Eq.binding_power().0 < BinaryOp::Concat.binding_power().0);
        assert!(BinaryOp::Concat.binding_power().0 < BinaryOp::Add.binding_power().0);
        assert!(BinaryOp::Add.binding_power().0 < BinaryOp::Mul.binding_power().0);
        assert!(BinaryOp::Mul.binding_power().0 < BinaryOp::Pow.binding_power().0);
        // Right-assoc marker: lbp == rbp.
        assert_eq!(
            BinaryOp::Pow.binding_power().0,
            BinaryOp::Pow.binding_power().1
        );
        assert!(BinaryOp::Add.binding_power().1 > BinaryOp::Add.binding_power().0);
        // Unary > power > postfix, per the Excel ladder.
        assert!(BinaryOp::Pow.binding_power().0 < UnaryOp::Neg.binding_power());
        assert!(UnaryOp::Neg.binding_power() < UnaryOp::Percent.binding_power());
        assert!(UnaryOp::Neg.is_prefix());
        assert!(!UnaryOp::Percent.is_prefix());
    }

    #[test]
    fn omitted_marker_only_null() {
        assert!(is_omitted_arg(&Expr::Error(ExcelError::Null)));
        assert!(!is_omitted_arg(&Expr::Error(ExcelError::NA)));
        assert!(!is_omitted_arg(&Expr::Number(0.0)));
        assert!(!is_omitted_arg(&Expr::Text(alloc::string::String::new())));
    }

    #[test]
    fn op_symbols() {
        assert_eq!(BinaryOp::Ne.symbol(), "<>");
        assert_eq!(BinaryOp::Ge.symbol(), ">=");
        assert_eq!(UnaryOp::Percent.symbol(), "%");
    }

    #[test]
    fn iterative_drop_handles_deep_and_stringy_trees() {
        // 20k-deep chain: recursive drop glue would overflow; ours must not.
        let mut e = Expr::Number(0.0);
        for _ in 0..20_000 {
            e = Expr::Binary {
                op: BinaryOp::Concat,
                left: Box::new(e),
                right: Box::new(Expr::Text(alloc::string::String::from("x"))),
            };
        }
        drop(e); // must not crash or leak the strings
                 // Function trees too.
        let mut f = Expr::Function {
            name: alloc::string::String::from("SUM"),
            args: alloc::vec::Vec::new(),
        };
        for _ in 0..5_000 {
            f = Expr::Function {
                name: alloc::string::String::from("SUM"),
                args: alloc::vec![f, Expr::Number(1.0)],
            };
        }
        drop(f);
    }
}
