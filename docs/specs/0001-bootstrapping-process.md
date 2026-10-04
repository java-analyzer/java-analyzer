# Spec 0001: Specification-Driven Bootstrapping

## Motivation
To ensure `java-analyzer` remains maintainable, scalable, and true to the `rust-analyzer` philosophy, all development must be guided by clear specifications.

## Process

### Phase 1: Define the Domain
Before implementing a feature (e.g., parsing Java classes), a spec must be written defining:
- The exact syntax rules (for `syntax`).
- The semantic implications and data structures (for `hir`).
- The expected IDE behavior (for `ide`).

### Phase 2: Scaffold Tests
Create test cases that reflect the specification.
- For parsing: Write inline tests that take a Java source string and assert the resulting CST.
- For IDE features: Write fixture-based tests demonstrating expected completions or hovers.

### Phase 3: Implementation
Implement the logic to satisfy the tests. Use the following tech stack guidelines:
- **Parser**: Hand-written recursive descent parser for error resilience.
- **Tree**: `rowan` for the Red/Green tree architecture.
- **State**: `salsa` for query-based, incremental semantic analysis.
- **LSP**: `lsp-types` and `lsp-server`.

### Next Steps for Bootstrapping
1. Integrate `rowan` into `crates/syntax` and define the basic Java token types.
2. Implement a lexer for a small subset of Java (e.g., `class`, `{`, `}`).
3. Integrate `salsa` into `crates/hir` and define the basic database trait.
