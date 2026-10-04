//! The cell-value abstraction — the boundary that keeps this crate a pure
//! evaluator.
//!
//! `formula-lang` never stores spreadsheet data. Host engines implement
//! [`CellResolver`] (a sheet model, a sparse map, a test fixture) and hand
//! it to [`evaluate`](crate::evaluate). Two ready-made implementations are
//! included: [`EmptyResolver`] and [`MapResolver`].
//!
//! # Range layout contract
//!
//! [`CellResolver::get_range`] returns values **row-major**, suitable for
//! indexing as `vec[(r - top) * width + (c - left)]` with `width = right -
//! left + 1`. The lookup functions (`VLOOKUP`, `HLOOKUP`, `INDEX`,
//! `MATCH`, `OFFSET`) rely on this layout. Out-of-grid coordinates inside a
//! requested range are `Value::Empty`.

use crate::ast::normalize_range;
use crate::value::Value;
use alloc::collections::BTreeMap;
use alloc::string::ToString;
use alloc::vec::Vec;

/// Resolves cell coordinates to values. Coordinates are 1-based; `None`
/// from [`get`](CellResolver::get) means "no such cell / empty".
pub trait CellResolver {
    /// The scalar value of one cell, or `None` when the cell is empty or
    /// outside the modeled grid.
    fn get(&self, col: u32, row: u32) -> Option<Value>;

    /// The values of a rectangular region, **row-major**. `start` and
    /// `end` corners may arrive in either order (the evaluator normalizes
    /// before calling, but implementations should not rely on that).
    ///
    /// The default implementation derives the rectangle from repeated
    /// [`get`](CellResolver::get) calls — correct for any resolver, but
    /// engines with bulk access should override for speed.
    fn get_range(&self, start: (u32, u32), end: (u32, u32)) -> Vec<Value> {
        let (c0, r0, c1, r1) = normalize_range(
            &crate::ast::CellRef::new(start.0, start.1),
            &crate::ast::CellRef::new(end.0, end.1),
        );
        let mut out = Vec::new();
        for row in r0..=r1 {
            for col in c0..=c1 {
                out.push(self.get(col, row).unwrap_or(Value::Empty));
            }
        }
        out
    }
}

/// A resolver where every cell is empty. Useful as the null engine in
/// tests and as the default of host pipelines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EmptyResolver;

impl CellResolver for EmptyResolver {
    fn get(&self, _col: u32, _row: u32) -> Option<Value> {
        None
    }
}

/// A sparse `BTreeMap`-backed resolver — the workhorse for tests, docs,
/// and small host models.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MapResolver {
    cells: BTreeMap<(u32, u32), Value>,
}

impl MapResolver {
    /// An empty map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets one cell (1-based coordinates).
    pub fn set(&mut self, col: u32, row: u32, value: Value) {
        self.cells.insert((col, row), value);
    }

    /// Convenience: sets one cell to a number.
    pub fn set_num(&mut self, col: u32, row: u32, n: f64) {
        self.set(col, row, Value::Number(n));
    }

    /// Convenience: sets one cell to text.
    pub fn set_text(&mut self, col: u32, row: u32, s: &str) {
        self.set(col, row, Value::Text(s.to_string()));
    }

    /// Number of populated cells.
    #[must_use]
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// True when no cells are set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

impl CellResolver for MapResolver {
    fn get(&self, col: u32, row: u32) -> Option<Value> {
        self.cells.get(&(col, row)).cloned()
    }

    fn get_range(&self, start: (u32, u32), end: (u32, u32)) -> Vec<Value> {
        let (c0, r0, c1, r1) = normalize_range(
            &crate::ast::CellRef::new(start.0, start.1),
            &crate::ast::CellRef::new(end.0, end.1),
        );
        let mut out = Vec::new();
        for row in r0..=r1 {
            for col in c0..=c1 {
                out.push(self.cells.get(&(col, row)).cloned().unwrap_or(Value::Empty));
            }
        }
        out
    }
}
