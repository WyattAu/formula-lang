//! Known-answer tests for the 14 text functions — UTF-16 semantics,
//! wildcards, TRIM's space-only rule, and the TEXT format engine.

// Test harness: assertions legitimately panic and index fixed positions;
// the lib target remains lint-clean. Float comparisons in known-answer
// tests are exact by construction.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing, clippy::float_cmp, clippy::approx_constant)]
#![allow(dead_code)]

mod common;

use common::*;
use formula_lang::{ExcelError, MapResolver, Value};

#[test]
fn concatenate() {
    assert_eq!(text("CONCATENATE(\"a\",\"b\",\"c\")"), "abc");
    assert_eq!(text("CONCATENATE(1,TRUE)"), "1TRUE");
    assert_eq!(text("CONCATENATE(\"a\")"), "a");
    assert_eq!(text("CONCATENATE(1,,2)"), "12"); // omitted slot = ""
    assert_err("CONCATENATE()", ExcelError::Value);
}

#[test]
fn left_right_mid() {
    assert_eq!(text("LEFT(\"hello\",2)"), "he");
    assert_eq!(text("LEFT(\"hello\")"), "h"); // default n=1
    assert_eq!(text("LEFT(\"hello\",0)"), "");
    assert_eq!(text("LEFT(\"hello\",99)"), "hello"); // clamp
    assert_err("LEFT(\"hello\",-1)", ExcelError::Value);

    assert_eq!(text("RIGHT(\"hello\",2)"), "lo");
    assert_eq!(text("RIGHT(\"hello\")"), "o");
    assert_eq!(text("RIGHT(\"hello\",99)"), "hello");
    assert_err("RIGHT(\"hello\",-1)", ExcelError::Value);

    assert_eq!(text("MID(\"hello\",2,3)"), "ell");
    assert_eq!(text("MID(\"hello\",1,99)"), "hello");
    assert_eq!(text("MID(\"hello\",4,99)"), "lo");
    assert_eq!(text("MID(\"hello\",99,1)"), "");
    assert_err("MID(\"hello\",0,1)", ExcelError::Value); // start < 1
    assert_err("MID(\"hello\",1,-1)", ExcelError::Value);
}

#[test]
fn utf16_indexing_like_excel() {
    // Excel counts UTF-16 code units: an astral emoji is 2.
    assert_eq!(num("LEN(\"abc\")"), 3.0);
    assert_eq!(num("LEN(\"\u{1F3B8}\")"), 2.0); // guitar = 2 units
    assert_eq!(num("LEN(\"\")"), 0.0);
    assert_eq!(num("LEN(42)"), 2.0); // numbers coerce to text first
    assert_eq!(text("LEFT(\"\u{1F3B8}ab\",3)"), "\u{1F3B8}a"); // 2 units + 1 char
                                                               // Excel string functions are UTF-16-unit based and NOT surrogate-aware:
                                                               // slicing through an astral char yields replacement chars, like Excel.
    assert_eq!(text("MID(\"\u{1F3B8}ab\",2,2)"), "\u{FFFD}a");
    assert_eq!(num("FIND(\"b\",\"\u{1F3B8}ab\")"), 4.0); // unit-based: emoji = 2 units
}

#[test]
fn case_functions() {
    assert_eq!(text("LOWER(\"MiXeD\")"), "mixed");
    assert_eq!(text("UPPER(\"MiXeD\")"), "MIXED");
    assert_eq!(text("PROPER(\"hello world\")"), "Hello World");
    assert_eq!(text("PROPER(\"o'neil 2nd\")"), "O'Neil 2Nd");
    assert_eq!(text("PROPER(\"foo-bar baz\")"), "Foo-Bar Baz");
    assert_eq!(text("PROPER(\"\")"), "");
    assert_eq!(num("LEN(UPPER(\"\u{00DF}\"))"), 2.0); // ß → SS
}

#[test]
fn trim_is_space_only() {
    assert_eq!(text("TRIM(\"  a  b  \")"), "a b");
    assert_eq!(text("TRIM(\"a    b\")"), "a b"); // internal collapse
    assert_eq!(text("TRIM(\" \")"), "");
    // Excel TRIM only ever touches U+0020 — other whitespace survives.
    assert_eq!(text("TRIM(\"\ta\t\")"), "\ta\t");
    assert_eq!(text("TRIM(\"a\")"), "a");
}

#[test]
fn substitute() {
    assert_eq!(text("SUBSTITUTE(\"aaa\",\"a\",\"b\")"), "bbb");
    assert_eq!(text("SUBSTITUTE(\"aaa\",\"a\",\"b\",2)"), "aba");
    assert_eq!(text("SUBSTITUTE(\"aaa\",\"a\",\"b\",3)"), "aab");
    assert_eq!(text("SUBSTITUTE(\"aaa\",\"a\",\"b\",4)"), "aaa"); // beyond → unchanged
    assert_err("SUBSTITUTE(\"aaa\",\"a\",\"b\",0)", ExcelError::Value);
    assert_eq!(text("SUBSTITUTE(\"abc\",\"\",\"x\")"), "abc"); // empty old = no-op
    assert_eq!(text("SUBSTITUTE(\"Abc\",\"a\",\"x\")"), "Abc"); // case-sensitive
}

#[test]
fn find_is_case_sensitive_no_wildcards() {
    assert_eq!(num("FIND(\"b\",\"abcabc\")"), 2.0);
    assert_eq!(num("FIND(\"B\",\"aBc\")"), 2.0);
    assert_err("FIND(\"b\",\"ABC\")", ExcelError::Value); // case differs
    assert_eq!(num("FIND(\"b\",\"abc\",2)"), 2.0);
    assert_eq!(num("FIND(\"b\",\"abcbc\",4)"), 4.0);
    assert_eq!(num("FIND(\"\",\"abc\")"), 1.0);
    assert_eq!(num("FIND(\"\",\"abc\",3)"), 3.0);
    assert_err("FIND(\"x\",\"abc\")", ExcelError::Value);
    assert_err("FIND(\"a\",\"abc\",9)", ExcelError::Value);
    assert_err("FIND(\"a\",\"abc\",0)", ExcelError::Value);
    // No wildcards in FIND: '*' is literal.
    assert_err("FIND(\"*\",\"abc\")", ExcelError::Value);
    assert_eq!(num("FIND(\"*\",\"a*c\")"), 2.0);
}

#[test]
fn search_is_case_insensitive_with_wildcards() {
    assert_eq!(num("SEARCH(\"B\",\"abc\")"), 2.0);
    assert_eq!(num("SEARCH(\"?c\",\"abc\")"), 2.0);
    assert_eq!(num("SEARCH(\"a*c\",\"xxabc\")"), 3.0);
    assert_eq!(num("SEARCH(\"a~*b\",\"a*b\")"), 1.0); // ~ escapes *
    assert_err("SEARCH(\"z\",\"abc\")", ExcelError::Value);
    assert_err("SEARCH(\"a\",\"abc\",0)", ExcelError::Value);
}

#[test]
fn rept() {
    assert_eq!(text("REPT(\"ab\",3)"), "ababab");
    assert_eq!(text("REPT(\"x\",0)"), "");
    assert_eq!(text("REPT(\"x\",1.9)"), "x"); // truncated
    assert_err("REPT(\"x\",-1)", ExcelError::Value);
    assert_err("REPT(\"x\",40000)", ExcelError::Value); // cell cap 32767
}

#[test]
fn text_numeric_formats() {
    assert_eq!(text("TEXT(1234.567,\"0\")"), "1235");
    assert_eq!(text("TEXT(1234.567,\"0.00\")"), "1234.57");
    assert_eq!(text("TEXT(0.5,\"0.000\")"), "0.500");
    assert_eq!(text("TEXT(1234567,\"#,##0\")"), "1,234,567");
    assert_eq!(text("TEXT(1234567.891,\"#,##0.00\")"), "1,234,567.89");
    assert_eq!(text("TEXT(-1234.5,\"#,##0.0\")"), "-1,234.5");
    assert_eq!(text("TEXT(0.2854,\"0.0%\")"), "28.5%");
    assert_eq!(text("TEXT(0.2854,\"0.00%\")"), "28.54%");
    assert_eq!(text("TEXT(12345.6789,\"0.00E+00\")"), "1.23E+04");
    assert_eq!(text("TEXT(0.00012345,\"0.00E+00\")"), "1.23E-04");
    assert_eq!(text("TEXT(7,\"General\")"), "7");
    assert_eq!(text("TEXT(3.5,\"@\")"), "3.5");
    // Half-away rounding in formats.
    assert_eq!(text("TEXT(2.5,\"0\")"), "3");
    assert_eq!(text("TEXT(-2.5,\"0\")"), "-3");
}

#[test]
fn text_date_formats() {
    // 45123 = 2023-07-16 (Sunday); 0.75 = 18:00.
    assert_eq!(text("TEXT(45123,\"yyyy-mm-dd\")"), "2023-07-16");
    assert_eq!(text("TEXT(45123,\"yyyy/mm/dd\")"), "2023/07/16");
    assert_eq!(text("TEXT(45123,\"mm/dd/yyyy\")"), "07/16/2023");
    assert_eq!(text("TEXT(45123,\"dd/mm/yyyy\")"), "16/07/2023");
    assert_eq!(text("TEXT(45123,\"d mmm yyyy\")"), "16 Jul 2023");
    assert_eq!(text("TEXT(45123,\"dddd\")"), "Sunday");
    assert_eq!(text("TEXT(45123.75,\"h:mm\")"), "18:00");
    assert_eq!(text("TEXT(45123.75,\"hh:mm:ss\")"), "18:00:00");
    assert_eq!(
        text("TEXT(45123.75,\"yyyy-mm-dd hh:mm\")"),
        "2023-07-16 18:00"
    );
    assert_eq!(text("TEXT(45123.75,\"h:mm AM/PM\")"), "6:00 PM");
    assert_eq!(text("TEXT(0.25,\"h:mm\")"), "6:00");
    // The Lotus bug, faithfully formatted.
    assert_eq!(text("TEXT(60,\"yyyy-mm-dd\")"), "1900-02-29");
    assert_eq!(text("TEXT(1,\"yyyy-mm-dd\")"), "1900-01-01");
    assert_eq!(text("TEXT(25569,\"yyyy-mm-dd\")"), "1970-01-01");
    // Out-of-range serial → #NUM! through TEXT.
    assert_err("TEXT(2958466,\"yyyy\")", ExcelError::Num);
}

#[test]
fn text_passthrough_and_types() {
    assert_eq!(text("TEXT(\"abc\",\"0.00\")"), "abc"); // text passes through
    assert_eq!(text("TEXT(\"abc\",\"@\")"), "abc");
    assert_eq!(text("TEXT(TRUE,\"0\")"), "1"); // bools coerce like Excel
    assert!(!text("TEXT(12,\"yyyy-mm-dd hh:mm\")").is_empty());
    assert_err("TEXT(,\"0\")", ExcelError::Value); // omitted value arg
}

#[test]
fn over_sheet_text_ops() {
    assert_eq!(text_over("LEFT(B1,2)"), "al");
    assert_eq!(text_over("UPPER(B1)"), "ALPHA");
    assert_eq!(num_over("LEN(B1)"), 5.0);
    assert_eq!(num_over("FIND(\"a\",B1)"), 1.0);
    let mut s = MapResolver::new();
    s.set_text(1, 1, "\u{1F3B8}strum");
    assert_eq!(
        match ev_over("LEFT(A1,3)", &s) {
            Ok(Value::Text(t)) => t,
            other => panic!("{other:?}"),
        },
        "\u{1F3B8}s" // 3 UTF-16 units = emoji (2) + 's'
    );
}
