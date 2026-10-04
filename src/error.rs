//! Typed, exhaustive error taxonomy for the formula language.
//!
//! Two distinct error types, mirroring the two failure domains of a
//! spreadsheet engine:
//!
//! - [`ExcelError`] — the seven **worksheet error values** (`#DIV/0!`,
//!   `#N/A`, …). These are *data*: a formula like `=IFERROR(1/0, "safe")`
//!   evaluates the `1/0`, observes the error, and recovers. Inside
//!   [`evaluate`](crate::evaluate) they surface as
//!   `Err(FormulaError::Eval(_))` when they are produced by *evaluation*
//!   (arithmetic, coercion, lookup misses), and as `Value::Error` when they
//!   are *stored* in a resolved cell (data passthrough — `=A1` where A1
//!   holds `#REF!` yields the error as a value, exactly what
//!   `ISERROR(A1)` needs to observe).
//!
//! - [`FormulaError`] — everything else: tokenization and parse failures
//!   (with byte positions), unknown function names, the recursion limit,
//!   and structurally invalid ranges. Never a panic: the lib target is
//!   deny-linted against `unwrap`/`expect`/`panic`/`indexing` and fuzzed.
//!
//! Both enums are deliberately **exhaustive** (no `#[non_exhaustive]`):
//! callers are expected to match every variant, and a new variant is a
//! semver-minor event by design — the same policy as `wire-kit`'s
//! `WireError` and `can-core`'s `CanError`.

use alloc::string::String;
use core::fmt;

/// The seven Excel worksheet error values, in `#LITERAL!` form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExcelError {
    /// `#DIV/0!` — division by zero, including `MOD(x, 0)`,
    /// `0^-1`, and `AVERAGE`/`STDEV`/`VAR` over zero numeric values.
    DivZero,
    /// `#N/A` — "no value available": lookup misses (`VLOOKUP`, `MATCH`),
    /// the `#N/A` literal, `ISNA`.
    NA,
    /// `#NAME?` — unrecognized name text. In this crate the literal
    /// `#NAME?` propagates as a value; unknown *function names* are a hard
    /// [`FormulaError::UnknownFunction`] instead (see the crate docs for
    /// the rationale).
    Name,
    /// `#NULL!` — the empty-intersection error. Also the crate's canonical
    /// **omitted-argument marker**: `IF(A1,,5)` stores `Error(Null)` in the
    /// AST for the empty second slot (see [`crate::is_omitted_arg`]).
    Null,
    /// `#NUM!` — a numeric problem: overflow to ±inf/NaN, `SQRT(-1)`,
    /// `LOG` of a non-positive number, `POWER(0, 0)`, sign-mismatched
    /// `CEILING`/`FLOOR`, a date serial outside 1900-9999.
    Num,
    /// `#REF!` — an invalid reference: out-of-grid `OFFSET`, an `INDEX`
    /// position past the edge of its range, a `VLOOKUP` column index wider
    /// than the table, the `#REF!` literal.
    Ref,
    /// `#VALUE!` — wrong type for the operation: arithmetic on
    /// non-numeric text, `MID` with a zero start, `REPT` with a negative
    /// count, wrong argument counts, a 2-D `MATCH` vector.
    Value,
}

impl ExcelError {
    /// The Excel worksheet literal for this error (`"#DIV/0!"`, `"#N/A"`, …).
    #[must_use]
    pub const fn literal(self) -> &'static str {
        match self {
            Self::DivZero => "#DIV/0!",
            Self::NA => "#N/A",
            Self::Name => "#NAME?",
            Self::Null => "#NULL!",
            Self::Num => "#NUM!",
            Self::Ref => "#REF!",
            Self::Value => "#VALUE!",
        }
    }

    /// Parses an Excel error literal (`"#DIV/0!"`, `"#n/a"`, …),
    /// case-insensitively. Returns `None` for anything else.
    #[must_use]
    pub fn from_literal(lit: &str) -> Option<Self> {
        // Longest literals first so prefixes cannot shadow ("#N/A" vs "#NA").
        const LITERALS: [(ExcelError, &str); 7] = [
            (ExcelError::DivZero, "#DIV/0!"),
            (ExcelError::Value, "#VALUE!"),
            (ExcelError::Name, "#NAME?"),
            (ExcelError::Null, "#NULL!"),
            (ExcelError::Num, "#NUM!"),
            (ExcelError::Ref, "#REF!"),
            (ExcelError::NA, "#N/A"),
        ];
        LITERALS
            .iter()
            .find(|(_, s)| lit.eq_ignore_ascii_case(s))
            .map(|(e, _)| *e)
    }
}

impl fmt::Display for ExcelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.literal())
    }
}

/// The single crate-level error for tokenizing, parsing, and evaluation.
///
/// Excel *values* that signal failure (`#DIV/0!` …) ride in
/// [`FormulaError::Eval`] when produced by evaluation; see the module docs
/// for the value-vs-error channel contract.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FormulaError {
    /// The tokenizer hit a character that cannot begin any token, a
    /// malformed `#error` literal, an unterminated string, or a malformed
    /// `$`-anchored reference. `pos` is the byte offset; `char` is the
    /// offending character (the `#` or `"` that began the bad token, or the
    /// unknown character itself).
    #[error("tokenize error at byte {pos}: unexpected {char:?}")]
    Tokenize {
        /// Byte offset of the offending character.
        pos: usize,
        /// The offending character.
        char: char,
    },
    /// The parser hit a token it could not accept. `pos` is the byte offset
    /// of the offending token, `expected` a description of what would have
    /// been legal, `got` the rendered offending token.
    #[error("parse error at byte {pos}: expected {expected}, got {got}")]
    Parse {
        /// Byte offset of the offending token.
        pos: usize,
        /// What the parser would have accepted at this position.
        expected: &'static str,
        /// Rendered form of the token that was actually there.
        got: String,
    },
    /// An Excel error value was produced by evaluation (arithmetic,
    /// coercion, a lookup miss, the `#…!` literal itself). Catchable by
    /// `IFERROR`/`IFNA`/`ISERROR`/`ISNA` *inside* the formula; at the top
    /// level this is the `Err` arm.
    #[error("evaluation error: {0}")]
    Eval(ExcelError),
    /// A function name not in the built-in table was called. Excel would
    /// show `#NAME?`; this crate makes unknown functions a hard, typed
    /// error instead (rationale in the crate docs). The name is uppercased.
    #[error("unknown function: {0}")]
    UnknownFunction(String),
    /// Parse or evaluation recursion exceeded
    /// [`MAX_DEPTH`](crate::MAX_DEPTH). Returned instead of overflowing the
    /// stack — the crate is stack-safe by construction, fuzz-verified.
    #[error("recursion limit exceeded")]
    RecursionLimit,
    /// A range argument was structurally impossible: a non-reference where
    /// a range is required (`VLOOKUP(1, 5, 2)`), or a range not shaped as
    /// its function demands (a 2-D table handed to `MATCH`).
    #[error("invalid range argument")]
    InvalidRange,
}

impl From<ExcelError> for FormulaError {
    fn from(e: ExcelError) -> Self {
        Self::Eval(e)
    }
}

impl FormulaError {
    /// The Excel worksheet error behind this failure, if any. Convenience
    /// for callers that want to render `#DIV/0!`-style results.
    #[must_use]
    pub const fn excel_error(&self) -> Option<ExcelError> {
        match self {
            Self::Eval(e) => Some(*e),
            _ => None,
        }
    }
}
