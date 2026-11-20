# java-analyzer

Java LSP in Rust, modeled after `rust-analyzer` using `rowan` (lossless CST) and `salsa` (incremental queries).

## Rules
1. **Architecture**: `syntax` (parser/CST), `hir` (semantics), `ide` (editor features), `java-analyzer` (LSP server). IO belongs only in the LSP crate.
2. **TDD**: Write failing tests first. Implement the minimum to pass.
3. **Specs First & Approval Gate**: All new feature development MUST be based on an agreed specification in `docs/specs/`. Never start implementation without an agreed spec. While crafting or updating a spec, always enter planning mode, draft it with `Status: Proposed`, present the key design decisions and open questions to the user, and **stop for explicit approval before writing implementation code**. Never mark a spec `Status: Accepted` without user sign-off.
4. **Subagent Delegation**: Keep orchestration, spec alignment, and final verification in the main agent; delegate concrete, well-scoped implementation, bug-fix, and code-review tasks to subagents running on `flash` (`Model: "flash"`).
5. **Deferred Work**: Record agreed follow-ups and deferred scope in `docs/plans/` rather than leaving them only in chat.

## Tooling
1. **Prefer jujutsu over git**: `jj` is a modern, feature-rich Git client.
   - `jj diff` when viewing diffs
   - In a Git-like linear workflow, for the equivalent of `git commit -m 'your commit message'`, run `jj bookmark advance` first to advance the bookmark to `@`, and then `jj commit -m 'your commit message'` to commit `@` with a message and create a new blank commit on top of it.
2. **Prefer single-quoted (`'...'`) strings in shell commands**: Single quotes prevent variable/command interpolation and allow multiline strings while keeping commands prefix-matchable for tool permission rules.
3. **Prefer sharp focused CLI tools to ad-hock scripts**: 
   - `ast-grep` when searching/replacng ove structured AST (e.g. `ast-grep find --ast-type `)
   - `fd` when searching for files (e.g. `fd `)
   - `jq` when parsing JSON (e.g. `jq <|user_cursor|>`)
   - `ripgrep` when searching for text (e.g. `ripgrep <|user_cursor|>`)
   - `yq` when parsing YAML (e.g. `yq <|user_cursor|>`)
