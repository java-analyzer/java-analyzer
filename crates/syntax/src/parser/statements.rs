//! Statement grammar (milestone M2b, see `docs/specs/0002-java-syntax-subset.md`).

use super::{
    expressions::{expr, is_pattern_ahead, pattern},
    grammar::{
        binding_name, dims, is_modifier_kw, is_primitive_kw, local_variable_declarator, modifiers,
        name, name_ref, type_, type_decl_kind, type_decl_rest,
    },
    lookahead::{skip_parens, skip_type, MAX_SCAN},
    CompletedMarker, Parser,
};
use crate::{
    language_level::SWITCH_EXPRESSIONS,
    syntax_kind::SyntaxKind::{self, *},
};

/// `{ statement* }`
///
/// Recovery: a block that is still open when `public`, `private` or `protected` shows up where a
/// statement should start is considered unclosed (those can never start a statement), so one
/// missing `}` does not swallow the rest of the class. Every enclosing block that is still open
/// reports its own missing `}`.
pub(super) fn block(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(L_CURLY);
    statements(p, |_| false);
    p.expect(R_CURLY, "expected '}'");
    m.complete(p, BLOCK);
}

/// Parses statements until `}`, end of input, a member-only modifier, or whatever `stop` says.
fn statements(p: &mut Parser<'_, '_>, stop: fn(&Parser<'_, '_>) -> bool) {
    while !p.at_eof() && !p.at(R_CURLY) && !at_member_only_modifier(p) && !stop(p) {
        let before = p.pos();
        statement(p);
        if p.pos() == before {
            // Every statement rule consumes something; this keeps a grammar bug from looping.
            debug_assert!(false, "statement made no progress at {:?}", p.current());
            let m = p.start();
            p.error_unexpected("unexpected token");
            p.bump_any();
            m.complete(p, ERROR);
        }
    }
}

fn at_member_only_modifier(p: &Parser<'_, '_>) -> bool {
    matches!(p.current(), PUBLIC_KW | PRIVATE_KW | PROTECTED_KW)
}

fn statement(p: &mut Parser<'_, '_>) {
    let parsed = p.nested(false, |p| {
        statement_inner(p);
        true
    });
    if !parsed {
        // Nested too deeply: consume one token so the caller makes progress.
        let m = p.start();
        p.bump_any();
        m.complete(p, ERROR);
    }
}

fn statement_inner(p: &mut Parser<'_, '_>) {
    match p.current() {
        L_CURLY => block(p),
        SEMICOLON => {
            let m = p.start();
            p.bump(SEMICOLON);
            m.complete(p, EMPTY_STMT);
        }
        IF_KW => if_stmt(p),
        WHILE_KW => while_stmt(p),
        DO_KW => do_stmt(p),
        FOR_KW => for_stmt(p),
        RETURN_KW => return_stmt(p),
        BREAK_KW => jump_stmt(p, BREAK_STMT),
        CONTINUE_KW => jump_stmt(p, CONTINUE_STMT),
        THROW_KW => keyword_expr_stmt(p, THROW_STMT),
        ASSERT_KW => assert_stmt(p),
        TRY_KW => try_stmt(p),
        SWITCH_KW => switch_stmt(p),
        SYNCHRONIZED_KW if p.nth_at(1, L_PAREN) => synchronized_stmt(p),
        ELSE_KW | CATCH_KW | FINALLY_KW | CASE_KW | DEFAULT_KW => stray_keyword(p),
        kind if is_modifier_kw(kind) || kind == AT => declaration_stmt(p),
        IDENT if is_yield_stmt(p) => keyword_expr_stmt(p, YIELD_STMT),
        IDENT if p.nth_at(1, COLON) => labeled_stmt(p),
        _ if type_decl_kind(p).is_some() || is_local_var_decl(p) || is_bare_primitive(p) => {
            declaration_stmt(p)
        }
        _ => expr_stmt(p),
    }
}

// ---- Declarations ----

/// A local variable or local class/interface/enum/record declaration, with its modifiers.
fn declaration_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    if is_modifier_kw(p.current()) || p.at(AT) {
        modifiers(p);
    }
    if let Some(kind) = type_decl_kind(p) {
        type_decl_rest(p, kind);
        m.complete(p, kind);
    } else {
        local_var_decl_rest(p, true);
        m.complete(p, LOCAL_VAR_DECL);
    }
}

/// `Type Declarator (',' Declarator)* ';'?`, after the modifiers. `semicolon` says whether the
/// declaration owns its `;` (it does not in a `for` header).
fn local_var_decl_rest(p: &mut Parser<'_, '_>, semicolon: bool) {
    if !type_(p) {
        if semicolon {
            p.eat(SEMICOLON);
        }
        return;
    }
    let mut named = local_variable_declarator(p);
    while p.eat(COMMA) {
        named &= local_variable_declarator(p);
    }
    // A missing name was already reported; do not pile a second error on top of it.
    if semicolon && !p.eat(SEMICOLON) && named {
        p.error_missing("expected ';'");
    }
}

/// Is there a type at the start of the statement that is directly followed by an identifier or `_`?
/// Nothing but a declaration can look like that (`Foo x`, `List<String> xs`, `int[] a`, `var _`).
fn is_local_var_decl(p: &Parser<'_, '_>) -> bool {
    matches!(skip_type(p, 0), Some(n) if matches!(p.nth(n), IDENT | UNDERSCORE_KW))
}

/// A primitive type keyword that is not the start of `int.class`, `int[].class` or `int[]::new`
/// can only be a declaration whose name is missing (`int ;`).
fn is_bare_primitive(p: &Parser<'_, '_>) -> bool {
    is_primitive_kw(p.current()) && !matches!(p.nth(1), DOT | L_BRACK | COLON2)
}

/// `yield` is a contextual keyword: it starts a yield statement unless it is clearly used as a
/// variable (`yield = 1;`, `yield.x();`, `yield++;`, ...). At language levels where switch
/// expressions are not available, `yield(...)` is a method call rather than a yield statement.
fn is_yield_stmt(p: &Parser<'_, '_>) -> bool {
    p.at_contextual_kw("yield")
        && !(p.nth_at(1, L_PAREN) && !p.level.is_available(&SWITCH_EXPRESSIONS))
        && !matches!(
            p.nth(1),
            EQ | DOT
                | L_BRACK
                | PLUS2
                | MINUS2
                | PLUSEQ
                | MINUSEQ
                | STAREQ
                | SLASHEQ
                | AMPEQ
                | PIPEEQ
                | CARETEQ
                | PERCENTEQ
                | SHLEQ
                | COLON
                | SEMICOLON
                | R_CURLY
                | EOF
        )
}

// ---- Simple statements ----

fn expr_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    let start = p.pos();
    expr(p);
    if p.pos() == start {
        // Not an expression (already reported): skip to the end of the statement. A `{` is
        // left for the next statement, so that its braces stay balanced.
        while !p.at_eof()
            && !matches!(p.current(), SEMICOLON | R_CURLY | L_CURLY)
            && !at_member_only_modifier(p)
        {
            p.bump_any();
        }
        p.eat(SEMICOLON);
        m.complete(p, ERROR);
        return;
    }
    p.expect(SEMICOLON, "expected ';'");
    m.complete(p, EXPR_STMT);
}

/// `throw expr;` and `yield expr;`: a keyword, an expression and a semicolon.
fn keyword_expr_stmt(p: &mut Parser<'_, '_>, kind: SyntaxKind) {
    let m = p.start();
    p.bump_any();
    expr(p);
    p.expect(SEMICOLON, "expected ';'");
    m.complete(p, kind);
}

fn return_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(RETURN_KW);
    if !p.at(SEMICOLON) && !p.at(R_CURLY) && !p.at_eof() {
        expr(p);
    }
    p.expect(SEMICOLON, "expected ';'");
    m.complete(p, RETURN_STMT);
}

/// `break label? ;` and `continue label? ;`
fn jump_stmt(p: &mut Parser<'_, '_>, kind: SyntaxKind) {
    let m = p.start();
    p.bump_any();
    if p.at(IDENT) {
        name_ref(p);
    }
    p.expect(SEMICOLON, "expected ';'");
    m.complete(p, kind);
}

fn assert_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(ASSERT_KW);
    expr(p);
    if p.eat(COLON) {
        expr(p);
    }
    p.expect(SEMICOLON, "expected ';'");
    m.complete(p, ASSERT_STMT);
}

fn labeled_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    name(p);
    p.bump(COLON);
    body_stmt(p);
    m.complete(p, LABELED_STMT);
}

/// `else`, `catch`, `finally`, `case` or `default` where a statement should start.
fn stray_keyword(p: &mut Parser<'_, '_>) {
    let m = p.start();
    let message = format!("unexpected '{}'", p.nth_text(0));
    p.error_unexpected(&message);
    p.bump_any();
    m.complete(p, ERROR);
}

// ---- Compound statements ----

/// The statement a control-flow construct governs. Reports a missing one when the enclosing
/// block ends right there.
fn body_stmt(p: &mut Parser<'_, '_>) {
    if p.at_eof() || p.at(R_CURLY) || at_member_only_modifier(p) {
        p.error_missing("expected a statement");
    } else {
        statement(p);
    }
}

fn block_or_error(p: &mut Parser<'_, '_>) {
    if p.at(L_CURLY) {
        block(p);
    } else {
        p.error_missing("expected '{'");
    }
}

/// `( expr )`
fn paren_cond(p: &mut Parser<'_, '_>) {
    if !p.expect(L_PAREN, "expected '('") {
        return;
    }
    if p.at(R_PAREN) {
        p.error_missing("expected an expression");
    } else {
        expr(p);
    }
    p.expect(R_PAREN, "expected ')'");
}

fn if_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(IF_KW);
    paren_cond(p);
    body_stmt(p);
    if p.eat(ELSE_KW) {
        body_stmt(p);
    }
    m.complete(p, IF_STMT);
}

fn while_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(WHILE_KW);
    paren_cond(p);
    body_stmt(p);
    m.complete(p, WHILE_STMT);
}

fn do_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(DO_KW);
    body_stmt(p);
    if p.eat(WHILE_KW) {
        paren_cond(p);
    } else {
        p.error_missing("expected 'while'");
    }
    p.expect(SEMICOLON, "expected ';'");
    m.complete(p, DO_STMT);
}

fn synchronized_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(SYNCHRONIZED_KW);
    paren_cond(p);
    block_or_error(p);
    m.complete(p, SYNCHRONIZED_STMT);
}

fn for_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(FOR_KW);
    let mut kind = FOR_STMT;
    if p.expect(L_PAREN, "expected '('") {
        if is_foreach_header(p) {
            kind = FOREACH_STMT;
            modifiers(p);
            type_(p);
            binding_name(p);
            // The lookahead saw `Type name :`, but a malformed type can leave the real parse
            // somewhere else, so the colon is not guaranteed.
            p.expect(COLON, "expected ':'");
            expr(p);
        } else {
            // Initializer: a declaration or expressions.
            if !p.at(SEMICOLON) {
                if is_modifier_kw(p.current()) || p.at(AT) || is_local_var_decl(p) {
                    let d = p.start();
                    modifiers(p);
                    local_var_decl_rest(p, false);
                    d.complete(p, LOCAL_VAR_DECL);
                } else {
                    expr_list(p);
                }
            }
            p.expect(SEMICOLON, "expected ';'");
            // Condition.
            if !p.at(SEMICOLON) && !p.at(R_PAREN) {
                expr(p);
            }
            p.expect(SEMICOLON, "expected ';'");
            // Update.
            if !p.at(R_PAREN) {
                expr_list(p);
            }
        }
        p.expect(R_PAREN, "expected ')'");
    }
    body_stmt(p);
    m.complete(p, kind);
}

/// `expr (',' expr)*`
fn expr_list(p: &mut Parser<'_, '_>) {
    expr(p);
    while p.eat(COMMA) {
        expr(p);
    }
}

/// `[final|@Anno]* Type (name | '_') :` — the header of an enhanced `for`.
fn is_foreach_header(p: &Parser<'_, '_>) -> bool {
    let mut n = 0;
    loop {
        if n > MAX_SCAN {
            return false;
        }
        match p.nth(n) {
            FINAL_KW => n += 1,
            AT if p.nth_at(n + 1, IDENT) => {
                n += 2;
                while p.nth_at(n, DOT) && p.nth_at(n + 1, IDENT) {
                    n += 2;
                    if n > MAX_SCAN {
                        return false;
                    }
                }
                if p.nth_at(n, L_PAREN) {
                    match skip_parens(p, n) {
                        Some(after) => n = after,
                        None => return false,
                    }
                }
            }
            _ => break,
        }
    }
    match skip_type(p, n) {
        Some(n) => matches!(p.nth(n), IDENT | UNDERSCORE_KW) && p.nth_at(n + 1, COLON),
        None => false,
    }
}

// ---- try ----

fn try_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(TRY_KW);
    let has_resources = p.at(L_PAREN);
    if has_resources {
        resource_list(p);
    }
    block_or_error(p);
    let mut has_handler = false;
    while p.at(CATCH_KW) {
        catch_clause(p);
        has_handler = true;
    }
    if p.at(FINALLY_KW) {
        let f = p.start();
        p.bump(FINALLY_KW);
        block_or_error(p);
        f.complete(p, FINALLY_CLAUSE);
        has_handler = true;
    }
    if !has_handler && !has_resources {
        p.error_missing("expected 'catch' or 'finally'");
    }
    m.complete(p, TRY_STMT);
}

/// `( resource (';' resource)* ';'? )`
fn resource_list(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(L_PAREN);
    while !p.at_eof() && !matches!(p.current(), R_PAREN | L_CURLY | R_CURLY) {
        resource(p);
        if !p.eat(SEMICOLON) {
            break;
        }
    }
    p.expect(R_PAREN, "expected ')'");
    m.complete(p, RESOURCE_LIST);
}

/// Either a declaration (`Type (name | '_') = expr`) or an expression naming an effectively final
/// variable.
fn resource(p: &mut Parser<'_, '_>) {
    let m = p.start();
    if is_modifier_kw(p.current()) || p.at(AT) || is_local_var_decl(p) {
        modifiers(p);
        if type_(p) {
            binding_name(p);
            if p.eat(EQ) {
                expr(p);
            } else {
                p.error_missing("expected '='");
            }
        }
    } else {
        expr(p);
    }
    m.complete(p, RESOURCE);
}

fn catch_clause(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(CATCH_KW);
    if p.expect(L_PAREN, "expected '('") {
        modifiers(p);
        type_(p);
        while p.eat(PIPE) {
            type_(p);
        }
        binding_name(p);
        dims(p);
        p.expect(R_PAREN, "expected ')'");
    }
    block_or_error(p);
    m.complete(p, CATCH_CLAUSE);
}

// ---- switch ----

fn switch_stmt(p: &mut Parser<'_, '_>) {
    let m = p.start();
    switch_rest(p);
    m.complete(p, SWITCH_STMT);
}

/// A `switch` in expression position.
pub(super) fn switch_expr(p: &mut Parser<'_, '_>) -> CompletedMarker {
    let m = p.start();
    switch_rest(p);
    m.complete(p, SWITCH_EXPR)
}

/// `switch ( expr ) { cases }`, shared by the statement and the expression form.
fn switch_rest(p: &mut Parser<'_, '_>) {
    p.bump(SWITCH_KW);
    paren_cond(p);
    if p.at(L_CURLY) {
        switch_body(p);
    } else {
        p.error_missing("expected '{'");
    }
}

fn at_switch_label(p: &Parser<'_, '_>) -> bool {
    p.at(CASE_KW) || (p.at(DEFAULT_KW) && matches!(p.nth(1), COLON | ARROW))
}

fn switch_body(p: &mut Parser<'_, '_>) {
    let m = p.start();
    p.bump(L_CURLY);
    while !p.at_eof() && !p.at(R_CURLY) && !at_member_only_modifier(p) {
        if at_switch_label(p) {
            switch_case(p);
        } else {
            // A statement before the first label (or after a block that ended a case).
            p.error_unexpected("expected 'case' or 'default'");
            statements(p, at_switch_label);
        }
    }
    p.expect(R_CURLY, "expected '}'");
    m.complete(p, SWITCH_BODY);
}

/// `case a, b:` / `default:` followed by statements, or `case a ->` / `default ->` followed by
/// a single expression statement, block or `throw`.
fn switch_case(p: &mut Parser<'_, '_>) {
    let m = p.start();
    let label = p.start();
    if p.eat(CASE_KW) {
        case_items(p);
    } else {
        p.bump(DEFAULT_KW);
    }
    let arrow = p.eat(ARROW);
    if !arrow && !p.eat(COLON) {
        p.error_missing("expected ':' or '->'");
    }
    label.complete(p, SWITCH_LABEL);

    if arrow {
        if p.at_eof() || p.at(R_CURLY) || at_member_only_modifier(p) || at_switch_label(p) {
            p.error_missing("expected a statement");
        } else {
            let parsed = p.nested(false, |p| {
                match p.current() {
                    L_CURLY => block(p),
                    THROW_KW => keyword_expr_stmt(p, THROW_STMT),
                    _ => expr_stmt(p),
                }
                true
            });
            if !parsed {
                let err = p.start();
                p.bump_any();
                err.complete(p, ERROR);
            }
        }
    } else {
        statements(p, at_switch_label);
    }
    m.complete(p, SWITCH_CASE);
}

/// `item (',' item)* guard?` after `case`, where an item is a pattern, an expression or
/// `default`. Lambdas are disabled so that `case FOO ->` is a rule and not a lambda.
fn case_items(p: &mut Parser<'_, '_>) {
    let outer = p.no_lambda;
    p.no_lambda = true;
    loop {
        if p.at(DEFAULT_KW) {
            p.bump_any();
        } else if is_pattern_ahead(p) {
            pattern(p);
        } else {
            expr(p);
        }
        if !p.eat(COMMA) {
            break;
        }
    }
    if p.at_contextual_kw("when") {
        let g = p.start();
        p.bump(IDENT);
        expr(p);
        g.complete(p, GUARD);
    }
    p.no_lambda = outer;
}
