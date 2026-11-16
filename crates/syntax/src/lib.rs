//! Lossless, error-resilient Java parser producing a `rowan` concrete syntax tree.
//! See `docs/specs/0002-java-syntax-subset.md`.

pub mod lexer;
mod parser;
pub mod syntax_kind;

use std::{fmt::Write as _, sync::Arc};

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

/// Parses `text`. Never fails or panics: malformed input yields a tree plus errors, and the
/// text of the tree always equals `text`.
pub fn parse(text: &str) -> Parse {
    let (tokens, lex_errors) = lexer::lex_with_errors(text);
    let input = parser::Input::new(text, &tokens);
    let events = parser::parse(&input);
    let (green, mut errors) = parser::tree_builder::build(text, &tokens, events);

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
