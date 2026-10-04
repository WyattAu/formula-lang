//! Tokenizer — zero-copy spans, every literal class, every error path.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

use formula_lang::{tokenize, ExcelError, FormulaError, TokenKind};

fn kinds(input: &str) -> Vec<TokenKind<'_>> {
    tokenize(input)
        .unwrap()
        .into_iter()
        .map(|t| t.kind)
        .collect()
}

#[test]
fn numbers() {
    assert!(matches!(kinds("42")[0], TokenKind::Number(n) if n == 42.0));
    assert!(matches!(kinds("3.14")[0], TokenKind::Number(n) if n == 3.14));
    assert!(matches!(kinds(".5")[0], TokenKind::Number(n) if n == 0.5));
    assert!(matches!(kinds("1e3")[0], TokenKind::Number(n) if n == 1000.0));
    assert!(matches!(kinds("1.5E-3")[0], TokenKind::Number(n) if n == 0.0015));
    assert!(matches!(kinds("2e+10")[0], TokenKind::Number(n) if n == 2e10));
    // Overflow lexes to inf; evaluation maps it to #NUM!.
    assert!(matches!(kinds("1e999")[0], TokenKind::Number(n) if n.is_infinite()));
}

#[test]
fn number_token_boundaries() {
    // `1ex` — the `e` cannot start a valid exponent: it becomes an
    // identifier, and the parser will reject the juxtaposition.
    let k = kinds("1ex");
    assert!(matches!(k[0], TokenKind::Number(n) if n == 1.0));
    assert!(matches!(k[1], TokenKind::Ident("ex")));
    // `1-2` splits cleanly (Number, Minus, Number, Eof).
    assert_eq!(kinds("1-2").len(), 4);
}

#[test]
fn strings_are_zero_copy_spans() {
    let input = String::from("LEFT(\"abc\", 1)");
    let toks = tokenize(&input).unwrap();
    let TokenKind::Str(s) = toks[2].kind else {
        panic!("expected str token, got {:?}", toks[2].kind)
    };
    assert_eq!(s, "abc");
    // The span points into the original buffer.
    assert!(core::ptr::eq(s.as_ptr(), input[6..9].as_ptr()));
}

#[test]
fn string_escapes_stay_raw_in_tokens() {
    // Token holds the raw inner span; the parser unescapes.
    let k = kinds("\"a\"\"b\"");
    assert!(matches!(k[0], TokenKind::Str(s) if s == "a\"\"b"));
}

#[test]
fn unterminated_string_is_typed() {
    assert_eq!(
        tokenize("\"abc"),
        Err(FormulaError::Tokenize { pos: 0, char: '"' })
    );
}

#[test]
fn identifiers_and_function_names() {
    assert!(matches!(kinds("SUM")[0], TokenKind::Ident("SUM")));
    assert!(matches!(kinds("abc_123")[0], TokenKind::Ident("abc_123")));
    // Dotted names (ISO.CEILING style).
    assert!(matches!(
        kinds("ISO.CEILING")[0],
        TokenKind::Ident("ISO.CEILING")
    ));
    // A lone trailing dot is not a token at all (typed error), and a dot
    // followed by a letter merges into a dotted name.
    assert!(matches!(
        tokenize("A1."),
        Err(FormulaError::Tokenize { pos: 2, char: '.' })
    ));
    assert!(matches!(kinds("A1.B")[0], TokenKind::Ident("A1.B")));
}

#[test]
fn dollar_anchored_refs() {
    assert!(matches!(
        kinds("$A$1")[0],
        TokenKind::Cell {
            col: 1,
            row: 1,
            col_abs: true,
            row_abs: true
        }
    ));
    assert!(matches!(
        kinds("$B2")[0],
        TokenKind::Cell {
            col: 2,
            row: 2,
            col_abs: true,
            row_abs: false
        }
    ));
    assert!(matches!(
        kinds("$ab12")[0],
        TokenKind::Cell {
            col: 28,
            row: 12,
            col_abs: true,
            row_abs: false
        }
    ));
}

#[test]
fn row_anchored_refs() {
    // `A$1` — the `$` comes after pure letters.
    assert!(matches!(
        kinds("A$1")[0],
        TokenKind::Cell {
            col: 1,
            row: 1,
            col_abs: false,
            row_abs: true
        }
    ));
    assert!(matches!(
        kinds("xfd$1048576")[0],
        TokenKind::Cell {
            col: 16384,
            row: 1048576,
            col_abs: false,
            row_abs: true
        }
    ));
}

#[test]
fn malformed_dollar_refs_are_typed() {
    assert!(matches!(
        tokenize("$1"),
        Err(FormulaError::Tokenize { pos: 0, char: '$' })
    ));
    assert!(matches!(
        tokenize("$ABCD1"),
        Err(FormulaError::Tokenize { pos: 0, char: '$' })
    ));
    assert!(matches!(
        tokenize("$A$"),
        Err(FormulaError::Tokenize { pos: 0, char: '$' })
    ));
    // A word with digits followed by `$` is not a ref — it splits.
    let k = kinds("A1$B2");
    assert!(matches!(k[0], TokenKind::Ident("A1")));
    assert!(matches!(
        k[1],
        TokenKind::Cell {
            col: 2,
            row: 2,
            col_abs: true,
            row_abs: false
        }
    ));
}

#[test]
fn error_literals_all_seven() {
    assert!(matches!(
        kinds("#DIV/0!")[0],
        TokenKind::ErrLit(ExcelError::DivZero)
    ));
    assert!(matches!(
        kinds("#n/a")[0],
        TokenKind::ErrLit(ExcelError::NA)
    ));
    assert!(matches!(
        kinds("#Name?")[0],
        TokenKind::ErrLit(ExcelError::Name)
    ));
    assert!(matches!(
        kinds("#null!")[0],
        TokenKind::ErrLit(ExcelError::Null)
    ));
    assert!(matches!(
        kinds("#NUM!")[0],
        TokenKind::ErrLit(ExcelError::Num)
    ));
    assert!(matches!(
        kinds("#Ref!")[0],
        TokenKind::ErrLit(ExcelError::Ref)
    ));
    assert!(matches!(
        kinds("#value!")[0],
        TokenKind::ErrLit(ExcelError::Value)
    ));
}

#[test]
fn malformed_error_literal_is_typed() {
    assert!(matches!(
        tokenize("#nope"),
        Err(FormulaError::Tokenize { pos: 0, char: '#' })
    ));
    assert!(matches!(
        tokenize("#NA!"),
        Err(FormulaError::Tokenize { pos: 0, char: '#' })
    ));
}

#[test]
fn operators() {
    let k = kinds("= <> < > <= >= + - * / ^ & % ( ) , :");
    assert_eq!(
        k,
        vec![
            TokenKind::Eq,
            TokenKind::Ne,
            TokenKind::Lt,
            TokenKind::Gt,
            TokenKind::Le,
            TokenKind::Ge,
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Caret,
            TokenKind::Amp,
            TokenKind::Percent,
            TokenKind::LParen,
            TokenKind::RParen,
            TokenKind::Comma,
            TokenKind::Colon,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn eof_always_last_at_input_len() {
    let toks = tokenize("A1").unwrap();
    assert_eq!(toks.last().unwrap().kind, TokenKind::Eof);
    assert_eq!(toks.last().unwrap().pos, 2);
}

#[test]
fn whitespace_is_skipped_and_positions_accurate() {
    let toks = tokenize(" SUM( 1 ) ").unwrap();
    assert_eq!(toks[0].pos, 1);
    assert_eq!(toks[2].pos, 6);
    assert_eq!(toks[3].pos, 8);
}

#[test]
fn unicode_inside_strings() {
    let k = kinds("\"héllo 🎉\"");
    assert!(matches!(k[0], TokenKind::Str(s) if s == "héllo 🎉"));
}

#[test]
fn unknown_character_is_typed_with_the_char() {
    assert_eq!(
        tokenize("1 @ 2"),
        Err(FormulaError::Tokenize { pos: 2, char: '@' })
    );
    // Multi-byte unknown characters report the char, not a byte.
    assert_eq!(
        tokenize("1 € 2"),
        Err(FormulaError::Tokenize {
            pos: 2, char: '€'
        })
    );
}

#[test]
fn empty_input_is_just_eof() {
    let toks = tokenize("").unwrap();
    assert_eq!(toks.len(), 1);
    assert_eq!(toks[0].kind, TokenKind::Eof);
}

#[test]
fn total_on_arbitrary_ascii_noise() {
    // No panics across a sweep of hostile fragments.
    for s in [
        "\"", "\"\"", "$", "#", "#!", ":", "::", "1.2.3", "1e", "1e+", "0x10", "$$$$", "$A$",
        "A$$$$1", "%%", "1%%", "<=", "=>", "><", "#N/A#", "&amp;",
    ] {
        let _ = tokenize(s);
    }
}
