//! Tokens — the zero-copy lexer output.
//!
//! [`Token`] borrows its text straight out of the input `&str`: a string
//! literal is the raw inner span (escapes unresolved, unescaped once at
//! parse time), an identifier is the input slice itself. The only token
//! that is *not* a borrow is [`TokenKind::Cell`] — `$`-anchored references
//! are parsed to column/row numbers eagerly because the `$` characters are
//! not part of any contiguous "word".
//!
//! Classification of bare words is deliberately *deferred to the parser*:
//! `A1` (cell ref), `LOG10` (function — it is followed by `(`), `TRUE`
//! (boolean), and `XFE1` (out-of-range → parse error) are all lexed as
//! [`TokenKind::Ident`]. The lexer has no context; the Pratt parser does.

use crate::error::ExcelError;
use alloc::format;
use alloc::string::{String, ToString};
use core::fmt;

/// The lexical class of a token, with its payload.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TokenKind<'a> {
    /// A numeric literal (`42`, `3.14`, `1e-5`, `.5`) — already parsed to a
    /// float. Overflowing literals (`1e999`) are kept as ±infinity; the
    /// evaluator maps any non-finite number to `#NUM!` at use, Excel-style.
    Number(f64),
    /// The raw inner span of a string literal (`"a""b"` → `a""b`), quotes
    /// stripped, `""` escapes unresolved. The parser materializes the
    /// owned, unescaped `String` once.
    Str(&'a str),
    /// A bare word: a function name (`SUM`), a cell reference (`A1`),
    /// `TRUE`/`FALSE`, or an invalid name (parse error). The parser
    /// classifies by lookahead and shape.
    Ident(&'a str),
    /// A `$`-containing cell reference (`$A$1`, `B$2`, `$C3`) — column and
    /// row are 1-based, already parsed. Range/limit checks happen in the
    /// parser.
    Cell {
        /// 1-based column number (`A` = 1, `XFD` = 16384).
        col: u32,
        /// 1-based row number (at most 1048576 for a legal reference).
        row: u32,
        /// Column is `$`-anchored.
        col_abs: bool,
        /// Row is `$`-anchored.
        row_abs: bool,
    },
    /// An Excel error literal: `#DIV/0!`, `#N/A`, `#NAME?`, `#NULL!`,
    /// `#NUM!`, `#REF!`, `#VALUE!` (case-insensitive).
    ErrLit(ExcelError),
    /// `(`
    LParen,
    /// `)`
    RParen,
    /// `,` — the argument separator.
    Comma,
    /// `:` — the range separator.
    Colon,
    /// `+`
    Plus,
    /// `-`
    Minus,
    /// `*`
    Star,
    /// `/`
    Slash,
    /// `^` — right-associative power.
    Caret,
    /// `&` — text concatenation.
    Amp,
    /// `%` — postfix percent.
    Percent,
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
    /// End of input — always the final token, at `input.len()`.
    Eof,
}

impl TokenKind<'_> {
    /// Short human-readable form, used verbatim in
    /// [`FormulaError::Parse`](crate::FormulaError::Parse) `got` fields.
    #[must_use]
    pub fn describe(&self) -> String {
        match *self {
            Self::Number(n) => {
                if n.is_finite() {
                    format!("{n}")
                } else {
                    "#NUM! (numeric overflow)".to_string()
                }
            }
            Self::Str(s) => format!("string \"{}\"", s.replace('"', "\"\"")),
            Self::Ident(s) => s.to_string(),
            Self::Cell { col, row, .. } => crate::ast::CellRef {
                col,
                row,
                col_abs: false,
                row_abs: false,
            }
            .to_string(),
            Self::ErrLit(e) => e.literal().to_string(),
            Self::LParen => "(".to_string(),
            Self::RParen => ")".to_string(),
            Self::Comma => ",".to_string(),
            Self::Colon => ":".to_string(),
            Self::Plus => "+".to_string(),
            Self::Minus => "-".to_string(),
            Self::Star => "*".to_string(),
            Self::Slash => "/".to_string(),
            Self::Caret => "^".to_string(),
            Self::Amp => "&".to_string(),
            Self::Percent => "%".to_string(),
            Self::Eq => "=".to_string(),
            Self::Ne => "<>".to_string(),
            Self::Lt => "<".to_string(),
            Self::Gt => ">".to_string(),
            Self::Le => "<=".to_string(),
            Self::Ge => ">=".to_string(),
            Self::Eof => "end of formula".to_string(),
        }
    }
}

/// A lexed token: a [`TokenKind`] plus its byte offset in the input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Token<'a> {
    /// The lexical class and payload.
    pub kind: TokenKind<'a>,
    /// Byte offset of the token's first byte in the input.
    pub pos: usize,
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.kind.describe())
    }
}
