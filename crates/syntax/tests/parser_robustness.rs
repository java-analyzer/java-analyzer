//! Robustness properties of `syntax::parse`, checked on mutations of the paired fixtures and on
//! generated soup: it never panics, the tree text equals the input, and every error range is
//! ordered, in bounds and on `char` boundaries.

mod support;

fn check(text: &str) {
    let parse = syntax::parse(text);
    assert_eq!(
        parse.syntax_node().text().to_string(),
        text,
        "tree text differs from {text:?}"
    );

    let mut previous = 0usize;
    for error in parse.errors() {
        let (start, end) = (
            usize::from(error.range.start()),
            usize::from(error.range.end()),
        );
        assert!(
            start <= end && end <= text.len(),
            "error range out of bounds for {text:?}"
        );
        assert!(
            text.is_char_boundary(start) && text.is_char_boundary(end),
            "error range splits a char in {text:?}"
        );
        assert!(start >= previous, "errors are not ordered for {text:?}");
        previous = start;
    }
}

fn fixture_sources() -> Vec<String> {
    ["parser/ok", "parser/err"]
        .into_iter()
        .flat_map(support::load_pairs)
        .map(|case| case.source)
        .collect()
}

#[test]
fn every_prefix_of_every_fixture() {
    for source in fixture_sources() {
        check(&source);
        for (end, _) in source.char_indices() {
            check(&source[..end]);
        }
    }
}

#[test]
fn every_suffix_of_every_fixture() {
    for source in fixture_sources() {
        for (start, _) in source.char_indices() {
            check(&source[start..]);
        }
    }
}

#[test]
fn single_character_deletions() {
    for source in fixture_sources() {
        for (start, c) in source.char_indices() {
            let mut mutated = String::with_capacity(source.len());
            mutated.push_str(&source[..start]);
            mutated.push_str(&source[start + c.len_utf8()..]);
            check(&mutated);
        }
    }
}

/// Deterministic pseudo-random concatenations of Java-ish fragments.
#[test]
fn fragment_soup() {
    const FRAGMENTS: &[&str] = &[
        "package",
        "import",
        "static",
        "class",
        "interface",
        "enum",
        "record",
        "@interface",
        "public",
        "final",
        "sealed",
        "non-sealed",
        "permits",
        "extends",
        "implements",
        "A",
        "b.c",
        " ",
        "\n",
        "{",
        "}",
        "(",
        ")",
        "[",
        "]",
        "<",
        ">",
        ">>",
        ",",
        ";",
        ".",
        "*",
        "&",
        "?",
        "@",
        "=",
        "\"",
        "'",
        "\"\"\"",
        "/*",
        "*/",
        "//",
        "1",
        "->",
        "::",
        "é",
        "😀",
        "\\",
    ];
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..5000 {
        let len = (next() % 30) as usize;
        let input: String = (0..len)
            .map(|_| FRAGMENTS[(next() % FRAGMENTS.len() as u64) as usize])
            .collect();
        check(&input);
    }
}
