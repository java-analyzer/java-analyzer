//! Grammar rules for milestone M1 (see `docs/specs/0002-java-syntax-subset.md`): the compilation
//! unit, type declarations, modifiers and annotations, and the minimal type syntax their headers
//! need. Class members are parsed shallowly into `MEMBER_PLACEHOLDER` nodes until M2.

use super::{Marker, Parser};
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
fn class_body(p: &mut Parser<'_, '_>) {
    if !p.at(L_CURLY) {
        p.error_missing("expected '{'");
        return;
    }
    let m = p.start();
    p.bump(L_CURLY);
    members(p);
    p.expect(R_CURLY, "expected '}'");
    m.complete(p, CLASS_BODY);
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
    while p.at(AT) {
        annotation(p);
    }
    name(p);
    if p.at(L_PAREN) {
        balanced_parens(p, ARG_LIST);
    }
    if p.at(L_CURLY) {
        class_body(p);
    }
    m.complete(p, ENUM_CONSTANT);
}

fn is_primitive_kw(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        BOOLEAN_KW | BYTE_KW | SHORT_KW | INT_KW | LONG_KW | CHAR_KW | FLOAT_KW | DOUBLE_KW
    )
}

fn at_type_start(p: &Parser<'_, '_>) -> bool {
    let kind = p.current();
    kind == IDENT || kind == VOID_KW || is_primitive_kw(kind)
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

    if p.at(L_PAREN) || (p.at(IDENT) && p.nth_at(1, L_PAREN)) {
        name(p);
        param_list(p, PARAM_LIST, PARAM);
        dims(p);
        throws_opt(p);
        if p.at(DEFAULT_KW) {
            let d = p.start();
            p.bump(DEFAULT_KW);
            expr_placeholder(p, false);
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
    let named = p.at(IDENT);
    name(p);
    dims(p);
    if p.eat(EQ) {
        expr_placeholder(p, true);
    }
    m.complete(p, VARIABLE_DECLARATOR);
    named
}

fn dims(p: &mut Parser<'_, '_>) {
    while p.at(L_BRACK) && p.nth_at(1, R_BRACK) {
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
            param(p, item_kind);
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

fn at_param_start(p: &Parser<'_, '_>) -> bool {
    let kind = p.current();
    kind == IDENT || kind == AT || is_primitive_kw(kind) || is_modifier_kw(kind)
}

fn param(p: &mut Parser<'_, '_>, kind: SyntaxKind) {
    let m = p.start();
    modifiers(p);
    if type_(p) {
        p.eat(DOT3);
        name(p);
        dims(p);
    }
    m.complete(p, kind);
}

/// A method, constructor or initializer body. Statements are not parsed yet (M2b): the body is a
/// brace-balanced token run. As a recovery aid, a body that is still open when a member-only
/// modifier shows up at its top level (`public`, `private`, `protected` can never start a
/// statement) is considered unclosed, so one missing `}` does not swallow the rest of the class.
fn block(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(L_CURLY);
    let mut depth = 1usize;
    loop {
        match p.current() {
            EOF => {
                p.error_missing("expected '}'");
                break;
            }
            PUBLIC_KW | PRIVATE_KW | PROTECTED_KW if depth == 1 => {
                p.error_missing("expected '}'");
                break;
            }
            L_CURLY => {
                depth += 1;
                p.bump_any();
            }
            R_CURLY => {
                depth -= 1;
                p.bump_any();
                if depth == 0 {
                    break;
                }
            }
            _ => p.bump_any(),
        }
    }
    m.complete(p, BLOCK);
}

/// A stand-in for an expression (M3): a token run that ends at a `;` or `}` outside braces, at a
/// member-only modifier or declaration keyword outside any brackets, and, if `stop_at_comma`, at
/// a comma that is followed by what looks like another declarator (`name =`, `name ,`, ...).
fn expr_placeholder(p: &mut Parser<'_, '_>, stop_at_comma: bool) {
    let m = p.start();
    let start = p.pos();
    let (mut parens, mut curlies, mut angles) = (0usize, 0usize, 0usize);
    let mut prev = EOF;
    loop {
        let kind = p.current();
        let top_level = parens == 0 && curlies == 0;
        match kind {
            EOF => break,
            SEMICOLON | R_CURLY if curlies == 0 => break,
            COMMA
                if stop_at_comma
                    && top_level
                    && angles == 0
                    && p.nth_at(1, IDENT)
                    && matches!(p.nth(2), EQ | COMMA | SEMICOLON | L_BRACK) =>
            {
                break
            }
            PUBLIC_KW | PRIVATE_KW | PROTECTED_KW | ABSTRACT_KW | STATIC_KW if top_level => break,
            CLASS_KW | INTERFACE_KW | ENUM_KW if top_level && prev != DOT => break,
            L_CURLY => curlies += 1,
            R_CURLY => curlies -= 1,
            L_PAREN | L_BRACK => parens += 1,
            R_PAREN | R_BRACK => parens = parens.saturating_sub(1),
            L_ANGLE if prev == IDENT => angles += 1,
            R_ANGLE => angles = angles.saturating_sub(1),
            _ => {}
        }
        p.bump_any();
        prev = kind;
    }
    if p.pos() == start {
        m.abandon(p);
        p.error_missing("expected an expression");
    } else {
        m.complete(p, EXPR_PLACEHOLDER);
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
