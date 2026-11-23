# java-analyzer

Java LSP in Rust, modeled after `rust-analyzer` using `rowan` (lossless CST) and `salsa` (incremental queries).

## Rules
1. **Architecture**: `syntax` (parser/CST), `hir` (semantics), `ide` (editor features), `java-analyzer` (LSP server). IO belongs only in the LSP crate.
2. **TDD**: Write failing tests first. Implement the minimum to pass.
3. **Specs First & Two-Stage Approval Gate**: All new feature development MUST be based on an agreed specification in `docs/specs/`. While crafting or updating a spec, enter planning mode, draft it with `Status: Proposed`, present key design decisions and open questions, and stop for user sign-off before marking it `Status: Accepted`. **Treat spec approval and implementation go-ahead as separate gates**: when the user asks only to draft, refine, or approve a spec/plan, do NOT start writing implementation code (or treat a spec/plan approval click as an instruction to implement) unless the user explicitly asks to proceed with implementation.
4. **Subagent Delegation**: Keep orchestration, spec alignment, and final verification in the main agent; delegate concrete, well-scoped implementation, bug-fix, and code-review tasks to subagents running on `flash` (`Model: "flash"`).
5. **Deferred Work**: Record agreed follow-ups and deferred scope in `docs/plans/` rather than leaving them only in chat.
6. **Rust, Salsa & LSP Discipline**:
   - Enforce strict layering: `java-analyzer` depends only on `ide` (`AnalysisHost`/`Analysis`), never directly on `hir` or `syntax`; `syntax` stays pure and Salsa-free.
   - **Build vs. Buy**: Keep custom implementations where justified (e.g., Java scanner and resilient AST parser), but prefer standard, well-maintained crates for infrastructure. Declare shared dependencies in root `[workspace.dependencies]`, reuse existing workspace crates, and never leave unused dependencies in `Cargo.toml`.
   - Avoid `.unwrap()`, `.expect()`, and unchecked indexing in non-test code; never let malformed Java source or broken manifests panic the parser or LSP server. Log recoverable parse/config failures with `tracing::warn!` instead of silently ignoring them.
   - Preserve Salsa early-cutoff and determinism: never store `TextRange`s in `FileItemTree`/`ModuleIndex`/`WorkspaceIndex` (resolve ranges lazily via `AstIdMap`), return `Arc<T>` directly from `#[salsa::tracked]` queries (no `_data` + `Arc` wrapper pairs), and use deterministic maps (`BTreeMap`, `FxHashMap`, `IndexMap`). Always drop any `Analysis` snapshot before mutating `AnalysisHost`/`RootDatabase` on the same thread (holding a snapshot on the mutating thread deadlocks Salsa).
   - Keep `GlobalState::snapshot()` O(1) (use `Arc` + `Arc::make_mut` on mutating paths). Prefer borrowing, `SmolStr`, and `la-arena` over `.clone()` and `String`/`Vec` allocations; keep visibility as tight as possible (`pub(crate)` or private over `pub`).
   - **Diagnostics UX**: Syntax and typechecker errors must use familiar `javac`-like primary messages enriched with `rustc`/`rust-analyzer`-grade usability (precise spans, secondary `DiagnosticRelatedInformation` locations, and actionable `help:`/`note:` hints). Extra IDE inspections (nullability, unused symbols, custom lints) may use custom wording with the same rich UX.
7. **Verification Gate**: Run `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` before committing.
8. **Project-Local Skills**: Consult the curated Rust skills in `.agents/skills/` (`rust-best-practices`, `ms-rust-guidelines`, `rust-2024-idioms`) when writing, reviewing, or refactoring Rust code, loading only the specific chapter/rule files relevant to the task. Rule 6 takes precedence on any conflict.

## Tooling
1. **Prefer jujutsu over git**: `jj` is a modern, feature-rich Git client.
   - `jj diff` when viewing diffs.
   - In a Git-like linear workflow, for the equivalent of `git commit -m 'your commit message'`, run `jj bookmark advance` first to advance the bookmark to `@`, and then `jj commit -m 'your commit message'` to commit `@` with a message and create a new blank commit on top of it.
   - For multi-step refactorings, start from a clean `jj status` and commit each step as a separate local change (do not push unless asked).
   - For non-trivial commits, follow the one-line subject with up to three short narrative paragraphs (1–2 sentences each): (1) the problem and why it mattered, (2) the approach introduced, and (3) optional reviewer notes or recommended top-down review order.
2. **Prefer single-quoted (`'...'`) strings in shell commands**: Single quotes prevent variable/command interpolation and allow multiline strings while keeping commands prefix-matchable for tool permission rules.
3. **Prefer sharp focused CLI tools to ad-hoc scripts**:
   - `ast-grep` (`sg`) when searching/replacing over structured AST (e.g., `ast-grep run -p '...'`)
   - `fd` when searching for files (e.g., `fd '...'`)
   - `jq` when parsing JSON (e.g., `jq '...'`)
   - `rg` (ripgrep) when searching for text (e.g., `rg '...'`)
   - `yq` when parsing YAML (e.g., `yq '...'`)