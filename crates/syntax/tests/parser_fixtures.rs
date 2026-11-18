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
    let parse = syntax::parse_with_level(source, case.language_level());
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

#[test]
fn every_feature_table_entry_is_gated_at_its_boundary_levels() {
    let snippets: &[(&str, &str)] = &[
        (
            "switch-expressions",
            "class A { int f(int x) { return switch (x) { default -> 1; }; } }",
        ),
        ("text-blocks", "class A { String s = \"\"\"\nhi\n\"\"\"; }"),
        ("records", "record R(int x) {}"),
        (
            "pattern-matching-instanceof",
            "class A { boolean f(Object o) { return o instanceof String s; } }",
        ),
        ("sealed-classes", "sealed class A permits B {}"),
        (
            "record-patterns",
            "class A { boolean f(Object o) { return o instanceof R(int x); } }",
        ),
        (
            "pattern-matching-switch",
            "class A { void f(Object o) { switch (o) { case String s when true -> {} default -> {} } } }",
        ),
        (
            "unnamed-variables",
            "class A { void f() { int _ = 1; } }",
        ),
        ("module-imports", "import module java.base;\nclass A {}"),
        ("compact-source-files", "int count = 1;\nvoid main() {}"),
    ];

    assert_eq!(
        snippets.len(),
        syntax::FEATURES.len(),
        "every row of FEATURES must have a boundary snippet"
    );

    for &(id, source) in snippets {
        let feature = syntax::Feature::by_id(id).unwrap_or_else(|| panic!("unknown feature {id}"));
        let stable = feature.stable_in.expect("stable release");
        let first_preview = feature.preview_in.first().copied().unwrap_or(stable);
        let last_preview = feature.preview_in.last().copied().unwrap_or(stable);

        // Before the preview window: requires stable_in.
        let before = syntax::LanguageLevel::new(syntax::Release::new(first_preview.number() - 1), false);
        let errors = syntax::parse_with_level(source, before).errors().to_vec();
        let expected_before = format!("{} require language level {}", feature.plural, stable.number());
        assert!(
            errors.iter().any(|e| e.message == expected_before),
            "{id} at {before:?}: expected {expected_before:?}, got {errors:?}"
        );

        // Edges of the preview window: rejected without preview, accepted with preview.
        for edge in [first_preview, last_preview] {
            let off = syntax::LanguageLevel::new(edge, false);
            let errors_off = syntax::parse_with_level(source, off).errors().to_vec();
            let expected_preview = format!(
                "{} are a preview feature in java {}; enable preview",
                feature.plural,
                edge.number()
            );
            assert!(
                errors_off.iter().any(|e| e.message == expected_preview),
                "{id} at {off:?}: expected {expected_preview:?}, got {errors_off:?}"
            );

            let on = syntax::LanguageLevel::new(edge, true);
            let errors_on = syntax::parse_with_level(source, on).errors().to_vec();
            assert!(
                errors_on.is_empty(),
                "{id} at {on:?}: expected no errors, got {errors_on:?}"
            );
        }

        // At stable_in - 1 without preview: rejected.
        let before_stable =
            syntax::LanguageLevel::new(syntax::Release::new(stable.number() - 1), false);
        assert!(
            !syntax::parse_with_level(source, before_stable)
                .errors()
                .is_empty(),
            "{id} at {before_stable:?} should be rejected"
        );

        // At stable_in without preview: accepted.
        let at_stable = syntax::LanguageLevel::new(stable, false);
        let errors_stable = syntax::parse_with_level(source, at_stable).errors().to_vec();
        assert!(
            errors_stable.is_empty(),
            "{id} at {at_stable:?}: expected no errors, got {errors_stable:?}"
        );
    }

    // At Java 8, `_` is an identifier in Java 8 constructs, but `_` in pattern constructs
    // (`String _`, `Point(_, int y)`) must still be diagnosed as an unnamed pattern rather than
    // faked as a Java 8 identifier.
    let java8 = syntax::LanguageLevel::new(syntax::Release::JAVA_8, false);
    for pattern_src in [
        "class A { boolean f(Object o) { return o instanceof String _; } }",
        "class A { boolean f(Object o) { return o instanceof Point(_, int y); } }",
    ] {
        let errors = syntax::parse_with_level(pattern_src, java8)
            .errors()
            .to_vec();
        assert!(
            errors
                .iter()
                .any(|e| e.message == "unnamed variables and patterns require language level 22"),
            "expected unnamed-variables error at Java 8 for {pattern_src:?}, got {errors:?}"
        );
    }
}
