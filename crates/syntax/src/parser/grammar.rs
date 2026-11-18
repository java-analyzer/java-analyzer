//! Grammar rules for milestones M1 and M2a (see `docs/specs/0002-java-syntax-subset.md`): the
//! compilation unit, type declarations, members, modifiers and annotations, and types. Statements
//! live in `statements.rs`; expressions are token-run placeholders until M3.

use super::{
    expressions::{arg_list, initializer},
    lookahead::{skip_annotations, skip_type, skip_type_args},
    statements::block,
    CompletedMarker, Marker, Parser,
};
use crate::syntax_kind::SyntaxKind::{self, *};

pub(super) fn source_file(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.eat(SHEBANG);
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
            } else if at_module_decl(p) {
                module_decl_rest(p);
                m.complete(p, MODULE_DECL);
            } else if at_top_level_member(p) {
                type_params_opt(p);
                method_or_field(p, m);
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

/// Is the parser at a top-level field or method declaration of a compact source file (JEP 512),
/// rather than malformed top-level tokens such as `foo bar baz;`?
fn at_top_level_member(p: &Parser<'_, '_>) -> bool {
    let mut n = 0;
    if p.nth_at(n, L_ANGLE) {
        let Some(after) = skip_type_args(p, n) else {
            return false;
        };
        n = after;
    }
    let after_type = if p.nth_at(n, VOID_KW) {
        n + 1
    } else if let Some(after) = skip_type(p, n) {
        after
    } else {
        return false;
    };
    p.nth_at(after_type, IDENT)
        && matches!(
            p.nth(after_type + 1),
            L_PAREN | EQ | SEMICOLON | COMMA | L_BRACK
        )
}

/// Tokens at which skipping unexpected top level input stops.
fn at_top_level_recovery(p: &Parser<'_, '_>) -> bool {
    let kind = p.current();
    matches!(kind, SEMICOLON | IMPORT_KW | PACKAGE_KW | AT | R_CURLY)
        || is_modifier_kw(kind)
        || type_decl_kind(p).is_some()
        || at_module_decl(p)
}

fn import_decl(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(IMPORT_KW);
    if !p.eat(STATIC_KW) && p.at_contextual_kw("module") && p.nth_at(1, IDENT) {
        p.bump(IDENT);
    }
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

fn at_module_decl(p: &Parser<'_, '_>) -> bool {
    p.at_contextual_kw("module")
        || (p.at_contextual_kw("open") && p.nth_at_contextual_kw(1, "module"))
}

fn at_module_directive(p: &Parser<'_, '_>) -> bool {
    p.at(IDENT)
        && matches!(
            p.nth_text(0),
            "requires" | "exports" | "opens" | "uses" | "provides"
        )
}

fn module_decl_rest(p: &mut Parser<'_, '_>) {
    if p.at_contextual_kw("open") {
        p.bump(IDENT);
    }
    p.bump(IDENT); // contextual keyword `module`
    path(p);
    module_body(p);
}

fn module_body(p: &mut Parser<'_, '_>) {
    if !p.at(L_CURLY) {
        p.error_missing("expected '{'");
        return;
    }
    let m = p.start();
    p.bump(L_CURLY);
    while !p.at_eof() && !p.at(R_CURLY) {
        let before = p.pos();
        if at_module_directive(p) {
            module_directive(p);
        } else {
            let err = p.start();
            p.error_unexpected("expected a module directive");
            p.bump_any();
            while !p.at_eof() && !p.at(R_CURLY) && !at_module_directive(p) {
                if p.eat(SEMICOLON) {
                    break;
                }
                p.bump_any();
            }
            err.complete(p, ERROR);
        }
        debug_assert!(p.pos() > before, "module body made no progress");
    }
    p.expect(R_CURLY, "expected '}'");
    m.complete(p, MODULE_BODY);
}

fn module_directive(p: &mut Parser<'_, '_>) {
    let m = p.start();
    let kw = p.nth_text(0);
    p.bump(IDENT);
    match kw {
        "requires" => {
            while p.at(STATIC_KW)
                || (p.at_contextual_kw("transitive")
                    && (p.nth_at(1, IDENT) || p.nth_at(1, STATIC_KW)))
            {
                p.bump_any();
            }
            path(p);
        }
        "exports" | "opens" => {
            path(p);
            if p.at_contextual_kw("to") {
                p.bump(IDENT);
                path_list(p);
            }
        }
        "uses" => path(p),
        "provides" => {
            path(p);
            if p.at_contextual_kw("with") {
                p.bump(IDENT);
            } else {
                p.error_missing("expected 'with'");
            }
            path_list(p);
        }
        _ => unreachable!("unexpected module directive: {kw}"),
    }
    p.expect(SEMICOLON, "expected ';'");
    m.complete(p, MODULE_DIRECTIVE);
}

fn path_list(p: &mut Parser<'_, '_>) {
    path(p);
    while p.eat(COMMA) {
        path(p);
    }
}

// ---- Names and paths ----

pub(super) fn name(p: &mut Parser<'_, '_>) {
    if p.at_ident() {
        let m = p.start();
        p.bump_any();
        m.complete(p, NAME);
    } else {
        p.error_missing("expected a name");
    }
}

/// A variable binding site: an identifier (`NAME`, including `_` at Java 8) or an unnamed
/// variable `_` (`UNDERSCORE_KW` at Java 9+). Returns whether a binding token was present.
pub(super) fn binding_name(p: &mut Parser<'_, '_>) -> bool {
    if p.at_ident() {
        let m = p.start();
        p.bump_any();
        m.complete(p, NAME);
        true
    } else if p.at(UNDERSCORE_KW) {
        p.bump(UNDERSCORE_KW);
        true
    } else {
        p.error_missing("expected a name");
        false
    }
}

pub(super) fn name_ref(p: &mut Parser<'_, '_>) {
    if p.at_ident() {
        let m = p.start();
        p.bump_any();
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

pub(super) fn is_modifier_kw(kind: SyntaxKind) -> bool {
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

pub(super) fn modifiers(p: &mut Parser<'_, '_>) {
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

pub(super) fn annotations(p: &mut Parser<'_, '_>) -> bool {
    let mut any = false;
    while p.at(AT) && !p.nth_at(1, INTERFACE_KW) {
        annotation(p);
        any = true;
    }
    any
}

pub(super) fn annotation(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(AT);
    if p.at(IDENT) {
        path(p);
    } else {
        p.error_missing("expected an annotation name");
    }
    if p.at(L_PAREN) {
        annotation_args(p);
    }
    m.complete(p, ANNOTATION);
}

fn at_annotation_args_end(p: &Parser<'_, '_>) -> bool {
    let kind = p.current();
    matches!(
        kind,
        EOF | R_PAREN | SEMICOLON | R_CURLY | PACKAGE_KW | IMPORT_KW
    ) || is_modifier_kw(kind)
        || type_decl_kind(p).is_some()
}

/// `( (Name '=' Value | Value) (',' ...)* )`. Stops at `;`, `}`, modifiers and type declarations
/// so that an unclosed annotation argument list does not swallow what follows.
fn annotation_args(p: &mut Parser<'_, '_>) {
    p.nested((), |p| {
        let m = p.start();
        p.bump(L_PAREN);
        while !at_annotation_args_end(p) {
            let before = p.pos();
            annotation_arg(p);
            if !p.eat(COMMA) {
                if p.pos() == before && !at_annotation_args_end(p) {
                    let err = p.start();
                    p.bump_any();
                    err.complete(p, ERROR);
                    continue;
                }
                break;
            }
            if p.at(R_PAREN) {
                p.error_missing("expected an expression");
            }
        }
        p.expect(R_PAREN, "expected ')'");
        m.complete(p, ANNOTATION_ARGS);
    });
}

fn annotation_arg(p: &mut Parser<'_, '_>) {
    let m = p.start();
    let before = p.pos();
    if p.at(IDENT) && p.nth_at(1, EQ) {
        name(p);
        p.bump(EQ);
    }
    initializer(p);
    if p.pos() == before {
        m.abandon(p);
    } else {
        m.complete(p, ANNOTATION_ARG);
    }
}

// ---- Type declarations ----

/// If the parser is at the keyword that starts a type declaration (after the modifiers),
/// returns the kind of node it declares.
pub(super) fn type_decl_kind(p: &Parser<'_, '_>) -> Option<SyntaxKind> {
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
pub(super) fn type_decl_rest(p: &mut Parser<'_, '_>, kind: SyntaxKind) {
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
            class_body(p);
        }
        INTERFACE_DECL => {
            p.bump(INTERFACE_KW);
            name(p);
            type_params_opt(p);
            if p.at(EXTENDS_KW) {
                type_clause(p, EXTENDS_KW, EXTENDS_CLAUSE);
            }
            permits_opt(p);
            class_body(p);
        }
        ENUM_DECL => {
            p.bump(ENUM_KW);
            name(p);
            if p.at(IMPLEMENTS_KW) {
                type_clause(p, IMPLEMENTS_KW, IMPLEMENTS_CLAUSE);
            }
            enum_body(p);
        }
        RECORD_DECL => {
            p.bump(IDENT); // contextual keyword `record`
            name(p);
            type_params_opt(p);
            if p.at(L_PAREN) {
                param_list(p, RECORD_COMPONENT_LIST, RECORD_COMPONENT);
            } else {
                p.error_missing("expected '('");
            }
            if p.at(IMPLEMENTS_KW) {
                type_clause(p, IMPLEMENTS_KW, IMPLEMENTS_CLAUSE);
            }
            class_body(p);
        }
        ANNOTATION_TYPE_DECL => {
            p.bump(AT);
            p.bump(INTERFACE_KW);
            name(p);
            class_body(p);
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

/// `{ member* }`
pub(super) fn class_body(p: &mut Parser<'_, '_>) {
    if !p.at(L_CURLY) {
        p.error_missing("expected '{'");
        return;
    }
    p.nested((), |p| {
        let m = p.start();
        p.bump(L_CURLY);
        members(p);
        p.expect(R_CURLY, "expected '}'");
        m.complete(p, CLASS_BODY);
    })
}

fn members(p: &mut Parser<'_, '_>) {
    while !p.at_eof() && !p.at(R_CURLY) {
        let before = p.pos();
        member(p);
        debug_assert!(p.pos() > before, "member made no progress");
    }
}

/// `{ Constant (',' Constant)* ','? (';' member*)? }`
fn enum_body(p: &mut Parser<'_, '_>) {
    if !p.at(L_CURLY) {
        p.error_missing("expected '{'");
        return;
    }
    let m = p.start();
    p.bump(L_CURLY);
    loop {
        if p.at_eof() || p.at(R_CURLY) || p.at(SEMICOLON) || !(p.at(IDENT) || p.at(AT)) {
            break;
        }
        enum_constant(p);
        if p.eat(COMMA) {
            continue;
        }
        // Without a comma, only something that clearly looks like another constant continues
        // the list; anything else is the start of the members (after a missing `;`).
        if p.at(IDENT) && matches!(p.nth(1), COMMA | L_PAREN | L_CURLY | SEMICOLON | R_CURLY) {
            p.error_missing("expected ','");
            continue;
        }
        break;
    }
    if !p.at_eof() && !p.at(R_CURLY) {
        if !p.eat(SEMICOLON) {
            p.error_missing("expected ';'");
        }
        members(p);
    }
    p.expect(R_CURLY, "expected '}'");
    m.complete(p, ENUM_BODY);
}

fn enum_constant(p: &mut Parser<'_, '_>) {
    let m = p.start();
    annotations(p);
    name(p);
    if p.at(L_PAREN) {
        arg_list(p);
    }
    if p.at(L_CURLY) {
        class_body(p);
    }
    m.complete(p, ENUM_CONSTANT);
}

pub(super) fn is_primitive_kw(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        BOOLEAN_KW | BYTE_KW | SHORT_KW | INT_KW | LONG_KW | CHAR_KW | FLOAT_KW | DOUBLE_KW
    )
}

fn at_type_start(p: &Parser<'_, '_>) -> bool {
    let kind = p.current();
    kind == IDENT
        || kind == VOID_KW
        || is_primitive_kw(kind)
        || matches!(
            skip_annotations(p, 0),
            Some(n) if n > 0 && (matches!(p.nth(n), IDENT | VOID_KW) || is_primitive_kw(p.nth(n)))
        )
}

fn member(p: &mut Parser<'_, '_>) {
    if p.at(SEMICOLON) {
        p.bump_any();
        return;
    }
    let m = p.start();
    let start = p.pos();
    modifiers(p);

    if let Some(kind) = type_decl_kind(p) {
        type_decl_rest(p, kind);
        m.complete(p, kind);
        return;
    }
    if p.at(L_CURLY) {
        block(p);
        m.complete(p, INITIALIZER);
        return;
    }

    type_params_opt(p);
    if p.at(IDENT) && p.nth_at(1, L_PAREN) {
        name(p);
        param_list(p, PARAM_LIST, PARAM);
        throws_opt(p);
        body_or_semicolon(p);
        m.complete(p, CONSTRUCTOR_DECL);
    } else if p.at(IDENT) && p.nth_at(1, L_CURLY) {
        // Compact record constructor.
        name(p);
        block(p);
        m.complete(p, CONSTRUCTOR_DECL);
    } else if at_type_start(p) {
        method_or_field(p, m);
    } else if p.pos() == start {
        // Nothing recognisable: skip to a place where a member can start again.
        p.error_unexpected("expected a member declaration");
        p.bump_any();
        loop {
            match p.current() {
                EOF | R_CURLY => break,
                SEMICOLON => {
                    p.bump_any();
                    break;
                }
                kind if is_modifier_kw(kind)
                    || kind == AT
                    || kind == VOID_KW
                    || is_primitive_kw(kind)
                    || type_decl_kind(p).is_some() =>
                {
                    break
                }
                _ => p.bump_any(),
            }
        }
        m.complete(p, ERROR);
    } else {
        // Modifiers (or type parameters) with nothing after them.
        p.error_missing("expected a member declaration");
        m.complete(p, ERROR);
    }
}

fn method_or_field(p: &mut Parser<'_, '_>, m: Marker) {
    if p.at(VOID_KW) {
        let t = p.start();
        p.bump(VOID_KW);
        t.complete(p, PRIMITIVE_TYPE);
    } else {
        type_(p);
    }

    if p.at(L_PAREN) || (p.at_ident() && p.nth_at(1, L_PAREN)) {
        name(p);
        param_list(p, PARAM_LIST, PARAM);
        dims(p);
        throws_opt(p);
        if p.at(DEFAULT_KW) {
            let d = p.start();
            p.bump(DEFAULT_KW);
            initializer(p);
            d.complete(p, DEFAULT_VALUE);
        }
        body_or_semicolon(p);
        m.complete(p, METHOD_DECL);
    } else {
        let mut named = variable_declarator(p);
        while p.eat(COMMA) {
            named &= variable_declarator(p);
        }
        // A missing name was already reported; do not pile a second error on top of it.
        if !p.eat(SEMICOLON) && named {
            p.error_missing("expected ';'");
        }
        m.complete(p, FIELD_DECL);
    }
}

/// `Name ('[' ']')* ('=' Initializer)?`. Returns whether the name was present.
fn variable_declarator(p: &mut Parser<'_, '_>) -> bool {
    let m = p.start();
    let named = p.at_ident();
    name(p);
    dims(p);
    if p.eat(EQ) {
        initializer(p);
    }
    m.complete(p, VARIABLE_DECLARATOR);
    named
}

/// `(Name | '_') ('[' ']')* ('=' Initializer)?`. Returns whether the name or `_` was present.
pub(super) fn local_variable_declarator(p: &mut Parser<'_, '_>) -> bool {
    let m = p.start();
    let named = binding_name(p);
    dims(p);
    if p.eat(EQ) {
        initializer(p);
    }
    m.complete(p, VARIABLE_DECLARATOR);
    named
}

fn at_array_dim_start(p: &Parser<'_, '_>) -> bool {
    (p.at(L_BRACK) && p.nth_at(1, R_BRACK))
        || matches!(
            skip_annotations(p, 0),
            Some(n) if p.nth_at(n, L_BRACK) && p.nth_at(n + 1, R_BRACK)
        )
}

pub(super) fn dims(p: &mut Parser<'_, '_>) {
    while at_array_dim_start(p) {
        annotations(p);
        p.bump(L_BRACK);
        p.bump(R_BRACK);
    }
}

fn throws_opt(p: &mut Parser<'_, '_>) {
    if p.at(THROWS_KW) {
        let m = p.start();
        p.bump(THROWS_KW);
        type_list(p);
        m.complete(p, THROWS_CLAUSE);
    }
}

fn body_or_semicolon(p: &mut Parser<'_, '_>) {
    if p.at(L_CURLY) {
        block(p);
    } else {
        p.expect(SEMICOLON, "expected '{' or ';'");
    }
}

/// `( param (',' param)* )`, producing `list_kind` containing `item_kind` nodes. Used for method
/// and constructor parameters as well as record components.
fn param_list(p: &mut Parser<'_, '_>, list_kind: SyntaxKind, item_kind: SyntaxKind) {
    if !p.at(L_PAREN) {
        p.error_missing("expected '('");
        return;
    }
    let m = p.start();
    p.bump(L_PAREN);
    while !p.at_eof() && !matches!(p.current(), R_PAREN | L_CURLY | R_CURLY | SEMICOLON) {
        if p.at(COMMA) {
            // `f(a, , b)`: the parameter between the commas is missing.
            p.error_missing("expected a parameter");
        } else if at_param_start(p) {
            param_inner(p, item_kind, false);
        } else {
            let e = p.start();
            p.error_unexpected("expected a parameter");
            p.bump_any();
            e.complete(p, ERROR);
        }
        if p.eat(COMMA) {
            continue;
        }
        if at_param_start(p) {
            // `f(int a int b)`: keep going as if the comma was there.
            p.error_missing("expected ','");
            continue;
        }
        break;
    }
    p.expect(R_PAREN, "expected ')'");
    m.complete(p, list_kind);
}

pub(super) fn at_param_start(p: &Parser<'_, '_>) -> bool {
    let kind = p.current();
    kind == IDENT || kind == AT || is_primitive_kw(kind) || is_modifier_kw(kind)
}

pub(super) fn lambda_param(p: &mut Parser<'_, '_>) {
    param_inner(p, PARAM, true);
}

fn param_inner(p: &mut Parser<'_, '_>, kind: SyntaxKind, allow_unnamed: bool) {
    let m = p.start();
    modifiers(p);
    if type_(p) {
        if p.at(DOT3) || matches!(skip_annotations(p, 0), Some(n) if p.nth_at(n, DOT3)) {
            annotations(p);
            p.bump(DOT3);
        }
        if allow_unnamed {
            binding_name(p);
        } else {
            name(p);
        }
        dims(p);
    }
    m.complete(p, kind);
}

/// Longest chain of array dimensions (`int[][][]...`) in one type. Each dimension nests the tree
/// one level deeper, and a tree that is too deep overflows the stack when it is dropped.
const MAX_DIMS: usize = 1024;

// ---- Types ----

/// Parses a type. Returns false (after reporting an error) if there is none.
pub(super) fn type_(p: &mut Parser<'_, '_>) -> bool {
    type_marker(p).is_some()
}

/// Parses a type and returns its node, or `None` (after reporting an error) if there is none.
pub(super) fn type_marker(p: &mut Parser<'_, '_>) -> Option<CompletedMarker> {
    let m = p.start();
    let has_annos = annotations(p);
    let mut completed = match p.current() {
        BOOLEAN_KW | BYTE_KW | SHORT_KW | INT_KW | LONG_KW | CHAR_KW | FLOAT_KW | DOUBLE_KW => {
            p.bump_any();
            m.complete(p, PRIMITIVE_TYPE)
        }
        IDENT => path_type(p, m),
        _ => {
            p.error_missing("expected a type");
            if has_annos {
                m.complete(p, ERROR);
            } else {
                m.abandon(p);
            }
            return None;
        }
    };
    let mut n_dims = 0;
    while at_array_dim_start(p) {
        if n_dims >= MAX_DIMS {
            p.error_unexpected("type is too long");
            break;
        }
        n_dims += 1;
        let m = completed.precede(p);
        annotations(p);
        p.bump(L_BRACK);
        p.bump(R_BRACK);
        completed = m.complete(p, ARRAY_TYPE);
    }
    Some(completed)
}

/// `a.b.C<D>.E<F>` (with optional type-use annotations on nested segments).
fn path_type(p: &mut Parser<'_, '_>, m: Marker) -> super::CompletedMarker {
    loop {
        name_ref(p);
        if p.at(L_ANGLE) {
            type_args(p);
        }
        if p.at(DOT) && (p.nth_at(1, IDENT) || (p.nth_at(1, AT) && !p.nth_at(2, INTERFACE_KW))) {
            p.bump(DOT);
            annotations(p);
        } else {
            break;
        }
    }
    m.complete(p, PATH_TYPE)
}

pub(super) fn type_args(p: &mut Parser<'_, '_>) {
    p.nested((), |p| {
        let m = p.start();
        p.bump(L_ANGLE);
        while !p.at(R_ANGLE) {
            if !type_arg(p) || !p.eat(COMMA) {
                break;
            }
        }
        p.expect(R_ANGLE, "expected '>'");
        m.complete(p, TYPE_ARGS);
    })
}

fn type_arg(p: &mut Parser<'_, '_>) -> bool {
    if p.at(QUESTION) || matches!(skip_annotations(p, 0), Some(n) if p.nth_at(n, QUESTION)) {
        let m = p.start();
        annotations(p);
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
    annotations(p);
    name(p);
    if p.eat(EXTENDS_KW) {
        type_(p);
        while p.eat(AMP) {
            type_(p);
        }
    }
    m.complete(p, TYPE_PARAM);
}
