//! Expression grammar (milestone M3, see `docs/specs/0002-java-syntax-subset.md`).
//!
//! A Pratt parser over the precedence table in the spec. Every function that parses an operand
//! returns the node it built (so callers can wrap it with `precede`), or `None` after reporting
//! `expected an expression` when the current token cannot start one. A `None` result never
//! consumes input.

use super::{
    grammar::{
        annotation, annotations, at_param_start, binding_name, class_body, is_primitive_kw,
        lambda_param, modifiers, name, name_ref, type_, type_args, type_marker,
    },
    lookahead::{skip_annotations, skip_parens, skip_type, MAX_SCAN},
    statements::{block, switch_expr},
    CompletedMarker, Parser,
};
use crate::{
    language_level::Release,
    syntax_kind::SyntaxKind::{self, *},
};

/// Longest left-associative chain (`a + b + ...`, `a.b.c...`, `f()()...`) of one expression. Each
/// link nests the tree one level deeper, and a tree that is too deep overflows the stack when it
/// is dropped.
const MAX_CHAIN: usize = 1024;

/// Lowest precedence of a binary operator (`||`).
const LOWEST_BINARY: u8 = 3;

/// A full expression: assignment, ternary, lambda, and everything below.
pub(super) fn expr(p: &mut Parser<'_, '_>) -> Option<CompletedMarker> {
    p.nested(None, |p| {
        let lhs = ternary(p)?;
        match assignment_op(p) {
            Some((kind, n_raw)) => {
                let m = lhs.precede(p);
                bump_op(p, kind, n_raw);
                expr(p);
                Some(m.complete(p, ASSIGN_EXPR))
            }
            None => Some(lhs),
        }
    })
}

/// A variable initializer or annotation element value: an expression, `{ ... }` or `@Anno`.
pub(super) fn initializer(p: &mut Parser<'_, '_>) {
    if p.at(L_CURLY) {
        array_init(p);
    } else if p.at(AT) && !p.nth_at(1, INTERFACE_KW) {
        annotation(p);
    } else {
        expr(p);
    }
}

/// `( expr, expr, ... )`. Stops at `;`, `}` and member-only modifiers so that a half-typed call
/// does not swallow what follows.
pub(super) fn arg_list(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(L_PAREN);
    p.with_no_lambda(false, |p| {
        while !p.at_eof()
            && !matches!(
                p.current(),
                R_PAREN | SEMICOLON | R_CURLY | PUBLIC_KW | PRIVATE_KW | PROTECTED_KW
            )
        {
            expr(p);
            if !p.eat(COMMA) {
                break;
            }
            if p.at(R_PAREN) {
                p.error_missing("expected an expression");
            }
        }
    });
    p.expect(R_PAREN, "expected ')'");
    m.complete(p, ARG_LIST);
}

/// `{ element, element, ... }` where an element is an expression or a nested array initializer.
pub(super) fn array_init(p: &mut Parser<'_, '_>) {
    p.nested((), |p| {
        p.with_no_lambda(false, |p| {
            let m = p.start();
            p.bump(L_CURLY);
            while !p.at_eof()
                && !matches!(
                    p.current(),
                    R_CURLY | R_PAREN | SEMICOLON | PUBLIC_KW | PRIVATE_KW | PROTECTED_KW
                )
            {
                initializer(p);
                if !p.eat(COMMA) {
                    break;
                }
            }
            p.expect(R_CURLY, "expected '}'");
            m.complete(p, ARRAY_INIT);
        })
    })
}

// ---- Operators ----

fn bump_op(p: &mut Parser<'_, '_>, kind: SyntaxKind, n_raw: usize) {
    if n_raw == 1 {
        p.bump_any();
    } else {
        p.bump_composite(kind, n_raw);
    }
}

fn too_long(p: &mut Parser<'_, '_>) {
    p.error_unexpected("expression is too long");
}

/// The operator made of `>` tokens that starts at the current token, which must be `>`. The
/// lexer never combines `>`, so adjacent tokens (no trivia between them) are merged here.
fn greater_family(p: &Parser<'_, '_>) -> (SyntaxKind, usize) {
    if !p.nth_is_joint(0) {
        return (R_ANGLE, 1);
    }
    match p.nth(1) {
        EQ => (GTEQ, 2),
        R_ANGLE if !p.nth_is_joint(1) => (SHR, 2),
        R_ANGLE => match p.nth(2) {
            EQ => (SHREQ, 3),
            R_ANGLE if p.nth_is_joint(2) && p.nth_at(3, EQ) => (USHREQ, 4),
            R_ANGLE => (USHR, 3),
            _ => (SHR, 2),
        },
        _ => (R_ANGLE, 1),
    }
}

/// An assignment operator at the current token, and how many raw tokens it spans.
fn assignment_op(p: &Parser<'_, '_>) -> Option<(SyntaxKind, usize)> {
    match p.current() {
        EQ | PLUSEQ | MINUSEQ | STAREQ | SLASHEQ | AMPEQ | PIPEEQ | CARETEQ | PERCENTEQ | SHLEQ => {
            Some((p.current(), 1))
        }
        R_ANGLE => match greater_family(p) {
            (kind @ (SHREQ | USHREQ), n) => Some((kind, n)),
            _ => None,
        },
        _ => None,
    }
}

struct BinaryOp {
    prec: u8,
    kind: SyntaxKind,
    n_raw: usize,
}

fn binary_op(p: &Parser<'_, '_>) -> Option<BinaryOp> {
    let (kind, n_raw) = match p.current() {
        R_ANGLE => greater_family(p),
        kind => (kind, 1),
    };
    let prec = match kind {
        PIPE2 => 3,
        AMP2 => 4,
        PIPE => 5,
        CARET => 6,
        AMP => 7,
        EQ2 | NEQ => 8,
        L_ANGLE | R_ANGLE | LTEQ | GTEQ | INSTANCEOF_KW => 9,
        SHL | SHR | USHR => 10,
        PLUS | MINUS => 11,
        STAR | SLASH | PERCENT => 12,
        // Includes SHREQ and USHREQ: those are assignments.
        _ => return None,
    };
    Some(BinaryOp { prec, kind, n_raw })
}

// ---- Precedence levels ----

fn ternary(p: &mut Parser<'_, '_>) -> Option<CompletedMarker> {
    let cond = binary(p, LOWEST_BINARY)?;
    if !p.at(QUESTION) {
        return Some(cond);
    }
    let m = cond.precede(p);
    p.bump(QUESTION);
    expr(p);
    p.expect(COLON, "expected ':'");
    p.nested(None, ternary);
    Some(m.complete(p, TERNARY_EXPR))
}

fn binary(p: &mut Parser<'_, '_>, min_prec: u8) -> Option<CompletedMarker> {
    let mut lhs = unary(p)?;
    let mut links = 0;
    while let Some(op) = binary_op(p) {
        if op.prec < min_prec {
            break;
        }
        if links >= MAX_CHAIN {
            too_long(p);
            break;
        }
        links += 1;
        let m = lhs.precede(p);
        if op.kind == INSTANCEOF_KW {
            p.bump(INSTANCEOF_KW);
            instanceof_rhs(p);
            lhs = m.complete(p, INSTANCEOF_EXPR);
        } else {
            bump_op(p, op.kind, op.n_raw);
            binary(p, op.prec + 1);
            lhs = m.complete(p, BINARY_EXPR);
        }
    }
    Some(lhs)
}

fn unary(p: &mut Parser<'_, '_>) -> Option<CompletedMarker> {
    if matches!(p.current(), PLUS | MINUS | PLUS2 | MINUS2 | BANG | TILDE) {
        return p.nested(None, |p| {
            let m = p.start();
            p.bump_any();
            unary(p);
            Some(m.complete(p, UNARY_EXPR))
        });
    }
    postfix(p)
}

fn postfix(p: &mut Parser<'_, '_>) -> Option<CompletedMarker> {
    let mut lhs = primary(p)?;
    let mut links = 0;
    loop {
        let callable = matches!(lhs.kind(), NAME_REF_EXPR | THIS_EXPR | SUPER_EXPR);
        let continues = match p.current() {
            DOT | L_BRACK | PLUS2 | MINUS2 | COLON2 => true,
            L_PAREN => callable,
            _ => false,
        };
        if !continues {
            break;
        }
        if links >= MAX_CHAIN {
            too_long(p);
            break;
        }
        links += 1;
        lhs = match p.current() {
            DOT => dot_suffix(p, lhs),
            L_PAREN => {
                let m = lhs.precede(p);
                arg_list(p);
                m.complete(p, CALL_EXPR)
            }
            L_BRACK => {
                let m = lhs.precede(p);
                p.bump(L_BRACK);
                p.with_no_lambda(false, expr);
                p.expect(R_BRACK, "expected ']'");
                m.complete(p, ARRAY_ACCESS_EXPR)
            }
            COLON2 => method_ref(p, lhs),
            _ => {
                let m = lhs.precede(p);
                p.bump_any();
                m.complete(p, POSTFIX_EXPR)
            }
        };
    }
    Some(lhs)
}

/// `lhs . ...`: field access, method call, `.class`, `.this`, `.super` or `.new`. A dot with
/// nothing usable after it still yields a field access with a missing name, so tools have a node
/// to work from while the user is typing.
fn dot_suffix(p: &mut Parser<'_, '_>, lhs: CompletedMarker) -> CompletedMarker {
    let m = lhs.precede(p);
    p.bump(DOT);
    let kind = match p.current() {
        IDENT => {
            name_ref(p);
            if p.at(L_PAREN) {
                arg_list(p);
                METHOD_CALL_EXPR
            } else {
                FIELD_ACCESS_EXPR
            }
        }
        L_ANGLE => {
            type_args(p);
            name_ref(p);
            if p.at(L_PAREN) {
                arg_list(p);
            } else {
                p.error_missing("expected '('");
            }
            METHOD_CALL_EXPR
        }
        THIS_KW => {
            p.bump_any();
            THIS_EXPR
        }
        SUPER_KW => {
            p.bump_any();
            SUPER_EXPR
        }
        CLASS_KW => {
            p.bump_any();
            CLASS_LITERAL
        }
        NEW_KW => {
            new_expr_rest(p);
            NEW_EXPR
        }
        _ => {
            p.error_missing("expected a name");
            FIELD_ACCESS_EXPR
        }
    };
    m.complete(p, kind)
}

/// `lhs :: name`, `lhs :: new`, `lhs :: <T>name`
fn method_ref(p: &mut Parser<'_, '_>, lhs: CompletedMarker) -> CompletedMarker {
    let m = lhs.precede(p);
    p.bump(COLON2);
    if p.at(L_ANGLE) {
        type_args(p);
    }
    if p.at(NEW_KW) {
        p.bump_any();
    } else {
        name_ref(p);
    }
    m.complete(p, METHOD_REF_EXPR)
}

// ---- Primary expressions ----

fn primary(p: &mut Parser<'_, '_>) -> Option<CompletedMarker> {
    p.nested(None, |p| match p.current() {
        INT_NUMBER | LONG_NUMBER | FLOAT_NUMBER | DOUBLE_NUMBER | CHAR_LITERAL | STRING_LITERAL
        | TEXT_BLOCK | TRUE_KW | FALSE_KW | NULL_KW => {
            let m = p.start();
            p.bump_any();
            Some(m.complete(p, LITERAL))
        }
        IDENT => Some(ident_primary(p)),
        UNDERSCORE_KW if p.nth_at(1, ARROW) && !p.no_lambda => Some(lambda(p)),
        UNDERSCORE_KW if p.level.release == Release::JAVA_8 => Some(ident_primary(p)),
        THIS_KW | SUPER_KW => {
            let kind = if p.at(THIS_KW) { THIS_EXPR } else { SUPER_EXPR };
            let m = p.start();
            p.bump_any();
            Some(m.complete(p, kind))
        }
        NEW_KW => Some(new_expr(p)),
        L_PAREN => Some(paren_primary(p)),
        SWITCH_KW => Some(switch_expr(p)),
        kind if is_primitive_kw(kind) || kind == VOID_KW => Some(type_primary(p)),
        _ => {
            p.error_missing("expected an expression");
            None
        }
    })
}

fn ident_primary(p: &mut Parser<'_, '_>) -> CompletedMarker {
    if p.nth_at(1, ARROW) && !p.no_lambda {
        return lambda(p);
    }
    if is_type_expr(p) {
        return type_primary(p);
    }
    let m = p.start();
    name_ref(p);
    m.complete(p, NAME_REF_EXPR)
}

/// Is this a type that cannot be a plain name expression, directly followed by `::` or
/// `.class`? That is a type with type arguments or array dimensions: `List<String>::new`,
/// `Foo[].class`.
fn is_type_expr(p: &Parser<'_, '_>) -> bool {
    let Some(n) = skip_type(p, 0) else {
        return false;
    };
    n > 1
        && matches!(p.nth(n - 1), R_ANGLE | R_BRACK)
        && (p.nth_at(n, COLON2) || (p.nth_at(n, DOT) && p.nth_at(n + 1, CLASS_KW)))
}

/// A type used as an expression: `int.class`, `void.class`, `int[]::new`, `List<String>::new`.
fn type_primary(p: &mut Parser<'_, '_>) -> CompletedMarker {
    let m = p.start();
    if p.at(VOID_KW) {
        let t = p.start();
        p.bump(VOID_KW);
        t.complete(p, PRIMITIVE_TYPE);
    } else {
        type_(p);
    }
    let kind = if p.at(DOT) && p.nth_at(1, CLASS_KW) {
        p.bump(DOT);
        p.bump(CLASS_KW);
        CLASS_LITERAL
    } else if p.at(COLON2) {
        p.bump(COLON2);
        if p.at(L_ANGLE) {
            type_args(p);
        }
        if p.at(NEW_KW) {
            p.bump_any();
        } else {
            name_ref(p);
        }
        METHOD_REF_EXPR
    } else {
        p.error_missing("expected '.class' or '::'");
        CLASS_LITERAL
    };
    m.complete(p, kind)
}

/// `( ... )`: a lambda, a cast or a parenthesised expression.
fn paren_primary(p: &mut Parser<'_, '_>) -> CompletedMarker {
    if !p.no_lambda && is_lambda_ahead(p) {
        return lambda(p);
    }
    let m = p.start();
    if is_cast_ahead(p) {
        p.bump(L_PAREN);
        type_(p);
        while p.eat(AMP) {
            type_(p);
        }
        p.expect(R_PAREN, "expected ')'");
        unary(p);
        m.complete(p, CAST_EXPR)
    } else {
        p.bump(L_PAREN);
        if p.at(R_PAREN) {
            p.error_missing("expected an expression");
        } else {
            p.with_no_lambda(false, expr);
        }
        p.expect(R_PAREN, "expected ')'");
        m.complete(p, PAREN_EXPR)
    }
}

fn is_lambda_ahead(p: &Parser<'_, '_>) -> bool {
    matches!(skip_parens(p, 0), Some(n) if p.nth_at(n, ARROW))
}

/// At `(`: is the parenthesised text a type (or an intersection of types) that is cast onto the
/// operand that follows? A single primitive keyword always is. Otherwise the next token must be
/// able to start an operand that is not `+`, `-`, `++` or `--`, as in Java.
fn is_cast_ahead(p: &Parser<'_, '_>) -> bool {
    let Some(mut n) = skip_type(p, 1) else {
        return false;
    };
    let single_primitive =
        skip_annotations(p, 1).is_some_and(|start| is_primitive_kw(p.nth(start)) && n == start + 1);
    let mut intersections = 0;
    while p.nth_at(n, AMP) {
        intersections += 1;
        if intersections > MAX_SCAN {
            return false;
        }
        match skip_type(p, n + 1) {
            Some(next) => n = next,
            None => return false,
        }
    }
    if !p.nth_at(n, R_PAREN) {
        return false;
    }
    if single_primitive {
        return true;
    }
    let after = p.nth(n + 1);
    is_primitive_kw(after)
        || after == VOID_KW
        || matches!(
            after,
            IDENT
                | UNDERSCORE_KW
                | INT_NUMBER
                | LONG_NUMBER
                | FLOAT_NUMBER
                | DOUBLE_NUMBER
                | CHAR_LITERAL
                | STRING_LITERAL
                | TEXT_BLOCK
                | TRUE_KW
                | FALSE_KW
                | NULL_KW
                | L_PAREN
                | BANG
                | TILDE
                | THIS_KW
                | SUPER_KW
                | NEW_KW
                | SWITCH_KW
        )
}

/// `x -> body`, `_ -> body`, `(a, b) -> body`, `(int a, var b) -> body`; the body is a block or
/// an expression.
fn lambda(p: &mut Parser<'_, '_>) -> CompletedMarker {
    let m = p.start();
    let params = p.start();
    if p.at(IDENT) || p.at(UNDERSCORE_KW) {
        let single = p.start();
        binding_name(p);
        single.complete(p, PARAM);
    } else {
        p.bump(L_PAREN);
        while !p.at_eof() && !matches!(p.current(), R_PAREN | ARROW | L_CURLY | R_CURLY | SEMICOLON)
        {
            if (p.at(IDENT) || p.at(UNDERSCORE_KW)) && matches!(p.nth(1), COMMA | R_PAREN) {
                let bare = p.start();
                binding_name(p);
                bare.complete(p, PARAM);
            } else if at_param_start(p) {
                lambda_param(p);
            } else {
                break;
            }
            if !p.eat(COMMA) {
                break;
            }
            if p.at(R_PAREN) {
                p.error_missing("expected a parameter");
            }
        }
        p.expect(R_PAREN, "expected ')'");
    }
    params.complete(p, LAMBDA_PARAMS);
    p.expect(ARROW, "expected '->'");
    if p.at(L_CURLY) {
        block(p);
    } else {
        expr(p);
    }
    m.complete(p, LAMBDA_EXPR)
}

// ---- Object and array creation ----

fn new_expr(p: &mut Parser<'_, '_>) -> CompletedMarker {
    let m = p.start();
    new_expr_rest(p);
    m.complete(p, NEW_EXPR)
}

fn at_new_array_dim_start(p: &Parser<'_, '_>) -> bool {
    p.at(L_BRACK) || matches!(skip_annotations(p, 0), Some(n) if p.nth_at(n, L_BRACK))
}

/// Everything of a `new` expression from the `new` keyword on. Also used after `outer.`
fn new_expr_rest(p: &mut Parser<'_, '_>) {
    p.bump(NEW_KW);
    if p.at(L_ANGLE) {
        type_args(p);
    }
    let Some(ty) = type_marker(p) else {
        return;
    };
    match p.current() {
        L_PAREN => {
            arg_list(p);
            if p.at(L_CURLY) {
                class_body(p);
            }
        }
        _ if at_new_array_dim_start(p) => {
            // Dimension expressions followed by empty dimensions: `new int[3][]`, `new int @A [3] @B []`.
            while at_new_array_dim_start(p) {
                annotations(p);
                p.bump(L_BRACK);
                if !p.at(R_BRACK) {
                    p.with_no_lambda(false, expr);
                }
                p.expect(R_BRACK, "expected ']'");
            }
            if p.at(L_CURLY) {
                array_init(p);
            }
        }
        L_CURLY if ty.kind() == ARRAY_TYPE => array_init(p),
        _ if ty.kind() == ARRAY_TYPE => p.error_missing("expected '{'"),
        _ => p.error_missing("expected '(' or '['"),
    }
}

// ---- Patterns ----

/// Is a pattern (rather than a plain type or expression) next? True for a modifier or a type
/// (with optional leading annotations) followed by an identifier (`Foo f`), `_` (`Foo _`), or `(`
/// (`Point(int x, int y)`).
pub(super) fn is_pattern_ahead(p: &Parser<'_, '_>) -> bool {
    let mut n = 0;
    let mut has_final = false;
    loop {
        if n > MAX_SCAN {
            return false;
        }
        if p.nth_at(n, FINAL_KW) {
            has_final = true;
            n += 1;
        } else if p.nth_at(n, AT) {
            match skip_annotations(p, n) {
                Some(after) if after > n => n = after,
                _ => return false,
            }
        } else {
            break;
        }
    }
    has_final
        || matches!(
            skip_type(p, n),
            Some(end) if matches!(p.nth(end), IDENT | UNDERSCORE_KW | L_PAREN)
        )
}

/// `Type name`, `Type _`, `final Type name` or `Type(pattern, ...)` .
pub(super) fn pattern(p: &mut Parser<'_, '_>) {
    p.nested((), |p| {
        let m = p.start();
        modifiers(p);
        type_(p);
        let kind = if p.at(L_PAREN) {
            p.bump(L_PAREN);
            while !p.at_eof()
                && !matches!(p.current(), R_PAREN | L_CURLY | R_CURLY | SEMICOLON | ARROW)
            {
                if p.at(UNDERSCORE_KW) {
                    let u = p.start();
                    p.bump(UNDERSCORE_KW);
                    u.complete(p, UNNAMED_PATTERN);
                } else {
                    pattern(p);
                }
                if !p.eat(COMMA) {
                    break;
                }
            }
            p.expect(R_PAREN, "expected ')'");
            RECORD_PATTERN
        } else if p.at(UNDERSCORE_KW) {
            p.bump(UNDERSCORE_KW);
            TYPE_PATTERN
        } else {
            name(p);
            TYPE_PATTERN
        };
        m.complete(p, kind);
    })
}

fn instanceof_rhs(p: &mut Parser<'_, '_>) {
    if is_pattern_ahead(p) {
        pattern(p);
    } else {
        type_(p);
    }
}
