//! Hand-written, context-free Java lexer. See `docs/specs/0002-java-syntax-subset.md`.
//!
//! Guarantees: lexing never fails or panics, every byte of the input belongs to exactly one
//! token, and token lengths always fall on `char` boundaries.

use crate::syntax_kind::SyntaxKind::{self, *};

/// A simple token produced by the lexer.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Token {
    pub kind: SyntaxKind,
    /// Length in bytes.
    pub len: usize,
}

/// A lexical error, located by byte offset into the input.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct LexError {
    pub message: &'static str,
    pub offset: usize,
    pub len: usize,
}

pub fn lex(input: &str) -> Vec<Token> {
    lex_with_errors(input).0
}

pub fn lex_with_errors(input: &str) -> (Vec<Token>, Vec<LexError>) {
    let mut tokens = Vec::new();
    let mut errors = Vec::new();
    let mut pos = 0;
    if input.starts_with("#!") {
        let len = input.find(['\n', '\r']).unwrap_or(input.len());
        tokens.push(Token {
            kind: SHEBANG,
            len,
        });
        pos = len;
    }
    while pos < input.len() {
        let (kind, len, error) = next_token(&input[pos..]);
        debug_assert!(len > 0 && input.is_char_boundary(pos + len));
        if let Some(message) = error {
            errors.push(LexError {
                message,
                offset: pos,
                len,
            });
        }
        tokens.push(Token { kind, len });
        pos += len;
    }
    (tokens, errors)
}

type Lexed = (SyntaxKind, usize, Option<&'static str>);

fn next_token(rest: &str) -> Lexed {
    let c = rest
        .chars()
        .next()
        .expect("next_token called on empty input");
    match c {
        c if is_whitespace(c) => {
            let len = rest.find(|c| !is_whitespace(c)).unwrap_or(rest.len());
            (WHITESPACE, len, None)
        }
        '/' if rest.starts_with("//") => {
            let len = rest.find(['\n', '\r']).unwrap_or(rest.len());
            (LINE_COMMENT, len, None)
        }
        '/' if rest.starts_with("/*") => match rest[2..].find("*/") {
            Some(i) => (BLOCK_COMMENT, i + 4, None),
            None => (
                BLOCK_COMMENT,
                rest.len(),
                Some("unterminated block comment"),
            ),
        },
        '"' if rest.starts_with("\"\"\"") => text_block(rest),
        '"' => quoted(rest, '"', STRING_LITERAL, "unterminated string literal"),
        '\'' => quoted(rest, '\'', CHAR_LITERAL, "unterminated character literal"),
        '0'..='9' => number(rest),
        '.' if rest.as_bytes().get(1).is_some_and(u8::is_ascii_digit) => number(rest),
        c if is_ident_start(c) => {
            let len = rest.find(|c| !is_ident_continue(c)).unwrap_or(rest.len());
            let kind = SyntaxKind::from_keyword(&rest[..len]).unwrap_or(IDENT);
            (kind, len, None)
        }
        c => {
            for (text, kind) in PUNCTUATION {
                if rest.starts_with(text) {
                    return (*kind, text.len(), None);
                }
            }
            (ERROR, c.len_utf8(), Some("unexpected character"))
        }
    }
}

// Java whitespace is ASCII only: space, tab, form feed and line terminators.
fn is_whitespace(c: char) -> bool {
    c.is_ascii_whitespace()
}

// Approximates `Character.isJavaIdentifierStart/Part` with Unicode alphabetic/alphanumeric.
fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Lexes a string or char literal. An unterminated literal stops before the line terminator.
fn quoted(rest: &str, quote: char, kind: SyntaxKind, unterminated: &'static str) -> Lexed {
    let mut chars = rest.char_indices().skip(1).peekable();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                if let Some(&(_, next)) = chars.peek() {
                    if next != '\n' && next != '\r' {
                        chars.next();
                    }
                }
            }
            '\n' | '\r' => return (kind, i, Some(unterminated)),
            c if c == quote => return (kind, i + 1, None),
            _ => {}
        }
    }
    (kind, rest.len(), Some(unterminated))
}

/// Lexes a text block (`""" ... """`), checking the opening line terminator and escape sequences.
/// An unterminated text block extends to the end of input.
fn text_block(rest: &str) -> Lexed {
    let bytes = rest.as_bytes();
    let mut i = 3;

    // JLS 3.10.7: the opening `"""` must be followed by optional horizontal whitespace and a line
    // terminator before the content (or closing delimiter) begins.
    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | 0x0c) {
        i += 1;
    }
    let mut error: Option<&'static str> = None;
    match bytes.get(i) {
        Some(b'\n') => i += 1,
        Some(b'\r') => {
            i += 1;
            if bytes.get(i) == Some(&b'\n') {
                i += 1;
            }
        }
        Some(_) => error = Some("expected line terminator after '\"\"\"'"),
        None => return (TEXT_BLOCK, rest.len(), Some("unterminated text block")),
    }

    while i < bytes.len() {
        match bytes[i] {
            b'"' if rest[i..].starts_with("\"\"\"") => return (TEXT_BLOCK, i + 3, error),
            b'\\' => {
                i += 1;
                let Some(c) = rest[i..].chars().next() else {
                    return (TEXT_BLOCK, rest.len(), Some("unterminated text block"));
                };
                match c {
                    'b' | 't' | 'n' | 'f' | 'r' | 's' | '"' | '\'' | '\\' | '0'..='7' | '\n' => {
                        i += 1;
                    }
                    '\r' => {
                        i += 1;
                        if bytes.get(i) == Some(&b'\n') {
                            i += 1;
                        }
                    }
                    'u' => {
                        while bytes.get(i) == Some(&b'u') {
                            i += 1;
                        }
                        let hex_ok = bytes
                            .get(i..i + 4)
                            .is_some_and(|slice| slice.iter().all(u8::is_ascii_hexdigit));
                        if hex_ok {
                            i += 4;
                        } else if error.is_none() {
                            error = Some("invalid escape sequence");
                        }
                    }
                    _ => {
                        i += c.len_utf8();
                        if error.is_none() {
                            error = Some("invalid escape sequence");
                        }
                    }
                }
            }
            _ => {
                let c = rest[i..].chars().next().unwrap();
                i += c.len_utf8();
            }
        }
    }
    (TEXT_BLOCK, rest.len(), Some("unterminated text block"))
}

/// Advances over bytes matching `pred` or `_` separators.
fn skip(bytes: &[u8], mut i: usize, pred: fn(&u8) -> bool) -> usize {
    while i < bytes.len() && (pred(&bytes[i]) || bytes[i] == b'_') {
        i += 1;
    }
    i
}

fn number(rest: &str) -> Lexed {
    let b = rest.as_bytes();

    if b[0] == b'0' && matches!(b.get(1), Some(b'x' | b'X')) {
        return hex_number(b);
    }
    if b[0] == b'0' && matches!(b.get(1), Some(b'b' | b'B')) {
        let i = skip(b, 2, |c| matches!(c, b'0' | b'1'));
        let error = (i == 2).then_some("expected binary digits");
        return if matches!(b.get(i), Some(b'l' | b'L')) {
            (LONG_NUMBER, i + 1, error)
        } else {
            (INT_NUMBER, i, error)
        };
    }

    let mut i = 0;
    let mut is_float = false;
    if b[0] != b'.' {
        i = skip(b, 0, u8::is_ascii_digit);
    }
    if b.get(i) == Some(&b'.') {
        // `1.` is a valid double, but `1..` and `1.foo` must not swallow the dot.
        let take_dot = match b.get(i + 1).copied() {
            Some(d) if d.is_ascii_digit() => true,
            Some(b'.') => false,
            Some(c) if c.is_ascii_alphabetic() || c == b'_' || c == b'$' => {
                matches!(c, b'e' | b'E' | b'f' | b'F' | b'd' | b'D')
            }
            Some(c) if c >= 0x80 => false,
            _ => true,
        };
        if b[0] == b'.' || take_dot {
            is_float = true;
            i = skip(b, i + 1, u8::is_ascii_digit);
        }
    }
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if b.get(j).is_some_and(u8::is_ascii_digit) {
            is_float = true;
            i = skip(b, j, u8::is_ascii_digit);
        }
    }
    match b.get(i) {
        Some(b'f' | b'F') => (FLOAT_NUMBER, i + 1, None),
        Some(b'd' | b'D') => (DOUBLE_NUMBER, i + 1, None),
        Some(b'l' | b'L') if !is_float => (LONG_NUMBER, i + 1, None),
        _ if is_float => (DOUBLE_NUMBER, i, None),
        _ => (INT_NUMBER, i, None),
    }
}

fn hex_number(b: &[u8]) -> Lexed {
    let mut i = skip(b, 2, u8::is_ascii_hexdigit);
    let error = (i == 2).then_some("expected hexadecimal digits");
    let mut is_float = false;
    if b.get(i) == Some(&b'.') && b.get(i + 1).is_some_and(u8::is_ascii_hexdigit) {
        is_float = true;
        i = skip(b, i + 1, u8::is_ascii_hexdigit);
    }
    if matches!(b.get(i), Some(b'p' | b'P')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        if b.get(j).is_some_and(u8::is_ascii_digit) {
            is_float = true;
            i = skip(b, j, u8::is_ascii_digit);
        }
    }
    if is_float {
        return match b.get(i) {
            Some(b'f' | b'F') => (FLOAT_NUMBER, i + 1, error),
            Some(b'd' | b'D') => (DOUBLE_NUMBER, i + 1, error),
            _ => (DOUBLE_NUMBER, i, error),
        };
    }
    if matches!(b.get(i), Some(b'l' | b'L')) {
        return (LONG_NUMBER, i + 1, error);
    }
    (INT_NUMBER, i, error)
}

// Longest match first. `>` is never combined with a following `>` or `=`: the parser merges
// adjacent tokens into `>>`, `>=`, etc. so that nested generics parse without splitting.
const PUNCTUATION: &[(&str, SyntaxKind)] = &[
    ("<<=", SHLEQ),
    ("...", DOT3),
    ("<<", SHL),
    ("<=", LTEQ),
    ("==", EQ2),
    ("!=", NEQ),
    ("&&", AMP2),
    ("||", PIPE2),
    ("++", PLUS2),
    ("--", MINUS2),
    ("->", ARROW),
    ("::", COLON2),
    ("+=", PLUSEQ),
    ("-=", MINUSEQ),
    ("*=", STAREQ),
    ("/=", SLASHEQ),
    ("&=", AMPEQ),
    ("|=", PIPEEQ),
    ("^=", CARETEQ),
    ("%=", PERCENTEQ),
    ("(", L_PAREN),
    (")", R_PAREN),
    ("{", L_CURLY),
    ("}", R_CURLY),
    ("[", L_BRACK),
    ("]", R_BRACK),
    (";", SEMICOLON),
    (",", COMMA),
    (".", DOT),
    ("@", AT),
    ("=", EQ),
    ("<", L_ANGLE),
    (">", R_ANGLE),
    ("!", BANG),
    ("~", TILDE),
    ("?", QUESTION),
    (":", COLON),
    ("+", PLUS),
    ("-", MINUS),
    ("*", STAR),
    ("/", SLASH),
    ("&", AMP),
    ("|", PIPE),
    ("^", CARET),
    ("%", PERCENT),
];

// The bulk of the lexer tests are text fixtures in `test_data/lexer/` (see
// `tests/lexer_fixtures.rs`). The tests below cover properties that are awkward to express there.
#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(input: &str) -> Vec<SyntaxKind> {
        lex(input).into_iter().map(|t| t.kind).collect()
    }

    /// Every byte belongs to exactly one token, and tokens never split a `char`.
    fn assert_lossless(input: &str) {
        let mut offset = 0;
        for token in lex(input) {
            assert!(token.len > 0, "empty token in {input:?}");
            offset += token.len;
            assert!(
                input.is_char_boundary(offset),
                "token splits a char in {input:?}"
            );
        }
        assert_eq!(offset, input.len(), "tokens do not cover {input:?}");
    }

    #[test]
    fn empty() {
        assert_eq!(lex(""), vec![]);
    }

    #[test]
    fn greater_than_never_combines() {
        assert_eq!(kinds(">>"), vec![R_ANGLE, R_ANGLE]);
        assert_eq!(kinds(">>>"), vec![R_ANGLE, R_ANGLE, R_ANGLE]);
        assert_eq!(kinds(">="), vec![R_ANGLE, EQ]);
        assert_eq!(kinds(">>="), vec![R_ANGLE, R_ANGLE, EQ]);
        assert_eq!(kinds(">>>="), vec![R_ANGLE, R_ANGLE, R_ANGLE, EQ]);
    }

    #[test]
    fn non_sealed_is_three_tokens() {
        assert_eq!(kinds("non-sealed"), vec![IDENT, MINUS, IDENT]);
    }

    #[test]
    fn lossless_on_tricky_inputs() {
        for input in [
            "",
            "\"",
            "'",
            "\"\"\"",
            "/*",
            "/**/",
            "//",
            "\\",
            "\\u0041",
            "0x",
            "1e+",
            ".",
            "..",
            "1..",
            "é😀 \u{a0} \u{2028}",
            "\"é\\",
            "'\\",
            "\"\"\"\\",
            "\r\n\r\n",
            "\0",
        ] {
            assert_lossless(input);
        }
    }

    /// Deterministic pseudo-random concatenations of Java-ish fragments must never panic and
    /// must always stay lossless.
    #[test]
    fn lossless_on_generated_inputs() {
        const FRAGMENTS: &[&str] = &[
            "class", " ", "\n", "Foo", "{", "}", "(", ")", ";", "\"", "'", "\"\"\"", "/*", "*/",
            "//", "1", "0x", "1.5e+", ".", "..", "...", ">", "<", "=", "\\", "é", "😀", "$", "_",
            "@", "::", "->", "\r", "\t", "\u{0}",
        ];
        let mut state: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..2000 {
            let len = (next() % 24) as usize;
            let input: String = (0..len)
                .map(|_| FRAGMENTS[(next() % FRAGMENTS.len() as u64) as usize])
                .collect();
            assert_lossless(&input);
        }
    }
}
