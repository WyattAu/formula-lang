//! The tokenizer — `&str` in, zero-copy [`Token`]s out.
//!
//! Total over all input: every byte position either extends a token or
//! yields a typed [`FormulaError::Tokenize`]; nothing panics (deny-linted,
//! fuzz-verified). Whitespace (space, tab, CR, LF) separates tokens and is
//! otherwise discarded.
//!
//! Zero-copy guarantee: [`TokenKind::Str`] and [`TokenKind::Ident`] are
//! slices of the input. The single allocation of the API is the returned
//! `Vec<Token<'_>>` itself.

use crate::error::{ExcelError, FormulaError};
use crate::token::{Token, TokenKind};
use alloc::vec::Vec;

/// Tokenizes a formula body (a leading `=` is allowed but not required —
/// the parser accepts it; the lexer treats it as the `=` operator like any
/// other).
///
/// ```
/// use formula_lang::{tokenize, TokenKind};
///
/// let toks = tokenize("SUM(A1:B2, 3.5)").unwrap();
/// let kinds: Vec<&TokenKind<'_>> = toks.iter().map(|t| &t.kind).collect();
/// assert!(matches!(kinds[0], TokenKind::Ident("SUM")));
/// assert_eq!(toks.last().unwrap().kind, TokenKind::Eof);
/// ```
///
/// # Errors
/// [`FormulaError::Tokenize`] for a character that cannot begin a token, a
/// malformed `#…` error literal, an unterminated string literal, or a
/// malformed `$`-anchored reference.
pub fn tokenize(input: &str) -> Result<Vec<Token<'_>>, FormulaError> {
    let b = input.as_bytes();
    let mut toks = Vec::new();
    let mut i = 0usize;
    while let Some(&c) = b.get(i) {
        // Whitespace separates tokens.
        if matches!(c, b' ' | b'\t' | b'\r' | b'\n') {
            i += 1;
            continue;
        }
        let pos = i;
        let kind = match c {
            b'0'..=b'9' => scan_number(input, &mut i)?,
            b'.' if b.get(i + 1).is_some_and(u8::is_ascii_digit) => scan_number(input, &mut i)?,
            b'"' => scan_string(input, &mut i)?,
            b'$' => scan_dollar_ref(input, &mut i)?,
            b'#' => scan_error_literal(input, &mut i)?,
            b'A'..=b'Z' | b'a'..=b'z' | b'_' => scan_word(input, &mut i)?,
            b'(' => bump(&mut i, TokenKind::LParen),
            b')' => bump(&mut i, TokenKind::RParen),
            b',' => bump(&mut i, TokenKind::Comma),
            b':' => bump(&mut i, TokenKind::Colon),
            b'+' => bump(&mut i, TokenKind::Plus),
            b'-' => bump(&mut i, TokenKind::Minus),
            b'*' => bump(&mut i, TokenKind::Star),
            b'/' => bump(&mut i, TokenKind::Slash),
            b'^' => bump(&mut i, TokenKind::Caret),
            b'&' => bump(&mut i, TokenKind::Amp),
            b'%' => bump(&mut i, TokenKind::Percent),
            b'=' => bump(&mut i, TokenKind::Eq),
            b'<' => {
                if b.get(i + 1) == Some(&b'>') {
                    i += 2;
                    TokenKind::Ne
                } else if b.get(i + 1) == Some(&b'=') {
                    i += 2;
                    TokenKind::Le
                } else {
                    i += 1;
                    TokenKind::Lt
                }
            }
            b'>' => {
                if b.get(i + 1) == Some(&b'=') {
                    i += 2;
                    TokenKind::Ge
                } else {
                    i += 1;
                    TokenKind::Gt
                }
            }
            _ => {
                let ch = input[pos..].chars().next().unwrap_or('#');
                return Err(FormulaError::Tokenize { pos, char: ch });
            }
        };
        toks.push(Token { kind, pos });
    }
    toks.push(Token {
        kind: TokenKind::Eof,
        pos: input.len(),
    });
    Ok(toks)
}

fn bump<'a>(i: &mut usize, kind: TokenKind<'a>) -> TokenKind<'a> {
    *i += 1;
    kind
}

/// Scans `[0-9]+ ('.' [0-9]*)? | '.' [0-9]+` plus an optional exponent.
/// Delegates the actual text→float conversion to `str::parse` (no
/// allocation); overflow lands on ±inf and is classified `#NUM!` at use.
fn scan_number<'input>(
    input: &'input str,
    i: &mut usize,
) -> Result<TokenKind<'input>, FormulaError> {
    let b = input.as_bytes();
    let start = *i;
    while b.get(*i).is_some_and(u8::is_ascii_digit) {
        *i += 1;
    }
    if b.get(*i) == Some(&b'.') {
        *i += 1;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
    }
    // Exponent: only when well-formed (`e`/`E`, optional sign, ≥1 digit);
    // otherwise the `e` belongs to a following identifier — the parser will
    // reject the juxtaposition, as Excel does.
    if matches!(b.get(*i), Some(b'e') | Some(b'E')) {
        let mut j = *i + 1;
        if matches!(b.get(j), Some(b'+') | Some(b'-')) {
            j += 1;
        }
        if b.get(j).is_some_and(u8::is_ascii_digit) {
            *i = j;
            while b.get(*i).is_some_and(u8::is_ascii_digit) {
                *i += 1;
            }
        }
    }
    let text = &input[start..*i];
    let n: f64 = text.parse().map_err(|_| FormulaError::Tokenize {
        pos: start,
        char: '#',
    })?;
    Ok(TokenKind::Number(n))
}

/// Scans `"…"` with `""` escapes. The token is the raw inner span; the
/// parser unescapes. Unterminated → `Tokenize` error at the opening quote.
fn scan_string<'input>(
    input: &'input str,
    i: &mut usize,
) -> Result<TokenKind<'input>, FormulaError> {
    let b = input.as_bytes();
    let open = *i;
    *i += 1; // opening quote
    loop {
        match b.get(*i) {
            Some(b'"') => {
                if b.get(*i + 1) == Some(&b'"') {
                    *i += 2; // escaped quote — keep scanning
                } else {
                    let inner = &input[open + 1..*i];
                    *i += 1; // closing quote
                    return Ok(TokenKind::Str(inner));
                }
            }
            Some(_) => {
                // Step over one full UTF-8 character.
                let ch = input[*i..].chars().next().unwrap_or('"');
                *i += ch.len_utf8();
            }
            None => {
                return Err(FormulaError::Tokenize {
                    pos: open,
                    char: '"',
                })
            }
        }
    }
}

/// Scans a `$`-anchored reference (`$A$1`, `$AB12`) after the leading `$`.
/// Eagerly parsed to column/row numbers — the anchors make this
/// unambiguously a reference (a bare `A1` cannot be classified until the
/// parser sees what follows).
fn scan_dollar_ref<'input>(
    input: &'input str,
    i: &mut usize,
) -> Result<TokenKind<'input>, FormulaError> {
    let b = input.as_bytes();
    let dollar = *i;
    *i += 1;
    let col_start = *i;
    while b.get(*i).is_some_and(|c| c.is_ascii_alphabetic()) {
        *i += 1;
    }
    let col_len = *i - col_start;
    if col_len == 0 || col_len > 3 {
        return Err(FormulaError::Tokenize {
            pos: dollar,
            char: '$',
        });
    }
    let mut col: u32 = 0;
    for c in input[col_start..*i].bytes() {
        col = col * 26 + u32::from(c.to_ascii_uppercase() - b'A' + 1);
    }
    let row_abs = b.get(*i) == Some(&b'$');
    if row_abs {
        *i += 1;
    }
    let row_start = *i;
    while b.get(*i).is_some_and(u8::is_ascii_digit) {
        *i += 1;
    }
    let row_text = &input[row_start..*i];
    // At most 7 digits — anything longer cannot be a legal row anyway and
    // would risk u32 overflow.
    if row_text.is_empty() || row_text.len() > 7 {
        return Err(FormulaError::Tokenize {
            pos: dollar,
            char: '$',
        });
    }
    let row: u32 = row_text.parse().map_err(|_| FormulaError::Tokenize {
        pos: dollar,
        char: '$',
    })?;
    Ok(TokenKind::Cell {
        col,
        row,
        col_abs: true,
        row_abs,
    })
}

/// Scans a `#…` error literal, case-insensitively. The seven literals have
/// distinct lengths and no literal is a prefix of another at the same
/// length, so trying every length 4..=8 is exact.
fn scan_error_literal<'input>(
    input: &'input str,
    i: &mut usize,
) -> Result<TokenKind<'input>, FormulaError> {
    let pos = *i;
    let rest = &input[pos..];
    let mut matched = None;
    for len in (4..=8).rev() {
        if let Some(head) = rest.get(..len) {
            if let Some(e) = ExcelError::from_literal(head) {
                matched = Some(e);
                break;
            }
        }
    }
    match matched {
        Some(e) => {
            *i += e.literal().len();
            Ok(TokenKind::ErrLit(e))
        }
        None => Err(FormulaError::Tokenize { pos, char: '#' }),
    }
}

/// Scans a bare word: `[A-Za-z_][A-Za-z0-9_]*` with `.` allowed between
/// word characters (Excel function names like `ISO.CEILING`). If the word
/// is pure letters immediately followed by `$`, it is re-read as a
/// row-anchored reference (`A$1`); the `$` characters make that shape
/// unambiguous at lex time.
fn scan_word<'input>(input: &'input str, i: &mut usize) -> Result<TokenKind<'input>, FormulaError> {
    let b = input.as_bytes();
    let start = *i;
    *i += 1; // first char is known alpha/underscore
    while let Some(&c) = b.get(*i) {
        if c.is_ascii_alphanumeric() || c == b'_' {
            *i += 1;
        } else if c == b'.'
            && b.get(*i + 1)
                .is_some_and(|n| n.is_ascii_alphabetic() || *n == b'_')
        {
            *i += 1; // consume '.', loop consumes the next word char
        } else {
            break;
        }
    }
    let word = &input[start..*i];
    // `A$1`-style: all letters, then `$`, then digits.
    if b.get(*i) == Some(&b'$') {
        let all_letters = word.bytes().all(|c| c.is_ascii_alphabetic());
        let len_ok = (1..=3).contains(&word.len());
        if all_letters && len_ok {
            let mut col: u32 = 0;
            for c in word.bytes() {
                col = col * 26 + u32::from(c.to_ascii_uppercase() - b'A' + 1);
            }
            *i += 1; // '$'
            let row_start = *i;
            while b.get(*i).is_some_and(u8::is_ascii_digit) {
                *i += 1;
            }
            let row_text = &input[row_start..*i];
            if !row_text.is_empty() && row_text.len() <= 7 {
                let row: u32 = row_text.parse().map_err(|_| FormulaError::Tokenize {
                    pos: start,
                    char: '$',
                })?;
                return Ok(TokenKind::Cell {
                    col,
                    row,
                    col_abs: false,
                    row_abs: true,
                });
            }
            // Malformed tail (`A$` with no digits): rewind — the parser
            // will reject `A` juxtaposed with `$…` with a proper message.
            *i = start + word.len();
        }
    }
    Ok(TokenKind::Ident(word))
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
    use alloc::vec;

    #[test]
    fn positions_survive_whitespace_and_multibyte() {
        let toks = tokenize("a1 + \"🎉\"").unwrap();
        assert_eq!(toks[0].pos, 0);
        assert_eq!(toks[1].pos, 3); // '+' (a1 = two bytes, space)
        assert_eq!(toks[2].pos, 5); // string open quote
                                    // eof = 5 + 1 quote + 4-byte emoji + 1 quote = 11
        assert_eq!(toks[3].pos, 11);
    }

    #[test]
    fn exponent_boundaries() {
        // Well-formed exponents attach; malformed ones split.
        assert!(matches!(tokenize("1e5").unwrap()[0].kind, TokenKind::Number(n) if n == 1e5));
        assert!(matches!(tokenize("1E-2").unwrap()[0].kind, TokenKind::Number(n) if n == 0.01));
        assert!(matches!(tokenize("1e+5").unwrap()[0].kind, TokenKind::Number(n) if n == 1e5));
        let bad = tokenize("1e").unwrap();
        assert!(matches!(bad[0].kind, TokenKind::Number(n) if n == 1.0));
        assert!(matches!(bad[1].kind, TokenKind::Ident("e")));
    }

    #[test]
    fn adjacent_refs_do_not_merge() {
        let toks = tokenize("A1B2").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::Ident("A1B2"))); // one word → name → parse error later
        let toks = tokenize("A1+B2").unwrap();
        assert_eq!(toks.len(), 4); // A1 + B2 Eof... plus operator = 4 total
    }

    #[test]
    fn dollar_ref_row_cap_seven_digits() {
        // 8+ digit rows are a tokenize error (cannot be a legal row).
        assert!(tokenize("$A$12345678").is_err());
        // 7 digits lex fine; the parser range-checks.
        assert!(tokenize("$A$9999999").is_ok());
    }

    #[test]
    fn underscore_idents() {
        assert!(matches!(
            tokenize("_x").unwrap()[0].kind,
            TokenKind::Ident("_x")
        ));
        assert!(matches!(
            tokenize("a_1").unwrap()[0].kind,
            TokenKind::Ident("a_1")
        ));
    }

    #[test]
    fn error_literal_boundaries() {
        // A literal followed by more text splits cleanly.
        let toks = tokenize("#N/A+1").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::ErrLit(ExcelError::NA)));
        assert!(matches!(toks[1].kind, TokenKind::Plus));
        // "#N/A!" — '!' is not part of #N/A and not a token: the lexer
        // yields the literal, then a typed error at the '!'.
        let toks = tokenize("#N/A").unwrap();
        assert!(matches!(toks[0].kind, TokenKind::ErrLit(ExcelError::NA)));
        assert!(matches!(
            tokenize("#N/A!"),
            Err(FormulaError::Tokenize { pos: 4, char: '!' })
        ));
    }

    #[test]
    fn crlf_tabs_newlines_skip() {
        assert!(tokenize("1\r\n+\t2").is_ok());
        assert!(tokenize("\n\nSUM ( 1 ) \r\n").is_ok());
    }

    #[test]
    fn comparisons_tokenize_greedily() {
        assert!(matches!(tokenize("<=").unwrap()[0].kind, TokenKind::Le));
        assert!(matches!(tokenize("<>").unwrap()[0].kind, TokenKind::Ne));
        assert!(matches!(tokenize(">=").unwrap()[0].kind, TokenKind::Ge));
        assert!(matches!(tokenize("=<").unwrap()[0].kind, TokenKind::Eq));
    }

    #[test]
    fn string_emoji_and_nul() {
        let toks = tokenize("\"a\u{0}b\"").unwrap();
        assert!(matches!(&toks[0].kind, TokenKind::Str(s) if *s == "a\u{0}b"));
    }

    #[test]
    fn word_dot_rules() {
        assert!(matches!(
            tokenize("A.B.C").unwrap()[0].kind,
            TokenKind::Ident("A.B.C")
        ));
        assert!(matches!(
            tokenize("A.B1").unwrap()[0].kind,
            TokenKind::Ident("A.B1")
        ));
        assert!(tokenize("A..B").is_err()); // dot-dot is not a token run
    }

    #[test]
    fn vec_macro_use() {
        let _ = vec![1];
    }
}
