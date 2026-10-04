//! The Pratt parser — tokens in, [`Expr`] out.
//!
//! Precedence climbs through the classic Excel ladder (loosest first):
//! comparison → `&` concat → `+ -` → `* /` → `^` power → prefix `- +` →
//! postfix `%` → primaries. `^` is right-associative; everything else is
//! left-associative; and because prefix unary sits *above* power in the
//! binding ladder, `-2^2` parses as `(-2)^2` — the Excel quirk, faithfully
//! reproduced.
//!
//! The parser is stack-safe: recursion depth (nested parens, unary chains,
//! right-nested `^`, nested calls) is capped at [`MAX_DEPTH`] with a typed
//! [`FormulaError::RecursionLimit`], fuzz-verified. Left-associative chains
//! (`1+1+1+…`) iterate and grow the AST, not the stack; their depth is
//! caught at evaluation by the same cap.

use crate::ast::{col_number, BinaryOp, CellRef, Expr, UnaryOp, MAX_COL, MAX_ROW};
use crate::error::FormulaError;
use crate::lexer::tokenize;
use crate::token::{Token, TokenKind};
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

/// Maximum parser/evaluator recursion depth. Excel itself caps nesting at
/// 64; 512 gives ample headroom while every recursion frame stays far below
/// any thread stack.
pub const MAX_DEPTH: usize = 512;

/// Parses formula text into an [`Expr`].
///
/// A single leading `=` (Excel's entry form) is accepted and skipped; both
/// `parse("1+1")` and `parse("=1+1")` work.
///
/// ```
/// use formula_lang::{parse, BinaryOp, Expr};
///
/// let e = parse("1+2*3").unwrap();
/// assert_eq!(e.to_string(), "1+2*3");
/// assert!(matches!(
///     e,
///     Expr::Binary { op: BinaryOp::Add, .. }
/// ));
/// ```
///
/// # Errors
/// [`FormulaError::Tokenize`] (from the lexer),
/// [`FormulaError::Parse`] for grammar violations, and
/// [`FormulaError::RecursionLimit`] when nesting exceeds [`MAX_DEPTH`].
pub fn parse(input: &str) -> Result<Expr, FormulaError> {
    let toks = tokenize(input)?;
    let mut p = Parser {
        toks: &toks,
        pos: 0,
        depth: 0,
    };
    // Optional leading `=` — Excel's formula entry form.
    if p.pos == 0 && matches!(p.peek().kind, TokenKind::Eq) {
        p.pos += 1;
    }
    let e = p.parse_expr(0)?;
    if p.peek().kind != TokenKind::Eof {
        return Err(p.err_here("end of formula"));
    }
    Ok(e)
}

struct Parser<'t, 'i> {
    toks: &'t [Token<'i>],
    pos: usize,
    depth: usize,
}

impl<'i> Parser<'_, 'i> {
    fn peek(&self) -> Token<'i> {
        // `tokenize` always appends `Eof`, so the slice is never empty and
        // the index stays in bounds (self.pos only advances onto Eof).
        *self
            .toks
            .get(self.pos)
            .unwrap_or(self.toks.last().unwrap_or(&Token {
                kind: TokenKind::Eof,
                pos: 0,
            }))
    }

    fn err_here(&self, expected: &'static str) -> FormulaError {
        let tok = self.peek();
        FormulaError::Parse {
            pos: tok.pos,
            expected,
            got: tok.kind.describe(),
        }
    }

    fn enter(&mut self) -> Result<(), FormulaError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(FormulaError::RecursionLimit);
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    /// `expr := comparison` — the Pratt loop over the infix ladder.
    fn parse_expr(&mut self, min_bp: usize) -> Result<Expr, FormulaError> {
        self.enter()?;
        let mut lhs = self.parse_unary()?;
        while let Some((lbp, rbp, op)) = self.peek_infix() {
            if lbp < min_bp {
                break;
            }
            self.pos += 1; // consume the operator token
            let rhs = self.parse_expr(rbp)?;
            lhs = Expr::Binary {
                op,
                left: Box::new(lhs),
                right: Box::new(rhs),
            };
        }
        self.leave();
        Ok(lhs)
    }

    fn peek_infix(&self) -> Option<(usize, usize, BinaryOp)> {
        let op = match self.peek().kind {
            TokenKind::Plus => BinaryOp::Add,
            TokenKind::Minus => BinaryOp::Sub,
            TokenKind::Star => BinaryOp::Mul,
            TokenKind::Slash => BinaryOp::Div,
            TokenKind::Caret => BinaryOp::Pow,
            TokenKind::Amp => BinaryOp::Concat,
            TokenKind::Eq => BinaryOp::Eq,
            TokenKind::Ne => BinaryOp::Ne,
            TokenKind::Lt => BinaryOp::Lt,
            TokenKind::Gt => BinaryOp::Gt,
            TokenKind::Le => BinaryOp::Le,
            TokenKind::Ge => BinaryOp::Ge,
            _ => return None,
        };
        let (lbp, rbp) = op.binding_power();
        Some((lbp, rbp, op))
    }

    /// `unary := ('-' | '+') unary | postfix` — prefix level, binding
    /// tighter than `^`.
    fn parse_unary(&mut self) -> Result<Expr, FormulaError> {
        self.enter()?;
        let e = match self.peek().kind {
            TokenKind::Minus => {
                self.pos += 1;
                Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(self.parse_unary()?),
                }
            }
            TokenKind::Plus => {
                self.pos += 1;
                Expr::Unary {
                    op: UnaryOp::Pos,
                    expr: Box::new(self.parse_unary()?),
                }
            }
            _ => self.parse_postfix()?,
        };
        self.leave();
        Ok(e)
    }

    /// `postfix := primary ('%')*`
    fn parse_postfix(&mut self) -> Result<Expr, FormulaError> {
        let mut e = self.parse_primary()?;
        while self.peek().kind == TokenKind::Percent {
            self.pos += 1;
            e = Expr::Unary {
                op: UnaryOp::Percent,
                expr: Box::new(e),
            };
        }
        Ok(e)
    }

    /// `primary := NUMBER | STRING | BOOLEAN | ERROR | cell_ref | range |
    ///              function_call | '(' expr ')'`
    fn parse_primary(&mut self) -> Result<Expr, FormulaError> {
        let tok = self.peek();
        match tok.kind {
            TokenKind::Number(n) => {
                self.pos += 1;
                Ok(Expr::Number(n))
            }
            TokenKind::Str(raw) => {
                self.pos += 1;
                Ok(Expr::Text(unescape(raw)))
            }
            TokenKind::ErrLit(e) => {
                self.pos += 1;
                Ok(Expr::Error(e))
            }
            TokenKind::Cell { .. } => {
                self.pos += 1;
                let r = self.finish_celllike(tok, None)?;
                self.maybe_range(r)
            }
            TokenKind::Ident(word) => {
                self.pos += 1;
                // Function call: `IDENT '(' args ')'` — checked first, so
                // ref-shaped function names (`LOG10`) win over references.
                if self.peek().kind == TokenKind::LParen {
                    self.pos += 1;
                    let args = self.parse_args()?;
                    return Ok(Expr::Function {
                        name: word.to_ascii_uppercase(),
                        args,
                    });
                }
                if word.eq_ignore_ascii_case("TRUE") {
                    return Ok(Expr::Boolean(true));
                }
                if word.eq_ignore_ascii_case("FALSE") {
                    return Ok(Expr::Boolean(false));
                }
                // Ref-shaped bare word (`A1`, `ab12`, `XFD1048576`)?
                let cr = match self.bare_word_ref(word) {
                    Ok(cr) => cr,
                    Err(expected) => return Err(self.err_here(expected)),
                };
                let r = self.finish_celllike(tok, Some(cr))?;
                self.maybe_range(r)
            }
            TokenKind::LParen => {
                self.pos += 1;
                self.enter()?;
                let e = self.parse_expr(0)?;
                if self.peek().kind != TokenKind::RParen {
                    return Err(self.err_here("')'"));
                }
                self.pos += 1;
                self.leave();
                Ok(e)
            }
            _ => Err(self.err_here("expression")),
        }
    }

    /// Builds a `CellRef` from a token, applying grid limits. `word` is the
    /// already-classified 1-based (col, row) for `Ident` tokens; `Cell`
    /// tokens carry it in the token itself.
    fn finish_celllike(
        &mut self,
        tok: Token<'_>,
        word: Option<(u32, u32)>,
    ) -> Result<CellRef, FormulaError> {
        let (col, row, col_abs, row_abs) = match tok.kind {
            TokenKind::Cell {
                col,
                row,
                col_abs,
                row_abs,
            } => (col, row, col_abs, row_abs),
            TokenKind::Ident(_) => {
                let (col, row) = word.unwrap_or((0, 0));
                (col, row, false, false)
            }
            _ => (0, 0, false, false),
        };
        if col == 0 || col > MAX_COL || row == 0 || row > MAX_ROW {
            return Err(FormulaError::Parse {
                pos: tok.pos,
                expected: "cell reference within A1:XFD1048576",
                got: tok.kind.describe(),
            });
        }
        Ok(CellRef {
            col,
            row,
            col_abs,
            row_abs,
        })
    }

    /// Classifies a bare word as an unanchored (col, row). `Err(expected)`
    /// distinguishes "not ref-shaped at all" (names are not in the grammar)
    /// from "ref-shaped but outside the grid" (both are parse errors, but
    /// the messages differ).
    fn bare_word_ref(&self, word: &str) -> Result<(u32, u32), &'static str> {
        let b = word.as_bytes();
        let letters = b.iter().take_while(|c| c.is_ascii_alphabetic()).count();
        let digits = b
            .get(letters..)
            .map_or(0, |rest| rest.iter().filter(|c| c.is_ascii_digit()).count());
        let ref_shaped =
            letters > 0 && letters <= 3 && digits > 0 && letters + digits == b.len() && digits <= 7;
        if !ref_shaped {
            return Err("cell reference, TRUE/FALSE, or function call");
        }
        let col = col_number(&word[..letters]).ok_or("cell reference within A1:XFD1048576")?;
        let row: u32 = word[letters..]
            .parse()
            .map_err(|_| "cell reference within A1:XFD1048576")?;
        if row > MAX_ROW {
            return Err("cell reference within A1:XFD1048576");
        }
        Ok((col, row))
    }

    /// After a cell reference, consume `':' ref` if present — the range
    /// production. The second corner must be a plain reference.
    fn maybe_range(&mut self, start: CellRef) -> Result<Expr, FormulaError> {
        if self.peek().kind != TokenKind::Colon {
            return Ok(Expr::CellRef {
                col: start.col,
                row: start.row,
                col_abs: start.col_abs,
                row_abs: start.row_abs,
            });
        }
        self.pos += 1;
        let tok = self.peek();
        let end = match tok.kind {
            TokenKind::Cell { .. } => {
                self.pos += 1;
                self.finish_celllike(tok, None)?
            }
            TokenKind::Ident(word) => {
                self.pos += 1;
                let (c, r) = self.bare_word_ref(word).map_err(|_| FormulaError::Parse {
                    pos: tok.pos,
                    expected: "cell reference after ':'",
                    got: tok.kind.describe(),
                })?;
                self.finish_celllike(tok, Some((c, r)))?
            }
            _ => return Err(self.err_here("cell reference after ':'")),
        };
        if self.peek().kind == TokenKind::Colon {
            return Err(self.err_here("end of range (single ':' only)"));
        }
        Ok(Expr::Range { start, end })
    }

    /// `args := [arg (',' arg)*]` with empty slots (`F(,2)`, `F(1,)`)
    /// represented by the omitted-argument marker
    /// ([`is_omitted_arg`]). `F()` parses to zero arguments.
    fn parse_args(&mut self) -> Result<Vec<Expr>, FormulaError> {
        self.enter()?;
        let mut args = Vec::new();
        if self.peek().kind == TokenKind::RParen {
            self.pos += 1;
            self.leave();
            return Ok(args);
        }
        loop {
            // One argument slot: empty (marker) or a full expression.
            if matches!(self.peek().kind, TokenKind::Comma | TokenKind::RParen) {
                args.push(Expr::Error(crate::error::ExcelError::Null));
            } else {
                args.push(self.parse_expr(0)?);
            }
            match self.peek().kind {
                TokenKind::Comma => self.pos += 1, // another slot follows
                TokenKind::RParen => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.err_here("',' or ')'")),
            }
        }
        self.leave();
        Ok(args)
    }
}

/// Unescapes a string literal's inner span: `""` → `"`. Allocation is one
/// `String` per literal — the parser path, not the eval path.
fn unescape(raw: &str) -> String {
    if !raw.contains('"') {
        return String::from(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '"' {
            // Doubled quote: consume the partner.
            if chars.next() == Some('"') {
                out.push('"');
            }
        } else {
            out.push(c);
        }
    }
    out
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
    use crate::token::Token;
    use alloc::format;
    use alloc::vec;
    use alloc::vec::Vec;

    fn toks(input: &str) -> Vec<Token<'_>> {
        tokenize(input).unwrap()
    }

    #[test]
    fn leading_eq_positions() {
        // `=` consumed only as the very first token.
        assert_eq!(parse("=1+1"), parse("1+1"));
        // Second `=` is the comparison operator.
        assert!(parse("1=1").is_ok());
        assert!(parse("==1").is_err()); // `=` `=1` → missing lhs
    }

    #[test]
    fn eof_sentinel_keeps_peek_in_bounds() {
        let t = toks("1");
        assert!(matches!(t.last().unwrap().kind, TokenKind::Eof));
    }

    #[test]
    fn unclosed_function_call() {
        assert!(matches!(
            parse("SUM(1,2"),
            Err(FormulaError::Parse { expected: "',' or ')'", got, .. }) if got == "end of formula"
        ));
    }

    #[test]
    fn empty_vs_omitted_args() {
        // F() → zero args.
        let e = parse("F()").unwrap();
        let Expr::Function { args, .. } = &e else {
            panic!()
        };
        assert_eq!(args.len(), 0);
        // F(1) → one.
        let e = parse("F(1)").unwrap();
        let Expr::Function { args, .. } = &e else {
            panic!()
        };
        assert_eq!(args.len(), 1);
        // F(,) → two omitted.
        let e = parse("F(,)").unwrap();
        let Expr::Function { args, .. } = &e else {
            panic!()
        };
        assert_eq!(args.len(), 2);
    }

    #[test]
    fn function_names_normalize_and_reject_bad_words() {
        assert_eq!(
            parse("min(1)").unwrap(),
            Expr::Function {
                name: "MIN".into(),
                args: vec![Expr::Number(1.0)]
            }
        );
        // `_xlfn.SUM(1)` — dotted names parse as calls.
        assert!(matches!(
            parse("_xlfn.SUM(1)").unwrap(),
            Expr::Function { ref name, .. } if name == "_XLFN.SUM"
        ));
    }

    #[test]
    fn mixed_case_refs_and_ranges() {
        assert_eq!(parse("Ab12").unwrap(), parse("aB12").unwrap());
        assert_eq!(parse("ab12:CD34").unwrap(), parse("AB12:cd34").unwrap());
    }

    #[test]
    fn unary_percent_chains() {
        // Deep unary chains cap at MAX_DEPTH.
        let deep = format!("{}1", "+".repeat(MAX_DEPTH + 5));
        assert_eq!(parse(&deep), Err(FormulaError::RecursionLimit));
        // Just under parses.
        let near = format!("{}1", "+".repeat(MAX_DEPTH - 2));
        assert!(parse(&near).is_ok());
    }

    #[test]
    fn unescape_doubled_quotes() {
        assert_eq!(unescape("plain"), "plain");
        assert_eq!(unescape("a\"\"b"), "a\"b");
        assert_eq!(unescape("\"\"\"\""), "\"\"");
        assert_eq!(unescape(""), "");
    }

    #[test]
    fn trailing_percent_after_ref() {
        assert!(parse("A1%").is_ok());
        assert!(parse("SUM(1)%").is_ok());
        assert!(parse("A1:B2%").is_ok());
    }

    #[test]
    fn parens_do_not_make_calls() {
        // `(A1)(B1)` — juxtaposition → parse error at second `(`.
        assert!(matches!(
            parse("(A1)(B1)"),
            Err(FormulaError::Parse {
                expected: "end of formula",
                ..
            })
        ));
    }
}
