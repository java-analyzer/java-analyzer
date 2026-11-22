# `java-analyzer` Architectural & Code-Reuse Audit

> **Scope:** Read-only audit of the `java-analyzer` Rust workspace (`crates/syntax`, `crates/hir`, `crates/ide`, `crates/java-analyzer`, and `xtask`).
> **Goal:** Assess `rust-analyzer`-style crate layering, Salsa incrementality, code reuse, build-vs-buy trade-offs, and idiomatic Rust / LSP reliability, culminating in a prioritized, behavior-preserving refactoring plan.

---

## 1. Crate Layering & Architecture (`rust-analyzer` Discipline)

### Current Workspace Map & Dependency Graph

```text
  xtask (dev tooling: conformance runner)
    └── syntax

  java-analyzer (LSP server + CLI: lsp, inspect, configure + workspace_config)
    ├── ide
    ├── hir
    └── syntax   <-- direct dependency bypasses hir/ide

  ide (editor features: hover, goto_definition, document_symbols, diagnostics)
    ├── hir
    └── syntax

  hir (Salsa RootDatabase, SourceDatabase, DefDatabase, symbols, resolve, workspace layout)
    └── syntax

  syntax (lossless Java 8–27 lexer, event-based parser, rowan CST, language_level)
    └── rowan
```

| Crate | Current Responsibilities | Assessment |
|---|---|---|
| [`crates/syntax`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/Cargo.toml) | Pure, IO-free, Salsa-free Java lexer, event-based parser, `rowan` CST, `LanguageLevel` / feature-gate checker. | **Clean.** Zero upward dependencies; does not depend on `salsa`. |
| [`crates/hir`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/Cargo.toml) | `FileId`, `SourceDatabase`, `DefDatabase`, `RootDatabase` (`salsa`), symbol extraction (`symbols.rs`), name resolution (`resolve.rs`), and workspace layout inference (`workspace.rs`). | **Overloaded.** Combines `base-db` (`FileId`, `SourceDatabase`), `hir-def` (`FileSymbols`, `WorkspaceIndex`, `resolve`), and project-layout modeling (`WorkspaceLayout`) in one crate. |
| [`crates/ide`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/ide/Cargo.toml) | `hover`, `goto_definition`, `document_symbols`, `diagnostics`. | **Thin & partially bypassed.** No `AnalysisHost` / `Analysis` snapshot facade; `document_symbols` bypasses `hir`. |
| [`crates/java-analyzer`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/Cargo.toml) | LSP stdio server (`server.rs`), URI/offset conversion (`convert.rs`), CLI (`cli.rs`, `inspect.rs`, `configure.rs`), and 3-tier workspace probing (`workspace_config/`). | **Leaks semantic & parsing logic.** Directly invokes `syntax::parse_with_level`, `hir::WorkspaceLayout::check_file_package`, and `RootDatabase` queries outside `ide`. |
| [`xtask`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/xtask/Cargo.toml) | OpenJDK/jtreg parser conformance runner. | **Clean.** Isolated from default workspace members. |

### Layering Violations

1. **`ide::document_symbols` bypasses `hir` entirely** ([`crates/ide/src/lib.rs#L210-L336`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/ide/src/lib.rs#L210-L336))
   - Takes `db: &dyn SourceDatabase` instead of `&dyn DefDatabase` and walks `db.syntax_tree(file_id)` directly via `collect_symbols()`, re-implementing type/member CST traversal, `is_type()`, and `collapsed_text()` that already exist in [`crates/hir/src/symbols.rs`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs).
   - **Fix:** Either derive `document_symbols` from `db.file_symbols(file_id)` (preserving broken-code hoisting if needed) or share a typed AST/CST layer in `syntax` so `ide` never duplicates raw `SyntaxKind` extraction.

2. **`ide::hover` re-parses stringified `hir` metadata and inspects raw CST tokens** ([`crates/ide/src/lib.rs#L34-L56`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/ide/src/lib.rs#L34-L56), [`crates/ide/src/lib.rs#L117-L178`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/ide/src/lib.rs#L117-L178))
   - Because [`ResolvedSymbol`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/resolve.rs#L94-L102) flattens return type and parameters into a single `detail: Option<String>` (e.g., `"@Anno(\"x\") String (int a)"`), `ide` had to write a 45-line quote- and paren-tracking mini-parser (`trailing_paren_group_start`) just to split return type from parameter list for hover formatting.
   - **Fix:** Give `ResolvedSymbol` structured semantic fields (e.g., `return_type: Option<String>`, `params: Option<String>`, or a typed `SymbolDetail` enum) so `ide` formats structured data rather than re-parsing strings.

3. **Semantic/workspace diagnostics leak into the LSP server handler** ([`crates/java-analyzer/src/server.rs#L172-L205`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L172-L205))
   - `GlobalState::diagnostics()` pulls `self.db.syntax_tree(file_id)`, normalizes the relative path, and calls `probe.layout.check_file_package(file_id, &norm, &syntax_tree)` directly in `java-analyzer`, bypassing `ide::diagnostics()`.
   - **Fix:** Store workspace layout / file relative paths as Salsa inputs (or in `ide-db`/`base-db`) so `ide::diagnostics()` is the single source of truth for syntax, package-mismatch, and future semantic diagnostics.

4. **`java-analyzer` depends directly on `syntax` to parse files outside Salsa** ([`crates/java-analyzer/Cargo.toml#L14`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/Cargo.toml#L14), [`crates/java-analyzer/src/workspace_config/mod.rs#L200-L203`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/workspace_config/mod.rs#L200-L203), [`#L312`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/workspace_config/mod.rs#L312))
   - `probe_workspace()` and `parse_shebang_level()` call `syntax::parse_with_level()` directly, completely outside `RootDatabase`.
   - Likewise, [`crates/java-analyzer/src/inspect.rs#L30-L55`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/inspect.rs#L30-L55) instantiates `RootDatabase` directly and calls `db.syntax_errors()` / `db.workspace_index()`, bypassing `ide`.

5. **No `AnalysisHost` / `Analysis` boundary**
   - `GlobalState` in [`crates/java-analyzer/src/server.rs#L32-L39`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L32-L39) owns `hir::RootDatabase` directly and calls `ide::hover(&self.db, ...)` / `hir::SourceDatabase` methods directly. Introducing an `ide::AnalysisHost` (mutating) + `ide::Analysis` (immutable snapshot) facade will cleanly decouple `java-analyzer` from `hir` and `salsa`, and enable concurrent request handling.

### Incrementality Killers

1. **`TextRange` stored inside Salsa-memoized `FileSymbols` and `WorkspaceIndex`** ([`crates/hir/src/symbols.rs#L34`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs#L34), [`#L79-L80`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs#L79-L80), [`#L128-L129`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs#L128-L129))
   - `extract_symbols` ([`crates/hir/src/lib.rs#L48-L51`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/lib.rs#L48-L51)) returns `Arc<FileSymbols>`, which embeds absolute byte `TextRange`s (`full_range`, `selection_range`, `ImportDecl::range`).
   - **Impact:** Any edit inside a method body that inserts or deletes even 1 byte shifts every subsequent `TextRange` in `FileSymbols`. Salsa's equality check (`PartialEq`) fails, early-cutoff never triggers, and `build_index` re-runs for the **entire workspace** on every keystroke! (Notably, the test `file_symbols_body_edit_invariance_and_incremental_index` at [`crates/hir/src/lib.rs#L430-L468`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/lib.rs#L430-L468) had to use a *same-byte-length* edit `same_len_body_edit` to pass.)
   - **Fix (aligned with Spec 0009):** Split position-independent signatures (`ItemTree` with stable `FileAstId`s) from source ranges (`AstIdMap`), so body edits never invalidate `FileSymbols` or `WorkspaceIndex`.

2. **Monolithic `WorkspaceIndex` query over all files** ([`crates/hir/src/lib.rs#L32-L35`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/lib.rs#L32-L35), [`#L53-L59`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/lib.rs#L53-L59))
   - `WorkspaceFiles` is a single `Arc<Vec<SourceFile>>` input. `build_index` iterates over every file in the workspace and deep-clones every `TypeSymbol` into a single `WorkspaceIndex`.
   - **Impact:** Adding/closing a file or editing a single signature in a 5,000-file repo rebuilds and deep-clones the entire workspace index.
   - **Fix:** Partition by `SourceRootId` / `ModuleId` (already modeled in `workspace.rs`), and store lightweight `(FileId, TypeId)` handles in the index instead of cloning full `TypeSymbol` structs with all their `MemberSymbol`s.

3. **Redundant full-file parsing outside Salsa (2x–4x per file)**
   - [`workspace_config::parse_shebang_level`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/workspace_config/mod.rs#L200-L203) calls `syntax::parse_with_level(text, LanguageLevel::latest())` on the **entire Java file** just to inspect a `#!` on line 1!
   - During `GlobalState::set_workspace_root` ([`crates/java-analyzer/src/server.rs#L65-L84`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L65-L84)), `probe_workspace()` reads every `.java` file from disk, parses it once (or twice if searching for shebangs), discards the text and CST, and then `set_workspace_root` reads every `.java` file from disk *again*, calls `parse_shebang_level` (full parse #2/#3), and inserts it into `RootDatabase` (full parse #3/#4 on first query).
   - On every `didChange` keystroke, `GlobalState::set_text` ([`crates/java-analyzer/src/server.rs#L117-L126`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L117-L126)) calls `language_level_for()` → `parse_shebang_level()` (a full parse outside Salsa!) immediately before `db.set_file_text()`.
   - **Fix:** Make `hir::workspace::parse_shebang_level` (which only inspects line 1 if `text.starts_with("#!")`) public, and return/retain scanned file texts from `probe_workspace` so files are read and parsed once.

4. **Standard `HashMap`/`HashSet` instead of `FxHashMap`/`FxHashSet`**
   - [`RootDatabase::files`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/lib.rs#L91) and [`GlobalState::{files, file_urls, open_files}`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L34-L36) use `std::collections::HashMap`/`HashSet` (cryptographic SipHash, non-deterministic iteration order).
   - Note: `WorkspaceIndex` and `WorkspaceLayout` properly use `BTreeMap`/`BTreeSet` for determinism, though `FxHashMap` (with deterministic insertion or `IndexMap`) will be faster for hot `types_by_fqn` lookups once indexes grow.

5. **Unbounded `SourceFile` retention in `RootDatabase::files`**
   - [`GlobalState::close`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L143-L147) removes non-disk/deleted files from `GlobalState::files` and `file_urls` and blanks their text in `RootDatabase`, but `RootDatabase::files` never removes the `FileId`, leaking the `SourceFile` Salsa input indefinitely.

---

## 2. Code Duplication & Reuse Opportunities

| Duplicated Logic | Locations | Proposed Shared Abstraction |
|---|---|---|
| **CST type-node predicate & whitespace-collapsed text** | `hir::symbols::{is_type_node, collapsed_text}` ([`crates/hir/src/symbols.rs#L592-L624`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs#L592-L624)) vs. `ide::{is_type, collapsed_text}` ([`crates/ide/src/lib.rs#L316-L336`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/ide/src/lib.rs#L316-L336)). Note subtle divergence: `hir` checks `token.kind().is_trivia()`, while `ide` hardcodes `WHITESPACE \| LINE_COMMENT \| BLOCK_COMMENT`. | Move `is_type_node` (or `SyntaxKind::is_type_node`) and `collapsed_text(&SyntaxNode)` into `syntax` (e.g., `syntax::ast` or `syntax::ext`). |
| **Raw CST `SyntaxKind` child-finding boilerplate** | Repeated `node.children().find(\|n\| n.kind() == NAME)` / `PATH` / `MODIFIER_LIST` / `PARAM_LIST` across `hir/src/symbols.rs`, `hir/src/resolve.rs`, `hir/src/workspace.rs`, `ide/src/lib.rs`, and `syntax/src/language_level.rs`. | Introduce a lightweight typed `syntax::ast` module (following `rust-analyzer`'s `AstNode` trait with `cast`, `name()`, `type_ref()`, `modifiers()`, etc.) for the top ~20 declaration/expression nodes. |
| **Document symbol extraction** | `hir::symbols::extract_file_symbols` ([`crates/hir/src/symbols.rs#L184`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs#L184)) vs. `ide::collect_symbols` ([`crates/ide/src/lib.rs#L216`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/ide/src/lib.rs#L216)). | Reuse `hir::FileSymbols` (or shared `syntax::ast` helpers) in `ide::document_symbols`. |
| **Path segment extraction from `PATH` CST nodes** | `hir::workspace::clean_path_text` ([`crates/hir/src/workspace.rs#L491`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/workspace.rs#L491)) vs. `hir::symbols::path_segments` ([`crates/hir/src/symbols.rs#L575`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs#L575)). | Expose a single `ast::Path::segments()` / `to_dotted_string()` helper in `syntax`. |
| **Shebang `--source`/`--release` parsing** | `hir::workspace::parse_shebang_level` ([`crates/hir/src/workspace.rs#L552`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/workspace.rs#L552), private, operates on line 1) vs. `workspace_config::parse_shebang_level` ([`crates/java-analyzer/src/workspace_config/mod.rs#L200`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/workspace_config/mod.rs#L200), parses the whole file). | Expose a pure `pub fn parse_shebang_level(text: &str) -> Option<LanguageLevel>` in `syntax::language_level` (or `hir::workspace`) that checks `text.starts_with("#!")` and parses only the first line. |
| **Enclosing source-root prefix matching** | Duplicated 3 times: `WorkspaceLayout::check_file_package` ([`crates/hir/src/workspace.rs#L360-L366`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/workspace.rs#L360-L366)), `infer_workspace_layout`'s `find_enclosing_root` ([`crates/hir/src/workspace.rs#L790-L798`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/workspace.rs#L790-L798)), and `WorkspaceProbe::language_level_for_path` ([`crates/java-analyzer/src/workspace_config/mod.rs#L607-L649`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/workspace_config/mod.rs#L607-L649)). | Add `WorkspaceLayout::enclosing_source_root(&self, norm_path: &str) -> Option<(&ModuleData, &SourceRoot)>` and a small `is_path_within(path, root)` helper in `hir::workspace`. |
| **Local variable `LocalSymbol` construction** | Repeated 9 times across `resolve_declaration_site` ([`crates/hir/src/resolve.rs#L285-L383`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/resolve.rs#L285-L383)) and `resolve_local_scope` ([`crates/hir/src/resolve.rs#L770-L960`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/resolve.rs#L770-L960)). | Extract `local_symbol_from_node(file_id, decl_node, name_node, kind, include_dims)` in `hir::resolve`. |
| **Line/column offset calculation** | `convert::offset_to_position` ([`crates/java-analyzer/src/convert.rs#L230`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/convert.rs#L230)) vs. `support::line_col` ([`crates/syntax/tests/support/mod.rs#L485`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/tests/support/mod.rs#L485)). | Replace production code with `line-index` (cached per file). |
| **Database & cursor-marker test helpers** | `db_with`, `db_with_files`, `cursor`, `slice` are copy-pasted verbatim in [`crates/hir/src/lib.rs#L210-L241`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/lib.rs#L210-L241) and [`crates/ide/src/lib.rs#L368-L399`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/ide/src/lib.rs#L368-L399). | Add a small `test_utils` crate (or `hir::test_fixture` behind a `test-utils` feature) with a standard `$0` / `/*marker*/` fixture parser. |
| **Temporary workspace directory test helper** | Hand-rolled `TempDir` / `TempWorkspace` using `SystemTime::now().duration_since(UNIX_EPOCH)` duplicated 5 times: [`configure.rs#L564`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/configure.rs#L564), [`inspect.rs#L318`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/inspect.rs#L318), [`workspace_config/tests.rs#L11`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/workspace_config/tests.rs#L11), [`tests/lsp_goto_definition.rs#L15`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/tests/lsp_goto_definition.rs#L15), and inline in [`server.rs#L430,L481`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L430). | Replace with `tempfile::TempDir` in `[dev-dependencies]` plus a tiny shared test helper in `tests/common/mod.rs`. |

---

## 3. Build vs. Buy Evaluation (Custom Code vs. Standard Crates)

### `[workspace.dependencies]` Audit

**Finding:** Root [`Cargo.toml`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/Cargo.toml) currently has **no `[workspace.dependencies]` section**. Every member crate pins its own dependency versions (`rowan = "0.17"`, `salsa = "0.28"`, `serde_json = "1"` duplicated in `java-analyzer` and `xtask`, etc.). Centralizing all external and internal dependencies under `[workspace.dependencies]` will prevent version drift and make crate upgrades atomic.

### Component Classification

#### Keep Custom (Justified)

| Component | Location | Why Keep Custom | Internal Cleanup / Performance Notes |
|---|---|---|---|
| **Java Lexer** | [`crates/syntax/src/lexer.rs`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/src/lexer.rs) | Requires lossless trivia, JLS text-block escape validation, keeping `>` unmerged for nested generics, and zero-panic error recovery. No existing Rust crate matches these invariants. | Remove `.unwrap()` in `text_block` (line 199); replace linear `PUNCTUATION` slice scan with a 1-byte `match` dispatch for throughput. |
| **Event-based Java Parser & Tree Builder** | [`crates/syntax/src/parser/`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/src/parser/mod.rs) | Full Java 8–27 grammar, contextual keywords, bounded lookahead (`lookahead.rs`), resilient error recovery, and tight `rowan` green-tree integration. | Keep architecture; add a thin typed `syntax::ast` layer over `SyntaxNode` to eliminate raw `SyntaxKind` traversal in `hir`/`ide`. |
| **Language Level & Feature Gates** | [`crates/syntax/src/language_level.rs`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/src/language_level.rs) | Domain-specific Java JEP release/preview/withdrawal table. | Consolidate `Release::from_str` with `workspace_config::versions::parse_version_string` where appropriate. |
| **Pure Workspace Layout Inference** | [`crates/hir/src/workspace.rs`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/workspace.rs) | Core Spec 0004 differentiator (zero-config consensus roots, `package-info.java` authority, JPMS `module-info.java`, `//DEPS`, `{ include, exclude }` deltas). | Extract shared `enclosing_source_root` helper; reduce redundant `String` clones in step 6. |
| **Symbol Extraction & Name Resolution** | [`crates/hir/src/symbols.rs`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs), [`resolve.rs`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/resolve.rs) | Domain-specific JLS scoping, shadowing, static/wildcard import precedence, and overload arity rules. | Evolve toward position-independent `ItemTree` + `la-arena` (Spec 0009); return borrowed references where possible. |
| **Gradle / Version-Pin / JDK Introspectors** | [`workspace_config/{gradle,versions,jdk}.rs`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/workspace_config/gradle.rs) | Lightweight static heuristics that avoid spawning a JVM. | Keep custom; ensure `reconcile_version_conflict` preserves formatting. |
| **Grammar Fixture Harness** | [`crates/syntax/tests/support/`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/tests/support/mod.rs) | Purpose-built for paired `.java`/`.scm` S-expression grammar tests with `UPDATE_EXPECT=1` and `!!` invariant guards; already uses `similar = "2"`. | Keep for grammar corpus tests. |

#### Replace with Standard Crate

| Hand-Rolled Component | Location | Recommended Crate | Trade-Off Justification |
|---|---|---|---|
| **Line/Column ↔ Byte Offset Conversion** | [`convert.rs#L202-L251`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/convert.rs#L202-L251) | **`line-index`** | **High value.** Current `offset_to_position` scans from byte 0 on every call (**O(N)** per range, making `document_symbols` and `diagnostics` **O(N·M)**). `line-index` (from `rust-analyzer`) gives **O(log L)** lookups, full UTF-16/UTF-8/UTF-32 and CRLF support, and tiny compile cost. |
| **`file://` URI ↔ `PathBuf` Conversion & Percent-Encoding** | [`convert.rs#L9-L196`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/convert.rs#L9-L196) | **`url`** (bridged to `lsp_types::Uri`) | Removes ~190 lines of subtle cross-platform RFC 3986 / Windows drive / UNC / percent-encoding logic. `url::Url::from_file_path` / `to_file_path` is battle-tested; low compile cost. |
| **XML String-Slicing Parser (Maven/Ant/Ivy)** | [`workspace_config/xml.rs`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/workspace_config/xml.rs) (185 LOC) | **`roxmltree`** | Current slicer breaks on tags with attributes (e.g., `<dependency ...>`), XML entities (`&amp;`), CDATA, and namespaces. `roxmltree` is zero-copy, pure safe Rust, read-only DOM, and compiles in ~1s. (Keep minimal targeted text patching only for `reconcile_version_conflict` in `versions.rs`.) |
| **`eprintln!` Logging** | [`main.rs#L10,L14`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/main.rs#L10) | **`tracing`** + **`tracing-subscriber`** | Enables structured stderr logging with `RUST_LOG` filtering for LSP debugging without risking stdout JSON-RPC corruption. |
| **`Box<dyn Error + Sync + Send>` & Ad-Hoc `io::Error`** | [`main.rs#L5`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/main.rs#L5), [`server.rs#L29`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L29), [`inspect.rs#L256`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/inspect.rs#L256) | **`anyhow`** (binary/CLI) / **`thiserror`** (libs) | Idiomatic error context (`.with_context(...)`), cleaner signatures, minimal compile overhead. |
| **Standard `HashMap`/`HashSet`** | [`hir/src/lib.rs#L91`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/lib.rs#L91), [`server.rs#L34-L36`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L34-L36) | **`rustc-hash`** (`FxHashMap`/`FxHashSet`) | Faster hashing for integer/URI keys without SipHash overhead; near-zero compile cost. |
| **Short Identifier `String`s in HIR** | [`hir/src/symbols.rs`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs), [`resolve.rs`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/resolve.rs) | **`smol_str`** | Identifiers/names ≤23 bytes are inline (zero heap alloc) and `O(1)` to clone; directly reduces memory/clone overhead in `FileSymbols` and `WorkspaceIndex`. |
| **Hand-Rolled `TempDir` / `TempWorkspace` in Tests** | 5 copies across `crates/java-analyzer/` | **`tempfile`** (`[dev-dependencies]`) | Eliminates 5 duplicated helpers and fixes potential nanosecond-timestamp collisions under parallel `cargo test`. |
| **Inline Snapshot Assertions in `hir`/`ide` Tests** | [`crates/ide/src/lib.rs#L643-L757`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/ide/src/lib.rs#L643-L757) | **`expect-test`** (`[dev-dependencies]`) | Standard `rust-analyzer` inline snapshot testing with `UPDATE_EXPECT=1` support; tiny zero-dependency crate. |
| **Manual Integer IDs (`FileId`, `ModuleId`, `SourceRootId`, future `ItemTree` IDs)** | [`hir/src/workspace.rs#L17-L23`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/workspace.rs#L17-L23) | **`la-arena`** (when adding `ItemTree`) | Worth adopting during Spec 0009 (`ItemTree`) for type-safe arena-allocated `Idx<T>` handles instead of raw `Vec` indexing. |
| **Normalized UTF-8 Workspace Paths** | `PathBuf` + `.to_string_lossy()` + `normalize_rel_path` | **`camino`** (`Utf8PathBuf`) | Optional but clean upgrade once `vfs` / file watching (Spec 0015) is added; avoids lossy OS-string conversions. |

---

## 4. Idiomatic Rust & LSP Reliability Issues

### 1. Panics & `.unwrap()` / `.expect()` in Production Code
- **Unconditional panic on unknown `FileId`** ([`crates/hir/src/lib.rs#L109-L114`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/lib.rs#L109-L114)): `RootDatabase::source_file()` panics with `unwrap_or_else(|| panic!("unknown file id: {file_id:?}"))`. Any race or stale `FileId` crashes the server.
- **No `catch_unwind` around LSP request/notification dispatch** ([`crates/java-analyzer/src/server.rs#L326-L342`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L326-L342)): The parser intentionally contains defensive assertions (`PARSER_STEP_LIMIT` at [`parser/mod.rs#L131`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/src/parser/mod.rs#L131), `bump` assert at [`#L193`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/src/parser/mod.rs#L193), `Drop for Marker` panic at [`#L312`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/src/parser/mod.rs#L312), `unreachable!` in `grammar.rs` and `tree_builder.rs`). Without a `catch_unwind` boundary per request, any parser edge-case bug terminates the entire LSP server instead of returning `ErrorCode::InternalError`.
- **Unwrap in lexer hot path** ([`crates/syntax/src/lexer.rs#L62`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/src/lexer.rs#L62), [`#L199`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/syntax/src/lexer.rs#L199)): `text_block()` calls `rest[i..].chars().next().unwrap()`. Prefer `let Some(c) = rest[i..].chars().next() else { break; };` so the lexer's documented "never panics" guarantee holds structurally.
- **Direct slice indexing in `configure.rs`** ([`crates/java-analyzer/src/configure.rs#L500-L509`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/configure.rs#L500-L509)): Indexes `kinds[0]` without checking `!kinds.is_empty()`.

### 2. Blocking LSP Main Loop, Missing Cancellation & Diagnostics Bug
- **Synchronous main loop** ([`crates/java-analyzer/src/server.rs#L307-L344`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L307-L344)): `set_workspace_root()` walks the filesystem and reads/parses all `.java` files synchronously inside `run()` right after `initialize`, blocking all editor requests until startup finishes. All requests (`Hover`, `GotoDefinition`, `DocumentSymbol`) and notifications run synchronously on the main loop with no worker pool and no `salsa::Cancelled` handling.
- **`dedup_by` without sorting** ([`crates/java-analyzer/src/server.rs#L214`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L214)): `diagnostics.dedup_by(|a, b| a.range == b.range && a.message == b.message)` only removes *consecutive* duplicates; because `diagnostics` is never sorted after appending syntax, conflict, and package-mismatch diagnostics, non-adjacent duplicates are not removed.
- **Workspace conflicts spammed on every open file** ([`crates/java-analyzer/src/server.rs#L173-L182`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/server.rs#L173-L182)): Every open `.java` file gets the workspace-level `probe.conflicts` warnings injected at `0..0`, rather than reporting them once on the conflicting manifest or via `window/showMessage`.

### 3. Excessive `.clone()` and Allocations in `hir`
- **Cloning entire `TypeSymbol`s on every lookup** ([`crates/hir/src/resolve.rs#L1307-L1336`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/resolve.rs#L1307-L1336)): `lookup_type_by_fqn()` and `enclosing_type_symbols()` deep-clone full `TypeSymbol`s (including every `MemberSymbol` and `ParamSummary` `Vec`) even when only checking a single field or nested type name.
- **Cloning all static methods into a temporary `Vec`** ([`crates/hir/src/resolve.rs#L1397-L1403`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/resolve.rs#L1397-L1403)): `lookup_static_member_or_nested()` clones every static method of a class into a fresh `Vec<MemberSymbol>` just to pass a slice to `select_method()`. Passing an `only_static: bool` parameter or an iterator avoids the allocation entirely.
- **Repeated `SyntaxNode::text().to_string()` allocations** ([`crates/hir/src/resolve.rs#L630,L644,L672,L683,L1577,L1595`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/resolve.rs#L630)): For `NAME`/`NAME_REF` nodes wrapping a single `IDENT` token, extracting the token's `&str` (or comparing `node.text() == name` via `rowan::SyntaxText`'s `PartialEq<&str>`) avoids heap allocation.

### 4. Overly Broad `pub` Visibility & Silent Error Swallowing
- **Leaked Salsa internals in `hir`** ([`crates/hir/src/lib.rs#L25-L35`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/lib.rs#L25-L35)): `SourceFile` and `WorkspaceFiles` and their fields are `pub`, even though callers interact through `SourceDatabase`/`DefDatabase` and `RootDatabase` setters.
- **Public `WorkspaceIndex` maps** ([`crates/hir/src/symbols.rs#L156-L163`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs#L156-L163)): `types_by_fqn`, `types_by_package`, and `package_type_map` are `pub` fields, tightly coupling external callers to the internal `BTreeMap` layout.
- **Dead alias**: `pub type ImportItem = ImportDecl;` ([`crates/hir/src/symbols.rs#L37`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/hir/src/symbols.rs#L37)) is unused.
- **Silent config parse failure** ([`crates/java-analyzer/src/workspace_config/toml_config.rs#L145-L148`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/crates/java-analyzer/src/workspace_config/toml_config.rs#L145-L148)): Malformed `.java-analyzer.toml` silently falls back to `ExplicitConfig::default()` with no warning/diagnostic.

---

## 5. Prioritized Refactoring Plan

Each step is small, independent, behavior-preserving, and leaves `cargo test` green.

### Step 1: Centralize `[workspace.dependencies]` and Test Helpers **[New Dependency]** *(Completed)*
- [x] Add `[workspace.dependencies]` to root [`Cargo.toml`](file:///Users/rolandtepp/Developer/luolong/java-analyzer/Cargo.toml) for all shared external and internal crates (`rowan`, `salsa`, `serde`, `serde_json`, `lsp-server`, `lsp-types`, `clap`, `ignore`, `toml`, `similar`, plus `tempfile` and `expect-test` as dev-dependencies).
- [x] Replace the hand-rolled `TempDir`/`TempWorkspace`/`FixtureDir` structs in `crates/java-analyzer/` with `tempfile::TempDir`.
- [x] Deduplicate `db_with`, `db_with_files`, `cursor`, and `slice` test helpers between `hir` and `ide`.

### Step 2: Eliminate Redundant Parsing (`parse_shebang_level` & `probe_workspace` File Retention) *(Completed)*
- [x] Promote a line-1-only `parse_shebang_level(text: &str) -> Option<LanguageLevel>` into `syntax::language_level` (reused by `hir::workspace` and `java-analyzer::workspace_config`).
- [x] Return or retain scanned `(rel_path, String)` texts from `probe_workspace()` so `GlobalState::set_workspace_root` and `inspect::run_with_io` do not re-read and re-parse every `.java` file from disk.
- [x] Remove the full-file parse on every `didChange` inside `GlobalState::language_level_for`.

### Step 3: Replace O(N) Offset Math and Hand-Rolled URI Encoding in `convert.rs` **[New Dependency]** *(Completed)*
- [x] Add `line-index` and `url` to `[workspace.dependencies]`.
- [x] Replace `position_to_offset` / `offset_to_position` / `text_range_to_range` with `line_index::LineIndex` (cached per `FileId` in Salsa or `GlobalState`), turning O(N·M) `document_symbols`/`diagnostics` conversions into O(M log L).
- [x] Replace the ~190-line `path_to_uri` / `uri_to_path` / percent-encoding implementation with thin wrappers around `url::Url::from_file_path` / `to_file_path`.

### Step 4: Replace Hand-Rolled XML Slicer with `roxmltree` **[New Dependency]** *(Completed)*
- [x] Add `roxmltree` to `java-analyzer`.
- [x] Rewrite `maven.rs` and `ant_ivy.rs` to query parsed `roxmltree::Document` nodes, deleting `workspace_config/xml.rs` (keeping `expand_props` as a small helper).

### Step 5: Add Structured Logging, Error Types, and LSP Panic/Reliability Hardening **[New Dependency]** *(Completed)*
- [x] Add `tracing`, `tracing-subscriber`, `anyhow`, and `rustc-hash`.
- [x] Replace `eprintln!` in `main.rs` with `tracing` (configured to stderr), switch `RootDatabase::files` and `GlobalState` maps to `FxHashMap`/`FxHashSet`, and wrap `handle_request` / `handle_notification` in `std::panic::catch_unwind` so a bug never crashes the LSP server.
- [x] Fix `GlobalState::close()` to clean up removed files in `RootDatabase`, sort `diagnostics` before `dedup_by`, and remove the `.unwrap()` in `lexer.rs:199` and unchecked `kinds[0]` in `configure.rs:500`.

### Step 6: Fix `ide` ↔ `hir` Layering & Deduplicate CST/AST Helpers **[Crate Boundary Change]** *(Completed)*
- [x] Add a lightweight `syntax::ast` / `syntax::ext` module housing `collapsed_text`, `is_type_node`, `path_segments`, and thin typed node wrappers.
- [x] Replace `ResolvedSymbol.detail: Option<String>` with structured fields (or a `SymbolDetail` enum) so `ide::hover` deletes `trailing_paren_group_start`.
- [x] Route `ide::document_symbols` through `hir::FileSymbols` (or shared `syntax::ast` helpers) and move `check_file_package` diagnostics into `ide::diagnostics()` so `java-analyzer` no longer touches `syntax` directly.
- [x] Tighten `pub` visibility on `hir::{SourceFile, WorkspaceFiles}`, `WorkspaceIndex` internals, and remove the unused `ImportItem` alias.

### Step 7: Reduce `hir::resolve` Cloning & Adopt `smol_str` **[New Dependency]** *(Completed)*
- [x] Switch identifier/name fields in `hir::symbols` and `hir::resolve` from `String` to `SmolStr`.
- [x] Refactor `lookup_type_by_fqn` and `select_method` to borrow `&TypeSymbol` / `&MemberSymbol` instead of cloning whole structs and temporary `Vec`s.
- [x] Extract `local_symbol_from_node` in `hir::resolve` and `WorkspaceLayout::enclosing_source_root` in `hir::workspace`.

### Step 8: Introduce `AnalysisHost` / `Analysis` Facade & Background Request Execution **[Crate Boundary Change]** *(Completed)*
- [x] Add `ide::AnalysisHost` (wrapping `RootDatabase`) and `ide::Analysis` (holding a Salsa snapshot) so `java-analyzer` depends only on `ide` (no direct `hir` or `syntax` dependency).
- [x] Offload `Hover`, `GotoDefinition`, and `DocumentSymbol` requests to a worker thread pool with `salsa::Cancelled` handling, and run initial workspace indexing asynchronously off the main loop.

### Step 9: Position-Independent `ItemTree` & Per-Module Indexing (Prep for Spec 0009) **[Salsa Query Change]** **[New Dependency]** *(Completed)*
- [x] Add `la-arena` and separate `FileSymbols` into a position-independent `ItemTree` (no `TextRange`s, preserving true Salsa early-cutoff on body edits of any length) plus a per-file `AstIdMap` for range lookup.
- [x] Replace the monolithic `WorkspaceFiles` / `build_index` query with per-module / per-source-root symbol index queries so edits in one module do not invalidate unrelated modules.
