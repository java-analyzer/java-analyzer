use crate::lexer::Token;
use crate::syntax_kind::SyntaxKind;
use rowan::{GreenNode, GreenNodeBuilder, Language};

pub struct Parser<'a> {
    text: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    text_pos: usize,
    builder: GreenNodeBuilder<'static>,
}

impl<'a> Parser<'a> {
    pub fn new(text: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            text,
            tokens,
            pos: 0,
            text_pos: 0,
            builder: GreenNodeBuilder::new(),
        }
    }

    pub fn parse(mut self) -> GreenNode {
        self.builder.start_node(crate::JavaLanguage::kind_to_raw(SyntaxKind::SOURCE_FILE));

        while self.pos < self.tokens.len() {
            self.parse_class_decl();
        }

        self.builder.finish_node();
        self.builder.finish()
    }

    fn parse_class_decl(&mut self) {
        let mut is_class = false;

        // Skip leading whitespace to see if it's a class
        let mut lookahead = self.pos;
        while lookahead < self.tokens.len() && self.tokens[lookahead].kind == SyntaxKind::WHITESPACE {
            lookahead += 1;
        }

        if lookahead < self.tokens.len() && self.tokens[lookahead].kind == SyntaxKind::CLASS_KW {
            is_class = true;
        }

        if is_class {
            self.builder.start_node(crate::JavaLanguage::kind_to_raw(SyntaxKind::CLASS_DECL));

            // Eat until we run out of tokens or hit something that obviously isn't part of this class (very naive)
            // For a real parser, we would do recursive descent properly.
            // Here we just consume CLASS_KW, IDENT, L_CURLY, R_CURLY and WHITESPACE.
            let mut depth = 0;
            let mut started = false;

            while self.pos < self.tokens.len() {
                let token = &self.tokens[self.pos];

                if token.kind == SyntaxKind::L_CURLY {
                    depth += 1;
                    started = true;
                } else if token.kind == SyntaxKind::R_CURLY {
                    depth -= 1;
                }

                self.eat();

                if started && depth == 0 {
                    break;
                }
            }

            self.builder.finish_node();
        } else {
            // Error recovery / skip unknown
            self.builder.start_node(crate::JavaLanguage::kind_to_raw(SyntaxKind::ERROR));
            self.eat();
            self.builder.finish_node();
        }
    }

    fn eat(&mut self) {
        if self.pos < self.tokens.len() {
            let token = &self.tokens[self.pos];
            let text = &self.text[self.text_pos..self.text_pos + token.len];
            self.builder.token(crate::JavaLanguage::kind_to_raw(token.kind), text);
            self.text_pos += token.len;
            self.pos += 1;
        }
    }
}

pub fn parse(text: &str) -> GreenNode {
    let tokens = crate::lexer::lex(text);
    let parser = Parser::new(text, tokens);
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SyntaxNode;

    #[test]
    fn test_parse_class() {
        let source = "class Main { }";
        let green = parse(source);
        let root = SyntaxNode::new_root(green);

        assert_eq!(root.kind(), SyntaxKind::SOURCE_FILE);
        let class_decl = root.children().next().unwrap();
        assert_eq!(class_decl.kind(), SyntaxKind::CLASS_DECL);

        let children: Vec<_> = class_decl.children_with_tokens().collect();
        assert_eq!(children.len(), 7); // class, ' ', Main, ' ', {, ' ', }

        assert_eq!(children[0].kind(), SyntaxKind::CLASS_KW.into());
        assert_eq!(children[2].kind(), SyntaxKind::IDENT.into());
    }
}
