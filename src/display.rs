//! Serializing an [`Expr`] back to formula text — iteratively.
//!
//! `Display for Expr` is the canonical `Expr → text` direction: precedence-
//! correct (re-parsing yields the identical AST — property-tested and fuzz-
//! asserted) and implemented with an explicit work stack, so even a
//! programmatically-built 100k-deep tree cannot overflow the real stack.

use crate::ast::{col_name, is_omitted_arg, Expr};
use crate::error::ExcelError;
use alloc::string::{String, ToString};
use alloc::vec;
use core::fmt;

/// Effective binding power of a node for parenthesization decisions —
/// primaries are "maximally tight" and never get wrapped.
fn node_bp(e: &Expr) -> usize {
    match e {
        Expr::Binary { op, .. } => op.binding_power().0,
        Expr::Unary { op, .. } => op.binding_power(),
        _ => usize::MAX,
    }
}

/// Writes a finite float with Rust's shortest round-trip representation —
/// the exactness guarantee the parse/print/parse round-trip relies on.
/// Non-finite values render as `1E+999`/`-1E+999` (which re-lex to ±inf)
/// or `#NUM!` for NaN (not representable; the one documented lossy case).
fn write_number(out: &mut String, n: f64) {
    if n.is_nan() {
        // Not representable in the grammar — documented lossy case.
        out.push_str(ExcelError::Num.literal());
    } else if n.is_infinite() {
        // Re-lexes to the same ±inf (`1E+999` overflows the float parser).
        out.push_str(if n > 0.0 { "1E+999" } else { "-1E+999" });
    } else {
        let _ = fmt::Write::write_fmt(out, format_args!("{n}"));
    }
}

enum Job<'a> {
    Str(&'static str),
    Expr {
        e: &'a Expr,
        /// Wrap in parens when `node_bp(child) < min_bp`.
        min_bp: usize,
        /// Also wrap when `node_bp(child) == min_bp` (associativity repair).
        wrap_eq: bool,
    },
    /// Renders function arguments, omitted slots as empty text.
    Args(&'a [Expr]),
}

impl fmt::Display for Expr {
    /// Renders the formula text for this expression — the dialect this
    /// crate parses (no leading `=`; add one yourself if you want the
    /// Excel entry form).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        let mut stack = vec![Job::Expr {
            e: self,
            min_bp: 0,
            wrap_eq: false,
        }];
        while let Some(job) = stack.pop() {
            match job {
                Job::Str(s) => out.push_str(s),
                Job::Args(args) => {
                    // The stack pops LIFO, so push args in reverse; each
                    // boundary comma follows its left neighbor. Omitted
                    // slots contribute nothing but keep their boundaries.
                    for (i, a) in args.iter().enumerate().rev() {
                        if !is_omitted_arg(a) {
                            stack.push(Job::Expr {
                                e: a,
                                min_bp: 0,
                                wrap_eq: false,
                            });
                        }
                        if i > 0 {
                            stack.push(Job::Str(","));
                        }
                    }
                }
                Job::Expr { e, min_bp, wrap_eq } => {
                    let bp = node_bp(e);
                    let wrap = bp < min_bp || (wrap_eq && bp == min_bp);
                    if wrap {
                        out.push('(');
                        // The closer is deferred until the node's children
                        // have rendered (children are pushed below, and the
                        // stack pops LIFO — push the closer LAST so it pops
                        // after them).
                        stack.push(Job::Str(")"));
                    }
                    match *e {
                        Expr::Number(n) => write_number(&mut out, n),
                        Expr::Text(ref s) => {
                            out.push('"');
                            out.push_str(&s.replace('"', "\"\""));
                            out.push('"');
                        }
                        Expr::Boolean(b) => out.push_str(if b { "TRUE" } else { "FALSE" }),
                        Expr::Error(err) => out.push_str(err.literal()),
                        Expr::CellRef {
                            col,
                            row,
                            col_abs,
                            row_abs,
                        } => {
                            if col_abs {
                                out.push('$');
                            }
                            out.push_str(&col_name(col));
                            if row_abs {
                                out.push('$');
                            }
                            let _ = fmt::Write::write_fmt(&mut out, format_args!("{row}"));
                        }
                        Expr::Range { ref start, ref end } => {
                            out.push_str(&start.to_string());
                            out.push(':');
                            out.push_str(&end.to_string());
                        }
                        Expr::Unary { op, ref expr } => {
                            // Operand context = the operator's own binding
                            // power (not +1): unary-of-unary chains render
                            // paren-free (`--1`, `50%%`), which keeps the
                            // rendered frame cost of a chain equal to the
                            // parsed one — the round-trip property for
                            // MAX_DEPTH-deep chains depends on it. Mixed
                            // binding (`-(1+2)`, `(−5)%`) still wraps.
                            if op.is_prefix() {
                                out.push_str(op.symbol());
                                stack.push(Job::Expr {
                                    e: expr,
                                    min_bp: op.binding_power(),
                                    wrap_eq: false,
                                });
                            } else {
                                stack.push(Job::Str(op.symbol()));
                                stack.push(Job::Expr {
                                    e: expr,
                                    min_bp: op.binding_power(),
                                    wrap_eq: false,
                                });
                            }
                        }
                        Expr::Binary {
                            op,
                            ref left,
                            ref right,
                        } => {
                            let (lbp, rbp) = op.binding_power();
                            let right_assoc = lbp == rbp;
                            // Pops LIFO: right child, symbol, left child.
                            stack.push(Job::Expr {
                                e: right,
                                min_bp: rbp,
                                wrap_eq: !right_assoc,
                            });
                            stack.push(Job::Str(op.symbol()));
                            stack.push(Job::Expr {
                                e: left,
                                min_bp: lbp,
                                wrap_eq: right_assoc,
                            });
                        }
                        Expr::Function { ref name, ref args } => {
                            out.push_str(&name.to_ascii_uppercase());
                            out.push('(');
                            // Pushed before Args so it pops after them.
                            stack.push(Job::Str(")"));
                            stack.push(Job::Args(args));
                        }
                    }
                }
            }
        }
        f.write_str(&out)
    }
}

/// Serializes an expression to formula text (convenience wrapper over
/// [`Display`]). Lossy only for `NaN` literals (rendered as `#NUM!`).
#[must_use]
pub fn to_formula(expr: &Expr) -> String {
    expr.to_string()
}
