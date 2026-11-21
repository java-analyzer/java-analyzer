//! Lossless, error-resilient Java parser producing a `rowan` concrete syntax tree.
//! See `docs/specs/0002-java-syntax-subset.md`.

pub mod language_level;
pub mod lexer;
mod parser;
pub mod syntax_kind;

use std::{fmt::Write as _, sync::Arc};

pub use language_level::{parse_shebang_level, Feature, LanguageLevel, Release, FEATURES};
use rowan::{NodeOrToken, WalkEvent};
pub use rowan::{TextRange, TextSize};
use syntax_kind::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum JavaLanguage {}

impl rowan::Language for JavaLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(
            raw.0 < SyntaxKind::__LAST as u16,
            "invalid raw syntax kind: {}",
            raw.0
        );
        // Safety: `SyntaxKind` is `#[repr(u16)]` with contiguous discriminants starting at 0,
        // and we just checked `raw` is below the `__LAST` sentinel.
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

pub type SyntaxNode = rowan::SyntaxNode<JavaLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<JavaLanguage>;
pub type SyntaxElement = rowan::SyntaxElement<JavaLanguage>;

/// A lexical or syntax error in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub message: String,
    pub range: TextRange,
}

/// The result of parsing a file: a green tree (cheap to clone and compare) and the errors found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    green: rowan::GreenNode,
    errors: Arc<Vec<SyntaxError>>,
}

impl Parse {
    pub fn syntax_node(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// Lexical and syntax errors, ordered by position.
    pub fn errors(&self) -> &[SyntaxError] {
        &self.errors
    }

    /// Renders the tree, one element per line, indented by depth: nodes as `KIND`, tokens as
    /// `KIND "text"`. Trivia tokens are included only if `include_trivia` is set.
    pub fn debug_dump(&self, include_trivia: bool) -> String {
        let mut out = String::new();
        let mut depth = 0usize;
        for event in self.syntax_node().preorder_with_tokens() {
            match event {
                WalkEvent::Enter(NodeOrToken::Node(node)) => {
                    let _ = writeln!(out, "{:indent$}{:?}", "", node.kind(), indent = depth * 2);
                    depth += 1;
                }
                WalkEvent::Enter(NodeOrToken::Token(token)) => {
                    if include_trivia || !token.kind().is_trivia() {
                        let _ = writeln!(
                            out,
                            "{:indent$}{:?} {:?}",
                            "",
                            token.kind(),
                            token.text(),
                            indent = depth * 2
                        );
                    }
                }
                WalkEvent::Leave(NodeOrToken::Node(_)) => depth -= 1,
                WalkEvent::Leave(NodeOrToken::Token(_)) => {}
            }
        }
        out
    }
}

/// Parses `text` at [`LanguageLevel::latest`]. Never fails or panics: malformed input yields a
/// tree plus errors, and the text of the tree always equals `text`.
pub fn parse(text: &str) -> Parse {
    parse_with_level(text, LanguageLevel::latest())
}

/// Parses `text` according to `level`. Never fails or panics: malformed input yields a tree plus
/// errors, and the text of the tree always equals `text`.
pub fn parse_with_level(text: &str, level: LanguageLevel) -> Parse {
    let (tokens, lex_errors) = lexer::lex_with_errors(text);
    let input = parser::Input::new(text, &tokens);
    let events = parser::parse(&input, level);
    let (green, mut errors) = parser::tree_builder::build(text, &tokens, events);

    let root = SyntaxNode::new_root(green.clone());
    language_level::check_feature_gates(&root, level, &mut errors);

    errors.extend(lex_errors.into_iter().map(|e| SyntaxError {
        message: e.message.to_string(),
        range: TextRange::at(
            TextSize::try_from(e.offset).expect("source text larger than 4 GiB"),
            TextSize::try_from(e.len).expect("source text larger than 4 GiB"),
        ),
    }));
    errors.sort_by_key(|e| (e.range.start(), e.range.end()));

    Parse {
        green,
        errors: Arc::new(errors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cast_to_void_class_parses_as_cast_expr() {
        let parse = parse("class T { <T> Class<T> f() { return (Class<T>) void.class; } }");
        assert!(
            parse.errors().is_empty(),
            "expected zero syntax errors, got: {:?}",
            parse.errors()
        );
        let root = parse.syntax_node();
        let cast_expr = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CAST_EXPR)
            .expect("expected a CAST_EXPR node");
        assert!(
            cast_expr
                .descendants()
                .any(|n| n.kind() == SyntaxKind::CLASS_LITERAL),
            "expected CAST_EXPR to contain a CLASS_LITERAL, tree:\n{}",
            parse.debug_dump(false)
        );
    }

    #[test]
    fn cast_onto_unnamed_lambda_parses_as_cast_expr() {
        let parse =
            parse("class T { void f() { list.add((InvalidationListener) _ -> list.remove(x)); } }");
        assert!(
            parse.errors().is_empty(),
            "expected zero syntax errors, got: {:?}",
            parse.errors()
        );
        let root = parse.syntax_node();
        let cast_expr = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::CAST_EXPR)
            .expect("expected a CAST_EXPR node");
        assert!(
            cast_expr
                .descendants()
                .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR),
            "expected CAST_EXPR to contain a LAMBDA_EXPR, tree:\n{}",
            parse.debug_dump(false)
        );
    }

    #[test]
    fn lambda_inside_switch_when_guard_parses_without_errors() {
        let parse = parse(
            "class T { void f(Object o) { switch (o) { case String s when s.chars().noneMatch(c -> ok(c)) && (s.isEmpty() || ok(s)) -> {} default -> {} } } }",
        );
        assert!(
            parse.errors().is_empty(),
            "expected zero syntax errors, got: {:?}",
            parse.errors()
        );
        let root = parse.syntax_node();
        let guard = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::GUARD)
            .expect("expected a GUARD node");
        assert!(
            guard
                .descendants()
                .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR),
            "expected GUARD to contain a LAMBDA_EXPR, tree:\n{}",
            parse.debug_dump(false)
        );
    }

    #[test]
    fn switch_when_guard_identifier_before_arrow_is_not_lambda() {
        let parse = parse(
            "class T { void f(Object o) { switch (o) { case String s when ok -> {} default -> {} } } }",
        );
        assert!(
            parse.errors().is_empty(),
            "expected zero syntax errors, got: {:?}",
            parse.errors()
        );
        let root = parse.syntax_node();
        let guard = root
            .descendants()
            .find(|n| n.kind() == SyntaxKind::GUARD)
            .expect("expected a GUARD node");
        assert!(
            guard
                .descendants()
                .any(|n| n.kind() == SyntaxKind::NAME_REF_EXPR),
            "expected GUARD to contain a NAME_REF_EXPR, tree:\n{}",
            parse.debug_dump(false)
        );
        assert!(
            !guard
                .descendants()
                .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR),
            "expected GUARD not to contain a LAMBDA_EXPR, tree:\n{}",
            parse.debug_dump(false)
        );
    }

    #[test]
    fn switch_when_guard_nested_delimiters_and_parenthesized_guard() {
        let parse = parse(
            "class T { void f(Object o) { switch (o) { case String s when (ok) -> {} case Integer i when (int) i -> {} case Double d when arr[idx(x -> x)] && ((Predicate<Double>) x -> ok(x)).test(d) -> {} default -> {} } } }",
        );
        assert!(
            parse.errors().is_empty(),
            "expected zero syntax errors, got: {:?}",
            parse.errors()
        );
        let root = parse.syntax_node();
        let guards: Vec<_> = root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::GUARD)
            .collect();
        assert_eq!(guards.len(), 3, "expected 3 GUARD nodes");
        assert!(
            !guards[0]
                .descendants()
                .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR),
            "parenthesized guard before -> must not be parsed as lambda"
        );
        assert!(
            !guards[1]
                .descendants()
                .any(|n| n.kind() == SyntaxKind::LAMBDA_EXPR),
            "cast guard before -> must not consume -> as lambda"
        );
        assert_eq!(
            guards[2]
                .descendants()
                .filter(|n| n.kind() == SyntaxKind::LAMBDA_EXPR)
                .count(),
            2,
            "expected 2 LAMBDA_EXPR nodes inside nested delimiters of guard"
        );
    }
}
