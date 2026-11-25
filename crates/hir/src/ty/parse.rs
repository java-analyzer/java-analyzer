//! Raw Java type-reference string parser.

use super::{Nullability, PrimitiveType, Ty, TypeArg, WildcardBound};

const MAX_PARSE_DEPTH: usize = 64;

/// Parses a raw Java type reference string into a [`Ty`], resolving non-primitive named types
/// through `resolve_type`.
pub(crate) fn parse_type_ref(
    raw: &str,
    resolve_type: &mut impl FnMut(&[&str]) -> Option<Ty>,
) -> Ty {
    let mut parser = TypeParser::new(raw);
    parser.parse_type(resolve_type, 0)
}

struct TypeParser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> TypeParser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    fn rest(&self) -> &'a str {
        self.src.get(self.pos..).unwrap_or("")
    }

    fn is_eof(&self) -> bool {
        self.pos >= self.src.len()
    }

    fn peek_byte(&self) -> Option<u8> {
        self.src.as_bytes().get(self.pos).copied()
    }

    fn starts_with(&self, pat: &str) -> bool {
        self.rest().starts_with(pat)
    }

    fn advance(&mut self, bytes: usize) {
        self.pos = (self.pos + bytes).min(self.src.len());
    }

    fn skip_ws(&mut self) {
        while let Some(ch) = self.rest().chars().next() {
            if ch.is_whitespace() {
                self.advance(ch.len_utf8());
            } else {
                break;
            }
        }
    }

    fn skip_ws_and_annotations(&mut self) {
        loop {
            self.skip_ws();
            if self.peek_byte() != Some(b'@') {
                break;
            }
            self.advance(1);
            self.skip_ws();
            // Skip qualified annotation name (`@Nullable`, `@com.example.Ann`).
            loop {
                let seg = self.read_ident();
                if seg.is_empty() {
                    break;
                }
                self.skip_ws();
                if self.peek_byte() == Some(b'.') {
                    self.advance(1);
                    self.skip_ws();
                } else {
                    break;
                }
            }
            self.skip_ws();
            if self.peek_byte() == Some(b'(') {
                self.skip_balanced_parens();
            }
        }
    }

    fn skip_balanced_parens(&mut self) {
        if self.peek_byte() != Some(b'(') {
            return;
        }
        let mut depth = 0usize;
        let mut in_string = false;
        let mut in_char = false;
        let mut escaped = false;
        while let Some(ch) = self.rest().chars().next() {
            self.advance(ch.len_utf8());
            if escaped {
                escaped = false;
                continue;
            }
            if in_string {
                if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    in_string = false;
                }
                continue;
            }
            if in_char {
                if ch == '\\' {
                    escaped = true;
                } else if ch == '\'' {
                    in_char = false;
                }
                continue;
            }
            match ch {
                '"' => in_string = true,
                '\'' => in_char = true,
                '(' => depth += 1,
                ')' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    fn read_ident(&mut self) -> &'a str {
        let start = self.pos;
        for ch in self.rest().chars() {
            if ch.is_alphanumeric() || ch == '_' || ch == '$' {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
        self.src.get(start..self.pos).unwrap_or("")
    }

    fn consume_nullability(&mut self) -> Option<Nullability> {
        match self.peek_byte() {
            Some(b'!') => {
                self.advance(1);
                Some(Nullability::NonNull)
            }
            Some(b'?') => {
                self.advance(1);
                Some(Nullability::Nullable)
            }
            _ => None,
        }
    }

    fn consume_keyword(&mut self, kw: &str) -> bool {
        let rest = self.rest();
        if let Some(after) = rest.strip_prefix(kw) {
            let boundary_ok = after
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() && c != '_' && c != '$');
            if boundary_ok {
                self.advance(kw.len());
                return true;
            }
        }
        false
    }

    fn parse_type(
        &mut self,
        resolve_type: &mut impl FnMut(&[&str]) -> Option<Ty>,
        depth: usize,
    ) -> Ty {
        if depth >= MAX_PARSE_DEPTH {
            return Ty::Unknown;
        }
        let first = self.parse_single_type(resolve_type, depth);
        self.skip_ws_and_annotations();
        if self.peek_byte() != Some(b'&') {
            return first;
        }
        let mut members = vec![first];
        while self.peek_byte() == Some(b'&') {
            self.advance(1);
            members.push(self.parse_single_type(resolve_type, depth + 1));
            self.skip_ws_and_annotations();
        }
        Ty::Intersection(members)
    }

    fn parse_single_type(
        &mut self,
        resolve_type: &mut impl FnMut(&[&str]) -> Option<Ty>,
        depth: usize,
    ) -> Ty {
        if depth >= MAX_PARSE_DEPTH {
            return Ty::Unknown;
        }
        self.skip_ws_and_annotations();
        if self.is_eof() || self.peek_byte() == Some(b'?') {
            return Ty::Unknown;
        }

        let mut segments: Vec<&'a str> = Vec::new();
        let mut final_args: Option<Vec<TypeArg>> = None;

        loop {
            self.skip_ws_and_annotations();
            let ident = self.read_ident();
            if ident.is_empty() {
                break;
            }
            segments.push(ident);

            self.skip_ws_and_annotations();
            if self.peek_byte() == Some(b'<') {
                final_args = Some(self.parse_type_args(resolve_type, depth + 1));
            } else {
                final_args = None;
            }

            self.skip_ws_and_annotations();
            if self.starts_with(".") && !self.starts_with("...") {
                self.advance(1);
            } else {
                break;
            }
        }

        if segments.is_empty() {
            return Ty::Unknown;
        }

        let mut base = if segments.len() == 1 && final_args.is_none() {
            match segments.first().copied() {
                Some("boolean") => Ty::Primitive(PrimitiveType::Boolean),
                Some("byte") => Ty::Primitive(PrimitiveType::Byte),
                Some("short") => Ty::Primitive(PrimitiveType::Short),
                Some("int") => Ty::Primitive(PrimitiveType::Int),
                Some("long") => Ty::Primitive(PrimitiveType::Long),
                Some("char") => Ty::Primitive(PrimitiveType::Char),
                Some("float") => Ty::Primitive(PrimitiveType::Float),
                Some("double") => Ty::Primitive(PrimitiveType::Double),
                Some("void") => Ty::Void,
                Some("var") => Ty::Unknown,
                _ => resolve_type(&segments).unwrap_or(Ty::Unknown),
            }
        } else {
            match resolve_type(&segments) {
                Some(Ty::Class {
                    fqn,
                    type_args,
                    nullability,
                }) => Ty::Class {
                    fqn,
                    type_args: final_args.unwrap_or(type_args),
                    nullability,
                },
                Some(other) => other,
                None => Ty::Unknown,
            }
        };

        self.skip_ws_and_annotations();
        if let Some(n) = self.consume_nullability() {
            base = base.with_nullability(n);
        }

        let mut dims = 0usize;
        loop {
            if dims >= MAX_PARSE_DEPTH {
                break;
            }
            self.skip_ws_and_annotations();
            if self.starts_with("...") {
                self.advance(3);
                if !matches!(base, Ty::Void | Ty::Unknown) {
                    base = Ty::array(base);
                }
                self.skip_ws_and_annotations();
                if let Some(n) = self.consume_nullability() {
                    base = base.with_nullability(n);
                }
                break;
            }
            if self.peek_byte() == Some(b'[') {
                self.advance(1);
                self.skip_ws_and_annotations();
                if self.peek_byte() == Some(b']') {
                    self.advance(1);
                    dims += 1;
                    if !matches!(base, Ty::Void | Ty::Unknown) {
                        base = Ty::array(base);
                    }
                    self.skip_ws_and_annotations();
                    if let Some(n) = self.consume_nullability() {
                        base = base.with_nullability(n);
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        base
    }

    fn parse_type_args(
        &mut self,
        resolve_type: &mut impl FnMut(&[&str]) -> Option<Ty>,
        depth: usize,
    ) -> Vec<TypeArg> {
        if self.peek_byte() != Some(b'<') || depth >= MAX_PARSE_DEPTH {
            return Vec::new();
        }
        self.advance(1);
        let mut args = Vec::new();

        loop {
            self.skip_ws_and_annotations();
            if self.is_eof() || self.peek_byte() == Some(b'>') {
                break;
            }

            let arg = if self.peek_byte() == Some(b'?') {
                self.advance(1);
                self.skip_ws_and_annotations();
                if self.consume_keyword("extends") {
                    let bound = self.parse_type(resolve_type, depth + 1);
                    TypeArg::Wildcard(WildcardBound::Extends(Box::new(bound)))
                } else if self.consume_keyword("super") {
                    let bound = self.parse_type(resolve_type, depth + 1);
                    TypeArg::Wildcard(WildcardBound::Super(Box::new(bound)))
                } else {
                    TypeArg::Wildcard(WildcardBound::Unbounded)
                }
            } else {
                TypeArg::Type(self.parse_type(resolve_type, depth + 1))
            };
            args.push(arg);

            self.skip_ws_and_annotations();
            if self.peek_byte() == Some(b',') {
                self.advance(1);
            } else {
                break;
            }
        }

        if self.peek_byte() == Some(b'>') {
            self.advance(1);
        }
        args
    }
}
