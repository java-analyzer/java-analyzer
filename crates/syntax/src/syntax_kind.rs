#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
#[repr(u16)]
pub enum SyntaxKind {
    // Tokens
    WHITESPACE,
    CLASS_KW,
    IDENT,
    L_CURLY,
    R_CURLY,
    ERROR,

    // Nodes
    SOURCE_FILE,
    CLASS_DECL,
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}
