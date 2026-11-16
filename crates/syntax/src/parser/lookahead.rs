//! Bounded token lookahead used to resolve the grammar's ambiguities (declaration or
//! expression, cast or parenthesis, lambda or parenthesis, ...).
//!
//! Every scan stops after [`MAX_SCAN`] tokens, which keeps the cost of a single decision bounded
//! and keeps pathological input (`a<a<a<a<...`) from looking like a parser that is stuck.

use super::{grammar::is_primitive_kw, Parser};
use crate::syntax_kind::SyntaxKind::*;

/// How far ahead a scan looks at most.
pub(super) const MAX_SCAN: usize = 512;

/// If a type starts at token `n`, returns the index just past it. Only looks at tokens that can
/// occur in a type, so an expression such as `a < b` is not mistaken for one.
pub(super) fn skip_type(p: &Parser<'_, '_>, mut n: usize) -> Option<usize> {
    let limit = n + MAX_SCAN;
    if is_primitive_kw(p.nth(n)) {
        n += 1;
    } else if p.nth_at(n, IDENT) {
        n += 1;
        loop {
            if n > limit {
                return None;
            }
            if p.nth_at(n, L_ANGLE) {
                n = skip_type_args(p, n)?;
            }
            if p.nth_at(n, DOT) && p.nth_at(n + 1, IDENT) {
                n += 2;
            } else {
                break;
            }
        }
    } else {
        return None;
    }
    while p.nth_at(n, L_BRACK) && p.nth_at(n + 1, R_BRACK) {
        n += 2;
        if n > limit {
            return None;
        }
    }
    Some(n)
}

/// Skips balanced `<...>` starting at token `n`. Fails on any token a type argument list cannot
/// contain.
fn skip_type_args(p: &Parser<'_, '_>, mut n: usize) -> Option<usize> {
    let limit = n + MAX_SCAN;
    let mut depth = 0usize;
    loop {
        if n > limit {
            return None;
        }
        match p.nth(n) {
            L_ANGLE => depth += 1,
            R_ANGLE => {
                depth -= 1;
                if depth == 0 {
                    return Some(n + 1);
                }
            }
            IDENT | DOT | COMMA | QUESTION | EXTENDS_KW | SUPER_KW | AMP | L_BRACK | R_BRACK
            | AT => {}
            kind if is_primitive_kw(kind) => {}
            _ => return None,
        }
        n += 1;
    }
}

/// Skips balanced `(...)` starting at token `n`; `None` if it never closes within reach.
pub(super) fn skip_parens(p: &Parser<'_, '_>, mut n: usize) -> Option<usize> {
    let limit = n + MAX_SCAN;
    let mut depth = 0usize;
    loop {
        if n > limit {
            return None;
        }
        match p.nth(n) {
            L_PAREN => depth += 1,
            R_PAREN => {
                depth -= 1;
                if depth == 0 {
                    return Some(n + 1);
                }
            }
            EOF => return None,
            _ => {}
        }
        n += 1;
    }
}
