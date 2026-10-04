# formula-lang

**Spreadsheet formula language — tokenizer, Pratt parser, AST, evaluator
core. Excel-compatible function semantics.**

`formula-lang` is the estate's **L0 leaf** for spreadsheet work: a pure,
total formula engine. No I/O, no cell storage, no allocation on the
numeric scalar path. Host engines implement one trait
([`CellResolver`]) and bring the data; the crate does everything else.

```rust
use formula_lang::{parse, evaluate, MapResolver, Value};

// A tiny sheet: A1=2, B1=3, C1 text.
let mut sheet = MapResolver::new();
sheet.set_num(1, 1, 2.0);
sheet.set_num(2, 1, 3.0);
sheet.set_text(3, 1, "unit");

let e = parse("=SUM(A1:B1) * 10 & \" \" & C1").unwrap();
assert_eq!(evaluate(&e, &sheet).unwrap(), Value::Text("50 unit".to_string()));
```

## The pipeline

| Stage | Entry point | Property |
|---|---|---|
| Tokenize | `tokenize(&str) -> Vec<Token>` | zero-copy (`&[u8]`-backed spans), total |
| Parse | `parse(&str) -> Expr` | Pratt, Excel precedence, stack-safe (`MAX_DEPTH` cap) |
| Serialize | `Display` / `to_formula(&Expr)` | iterative, re-parses to the identical AST |
| Evaluate | `evaluate(&Expr, &dyn CellResolver)` | depth-capped, typed errors, lazy `IF` |
| Inspect | `is_volatile(&Expr)`, `builtin_functions()` | volatile = `TODAY`/`NOW`/`OFFSET` |

## Excel compatibility, stated precisely

- **Precedence**: comparison < concat < add < mul < power (right-assoc) <
  unary < postfix `%`. The quirk is faithful: `-2^2` = `(-2)^2` = `4`,
  and `2^3^2` = `512`.
- **Coercion**: arithmetic coerces `TRUE`→1, `"2.5"`→2.5, empty→0;
  comparisons rank Number < Text < Boolean and never cross-coerce
  (`"1" = 1` is FALSE); text equality is case-insensitive; empty
  substitutes the other side's zero value.
- **Errors**: the seven `#…!` values propagate; `IFERROR`/`IFNA`/
  `ISERROR`/`ISNA` catch them. Unknown function names are a hard
  `FormulaError::UnknownFunction` (typed over forgiving); a bare range as
  a scalar is `#VALUE!` (scalar core, no spills).
- **Rounding**: `ROUND` half-away-from-zero, `INT` floors, `TRUNC`/
  `ROUNDDOWN` truncate, `MOD` takes the divisor's sign, `CEILING`/`FLOOR`
  carry Excel's significance-sign rules.
- **Text**: UTF-16 code-unit indexing (`LEN("𐍈")` = 2, like Excel),
  `TRIM` touches only U+0020, `SEARCH` honors `?`/`*`/`~` wildcards while
  `FIND` does not, 32767-unit cell cap on `REPT`.
- **Dates**: the Lotus leap bug is preserved — serial 60 is the phantom
  1900-02-29, 1970-01-01 is serial 25569. `TEXT` implements the numeric
  subset plus date/time codes.
- **Volatile functions**: `TODAY`/`NOW`/`OFFSET` — reported by
  `is_volatile`, clocked deterministically by `evaluate_with_clock`.

## The omitted-argument marker

The AST has no dedicated "omitted" variant, so empty argument slots
(`IF(A1,,5)`, `SUM(1,)`) are stored as `Expr::Error(ExcelError::Null)`
and detected with [`is_omitted_arg`]. One documented trade-off: a literal
`#NULL!` in a direct argument slot is treated as omitted.

## Safety, totality, allocation

- `#![no_std]` + `alloc`; `libm` for transcendentals (bit-identical
  results across feature states).
- `unsafe_code = deny`; `unwrap`/`expect`/`panic`/`indexing_slicing`
  deny-linted on the lib target.
- **Total** on hostile input: fuzz-verified (three cargo-fuzz targets,
  ~4.4M executions pre-release). Hostile nesting yields
  `FormulaError::RecursionLimit`, never a stack overflow. The AST's
  `Drop` and `Display` are iterative — even a 100k-deep tree is safe.
- Zero allocation on the numeric scalar path; text results, aggregate
  operand vectors, and range materialization are the documented edges.

## Quality gates

| Gate | Status |
|---|---|
| Tests | 219 (unit + integration + 500-case properties) |
| Coverage | 95.6% lines (`cargo llvm-cov --all-features`, gate ≥ 90%) |
| Clippy | `-D warnings` clean (tier-a deny lints) |
| Fuzz | `tokenize`, `parse`, `parse_eval` — clean runs in CI (30 s each) |
| no_std | `thumbv7em-none-eabihf` + native `--no-default-features` |

## Layer

**L0 — leaf** (see `docs/layers.md` in
[WyattAu/engineering-standards](https://github.com/WyattAu/engineering-standards)):
zero estate-internal dependencies; the only runtime dependency is
`thiserror` (2.x, `no_std`) plus `libm` for `no_std` float math.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option.
