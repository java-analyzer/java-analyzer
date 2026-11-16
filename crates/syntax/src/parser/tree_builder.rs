//! Replays parser events into a `rowan` green tree, re-inserting trivia.
//!
//! Trivia placement (see the spec): a node starts at its first significant token and ends at its
//! last, so leading and trailing trivia belong to the *parent*. Doc comments therefore end up as
//! siblings of the declaration they precede. Trivia before the end of the root node is kept
//! inside the root.

use std::mem;

use rowan::{GreenNode, GreenNodeBuilder, Language, TextRange, TextSize};

use super::{Event, TOMBSTONE};
use crate::{lexer::Token, syntax_kind::SyntaxKind, JavaLanguage, SyntaxError};

pub(crate) fn build(
    text: &str,
    tokens: &[Token],
    mut events: Vec<Event>,
) -> (GreenNode, Vec<SyntaxError>) {
    let mut builder = Builder {
        text,
        tokens,
        raw_pos: 0,
        text_pos: 0,
        last_significant_end: 0,
        depth: 0,
        inner: GreenNodeBuilder::new(),
        errors: Vec::new(),
    };

    for i in 0..events.len() {
        match mem::replace(&mut events[i], Event::Tombstone) {
            Event::Start {
                kind,
                forward_parent,
            } => {
                // Collect the chain of nodes that must be opened before this one, outermost last.
                let mut kinds = vec![kind];
                let mut idx = i;
                let mut forward = forward_parent;
                while let Some(offset) = forward {
                    idx += offset;
                    forward = match mem::replace(&mut events[idx], Event::Tombstone) {
                        Event::Start {
                            kind,
                            forward_parent,
                        } => {
                            kinds.push(kind);
                            forward_parent
                        }
                        other => unreachable!("forward parent is not a Start event: {other:?}"),
                    };
                }
                for kind in kinds.into_iter().rev() {
                    if kind != TOMBSTONE {
                        builder.start_node(kind);
                    }
                }
            }
            Event::Finish => builder.finish_node(),
            Event::Token { kind, n_raw } => builder.token(kind, n_raw),
            Event::Error { message, missing } => builder.error(message, missing),
            Event::Tombstone => {}
        }
    }

    debug_assert_eq!(builder.depth, 0, "unbalanced Start/Finish events");
    debug_assert_eq!(
        builder.raw_pos,
        tokens.len(),
        "not all tokens were consumed"
    );
    (builder.inner.finish(), builder.errors)
}

struct Builder<'a> {
    text: &'a str,
    tokens: &'a [Token],
    raw_pos: usize,
    text_pos: usize,
    /// End offset of the last significant (non-trivia) token emitted so far.
    last_significant_end: usize,
    depth: usize,
    inner: GreenNodeBuilder<'static>,
    errors: Vec<SyntaxError>,
}

impl Builder<'_> {
    fn start_node(&mut self, kind: SyntaxKind) {
        // The root node opens before any trivia so that leading trivia lands inside it.
        if self.depth > 0 {
            self.eat_trivia();
        }
        self.depth += 1;
        self.inner.start_node(JavaLanguage::kind_to_raw(kind));
    }

    fn finish_node(&mut self) {
        self.depth -= 1;
        if self.depth == 0 {
            self.eat_trivia();
        }
        self.inner.finish_node();
    }

    fn token(&mut self, kind: SyntaxKind, n_raw: usize) {
        self.eat_trivia();
        let len: usize = self.tokens[self.raw_pos..self.raw_pos + n_raw]
            .iter()
            .map(|t| t.len)
            .sum();
        self.emit(kind, len);
        self.raw_pos += n_raw;
        self.last_significant_end = self.text_pos;
    }

    fn eat_trivia(&mut self) {
        while let Some(token) = self.tokens.get(self.raw_pos) {
            if !token.kind.is_trivia() {
                break;
            }
            self.emit(token.kind, token.len);
            self.raw_pos += 1;
        }
    }

    fn emit(&mut self, kind: SyntaxKind, len: usize) {
        let text = &self.text[self.text_pos..self.text_pos + len];
        self.inner.token(JavaLanguage::kind_to_raw(kind), text);
        self.text_pos += len;
    }

    fn error(&mut self, message: String, missing: bool) {
        let range = if missing {
            let at = size(self.last_significant_end);
            TextRange::empty(at)
        } else {
            // The next significant token, without consuming it.
            let mut start = self.text_pos;
            let mut next = None;
            for token in &self.tokens[self.raw_pos..] {
                if !token.kind.is_trivia() {
                    next = Some(token.len);
                    break;
                }
                start += token.len;
            }
            match next {
                Some(len) => TextRange::at(size(start), size(len)),
                None => TextRange::empty(size(self.text.len())),
            }
        };
        self.errors.push(SyntaxError { message, range });
    }
}

fn size(offset: usize) -> TextSize {
    TextSize::try_from(offset).expect("source text larger than 4 GiB")
}
