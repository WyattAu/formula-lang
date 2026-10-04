//! Fuzz target — the Pratt parser. REQ: Err-not-panic, never a stack
//! overflow on hostile nesting, and Display re-parse identity for every
//! accepted formula.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = core::str::from_utf8(data) else { return };
    if let Ok(e) = formula_lang::parse(s) {
        // Round-trip identity: render must re-parse to the same AST.
        let out = e.to_string();
        match formula_lang::parse(&out) {
            Ok(e2) => assert_eq!(e, e2, "input {s:?} rendered {out:?}"),
            Err(err) => panic!("input {s:?} rendered {out:?} failed to re-parse: {err:?}"),
        }
        // Rendering is a fixed point.
        let twice = formula_lang::parse(&out).unwrap().to_string();
        assert_eq!(out, twice);
    }
});
