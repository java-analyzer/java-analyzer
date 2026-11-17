//! Robustness properties of `syntax::parse`, checked on mutations of the paired fixtures and on
//! generated soup: it never panics, the tree text equals the input, and every error range is
//! ordered, in bounds and on `char` boundaries.

mod support;

/// Checks the properties for `text`, printing the offending input if any of them fails.
fn check(text: &str) {
    if let Err(panic) = std::panic::catch_unwind(|| check_properties(text)) {
        let shown: String = text.chars().take(300).collect();
        eprintln!("robustness check failed for input (first 300 chars): {shown:?}");
        std::panic::resume_unwind(panic);
    }
}

fn check_properties(text: &str) {
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

#[test]
fn realistic_nesting_is_accepted_and_absurd_nesting_is_reported() {
    let parens = |n: usize| format!("class A {{ int x = {}1{}; }}", "(".repeat(n), ")".repeat(n));
    let messages = |text: &str| -> Vec<String> {
        syntax::parse(text)
            .errors()
            .iter()
            .map(|e| e.message.clone())
            .collect()
    };

    assert_eq!(messages(&parens(100)), Vec::<String>::new());
    assert!(messages(&parens(100_000)).contains(&"nesting is too deep".to_string()));

    let chain = |n: usize| format!("class A {{ int x = 1{}; }}", " + 1".repeat(n));
    assert_eq!(messages(&chain(1000)), Vec::<String>::new());
    assert!(messages(&chain(5000)).contains(&"expression is too long".to_string()));

    let else_if = |n: usize| {
        format!(
            "class A {{ void f() {{ {}; }} }}",
            "if (a) {} else ".repeat(n)
        )
    };
    assert_eq!(messages(&else_if(100)), Vec::<String>::new());
}

/// Input nested or chained far deeper than any real code must not overflow the stack, neither
/// while parsing nor when the tree is dropped. This test runs on the default 2 MiB test thread
/// stack, so passing proves the limits keep both well below that.
#[test]
fn pathologically_deep_input_does_not_overflow_the_stack() {
    // Several times past every limit the parser has (nesting 256, chains 1024, scans 512).
    const N: usize = 6_000;
    let in_method =
        |before: &str, after: &str| format!("class A {{ void f() {{ {before}{after} }} }}");
    let inputs = [
        // Expression nesting.
        in_method(&"(".repeat(N), &")".repeat(N)),
        in_method(&"x = ".repeat(N), "1;"),
        in_method("x = ", &format!("{}1;", "-".repeat(N))),
        in_method("x = ", &format!("{}1;", "!".repeat(N))),
        in_method("x = ", &format!("{}y;", "(A) ".repeat(N))),
        in_method("x = ", &format!("{}1;", "a ? b : ".repeat(N))),
        in_method("x = ", &format!("{}1;", "a ? ".repeat(N))),
        in_method("x = ", &format!("{}1;", "f(".repeat(N))),
        in_method("x = ", &format!("{}1;", "x -> ".repeat(N))),
        in_method("x = ", &format!("{}1;", "() -> ".repeat(N))),
        in_method("x = ", &format!("{}1;", "new int[] {".repeat(N))),
        in_method(&"@A(".repeat(N), "int x;"),
        in_method("x = ", &format!("{}1;", "a[".repeat(N))),
        in_method("x = ", &format!("{}y;", "new Foo(".repeat(N))),
        in_method(
            "x = ",
            &format!("{}y;", "switch (a) { case 1 -> ".repeat(N)),
        ),
        // Long left-associative chains.
        in_method("x = ", &format!("{}1;", "a + ".repeat(N))),
        in_method("x = ", &format!("a{};", ".b".repeat(N))),
        in_method("x = ", &format!("a{};", "()".repeat(N))),
        in_method("x = ", &format!("a{};", "[0]".repeat(N))),
        in_method("x = ", &format!("a{};", "++".repeat(N))),
        in_method("x = ", &format!("a{};", ".b()".repeat(N))),
        in_method("x = ", &format!("a{};", " instanceof B".repeat(N))),
        // Statement and declaration nesting.
        in_method(&"{".repeat(N), ""),
        in_method(&"if (a) ".repeat(N), ";"),
        in_method(&"if (a) {} else ".repeat(N), ";"),
        in_method(&"while (a) ".repeat(N), ";"),
        in_method(&"for (;;) ".repeat(N), ";"),
        in_method(&"x: ".repeat(N), ";"),
        in_method(&"try {".repeat(N), ""),
        in_method(&"switch (a) { case 1: ".repeat(N), ""),
        in_method(&"class B {".repeat(N), ""),
        in_method(&"new Object() {".repeat(N), ""),
        "class A {".repeat(N),
        "class A { class B ".repeat(N),
        "@interface A {".repeat(N),
        "enum E { A {".repeat(N),
        // Type nesting.
        in_method(&"List<".repeat(N), "x;"),
        in_method("List<A", &"[]".repeat(N)),
        in_method("int", &format!("{} x;", "[]".repeat(N))),
        in_method("x = (", &format!("{}) y;", "A<".repeat(N))),
        in_method("x = o instanceof ", &format!("{}y;", "A(".repeat(N))),
        in_method("x = ", &format!("{}y;", "A<B>::".repeat(N))),
        // Lookahead scans over token runs far longer than the scan limit.
        in_method(&format!("a<{}", "a,".repeat(N)), ";"),
        in_method(&format!("{}x;", "a.".repeat(N)), ""),
        in_method("for (", &format!("{}x : y) {{}}", "final ".repeat(N))),
        in_method(&format!("x = ({}", "a, ".repeat(N)), ") -> 1;"),
        in_method(&format!("x = ({}", "A & ".repeat(N)), ") y;"),
    ];
    for input in inputs {
        let started = std::time::Instant::now();
        check(&input);
        let elapsed = started.elapsed();
        if elapsed > std::time::Duration::from_millis(400) {
            let shown: String = input.chars().take(60).collect();
            eprintln!("slow ({elapsed:?}): {shown:?}");
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

/// Statement and expression fragments, in a method body (where they are parsed for real) and in
/// the field initializer position, in random combinations.
#[test]
fn statement_and_expression_soup() {
    const FRAGMENTS: &[&str] = &[
        "if",
        "else",
        "while",
        "do",
        "for",
        "switch",
        "case",
        "default",
        "try",
        "catch",
        "finally",
        "return",
        "break",
        "continue",
        "throw",
        "yield",
        "assert",
        "synchronized",
        "new",
        "this",
        "super",
        "instanceof",
        "var",
        "int",
        "void",
        "final",
        "class",
        "when",
        "null",
        "true",
        "x",
        "y.z",
        "Foo",
        "Foo<Bar>",
        "1",
        "2.5",
        "'c'",
        "\"s\"",
        "(",
        ")",
        "{",
        "}",
        "[",
        "]",
        ";",
        ",",
        ".",
        ":",
        "::",
        "->",
        "?",
        "=",
        "==",
        "+",
        "-",
        "*",
        "/",
        "++",
        "--",
        "!",
        "~",
        "&&",
        "||",
        "&",
        "|",
        "^",
        "<",
        ">",
        ">>",
        ">>>",
        ">=",
        ">>=",
        "<<",
        "+=",
        "@",
        " ",
        "\n",
    ];
    let mut state: u64 = 0x1234_5678_9abc_def1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..20_000 {
        let len = (next() % 40) as usize;
        let body: String = (0..len)
            .map(|_| FRAGMENTS[(next() % FRAGMENTS.len() as u64) as usize])
            .collect();
        check(&format!("class A {{ void f() {{ {body} }} }}"));
        check(&format!("class A {{ int x = {body}; int y; }}"));
    }
}
