//! Grammar rules for milestone M1 (see `docs/specs/0002-java-syntax-subset.md`): the compilation
//! unit, type declarations, modifiers and annotations, and the minimal type syntax their headers
//! need. Class members are parsed shallowly into `MEMBER_PLACEHOLDER` nodes until M2.

use super::Parser;
use crate::syntax_kind::SyntaxKind::{self, *};

pub(super) fn source_file(p: &mut Parser<'_, '_>) {
    let m = p.start();
    while !p.at_eof() {
        let before = p.pos();
        top_level_item(p);
        debug_assert!(p.pos() > before, "top level item made no progress");
    }
    m.complete(p, SOURCE_FILE);
}

fn top_level_item(p: &mut Parser<'_, '_>) {
    match p.current() {
        SEMICOLON => p.bump_any(),
        IMPORT_KW => import_decl(p),
        R_CURLY => {
            let m = p.start();
            p.error_unexpected("unmatched '}'");
            p.bump_any();
            m.complete(p, ERROR);
        }
        _ => {
            let m = p.start();
            let start = p.pos();
            modifiers(p);
            if p.at(PACKAGE_KW) {
                package_rest(p);
                m.complete(p, PACKAGE_DECL);
            } else if let Some(kind) = type_decl_kind(p) {
                type_decl_rest(p, kind);
                m.complete(p, kind);
            } else {
                p.error_unexpected("expected a class, interface, enum or record declaration");
                if p.pos() == start {
                    p.bump_any();
                }
                while !p.at_eof() && !at_top_level_recovery(p) {
                    p.bump_any();
                }
                m.complete(p, ERROR);
            }
        }
    }
}

/// Tokens at which skipping unexpected top level input stops.
fn at_top_level_recovery(p: &Parser<'_, '_>) -> bool {
    let kind = p.current();
    matches!(kind, SEMICOLON | IMPORT_KW | PACKAGE_KW | AT | R_CURLY)
        || is_modifier_kw(kind)
        || type_decl_kind(p).is_some()
}

fn import_decl(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(IMPORT_KW);
    p.eat(STATIC_KW);
    path(p);
    if p.at(DOT) && p.nth_at(1, STAR) {
        p.bump(DOT);
        p.bump(STAR);
    }
    p.expect(SEMICOLON, "expected ';'");
    m.complete(p, IMPORT_DECL);
}

fn package_rest(p: &mut Parser<'_, '_>) {
    p.bump(PACKAGE_KW);
    path(p);
    p.expect(SEMICOLON, "expected ';'");
}

// ---- Names and paths ----

fn name(p: &mut Parser<'_, '_>) {
    if p.at(IDENT) {
        let m = p.start();
        p.bump(IDENT);
        m.complete(p, NAME);
    } else {
        p.error_missing("expected a name");
    }
}

fn name_ref(p: &mut Parser<'_, '_>) {
    if p.at(IDENT) {
        let m = p.start();
        p.bump(IDENT);
        m.complete(p, NAME_REF);
    } else {
        p.error_missing("expected a name");
    }
}

/// `a.b.c`. A trailing dot (half-typed code) is kept, with an error. A dot followed by `*` is
/// left for the caller (wildcard imports).
fn path(p: &mut Parser<'_, '_>) {
    let m = p.start();
    name_ref(p);
    while p.at(DOT) && !p.nth_at(1, STAR) {
        p.bump(DOT);
        if p.at(IDENT) {
            name_ref(p);
        } else {
            p.error_missing("expected a name");
            break;
        }
    }
    m.complete(p, PATH);
}

// ---- Modifiers and annotations ----

fn is_modifier_kw(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        PUBLIC_KW
            | PROTECTED_KW
            | PRIVATE_KW
            | ABSTRACT_KW
            | STATIC_KW
            | FINAL_KW
            | STRICTFP_KW
            | NATIVE_KW
            | SYNCHRONIZED_KW
            | TRANSIENT_KW
            | VOLATILE_KW
            | DEFAULT_KW
    )
}

/// Can token `n` follow the contextual modifiers `sealed` / `non-sealed`?
fn modifier_may_follow(p: &Parser<'_, '_>, n: usize) -> bool {
    let kind = p.nth(n);
    is_modifier_kw(kind)
        || matches!(kind, CLASS_KW | INTERFACE_KW | AT)
        || p.nth_at_contextual_kw(n, "sealed")
        || p.nth_at_contextual_kw(n, "non")
}

fn modifiers(p: &mut Parser<'_, '_>) {
    let m = p.start();
    let mut any = false;
    loop {
        match p.current() {
            AT if !p.nth_at(1, INTERFACE_KW) => annotation(p),
            kind if is_modifier_kw(kind) => p.bump_any(),
            IDENT if p.at_contextual_kw("sealed") && modifier_may_follow(p, 1) => p.bump_any(),
            IDENT
                if p.at_contextual_kw("non")
                    && p.nth_at(1, MINUS)
                    && p.nth_at_contextual_kw(2, "sealed")
                    && p.nth_is_joint(0)
                    && p.nth_is_joint(1)
                    && modifier_may_follow(p, 3) =>
            {
                p.bump_composite(NON_SEALED_KW, 3)
            }
            _ => break,
        }
        any = true;
    }
    if any {
        m.complete(p, MODIFIER_LIST);
    } else {
        m.abandon(p);
    }
}

fn annotation(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(AT);
    if p.at(IDENT) {
        path(p);
    } else {
        p.error_missing("expected an annotation name");
    }
    if p.at(L_PAREN) {
        balanced_parens(p, ANNOTATION_ARGS);
    }
    m.complete(p, ANNOTATION);
}

/// Consumes a parenthesised token run, tracking nesting. Used for annotation arguments and record
/// components, where neither `;`, a modifier nor a type declaration keyword can legitimately
/// appear (a class literal such as `Foo.class` is the one exception). Unclosed input therefore
/// stops at any of those, at a `}` with no matching `{` inside the run, or at end of input, so a
/// half-typed construct does not swallow the declarations that follow it.
fn balanced_parens(p: &mut Parser<'_, '_>, kind: SyntaxKind) {
    let m = p.start();
    p.bump(L_PAREN);
    let mut parens = 1usize;
    let mut curlies = 0usize;
    let mut prev = L_PAREN;
    loop {
        let current = p.current();
        match current {
            EOF => {
                p.error_missing("expected ')'");
                break;
            }
            SEMICOLON if curlies == 0 => {
                p.error_missing("expected ')'");
                break;
            }
            R_CURLY if curlies == 0 => {
                p.error_missing("expected ')'");
                break;
            }
            CLASS_KW | INTERFACE_KW | ENUM_KW if prev != DOT => {
                p.error_missing("expected ')'");
                break;
            }
            kind if is_modifier_kw(kind) => {
                p.error_missing("expected ')'");
                break;
            }
            L_CURLY => {
                curlies += 1;
                p.bump_any();
            }
            R_CURLY => {
                curlies -= 1;
                p.bump_any();
            }
            L_PAREN => {
                parens += 1;
                p.bump_any();
            }
            R_PAREN => {
                parens -= 1;
                p.bump_any();
                if parens == 0 {
                    break;
                }
            }
            _ => p.bump_any(),
        }
        prev = current;
    }
    m.complete(p, kind);
}

// ---- Type declarations ----

/// If the parser is at the keyword that starts a type declaration (after the modifiers),
/// returns the kind of node it declares.
fn type_decl_kind(p: &Parser<'_, '_>) -> Option<SyntaxKind> {
    match p.current() {
        CLASS_KW => Some(CLASS_DECL),
        INTERFACE_KW => Some(INTERFACE_DECL),
        ENUM_KW => Some(ENUM_DECL),
        AT if p.nth_at(1, INTERFACE_KW) => Some(ANNOTATION_TYPE_DECL),
        IDENT
            if p.at_contextual_kw("record")
                && p.nth_at(1, IDENT)
                && matches!(p.nth(2), L_PAREN | L_ANGLE) =>
        {
            Some(RECORD_DECL)
        }
        _ => None,
    }
}

/// Parses everything of a type declaration after its modifiers.
fn type_decl_rest(p: &mut Parser<'_, '_>, kind: SyntaxKind) {
    match kind {
        CLASS_DECL => {
            p.bump(CLASS_KW);
            name(p);
            type_params_opt(p);
            if p.at(EXTENDS_KW) {
                type_clause(p, EXTENDS_KW, EXTENDS_CLAUSE);
            }
            if p.at(IMPLEMENTS_KW) {
                type_clause(p, IMPLEMENTS_KW, IMPLEMENTS_CLAUSE);
            }
            permits_opt(p);
            class_body(p, false);
        }
        INTERFACE_DECL => {
            p.bump(INTERFACE_KW);
            name(p);
            type_params_opt(p);
            if p.at(EXTENDS_KW) {
                type_clause(p, EXTENDS_KW, EXTENDS_CLAUSE);
            }
            permits_opt(p);
            class_body(p, false);
        }
        ENUM_DECL => {
            p.bump(ENUM_KW);
            name(p);
            if p.at(IMPLEMENTS_KW) {
                type_clause(p, IMPLEMENTS_KW, IMPLEMENTS_CLAUSE);
            }
            class_body(p, true);
        }
        RECORD_DECL => {
            p.bump(IDENT); // contextual keyword `record`
            name(p);
            type_params_opt(p);
            if p.at(L_PAREN) {
                balanced_parens(p, RECORD_COMPONENT_LIST);
            } else {
                p.error_missing("expected '('");
            }
            if p.at(IMPLEMENTS_KW) {
                type_clause(p, IMPLEMENTS_KW, IMPLEMENTS_CLAUSE);
            }
            class_body(p, false);
        }
        ANNOTATION_TYPE_DECL => {
            p.bump(AT);
            p.bump(INTERFACE_KW);
            name(p);
            class_body(p, false);
        }
        _ => unreachable!("not a type declaration kind: {kind:?}"),
    }
}

fn type_clause(p: &mut Parser<'_, '_>, keyword: SyntaxKind, node: SyntaxKind) {
    let m = p.start();
    p.bump(keyword);
    type_list(p);
    m.complete(p, node);
}

fn permits_opt(p: &mut Parser<'_, '_>) {
    if p.at_contextual_kw("permits") {
        let m = p.start();
        p.bump(IDENT);
        type_list(p);
        m.complete(p, PERMITS_CLAUSE);
    }
}

fn type_list(p: &mut Parser<'_, '_>) {
    type_(p);
    while p.eat(COMMA) {
        type_(p);
    }
}

/// `{ member* }`. `is_enum` suppresses the missing-`;` error for enum constants.
fn class_body(p: &mut Parser<'_, '_>, is_enum: bool) {
    if !p.at(L_CURLY) {
        p.error_missing("expected '{'");
        return;
    }
    let m = p.start();
    p.bump(L_CURLY);
    while !p.at_eof() && !p.at(R_CURLY) {
        let before = p.pos();
        member(p, is_enum);
        debug_assert!(p.pos() > before, "member made no progress");
    }
    p.expect(R_CURLY, "expected '}'");
    m.complete(p, CLASS_BODY);
}

fn member(p: &mut Parser<'_, '_>, is_enum: bool) {
    let m = p.start();
    modifiers(p);
    if let Some(kind) = type_decl_kind(p) {
        type_decl_rest(p, kind);
        m.complete(p, kind);
    } else {
        member_placeholder(p, is_enum);
        m.complete(p, MEMBER_PLACEHOLDER);
    }
}

/// Consumes one member as a run of tokens: up to and including a `;` outside any braces, or a
/// braced block that is not an initializer (`= { ... }`, `= new X() { ... }`). It never consumes
/// the `}` that closes the enclosing body.
fn member_placeholder(p: &mut Parser<'_, '_>, is_enum: bool) {
    let mut curlies = 0usize;
    let mut seen_eq = false;
    let mut consumed = false;
    let mut clean_end = false;
    loop {
        match p.current() {
            EOF => break,
            SEMICOLON if curlies == 0 => {
                p.bump_any();
                consumed = true;
                clean_end = true;
                break;
            }
            R_CURLY if curlies == 0 => break,
            L_CURLY => {
                curlies += 1;
                p.bump_any();
            }
            R_CURLY => {
                curlies -= 1;
                p.bump_any();
                if curlies == 0 && !seen_eq {
                    consumed = true;
                    clean_end = true;
                    break;
                }
            }
            EQ if curlies == 0 => {
                seen_eq = true;
                p.bump_any();
            }
            _ => p.bump_any(),
        }
        consumed = true;
    }
    if !consumed {
        p.error_missing("expected a member declaration");
    } else if !clean_end && !is_enum {
        p.error_missing("expected ';'");
    }
}

// ---- Types ----

/// Parses a type. Returns false (after reporting an error) if there is none.
fn type_(p: &mut Parser<'_, '_>) -> bool {
    let mut completed = match p.current() {
        BOOLEAN_KW | BYTE_KW | SHORT_KW | INT_KW | LONG_KW | CHAR_KW | FLOAT_KW | DOUBLE_KW => {
            let m = p.start();
            p.bump_any();
            m.complete(p, PRIMITIVE_TYPE)
        }
        IDENT => path_type(p),
        _ => {
            p.error_missing("expected a type");
            return false;
        }
    };
    while p.at(L_BRACK) && p.nth_at(1, R_BRACK) {
        let m = completed.precede(p);
        p.bump(L_BRACK);
        p.bump(R_BRACK);
        completed = m.complete(p, ARRAY_TYPE);
    }
    true
}

/// `a.b.C<D>.E<F>`
fn path_type(p: &mut Parser<'_, '_>) -> super::CompletedMarker {
    let m = p.start();
    loop {
        name_ref(p);
        if p.at(L_ANGLE) {
            type_args(p);
        }
        if p.at(DOT) && p.nth_at(1, IDENT) {
            p.bump(DOT);
        } else {
            break;
        }
    }
    m.complete(p, PATH_TYPE)
}

fn type_args(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(L_ANGLE);
    while !p.at(R_ANGLE) {
        if !type_arg(p) || !p.eat(COMMA) {
            break;
        }
    }
    p.expect(R_ANGLE, "expected '>'");
    m.complete(p, TYPE_ARGS);
}

fn type_arg(p: &mut Parser<'_, '_>) -> bool {
    if p.at(QUESTION) {
        let m = p.start();
        p.bump(QUESTION);
        if p.at(EXTENDS_KW) || p.at(SUPER_KW) {
            p.bump_any();
            type_(p);
        }
        m.complete(p, WILDCARD_TYPE);
        true
    } else {
        type_(p)
    }
}

fn type_params_opt(p: &mut Parser<'_, '_>) {
    if !p.at(L_ANGLE) {
        return;
    }
    let m = p.start();
    p.bump(L_ANGLE);
    loop {
        type_param(p);
        if !p.eat(COMMA) {
            break;
        }
    }
    p.expect(R_ANGLE, "expected '>'");
    m.complete(p, TYPE_PARAMS);
}

fn type_param(p: &mut Parser<'_, '_>) {
    let m = p.start();
    name(p);
    if p.eat(EXTENDS_KW) {
        type_(p);
        while p.eat(AMP) {
            type_(p);
        }
    }
    m.complete(p, TYPE_PARAM);
}
