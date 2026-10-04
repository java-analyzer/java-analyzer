use crate::syntax_kind::SyntaxKind;

/// A simple token produced by the lexer.
#[derive(Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub len: usize,
}

pub fn lex(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if c.is_whitespace() {
            let mut len = c.len_utf8();
            while let Some(&(_, next_c)) = chars.peek() {
                if next_c.is_whitespace() {
                    len += next_c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(Token { kind: SyntaxKind::WHITESPACE, len });
        } else if c == '{' {
            tokens.push(Token { kind: SyntaxKind::L_CURLY, len: 1 });
        } else if c == '}' {
            tokens.push(Token { kind: SyntaxKind::R_CURLY, len: 1 });
        } else if c.is_alphabetic() || c == '_' {
            let mut len = c.len_utf8();
            while let Some(&(_, next_c)) = chars.peek() {
                if next_c.is_alphanumeric() || next_c == '_' {
                    len += next_c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let text = &input[i..i + len];
            let kind = match text {
                "class" => SyntaxKind::CLASS_KW,
                _ => SyntaxKind::IDENT,
            };
            tokens.push(Token { kind, len });
        } else {
            // Very simple fallback for ERROR, capturing just one char for now
            tokens.push(Token { kind: SyntaxKind::ERROR, len: c.len_utf8() });
        }
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use SyntaxKind::*;

    #[test]
    fn test_lex_empty() {
        assert_eq!(lex(""), vec![]);
    }

    #[test]
    fn test_lex_keywords_and_symbols() {
        let input = "class { }";
        let tokens = lex(input);
        assert_eq!(
            tokens,
            vec![
                Token {
                    kind: CLASS_KW,
                    len: 5
                },
                Token {
                    kind: WHITESPACE,
                    len: 1
                },
                Token {
                    kind: L_CURLY,
                    len: 1
                },
                Token {
                    kind: WHITESPACE,
                    len: 1
                },
                Token {
                    kind: R_CURLY,
                    len: 1
                },
            ]
        );
    }

    #[test]
    fn test_lex_identifiers() {
        let input = "class Foo {}";
        let tokens = lex(input);
        assert_eq!(
            tokens,
            vec![
                Token {
                    kind: CLASS_KW,
                    len: 5
                },
                Token {
                    kind: WHITESPACE,
                    len: 1
                },
                Token {
                    kind: IDENT,
                    len: 3
                },
                Token {
                    kind: WHITESPACE,
                    len: 1
                },
                Token {
                    kind: L_CURLY,
                    len: 1
                },
                Token {
                    kind: R_CURLY,
                    len: 1
                },
            ]
        );
    }
}
