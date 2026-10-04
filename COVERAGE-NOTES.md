# Coverage notes — formula-lang

Method: `cargo llvm-cov --all-features` (the shared tier-a gate),
2026-10-04, rustc 1.99.0.

| Metric | Value | Gate |
|---|---|---|
| Lines | **95.65%** (2,873 lines, 125 missed) | ≥ 90% ✅ |
| Functions | 95.72% executed | — |
| Regions | 93.40% | — |

Weakest files and why:

- `token.rs` 76% — `TokenKind::describe()`'s per-variant render strings;
  the parser exercises a subset of `got` messages. The variants are
  trivial `write!` calls, pinned structurally by the tokenize suite.
- `resolver.rs` 87.5% — the `CellResolver::get_range` default
  implementation (the `MapResolver` override shadows it in tests); it is
  the documented fallback path for custom resolvers.
- `funcs/*` 91–95% — deep `TEXT` format-code permutations (weekday names,
  AM/PM edge combinations) and lookup approximate-match branch tails.

Unmeasured by design: `fuzz/` and `benches/` (not part of the lib
target's coverage denominator under `cargo llvm-cov` without
`--all-targets`).

The three fuzz targets double as semantic oracles beyond line coverage:
tokenizer position invariants, Display↔parse identity, and evaluation
totality over hostile input.
