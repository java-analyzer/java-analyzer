//! All token and node kinds of the Java CST. See `docs/specs/0002-java-syntax-subset.md`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(non_camel_case_types)]
#[repr(u16)]
pub enum SyntaxKind {
    // ---- Tokens: trivia ----
    WHITESPACE,
    LINE_COMMENT,
    BLOCK_COMMENT,

    // ---- Tokens: identifiers and literals ----
    IDENT,
    INT_NUMBER,
    LONG_NUMBER,
    FLOAT_NUMBER,
    DOUBLE_NUMBER,
    CHAR_LITERAL,
    STRING_LITERAL,
    TEXT_BLOCK,

    // ---- Tokens: reserved keywords ----
    ABSTRACT_KW,
    ASSERT_KW,
    BOOLEAN_KW,
    BREAK_KW,
    BYTE_KW,
    CASE_KW,
    CATCH_KW,
    CHAR_KW,
    CLASS_KW,
    CONST_KW,
    CONTINUE_KW,
    DEFAULT_KW,
    DO_KW,
    DOUBLE_KW,
    ELSE_KW,
    ENUM_KW,
    EXTENDS_KW,
    FINAL_KW,
    FINALLY_KW,
    FLOAT_KW,
    FOR_KW,
    GOTO_KW,
    IF_KW,
    IMPLEMENTS_KW,
    IMPORT_KW,
    INSTANCEOF_KW,
    INT_KW,
    INTERFACE_KW,
    LONG_KW,
    NATIVE_KW,
    NEW_KW,
    PACKAGE_KW,
    PRIVATE_KW,
    PROTECTED_KW,
    PUBLIC_KW,
    RETURN_KW,
    SHORT_KW,
    STATIC_KW,
    STRICTFP_KW,
    SUPER_KW,
    SWITCH_KW,
    SYNCHRONIZED_KW,
    THIS_KW,
    THROW_KW,
    THROWS_KW,
    TRANSIENT_KW,
    TRY_KW,
    VOID_KW,
    VOLATILE_KW,
    WHILE_KW,
    UNDERSCORE_KW,

    // ---- Tokens: literal keywords ----
    TRUE_KW,
    FALSE_KW,
    NULL_KW,

    // ---- Tokens: separators ----
    L_PAREN,
    R_PAREN,
    L_CURLY,
    R_CURLY,
    L_BRACK,
    R_BRACK,
    SEMICOLON,
    COMMA,
    DOT,
    DOT3,
    AT,
    COLON2,

    // ---- Tokens: operators ----
    EQ,
    L_ANGLE,
    R_ANGLE,
    BANG,
    TILDE,
    QUESTION,
    COLON,
    ARROW,
    EQ2,
    LTEQ,
    NEQ,
    AMP2,
    PIPE2,
    PLUS2,
    MINUS2,
    PLUS,
    MINUS,
    STAR,
    SLASH,
    AMP,
    PIPE,
    CARET,
    PERCENT,
    SHL,
    PLUSEQ,
    MINUSEQ,
    STAREQ,
    SLASHEQ,
    AMPEQ,
    PIPEEQ,
    CARETEQ,
    PERCENTEQ,
    SHLEQ,

    // ---- Tokens: composite, produced by the parser by merging adjacent lexer tokens ----
    GTEQ,
    SHR,
    USHR,
    SHREQ,
    USHREQ,
    NON_SEALED_KW,

    // ---- Token or node: input the parser could not place ----
    ERROR,

    /// Parser-only pseudo token meaning "past the end of input". Never appears in a tree.
    EOF,

    // ---- Nodes: compilation unit ----
    SOURCE_FILE,
    PACKAGE_DECL,
    IMPORT_DECL,

    // ---- Nodes: names and paths ----
    NAME,
    NAME_REF,
    PATH,

    // ---- Nodes: modifiers and annotations ----
    MODIFIER_LIST,
    ANNOTATION,
    ANNOTATION_ARGS,

    // ---- Nodes: type declarations ----
    CLASS_DECL,
    INTERFACE_DECL,
    ENUM_DECL,
    RECORD_DECL,
    ANNOTATION_TYPE_DECL,
    CLASS_BODY,
    RECORD_COMPONENT_LIST,
    RECORD_COMPONENT,
    EXTENDS_CLAUSE,
    IMPLEMENTS_CLAUSE,
    PERMITS_CLAUSE,
    ENUM_BODY,
    ENUM_CONSTANT,

    // ---- Nodes: members ----
    FIELD_DECL,
    VARIABLE_DECLARATOR,
    METHOD_DECL,
    CONSTRUCTOR_DECL,
    INITIALIZER,
    PARAM_LIST,
    PARAM,
    THROWS_CLAUSE,
    DEFAULT_VALUE,

    // ---- Nodes: expressions ----
    LITERAL,
    NAME_REF_EXPR,
    FIELD_ACCESS_EXPR,
    /// `receiver . TYPE_ARGS? NAME_REF ARG_LIST`
    METHOD_CALL_EXPR,
    /// `callee ARG_LIST`, for unqualified calls such as `foo(x)`, `this(x)` and `super(x)`.
    CALL_EXPR,
    NEW_EXPR,
    ARRAY_INIT,
    ARRAY_ACCESS_EXPR,
    UNARY_EXPR,
    POSTFIX_EXPR,
    BINARY_EXPR,
    ASSIGN_EXPR,
    TERNARY_EXPR,
    CAST_EXPR,
    INSTANCEOF_EXPR,
    LAMBDA_EXPR,
    LAMBDA_PARAMS,
    METHOD_REF_EXPR,
    PAREN_EXPR,
    THIS_EXPR,
    SUPER_EXPR,
    SWITCH_EXPR,
    CLASS_LITERAL,
    TYPE_PATTERN,
    RECORD_PATTERN,
    UNNAMED_PATTERN,
    GUARD,

    // ---- Nodes: statements ----
    BLOCK,
    LOCAL_VAR_DECL,
    EXPR_STMT,
    EMPTY_STMT,
    IF_STMT,
    WHILE_STMT,
    DO_STMT,
    FOR_STMT,
    FOREACH_STMT,
    RETURN_STMT,
    BREAK_STMT,
    CONTINUE_STMT,
    THROW_STMT,
    YIELD_STMT,
    ASSERT_STMT,
    SYNCHRONIZED_STMT,
    LABELED_STMT,
    TRY_STMT,
    RESOURCE_LIST,
    RESOURCE,
    CATCH_CLAUSE,
    FINALLY_CLAUSE,
    SWITCH_STMT,
    SWITCH_BODY,
    SWITCH_CASE,
    SWITCH_LABEL,
    /// Enum constant arguments for now; becomes the real argument list in M3.
    ARG_LIST,

    // ---- Nodes: types ----
    TYPE_PARAMS,
    TYPE_PARAM,
    TYPE_ARGS,
    PATH_TYPE,
    PRIMITIVE_TYPE,
    ARRAY_TYPE,
    WILDCARD_TYPE,

    #[doc(hidden)]
    __LAST,
}

impl SyntaxKind {
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            SyntaxKind::WHITESPACE | SyntaxKind::LINE_COMMENT | SyntaxKind::BLOCK_COMMENT
        )
    }

    /// Returns the keyword kind for `text`, if it is a reserved keyword, literal keyword or `_`.
    /// Contextual keywords (`var`, `record`, ...) are intentionally not handled here.
    pub fn from_keyword(text: &str) -> Option<SyntaxKind> {
        use SyntaxKind::*;
        let kind = match text {
            "abstract" => ABSTRACT_KW,
            "assert" => ASSERT_KW,
            "boolean" => BOOLEAN_KW,
            "break" => BREAK_KW,
            "byte" => BYTE_KW,
            "case" => CASE_KW,
            "catch" => CATCH_KW,
            "char" => CHAR_KW,
            "class" => CLASS_KW,
            "const" => CONST_KW,
            "continue" => CONTINUE_KW,
            "default" => DEFAULT_KW,
            "do" => DO_KW,
            "double" => DOUBLE_KW,
            "else" => ELSE_KW,
            "enum" => ENUM_KW,
            "extends" => EXTENDS_KW,
            "final" => FINAL_KW,
            "finally" => FINALLY_KW,
            "float" => FLOAT_KW,
            "for" => FOR_KW,
            "goto" => GOTO_KW,
            "if" => IF_KW,
            "implements" => IMPLEMENTS_KW,
            "import" => IMPORT_KW,
            "instanceof" => INSTANCEOF_KW,
            "int" => INT_KW,
            "interface" => INTERFACE_KW,
            "long" => LONG_KW,
            "native" => NATIVE_KW,
            "new" => NEW_KW,
            "package" => PACKAGE_KW,
            "private" => PRIVATE_KW,
            "protected" => PROTECTED_KW,
            "public" => PUBLIC_KW,
            "return" => RETURN_KW,
            "short" => SHORT_KW,
            "static" => STATIC_KW,
            "strictfp" => STRICTFP_KW,
            "super" => SUPER_KW,
            "switch" => SWITCH_KW,
            "synchronized" => SYNCHRONIZED_KW,
            "this" => THIS_KW,
            "throw" => THROW_KW,
            "throws" => THROWS_KW,
            "transient" => TRANSIENT_KW,
            "try" => TRY_KW,
            "void" => VOID_KW,
            "volatile" => VOLATILE_KW,
            "while" => WHILE_KW,
            "_" => UNDERSCORE_KW,
            "true" => TRUE_KW,
            "false" => FALSE_KW,
            "null" => NULL_KW,
            _ => return None,
        };
        Some(kind)
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}
