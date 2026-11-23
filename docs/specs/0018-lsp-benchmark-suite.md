# Spec 0018: Reproducible Java LSP Benchmark Suite

Status: Accepted

## Motivation

`java-analyzer` is designed to deliver rust-analyzer-class speed and low memory footprint for Java editing. To measure progress objectively across project sizes, build systems, and feature milestones (Specs 0008–0016), we need an apples-to-apples, reproducible benchmark suite comparing `java-analyzer` against established Java language servers:

- **`java-analyzer`** (this project — Rust, `rowan` + `salsa`)
- **Eclipse JDT.LS (`jdtls`)** — Eclipse ECJ-based Java language server (VS Code / Zed / Neovim standard)
- **`georgewfraser/java-language-server` (`jls`)** — `javac`-based Java language server
- **Apache NetBeans / Oracle Java LSP (`nbls`)** — NetBeans `nb-javac`-based Java language server

This spec defines a pure-Rust LSP benchmark runner in `xtask` (`cargo xtask bench`) paired with `mise.toml` tasks (`mise run bench-fetch`, `mise run bench`) that auto-fetch pinned servers and a full matrix of real-world & synthetic Java projects, measure cold start, warm start, steady-state warmed performance, memory, and hit rates across six LSP operation categories, and emit `target/bench/results.json`, aligned stdout tables, and `target/bench/BENCHMARKS.md`.

## Goals

1. **Project Size × Shape Matrix**:
   - Benchmark across **Small**, **Medium**, and **Large** repositories spanning **Single-Module Maven**, **Multi-Module Maven**, **Gradle (single & multi-module)**, **Ant (+Ivy)**, and **Plain Java (zero-config)** — including controlled type-hierarchy complexity shapes (simple, deep chain, diamond, wide fan-out).
2. **Cold Start, Warm Start & Post-Warmup Steady-State**:
   - Measure **Cold Start** (clean server cache / `-data` / `--userdir` / `XDG_CACHE_HOME`), **Warm Start** (second server launch reusing persisted cache), and **Post-Warmup Interactive Performance** (dedicated warmup phase before measured p50/p95/max sampling so JIT-based JVM servers and Salsa caches are both measured fairly at steady state).
3. **Comprehensive, Capability-Aware Operation Matrix**:
   - Cover 7 operation families: Lifecycle & Indexing, Navigation & Resolution, Type Hierarchies (varying complexities), Cross-References & Refactoring (`references`, `rename`, `codeAction`), **Autocompletion** (dot workspace/JDK/generic, scope, auto-import, override, `completionItem/resolve`), Incremental Editing & Diagnostics (`didChange` body vs. signature edits, `signatureHelp`), and Presentation (`semanticTokens`, `inlayHint`, `formatting`).
   - Gate each operation automatically on the server's advertised `ServerCapabilities`, reporting `N/A` for unsupported capabilities (so as `java-analyzer` implements Specs 0008–0014, new cells automatically activate without changing the harness).
   - Measure and report **three primary dimensions** on every `(server, repo, operation)` cell:
     1. **Response time / latency**: `p50`, `p95`, `max` (ms)
     2. **Memory footprint**: `peak_rss_mb` and `steady_rss_mb` across the full process tree
     3. **Error rate (%) & Hit rate (%)**:
        - **Error rate (%)**: `(jsonrpc_errors + timeouts + server_crashes) / total_requests` (hard protocol/runtime failures), plus **clean-file error diagnostic count** (false-positive `ERROR` diagnostics on known-valid repo files).
        - **Hit rate (%)**: `expected_matches / total_requests` (so fast-but-empty responses cannot masquerade as fast semantic resolution).
4. **Full Process-Tree Memory Accounting & Clean Teardown**:
   - Sample RSS every 250 ms across the server PID **and all descendant processes** (catching Maven/Gradle/javac helper daemons) and terminate the entire process group cleanly after every run.
5. **Deterministic Reporting**:
   - Emit `target/bench/results.json` (schema v1), column-aligned stdout summary matrices, and `target/bench/BENCHMARKS.md` (with `--md-out BENCHMARKS.md` when intentionally refreshing a tracked snapshot).

## Non-goals (for this spec)

- Modifying `crates/syntax`, `crates/hir`, `crates/ide`, or `crates/java-analyzer` (strictly `xtask` + `mise.toml` tooling).
- CI-gating PRs on absolute wall-clock timings on shared runners.

---

## Design

### 1. Benchmarked Servers (`mise run bench-fetch`)

Servers are downloaded/unpacked once into `target/bench/servers/<id>/` (cached unless `FORCE=1`):

| Server ID | Distribution Source | Launch Mode | Persisted Cache Dir (for Cold vs. Warm) |
| :--- | :--- | :--- | :--- |
| `java-analyzer` | Local `cargo build --release -p java-analyzer` | `target/release/java-analyzer lsp` (stdio) | `XDG_CACHE_HOME=<cache_dir>` |
| `jdtls` | Eclipse JDT.LS milestone/snapshot `.tar.gz` (`download.eclipse.org/jdtls/`) | `bin/jdtls -data <cache_dir>` (stdio) | `-data <cache_dir>` |
| `jls` | `georgewfraser.vscode-javac` `.vsix` (ZIP) | `dist/lang_server_{mac,linux}.sh` (stdio) | Temp working dir |
| `nbls` | Open VSX `oracle.oracle-java` `.vsix` (ZIP, bundles `nbcode`) | `nbcode --userdir <cache_dir> --start-java-language-server` | `--userdir <cache_dir>` |

Any locally installed or custom Java LSP can also be passed via `--server <name>=<cmd>`.

---

### 2. Project Size × Shape Matrix

The benchmark matrix spans **3 size tiers** and **6 project shapes / build systems**, combining pinned real-world OSS repositories with a deterministic **plain-Java synthetic generator** (so hierarchy depth/breadth can be scaled cleanly with zero build-tool confounders):

| Corpus ID | Size Tier | Shape / Build System | Approx. Files | Hierarchy / Codebase Characteristics |
| :--- | :--- | :--- | ---: | :--- |
| `plain-small` | Small | Plain Java (zero-config, generated) | ~150 | Controlled: deep chain (depth 8), diamond interfaces, wide fan-out (50 impls), generics |
| `jackson-core` | Small | Single-module Maven | ~250 | Tight parser/stream hierarchy (`JsonParser` / `JsonGenerator` subclasses) |
| `gson` | Small | Multi-module Maven | ~250 | Multi-module POM, reflection/generics, `TypeAdapter` hierarchy |
| `commons-lang` | Medium | Single-module Maven | ~500 | Large files (`StringUtils` 10k+ LOC), heavy method overloads, `Builder<T>` hierarchy |
| `ant-ivy` | Medium | Ant + Ivy | ~700 | Classic Ant `build.xml` + `ivy.xml`, deep resolver/module hierarchies |
| `gradle-medium` | Medium | Single/Multi-module Gradle (e.g. `picocli` or pinned Gradle repo) | ~400–600 | Gradle Kotlin/Groovy DSL, annotation-heavy + command hierarchies |
| `tomcat` | Large | Large Ant (multi-root) | ~2,500+ | Wide `Lifecycle` / `Valve` / `Container` hierarchies, large real-world server |
| `junit-framework` | Large | Multi-module Gradle | ~2,500+ | Deep multi-module Gradle build, heavy generics/annotations/extensions |
| `error-prone` | Large | Multi-module Maven | ~3,500+ | Massive multi-module Maven, wide `BugChecker` fan-out (500+ subclasses!) |

> **Filtering:** Users can run the full matrix (`mise run bench`) or slice by `--tier small|medium|large`, `--shape maven|gradle|ant|plain|single-module|multi-module`, or explicit `--repo <id>`.

---

### 3. Proposed Benchmark Operation Matrix (7 Categories)

Every operation is gated on the server's advertised `ServerCapabilities` (unsupported operations record `status: "unsupported"` / `N/A`) and reports **Latency (`p50` / `p95` / `max` ms)**, **Memory (`peak_rss_mb` / `steady_rss_mb`)**, **Error Rate (%)**, and **Hit Rate (%)**:

#### Category A — Lifecycle, First Response, Indexing & Memory
| Op ID | Description | Metrics |
| :--- | :--- | :--- |
| `cold_init` | Fresh empty cache dir → spawn → `initialize` response | ms |
| `cold_first_response` | Fresh empty cache dir → spawn → `initialized` + `didOpen` → **first non-empty user-visible response** (`documentSymbol` / `hover` sent immediately without waiting for background indexing) | ms |
| `cold_ready` | Fresh empty cache dir → spawn → full workspace + JDK indexing settled & cross-file semantic probe succeeds | ms, Peak RSS (MB) |
| `warm_init` | Second launch reusing cache dir → `initialize` response | ms |
| `warm_first_response` | Second launch reusing cache dir → `initialized` + `didOpen` → **first non-empty user-visible response** | ms |
| `warm_ready` | Second launch reusing cache dir → full workspace + JDK readiness | ms, Steady RSS (MB) |

> In addition, every interactive operation in Categories B–G records **`first_ms`** (its unwarmed first-call latency right after readiness, before the warmup loop) alongside post-warmup **`p50` / `p95` / `max` (ms)**.

#### Category B — Navigation & Resolution (Post-Warmup)
| Op ID | LSP Method | Probe Scenarios |
| :--- | :--- | :--- |
| `hover_workspace` | `textDocument/hover` | Local var, field, method call, generic/chained expression |
| `hover_jdk` | `textDocument/hover` | JDK stdlib types & methods (`String`, `List`, `Map`, `CharSequence`) |
| `def_workspace` | `textDocument/definition` | Same-file, cross-file, cross-module symbol |
| `def_jdk` | `textDocument/definition` | Jump to JDK stdlib class/method |
| `type_def` | `textDocument/typeDefinition` | Variable/parameter/field/`var` expression → type declaration |
| `doc_symbol_small` | `textDocument/documentSymbol` | Typical ~200 LOC file |
| `doc_symbol_large` | `textDocument/documentSymbol` | Large 2,000–10,000 LOC file (e.g. `StringUtils.java`) |
| `workspace_symbol` | `workspace/symbol` | Exact class name & 4-char prefix query |

#### Category C — Type Hierarchies (Varying Complexities)
| Op ID | LSP Method | Complexity Shape |
| :--- | :--- | :--- |
| `impl_simple` | `textDocument/implementation` | Interface/base class with 1–5 concrete implementations |
| `impl_deep` | `textDocument/implementation` | Overridden method across a 5–8 level inheritance chain |
| `impl_wide` | `textDocument/implementation` | Wide fan-out interface (50–500+ implementations, e.g. `Lifecycle`, `BugChecker`) |
| `hierarchy_super` | `prepareTypeHierarchy` + `typeHierarchy/supertypes` | Deep chain & diamond interface graph |
| `hierarchy_sub` | `prepareTypeHierarchy` + `typeHierarchy/subtypes` | Wide fan-out & multi-level subtype tree |

#### Category D — Cross-References & Refactoring Operations
| Op ID | LSP Method | Refactoring Scope |
| :--- | :--- | :--- |
| `refs_local` | `textDocument/references` | Local variable / private field (single file) |
| `refs_cross_file` | `textDocument/references` | Widely-referenced public utility method or interface across repo |
| `rename_local` | `prepareRename` + `rename` | Local variable rename (dry-run: computes `WorkspaceEdit`, does not mutate disk) |
| `rename_private` | `prepareRename` + `rename` | Private method/field rename within a large class |
| `rename_public` | `prepareRename` + `rename` | Public class or overridden interface method rename across files/hierarchy |
| `code_action_fix` | `textDocument/codeAction` | Quick-fix on an injected error (e.g. missing import / unimplemented method), then reverted |
| `code_action_refactor` | `textDocument/codeAction` | Refactoring actions on a valid expression/method selection |

#### Category E — Autocompletion (In-Memory Trigger + Revert)
| Op ID | LSP Method | Scenario & Expected Hit Check |
| :--- | :--- | :--- |
| `completion_dot_workspace` | `textDocument/completion` | Member dot-completion on a workspace class/interface receiver (`obj.`) — checks direct & inherited member label |
| `completion_dot_jdk` | `textDocument/completion` | Member dot-completion on a JDK stdlib receiver (`str.`, `list.`) — checks e.g. `substring` / `size` |
| `completion_dot_generic` | `textDocument/completion` | Member dot-completion on a generic/chained expression (`list.get(0).`) — checks substituted type member |
| `completion_scope` | `textDocument/completion` | Unqualified lexical scope completion inside a method body (locals, params, fields, methods) |
| `completion_auto_import` | `textDocument/completion` | Type-name prefix completion (`ArrayLi`) requiring index search + auto-import `additionalTextEdits` |
| `completion_override` | `textDocument/completion` | Override method completion inside a subclass body |
| `completion_resolve` | `completionItem/resolve` | Lazy detail/Javadoc resolution on the top returned completion item (when `resolveProvider` is advertised) |

> Each completion probe injects its trigger edit via in-memory `didChange`, invokes `textDocument/completion` (and `completionItem/resolve`), records latency/errors/hit-rate, and reverts the buffer via `didChange` without touching disk.

#### Category F — Incremental Editing, Diagnostics & Signature Help
| Op ID | LSP Method | Scenario |
| :--- | :--- | :--- |
| `edit_body` | `didChange` (inside method body) → `hover` + `publishDiagnostics` | Tests incremental early-cutoff (e.g. Salsa `FileItemTree` vs. full recompile) |
| `edit_signature` | `didChange` (modify public method/field signature) → `hover` + `publishDiagnostics` | Tests cross-file invalidation & re-indexing cost |
| `signature_help` | `textDocument/signatureHelp` | Overloaded method call argument tracking |
| `clean_diagnostics` | `textDocument/publishDiagnostics` | False-positive `ERROR` diagnostic count on untouched valid probe files |

#### Category G — Presentation & Formatting
| Op ID | LSP Method | Scenario |
| :--- | :--- | :--- |
| `semantic_tokens` | `textDocument/semanticTokens/full` | Large file full semantic tokenization |
| `inlay_hints` | `textDocument/inlayHint` | Range containing `var`, chained calls, and method arguments |
| `formatting` | `textDocument/formatting` | Full-document formatting on a medium/large file |

---

### 4. Execution Protocol & Fairness Rules (`xtask/src/bench/`)

1. **Cold Start → First-Call Capture → Warmup Period → Measured Steady-State → Warm Restart**:
   - **Phase 1 (Cold Start & Cold First Response):** Start server with an empty cache dir, measure `cold_init`, immediately open the initial probe file and fire a `documentSymbol` / `hover` request to measure `cold_first_response` (time to first useful editor response), then wait for full semantic readiness (`cold_ready`, bounded by `--ready-timeout`, default 180 s), sampling full process-tree RSS every 250 ms.
   - **Phase 2 (Unwarmed First-Call & Warmup Period):** Record the unwarmed `first_ms` latency of each supported probe on pass 1, then execute `--warmup` full passes (default `5`, or `--warmup-secs`, default `3s` minimum) across all supported probes so JIT compilers (HotSpot C2 in JDT.LS/JLS/NBLS) and incremental caches (Salsa in `java-analyzer`) reach steady state.
   - **Phase 3 (Measured Interactive Workload):** Execute `--iterations` measured runs (default `15`) per probe in deterministic order, recording `p50`, `p95`, `max`, `error_rate`, `hit_rate`, and `steady_rss_mb`. For mutating probes (`edit_body`, `edit_signature`, `completion_*`, `code_action_fix`), send `didChange`, measure follow-up response, and restore original buffer via `didChange` (never mutating files on disk; `rename` is dry-run inspecting the returned `WorkspaceEdit`).
   - **Phase 4 (Warm Restart & Warm First Response):** Cleanly shut down the server (`shutdown` + `exit`), restart it against the **same** cache dir, and measure `warm_init`, `warm_first_response`, and `warm_ready`.
   - **Teardown:** Send `shutdown` + `exit`, wait up to 5 s, then terminate the entire process group so zero orphan JVMs or Gradle/Maven daemons remain.
2. **Anchor-Based Probes (`xtask/bench/probes.toml`)**:
   - Probes specify `(file, anchor, symbol)` rather than hardcoded line numbers and resolve to exact UTF-16 `(line, character)` offsets at runtime.
   - For `plain-small`, `xtask` generates a deterministic plain-Java project under `target/bench/corpus/plain-small/` with known deep/diamond/wide hierarchies and matching probes.
3. **Server-Initiated Request Handling**:
   - Automatically replies cleanly to `window/workDoneProgress/create`, `client/registerCapability`, `workspace/configuration`, and `client/unregisterCapability` so JVM servers never stall.
   - Adapts `didChange` payloads to each server's advertised `textDocumentSync` (full vs. incremental).
4. **Full Process-Tree Memory Sampling**:
   - Uses portable `ps -o pid=,ppid=,rss=` (macOS & Linux, zero external crates) to sum RSS recursively over the server PID and all descendant processes.

---

### 5. CLI & `mise.toml` Interface

```text
Usage: cargo xtask bench [OPTIONS]

Options:
  --server <NAME=CMD>        Server spec to benchmark (repeatable)
  --repo <ID=PATH>           Repository path to benchmark (repeatable)
  --tier <small|medium|large> Filter matrix by size tier (repeatable)
  --shape <SHAPE>            Filter matrix by shape: plain|maven|gradle|ant|single-module|multi-module
  --ops <CATEGORIES>         Comma-separated op categories: lifecycle,nav,hierarchy,refactor,completion,edit,present [all]
  --probes <PATH>            TOML probe manifest [embedded xtask/bench/probes.toml]
  --warmup <N>               Warmup iterations per probe [5]
  --iterations <N>           Measured iterations per probe [15]
  --ready-timeout <SECS>     Max seconds to wait for server readiness [180]
  --request-timeout <SECS>   Max seconds per individual LSP request [30]
  --skip-warm-restart        Skip Phase 4 warm-restart measurement
  --json-out <PATH>          Output JSON path [target/bench/results.json]
  --md-out <PATH>            Output Markdown path [target/bench/BENCHMARKS.md]
  -h, --help                 Show this help
```

`mise.toml` tasks:
- `mise run bench-fetch`: Downloads & unpacks pinned servers (`jdtls`, `jls`, `nbls`) and pinned corpora into `target/bench/` in parallel using `xh`, plus generates `plain-small`.
- `mise run bench [ARGS...]`: Builds release `java-analyzer` and `xtask`, ensures assets via `bench-fetch`, runs `cargo xtask bench`, prints aligned summary matrices to stdout, and writes `target/bench/results.json` + `target/bench/BENCHMARKS.md` (use `--md-out BENCHMARKS.md` to refresh a tracked repo snapshot).

---

### 6. Output Report Structure (`BENCHMARKS.md`)

1. **Environment & Revisions**: OS/arch, CPU model/cores, RAM, `java -version`, `java-analyzer` revision, and exact versions of `jdtls`, `jls`, `nbls`.
2. **Matrix 1 — Lifecycle, First Response, Memory & Reliability by Project Size × Shape**:
   - Rows: `(repo, tier, shape, files)` × `server`
   - Columns: `Cold Init`, `Cold 1st Resp`, `Cold Ready`, `Warm Init`, `Warm 1st Resp`, `Warm Ready`, `Peak RSS`, `Steady RSS`, `Error Rate`, `Clean-File Errors`
3. **Matrix 2 — Interactive Latency (Unwarmed 1st Call + Post-Warmup p50 / p95), Error Rate & Hit Rate by Operation Category**:
   - Navigation (`hover`, `definition`, `typeDefinition`, `documentSymbol`, `workspace/symbol`)
   - Type Hierarchies (`impl_simple`, `impl_deep`, `impl_wide`, `supertypes`, `subtypes`)
   - Cross-References & Refactoring (`references`, `rename_local`, `rename_private`, `rename_public`, `codeAction`)
   - Autocompletion (`completion_dot_workspace`, `completion_dot_jdk`, `completion_dot_generic`, `completion_scope`, `completion_auto_import`, `completion_override`, `completion_resolve`)
   - Incremental Editing & Diagnostics (`edit_body`, `edit_signature`, `signatureHelp`, `clean_diagnostics`)
   - Presentation (`semanticTokens`, `inlayHint`, `formatting`)
4. **Capability Coverage Matrix**: Explicit `supported` / `N/A` matrix showing which operations each server currently advertises.

---

## Verification & Acceptance Criteria

1. **Unit & mock-LSP tests (`xtask/tests/bench.rs`)**:
   - Anchor-to-UTF-16 position resolution, plain-Java synthetic project generator (verifying deep/diamond/wide hierarchy shapes).
   - Process-tree RSS parser across multi-level parent/child trees.
   - Percentile (`p50`/`p95`/`max`), hit-rate, capability-gating (`N/A`), JSON v1 schema, and deterministic Markdown/stdout matrix formatting.
   - End-to-end LSP harness test against a mock LSP and against `java-analyzer lsp` verifying all 4 phases (cold start, warmup, measured ops, warm restart) and clean teardown.
2. **Verification gate**: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --all-targets` pass cleanly.
