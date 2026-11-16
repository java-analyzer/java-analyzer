//! Runs the paired fixtures in `test_data/parser/{ok,err}/`: `name.java` is the source and
//! `name.scm` the expected tree as an S-expression (`name.trivia.scm` when whitespace and
//! comments are part of the expectation). See `tests/support/mod.rs`.
//!
//! ```text
//! (SOURCE_FILE
//!   (CLASS_DECL
//!     (CLASS_KW "class")
//!     (NAME (IDENT "Foo"))
//!     (CLASS_BODY (L_CURLY "{") (R_CURLY "}"))))
//! (errors
//!   (error "expected '}'" "3:1..3:1"))
//! ```
//!
//! Nodes are `(KIND child...)`, tokens are `(KIND "text")`. The optional `errors` form lists every
//! syntax error with its range as `line:col..line:col`.

mod support;

use rowan::NodeOrToken;
use support::{line_col, sexp::Sexp, PairCase};
use syntax::SyntaxNode;

fn node_to_sexp(node: &SyntaxNode, include_trivia: bool) -> Sexp {
    let mut items = vec![Sexp::atom(format!("{:?}", node.kind()))];
    for element in node.children_with_tokens() {
        match element {
            NodeOrToken::Node(child) => items.push(node_to_sexp(&child, include_trivia)),
            NodeOrToken::Token(token) => {
                if include_trivia || !token.kind().is_trivia() {
                    items.push(Sexp::list([
                        Sexp::atom(format!("{:?}", token.kind())),
                        Sexp::string(token.text()),
                    ]));
                }
            }
        }
    }
    Sexp::List(items)
}

fn render(case: &PairCase) -> Vec<Sexp> {
    let source = &case.source;
    let parse = syntax::parse(source);
    let root = parse.syntax_node();

    let mut forms = Vec::new();
    if root.text().to_string() != *source {
        forms.push(Sexp::list([
            Sexp::atom("!!"),
            Sexp::string("lossless invariant violated: the tree text differs from the source"),
        ]));
    }
    forms.push(node_to_sexp(&root, case.include_trivia));

    if !parse.errors().is_empty() {
        let mut errors = vec![Sexp::atom("errors")];
        for error in parse.errors() {
            let (start_line, start_col) = line_col(source, usize::from(error.range.start()));
            let (end_line, end_col) = line_col(source, usize::from(error.range.end()));
            errors.push(Sexp::list([
                Sexp::atom("error"),
                Sexp::string(&error.message),
                Sexp::string(format!("{start_line}:{start_col}..{end_line}:{end_col}")),
            ]));
        }
        forms.push(Sexp::List(errors));
    }
    forms
}

fn has_errors(forms: &[Sexp]) -> bool {
    forms.len() > 1
        && matches!(forms.last(), Some(Sexp::List(items)) if items.first() == Some(&Sexp::atom("errors")))
}

#[test]
fn parser_ok_fixtures() {
    support::run_pair_fixtures("parser/ok", |case| {
        let forms = render(case);
        assert!(
            !has_errors(&forms),
            "{}.java is in `ok` but has syntax errors; fix the grammar or move it to `err`",
            case.name
        );
        forms
    });
}

#[test]
fn parser_err_fixtures() {
    support::run_pair_fixtures("parser/err", |case| {
        let forms = render(case);
        assert!(
            has_errors(&forms),
            "{}.java is in `err` but parses without syntax errors",
            case.name
        );
        forms
    });
}
