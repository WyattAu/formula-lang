//! Fuzz target — the full pipeline. REQ: evaluation is total (Ok or typed
//! error, never a panic, never a stack overflow) over arbitrary formulas
//! and arbitrary (empty) sheets; volatile detection is crash-free too.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = core::str::from_utf8(data) else { return };
    if let Ok(e) = formula_lang::parse(s) {
        let empty = formula_lang::EmptyResolver;
        // Depth cap converts hostile nesting into a typed error.
        let _ = formula_lang::evaluate_with_clock(&e, &empty, 45_123.75);
        let _ = formula_lang::is_volatile(&e);
    }
});
