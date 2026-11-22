//! Runs the text fixtures in `test_data/lexer/`. See `tests/support/mod.rs` for the format.

mod support;

use support::{Case, line_col};
use syntax::lexer::lex_with_errors;

fn render(case: &Case) -> Vec<String> {
    let include_trivia = case.has_option("trivia");
    let source = &case.source;
    let (tokens, errors) = lex_with_errors(source);

    let covered: usize = tokens.iter().map(|t| t.len).sum();
    let mut lines = Vec::new();
    if covered != source.len() {
        lines.push(format!(
            "!! lossless invariant violated: tokens cover {covered} of {} bytes",
            source.len()
        ));
    }

    let mut errors = errors.iter().peekable();
    let mut offset = 0;
    for token in &tokens {
        if include_trivia || !token.kind.is_trivia() {
            let text = &source[offset..offset + token.len];
            let (line, col) = line_col(source, offset);
            lines.push(format!("{:?} {text:?} @ {line}:{col}", token.kind));
        }
        while let Some(error) = errors.next_if(|e| e.offset == offset) {
            let (line, col) = line_col(source, error.offset);
            lines.push(format!("error: {} @ {line}:{col}", error.message));
        }
        offset += token.len;
    }
    lines
        .extend(errors.map(|e| format!("!! error not attached to a token at offset {}", e.offset)));
    lines
}

#[test]
fn lexer_fixtures() {
    support::run_fixtures("lexer", render);
}
