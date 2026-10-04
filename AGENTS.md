# java-analyzer

Java LSP in Rust, modeled after `rust-analyzer` using `rowan` (lossless CST) and `salsa` (incremental queries).

## Rules
1. **Architecture**: `syntax` (parser/CST), `hir` (semantics), `ide` (editor features), `java-analyzer` (LSP server). IO belongs only in the LSP crate.
2. **TDD**: Write failing tests first. Implement the minimum to pass.
3. **Specs**: Read `docs/specs/` ONLY when you lack domain context for a task. If a spec is missing, propose it.
