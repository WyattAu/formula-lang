# Changelog

All notable changes to this project will be documented in this file.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning:
[SemVer](https://semver.org/).

## [0.1.0] — 2026-10-04

Initial release — L0 leaf, zero estate deps, `no_std` + `alloc`.

### Added

- **Tokenizer** (`tokenize`): zero-copy tokens borrowed from the input;
  numbers (decimals, exponents), strings with `""` escapes, all seven
  Excel error literals (case-insensitive), `$`-anchored and row-anchored
  cell references, operators, and a guaranteed `Eof` sentinel. Total:
  every malformed position yields a typed `FormulaError::Tokenize`.
- **Parser** (`parse`): Pratt parser over the Excel precedence ladder —
  comparison < concat < add < mul < **right-associative** power < prefix
  unary < postfix `%` — including the Excel quirk `-2^2 = 4`. Omitted
  function-argument slots (`F(1,)`, `IF(A1,,5)`) parse via the documented
  `is_omitted_arg` marker. Optional leading `=`. Stack-safe: nesting
  beyond `MAX_DEPTH` (512) returns `FormulaError::RecursionLimit`, never a
  stack overflow (fuzz-verified).
- **AST** (`Expr`, `BinaryOp`, `UnaryOp`, `CellRef`): the public,
  inspectable tree — with a stack-safe *iterative* `Drop` (fuzz-found leak
  and overflow fixed) and a precedence-correct **iterative** `Display` /
  `to_formula` whose output re-parses to the identical AST (property-
  tested and fuzz-asserted).
- **Evaluator** (`evaluate`, `evaluate_with_clock`): Excel coercion rules
  (arithmetic coerces bool→1/0, numeric text→number, empty→0;
  comparisons rank Number < Text < Boolean and never cross-coerce; text
  compares case-insensitively), the two-channel error contract
  (`IFERROR`/`IFNA`/`ISERROR`/`ISNA` catch evaluation errors; stored cell
  errors stay data until consumed), lazy `IF` branches, and a depth cap
  mirroring the parser's.
- **63 built-in functions** with Excel semantics: arithmetic (18),
  statistical (9), logical (8, incl. `TRUE()`/`FALSE()`), text (14,
  UTF-16 indexing like Excel), lookup (`VLOOKUP`, `HLOOKUP`, `INDEX`,
  `MATCH`, `OFFSET`), volatile date functions (`TODAY`, `NOW` — clocked
  via `evaluate_with_clock`), and information (7). `TEXT` implements the
  numeric subset plus date/time codes with the Lotus leap bug preserved
  (serial 60 = the phantom 1900-02-29).
- **Resolver contract** (`CellResolver`, `EmptyResolver`, `MapResolver`):
  the crate stores no data; hosts bring the sheet. Row-major range
  layout, documented.
- **Public date API** (`date` module): Excel serial ↔ civil date, the
  Lotus bug, `fraction_to_hms`, weekday names — pure integer math.
- **Fuzz targets**: `tokenize`, `parse`, `parse_eval` (cargo-fuzz) —
  totality, position invariants, Display round-trip identity.
- **Proptest suite**: totality, serialization round-trips and
  idempotence, commutativity/identity laws, depth-cap typing — 500 cases
  per property.
- **Benchmarks** (criterion): scalar/function/lookup eval and parse.

### Quality gates (this release)

- 231 tests (unit + integration + property), all passing.
- Line coverage **95.6%** (`cargo llvm-cov --all-features`; gate ≥ 90%).
- `cargo clippy --all-features --all-targets -- -D warnings` clean
  (tier-a deny lints: `unwrap`, `expect`, `panic`, `indexing_slicing`).
- `no_std` verified: `thumbv7em-none-eabihf` and native
  `--no-default-features` checks pass (`libm` for transcendentals).
- Fuzz: ~4.4M combined executions across three targets; three findings
  fixed pre-release (iterative-Drop leak, render-depth inflation on
  unary chains, single-omited-argument render documented as lossy).
