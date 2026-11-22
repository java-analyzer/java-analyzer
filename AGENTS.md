# java-analyzer

Java LSP in Rust, modeled after `rust-analyzer` using `rowan` (lossless CST) and `salsa` (incremental queries).

## Rules
1. **Architecture**: `syntax` (parser/CST), `hir` (semantics), `ide` (editor features), `java-analyzer` (LSP server). IO belongs only in the LSP crate.
2. **TDD**: Write failing tests first. Implement the minimum to pass.
3. **Specs First & Approval Gate**: All new feature development MUST be based on an agreed specification in `docs/specs/`. Never start implementation without an agreed spec. While crafting or updating a spec, always enter planning mode, draft it with `Status: Proposed`, present the key design decisions and open questions to the user, and **stop for explicit approval before writing implementation code**. Never mark a spec `Status: Accepted` without user sign-off.
4. **Subagent Delegation**: Keep orchestration, spec alignment, and final verification in the main agent; delegate concrete, well-scoped implementation, bug-fix, and code-review tasks to subagents running on `flash` (`Model: "flash"`).
5. **Deferred Work**: Record agreed follow-ups and deferred scope in `docs/plans/` rather than leaving them only in chat.
6. **Rust, Salsa & LSP Discipline**:
   - Enforce strict layering: `java-analyzer` depends only on `ide` (`AnalysisHost`/`Analysis`), never directly on `hir` or `syntax`; `syntax` stays pure and Salsa-free.
   - Declare shared dependencies in root `[workspace.dependencies]` and reuse existing workspace crates before writing custom utilities.
   - Avoid `.unwrap()`, `.expect()`, and unchecked indexing in non-test code; never let malformed Java source panic the parser or LSP server.
   - Preserve Salsa early-cutoff: never store `TextRange`s in `FileItemTree`/`ModuleIndex`/`WorkspaceIndex` (resolve ranges lazily via `AstIdMap`). Always drop any `Analysis` snapshot before mutating `AnalysisHost`/`RootDatabase` on the same thread (holding a snapshot on the mutating thread deadlocks Salsa).
   - Prefer borrowing, `SmolStr`, and `la-arena` over `.clone()` and `String`/`Vec` allocations; keep visibility as tight as possible (`pub(crate)` or private over `pub`).
7. **Verification Gate**: Run `cargo clippy --all-targets -- -D warnings` (zero warnings in all touched files) and `cargo test` before committing.

## Tooling
1. **Prefer jujutsu over git**: `jj` is a modern, feature-rich Git client.
   - `jj diff` when viewing diffs
   - In a Git-like linear workflow, for the equivalent of `git commit -m 'your commit message'`, run `jj bookmark advance` first to advance the bookmark to `@`, and then `jj commit -m 'your commit message'` to commit `@` with a message and create a new blank commit on top of it.
   - For multi-step refactorings, start from a clean `jj status` and commit each step as a separate local change (do not push unless asked).
   - For non-trivial commits, follow the one-line subject with up to three short narrative paragraphs (1–2 sentences each): (1) the problem and why it mattered, (2) the approach introduced, and (3) optional reviewer notes or recommended top-down review order.
2. **Prefer single-quoted (`'...'`) strings in shell commands**: Single quotes prevent variable/command interpolation and allow multiline strings while keeping commands prefix-matchable for tool permission rules.
3. **Prefer sharp focused CLI tools to ad-hock scripts**: 
   - `ast-grep` when searching/replacng ove structured AST (e.g. `ast-grep find --ast-type `)
   - `fd` when searching for files (e.g. `fd `)
   - `jq` when parsing JSON (e.g. `jq <|user_cursor|>`)
   - `ripgrep` when searching for text (e.g. `ripgrep <|user_cursor|>`)
   - `yq` when parsing YAML (e.g. `yq <|user_cursor|>`)
