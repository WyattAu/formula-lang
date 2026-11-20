# Pattern adoption

Spreadsheet formula engine: parse at the boundary, pure evaluation core,
property and fuzz coverage on the parser. The typed error taxonomy in
src/error.rs (ExcelError data values vs FormulaError engine failures)
has no canon ID and is claimed as none.

| Pattern | Status | Evidence |
|---|---|---|
| T1 | adopted | src/parser.rs:49 (parse() turns formula text into a typed Expr or FormulaError with byte positions; nothing downstream re-validates) |
| T13 | adopted | src/lib.rs:125 (pure, total engine with no I/O; hosts inject sheet access through CellResolver) |
| P64 | adopted | tests/properties.rs:154 (proptest suite over coercion and evaluation) |
| P67 | adopted | fuzz/fuzz_targets/parse.rs (tokenize, parse, and parse-eval fuzz targets) |
