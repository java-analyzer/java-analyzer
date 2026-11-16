pub mod lexer;
pub mod parser;
pub mod syntax_kind;

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
