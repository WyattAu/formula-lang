//! Fuzz target — the tokenizer. REQ: Err-not-panic on arbitrary input, and
//! token positions that stay ordered and in bounds.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = core::str::from_utf8(data) else { return };
    if let Ok(toks) = formula_lang::tokenize(s) {
        // Invariant: tokens are ordered, non-empty-spanned where claimed,
        // and exactly one Eof terminates the stream.
        let mut last_pos = 0usize;
        let mut saw_eof = false;
        for t in &toks {
            assert!(t.pos >= last_pos, "token position went backwards");
            assert!(t.pos <= s.len(), "token position out of bounds");
            last_pos = t.pos;
            if matches!(t.kind, formula_lang::TokenKind::Eof) {
                assert!(!saw_eof, "multiple Eof tokens");
                assert_eq!(t.pos, s.len(), "Eof must sit at input end");
                saw_eof = true;
            }
        }
        assert!(saw_eof, "token stream must end with Eof");
    }
});
