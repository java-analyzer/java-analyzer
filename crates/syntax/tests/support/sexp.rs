//! A tiny S-expression reader and canonical printer for tree fixtures.
//!
//! Syntax: lists `( ... )`, double-quoted strings with the usual escapes (`\n \t \r \0 \\ \"
//! \u{XXXX}`, i.e. Rust's `{:?}` escaping), bare atoms, and `;` comments to end of line.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sexp {
    Atom(String),
    Str(String),
    List(Vec<Sexp>),
}

impl Sexp {
    pub fn atom(text: impl Into<String>) -> Sexp {
        Sexp::Atom(text.into())
    }

    pub fn string(text: impl Into<String>) -> Sexp {
        Sexp::Str(text.into())
    }

    pub fn list(items: impl IntoIterator<Item = Sexp>) -> Sexp {
        Sexp::List(items.into_iter().collect())
    }

    /// A list is printed on a single line when it has at most one list child and that child is
    /// itself printed on a single line. Tokens and small chains such as `(PATH (NAME_REF
    /// (IDENT "a")))` stay compact; anything branching is spread over several lines.
    fn fits_on_line(&self) -> bool {
        match self {
            Sexp::List(items) => {
                let mut lists = items.iter().filter(|i| matches!(i, Sexp::List(_)));
                match (lists.next(), lists.next()) {
                    (None, _) => true,
                    (Some(only), None) => only.fits_on_line(),
                    _ => false,
                }
            }
            _ => true,
        }
    }
}

// ---- Reading ----

pub fn parse_all(text: &str) -> Result<Vec<Sexp>, String> {
    let mut reader = Reader {
        chars: text.char_indices().peekable(),
        text,
    };
    let mut forms = Vec::new();
    loop {
        reader.skip_trivia();
        match reader.chars.peek() {
            None => return Ok(forms),
            Some(&(at, ')')) => return Err(format!("unexpected ')' at {}", reader.position(at))),
            Some(_) => forms.push(reader.form()?),
        }
    }
}

struct Reader<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    text: &'a str,
}

impl Reader<'_> {
    fn position(&self, offset: usize) -> String {
        let before = &self.text[..offset];
        let line = before.matches('\n').count() + 1;
        let col = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        format!("{line}:{col}")
    }

    fn skip_trivia(&mut self) {
        while let Some(&(_, c)) = self.chars.peek() {
            if c.is_whitespace() {
                self.chars.next();
            } else if c == ';' {
                for (_, c) in self.chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
    }

    fn form(&mut self) -> Result<Sexp, String> {
        self.skip_trivia();
        let &(start, c) = self.chars.peek().ok_or("unexpected end of input")?;
        match c {
            '(' => {
                self.chars.next();
                let mut items = Vec::new();
                loop {
                    self.skip_trivia();
                    match self.chars.peek() {
                        None => {
                            return Err(format!("unclosed '(' opened at {}", self.position(start)));
                        }
                        Some(&(_, ')')) => {
                            self.chars.next();
                            return Ok(Sexp::List(items));
                        }
                        Some(_) => items.push(self.form()?),
                    }
                }
            }
            '"' => {
                self.chars.next();
                self.string(start)
            }
            _ => {
                let mut atom = String::new();
                while let Some(&(_, c)) = self.chars.peek() {
                    if c.is_whitespace() || matches!(c, '(' | ')' | '"' | ';') {
                        break;
                    }
                    atom.push(c);
                    self.chars.next();
                }
                Ok(Sexp::Atom(atom))
            }
        }
    }

    fn string(&mut self, start: usize) -> Result<Sexp, String> {
        let mut out = String::new();
        loop {
            let Some((_, c)) = self.chars.next() else {
                return Err(format!(
                    "unterminated string starting at {}",
                    self.position(start)
                ));
            };
            match c {
                '"' => return Ok(Sexp::Str(out)),
                '\\' => match self.chars.next().map(|(_, c)| c) {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('0') => out.push('\0'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some('\'') => out.push('\''),
                    Some('u') => out.push(self.unicode_escape(start)?),
                    other => {
                        return Err(format!(
                            "invalid escape \\{} in string starting at {}",
                            other.map(String::from).unwrap_or_default(),
                            self.position(start)
                        ));
                    }
                },
                c => out.push(c),
            }
        }
    }

    fn unicode_escape(&mut self, start: usize) -> Result<char, String> {
        let bad = |this: &Self| {
            format!(
                "invalid \\u escape in string starting at {}",
                this.position(start)
            )
        };
        if self.chars.next().map(|(_, c)| c) != Some('{') {
            return Err(bad(self));
        }
        let mut digits = String::new();
        loop {
            match self.chars.next().map(|(_, c)| c) {
                Some('}') => break,
                Some(c) if c.is_ascii_hexdigit() => digits.push(c),
                _ => return Err(bad(self)),
            }
        }
        u32::from_str_radix(&digits, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| bad(self))
    }
}

// ---- Printing ----

/// Canonical pretty form. A list that [fits on a line](Sexp::fits_on_line) is printed on one
/// line; otherwise a leading atom stays on the opening line and every other child goes on its own
/// line, indented by two.
pub fn print(sexp: &Sexp) -> String {
    let mut out = String::new();
    write(&mut out, sexp, 0);
    out
}

pub fn print_all(forms: &[Sexp]) -> String {
    forms.iter().map(print).collect::<Vec<_>>().join("\n")
}

fn write(out: &mut String, sexp: &Sexp, indent: usize) {
    match sexp {
        Sexp::Atom(text) => out.push_str(text),
        Sexp::Str(text) => out.push_str(&format!("{text:?}")),
        Sexp::List(items) => {
            out.push('(');
            if sexp.fits_on_line() {
                for (n, item) in items.iter().enumerate() {
                    if n > 0 {
                        out.push(' ');
                    }
                    write(out, item, indent);
                }
            } else {
                let mut rest = items.iter();
                if let Some(first @ (Sexp::Atom(_) | Sexp::Str(_))) = items.first() {
                    write(out, first, indent);
                    rest.next();
                }
                for item in rest {
                    out.push('\n');
                    out.push_str(&" ".repeat(indent + 2));
                    write(out, item, indent + 2);
                }
            }
            out.push(')');
        }
    }
}
