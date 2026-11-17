//! Event-based, error-resilient recursive descent parser.
//!
//! The parser never builds the tree itself. It reads a filtered token stream (no trivia) and
//! produces a flat list of [`Event`]s; [`tree_builder`] replays the events together with the raw
//! token stream (re-inserting trivia) into a `rowan` green tree. See
//! `docs/specs/0002-java-syntax-subset.md`.

mod expressions;
mod grammar;
mod lookahead;
mod statements;
pub(crate) mod tree_builder;

use std::cell::Cell;

use crate::{
    language_level::{LanguageLevel, Release},
    lexer,
    syntax_kind::SyntaxKind::{self, *},
};

/// Maximum number of lookahead calls without consuming a token. Exceeding it means a grammar
/// rule loops without making progress, which is always a bug.
const PARSER_STEP_LIMIT: u32 = 100_000;

/// Maximum nesting of expressions, statements, class bodies and type arguments. Deeper input is
/// reported as an error instead of recursing further, so the parser cannot overflow the stack.
const MAX_NESTING: u32 = 256;

/// Marks a `Start` event that was never completed, or was abandoned.
const TOMBSTONE: SyntaxKind = SyntaxKind::__LAST;

/// The parser's view of the lexer output: significant (non-trivia) tokens only.
pub(crate) struct Input<'t> {
    kinds: Vec<SyntaxKind>,
    texts: Vec<&'t str>,
    /// `joint[i]` is true when token `i + 1` follows token `i` with no trivia in between.
    joint: Vec<bool>,
}

impl<'t> Input<'t> {
    pub(crate) fn new(text: &'t str, tokens: &[lexer::Token]) -> Input<'t> {
        let mut input = Input {
            kinds: Vec::new(),
            texts: Vec::new(),
            joint: Vec::new(),
        };
        let mut offset = 0;
        let mut prev_was_significant = false;
        for token in tokens {
            if token.kind.is_trivia() {
                prev_was_significant = false;
            } else {
                if prev_was_significant {
                    *input.joint.last_mut().expect("a previous token") = true;
                }
                input.kinds.push(token.kind);
                input.texts.push(&text[offset..offset + token.len]);
                input.joint.push(false);
                prev_was_significant = true;
            }
            offset += token.len;
        }
        input
    }
}

#[derive(Debug)]
pub(crate) enum Event {
    /// Starts a node. `forward_parent` is the relative index of an event that starts a node
    /// which must become the *parent* of this one (see [`CompletedMarker::precede`]).
    Start {
        kind: SyntaxKind,
        forward_parent: Option<usize>,
    },
    Finish,
    /// A token of `kind`, covering `n_raw` consecutive input tokens (more than one for
    /// composite tokens such as `non-sealed`).
    Token {
        kind: SyntaxKind,
        n_raw: usize,
    },
    /// A syntax error. `missing` errors are reported just after the previous token (something
    /// expected is absent); the others are reported on the next token (something unexpected).
    Error {
        message: String,
        missing: bool,
    },
    /// Placeholder left behind when the tree builder takes an event.
    Tombstone,
}

pub(crate) struct Parser<'a, 't> {
    input: &'a Input<'t>,
    pos: usize,
    events: Vec<Event>,
    steps: Cell<u32>,
    /// Current nesting, see [`MAX_NESTING`].
    depth: u32,
    nesting_error_reported: bool,
    /// Set while parsing `case` labels, where `name ->` is a switch rule and not a lambda.
    no_lambda: bool,
    /// Declared language level of the compilation unit.
    level: LanguageLevel,
}

/// Parses `input` into events.
pub(crate) fn parse(input: &Input<'_>, level: LanguageLevel) -> Vec<Event> {
    let mut p = Parser {
        input,
        pos: 0,
        events: Vec::new(),
        steps: Cell::new(0),
        depth: 0,
        nesting_error_reported: false,
        no_lambda: false,
        level,
    };
    grammar::source_file(&mut p);
    p.events
}

impl<'a, 't> Parser<'a, 't> {
    /// Index of the next unconsumed input token. Used to check for progress.
    fn pos(&self) -> usize {
        self.pos
    }

    fn nth(&self, n: usize) -> SyntaxKind {
        let steps = self.steps.get() + 1;
        assert!(
            steps <= PARSER_STEP_LIMIT,
            "the parser seems stuck at token {}",
            self.pos
        );
        self.steps.set(steps);
        self.input.kinds.get(self.pos + n).copied().unwrap_or(EOF)
    }

    fn current(&self) -> SyntaxKind {
        self.nth(0)
    }

    fn at(&self, kind: SyntaxKind) -> bool {
        self.current() == kind
    }

    fn nth_at(&self, n: usize, kind: SyntaxKind) -> bool {
        self.nth(n) == kind
    }

    fn at_eof(&self) -> bool {
        self.at(EOF)
    }

    fn nth_text(&self, n: usize) -> &'t str {
        self.input.texts.get(self.pos + n).copied().unwrap_or("")
    }

    /// Is token `n` an identifier with exactly this text? Used for contextual keywords.
    fn nth_at_contextual_kw(&self, n: usize, kw: &str) -> bool {
        self.nth_at(n, IDENT) && self.nth_text(n) == kw
    }

    fn at_contextual_kw(&self, kw: &str) -> bool {
        self.nth_at_contextual_kw(0, kw)
    }

    /// Is the current token an identifier (`IDENT`, or `_` at language level 8)?
    fn at_ident(&self) -> bool {
        self.at(IDENT) || (self.at(UNDERSCORE_KW) && self.level.release == Release::JAVA_8)
    }

    /// Is token `n` immediately followed by token `n + 1`, with no trivia between them?
    fn nth_is_joint(&self, n: usize) -> bool {
        self.input.joint.get(self.pos + n).copied().unwrap_or(false)
    }

    fn start(&mut self) -> Marker {
        let pos = self.events.len();
        self.events.push(Event::Start {
            kind: TOMBSTONE,
            forward_parent: None,
        });
        Marker {
            pos,
            completed: false,
        }
    }

    /// Consumes the current token, which must be `kind`.
    fn bump(&mut self, kind: SyntaxKind) {
        assert!(self.at(kind), "expected to be at {kind:?}");
        self.bump_any();
    }

    /// Consumes the current token, whatever it is. Does nothing at end of input.
    fn bump_any(&mut self) {
        let kind = self.current();
        if kind == EOF {
            return;
        }
        self.do_bump(kind, 1);
    }

    /// Consumes `n_raw` adjacent tokens as a single token of `kind`.
    fn bump_composite(&mut self, kind: SyntaxKind, n_raw: usize) {
        self.do_bump(kind, n_raw);
    }

    fn do_bump(&mut self, kind: SyntaxKind, n_raw: usize) {
        self.pos += n_raw;
        self.steps.set(0);
        self.events.push(Event::Token { kind, n_raw });
    }

    fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump_any();
            true
        } else {
            false
        }
    }

    /// Consumes `kind` or reports `message` (without consuming anything).
    fn expect(&mut self, kind: SyntaxKind, message: &str) -> bool {
        if self.eat(kind) {
            return true;
        }
        self.error_missing(message);
        false
    }

    /// Reports that something expected is absent, just after the previous token.
    fn error_missing(&mut self, message: &str) {
        self.events.push(Event::Error {
            message: message.to_string(),
            missing: true,
        });
    }

    /// Reports the current token as unexpected. Does not consume it.
    fn error_unexpected(&mut self, message: &str) {
        self.events.push(Event::Error {
            message: message.to_string(),
            missing: false,
        });
    }

    /// Runs `f` one nesting level deeper. Past [`MAX_NESTING`] it reports the problem (once per
    /// file) and returns `on_overflow` without running `f` or consuming anything; callers must
    /// cope with having made no progress.
    fn nested<T>(&mut self, on_overflow: T, f: impl FnOnce(&mut Self) -> T) -> T {
        if self.depth >= MAX_NESTING {
            if !self.nesting_error_reported {
                self.nesting_error_reported = true;
                self.error_unexpected("nesting is too deep");
            }
            return on_overflow;
        }
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        result
    }
}

/// A started, not yet completed node. Must be completed or abandoned.
#[must_use]
struct Marker {
    pos: usize,
    completed: bool,
}

impl Marker {
    fn complete(mut self, p: &mut Parser<'_, '_>, kind: SyntaxKind) -> CompletedMarker {
        self.completed = true;
        match &mut p.events[self.pos] {
            Event::Start { kind: slot, .. } => *slot = kind,
            _ => unreachable!("marker does not point at a Start event"),
        }
        p.events.push(Event::Finish);
        CompletedMarker {
            pos: self.pos,
            kind,
        }
    }

    /// Discards the marker. Nothing may have been parsed since it was started that is
    /// supposed to be inside the node (any such events simply become siblings).
    fn abandon(mut self, p: &mut Parser<'_, '_>) {
        self.completed = true;
        if self.pos == p.events.len() - 1 {
            p.events.pop();
        }
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        if !self.completed && !std::thread::panicking() {
            panic!("a parser marker was dropped without being completed or abandoned");
        }
    }
}

#[derive(Clone, Copy)]
struct CompletedMarker {
    pos: usize,
    kind: SyntaxKind,
}

impl CompletedMarker {
    fn kind(self) -> SyntaxKind {
        self.kind
    }
    /// Starts a new node that becomes the parent of this already completed node.
    fn precede(self, p: &mut Parser<'_, '_>) -> Marker {
        let new = p.start();
        match &mut p.events[self.pos] {
            Event::Start { forward_parent, .. } => *forward_parent = Some(new.pos - self.pos),
            _ => unreachable!("completed marker does not point at a Start event"),
        }
        new
    }
}
