# Spec 0009: Module-Scoped Visibility & Multi-Release Source Roots (JEP 238)

Status: Accepted

## Motivation

`java-analyzer` already discovers multi-module workspaces, `Main`/`Test` source roots, and `ModuleDependency` edges (`crates/hir/src/workspace.rs`, Spec 0004), and already lowers files into position-independent `FileItemTree` and `AstIdMap` structures (originally scoped for Spec 0009, already shipped on `main`). However, two critical multi-module and multi-release gaps remain before cutting **`v0.1.0`**:

1. **Flat workspace symbol visibility:** `build_workspace_index_query` merges all `ModuleIndex`es into a single flat `WorkspaceIndex` regardless of module dependencies, `Main` vs. `Test` scope, or JPMS `module-info.java` `requires`/`exports` directives:
   - Production (`Main`) code can accidentally resolve `Test`-only classes (`src/test/java`).
   - Unrelated sibling modules leak types into each other's name resolution and `import *` scopes.
   - Duplicate FQNs across sibling modules or test fixtures overwrite each other globally instead of resolving relative to the caller's module classpath.
   - JPMS `exports pkg` and `exports pkg to friend.mod` encapsulation boundaries are not enforced between named workspace/JDK modules.
2. **No Multi-Release JAR (JEP 238) source-tree support:** Real-world libraries (Gson, Jackson, Log4j, Netty, JUnit) maintain version-specific source directories such as `src/main/java9`, `src/main/java17`, `src/main/java21`, or `src/main/versions/<N>/java`:
   - Today those directories either inherit the module's base language level (e.g. Java 8, causing false-positive syntax errors on Java 17 `record`s or `module-info.java`) or collide with the base `src/main/java` class of the same FQN instead of overlaying cleanly by target release.

This spec closes out **Phase 1 (`v0.1.0`)** by introducing module- and scope-aware symbol visibility (`Main`/`Test` + JPMS `requires`/`exports`) and JEP 238 multi-release source-root discovery, language-level assignment, and versioned FQN overlay resolution.

---

## Goals

1. **Enforce `ModuleId` & `RootScope` (`Main` vs. `Test`) Boundaries in `hir`:**
   - `Main` sources in module `M` see `M`'s `Main` roots plus `Main` roots of declared `Main`-scope module dependencies (and transitive `exported: true` dependencies) — **never** `Test` roots of `M` or any dependency.
   - `Test` sources in module `M` see `M`'s `Test` and `Main` roots (`Test` shadowing `Main` if patched), plus `Main` roots of `M`'s `Main`- and `Test`-scope module dependencies (and their transitive `exported: true` `Main` dependencies).
   - Sibling modules with duplicate FQNs resolve deterministically to the caller's own module / reachable dependency graph without global FQN clobbering.
2. **Enforce JPMS `module-info.java` Readability & Package Exports:**
   - Lower `module-info.java` `open module`, `requires [transitive] [static]`, and `exports pkg [to m1, m2]` into a position-independent `JpmsModuleSummary` on `FileItemTree` / `ModuleIndex` (zero `TextRange`s).
   - When a `Main` file in a **named JPMS module** `A` resolves a type in another **named JPMS module** `B` (workspace or JDK binary module):
     - `B` must be readable by `A` (`java.base` implicitly, direct `requires` / `requires static`, or transitive `requires transitive`).
     - `B` must export the target package unqualified or via `exports pkg to ..., A, ...`.
   - Unnamed modules (standard classpath modules without `module-info.java`) and `Test` roots without their own `module-info.java` continue to read all packages of their declared module dependencies and all exported JDK modules (matching `javac` unnamed-module / `--patch-module` test compilation semantics).
3. **JEP 238 Multi-Release Source Roots (`workspace.rs`, `workspace_config`, `hir`):**
   - Detect versioned source-root directories automatically from standard Maven/Gradle/layout conventions (`src/main/java<N>`, `src/main/java-<N>`, `src/main/versions/<N>[/java]`, `src/versions/<N>[/java]` for `9 <= N <= Release::LATEST`) and optional explicit `.java-analyzer.toml` overrides.
   - Assign `SourceRoot.release_override: Option<Release>` so every file under a `java17` root parses at `Release(17)` even when the enclosing module's base release is Java 8 or 11, with zero false-positive `workspace/package-mismatch` diagnostics.
   - Overlay versioned `Main` types (and versioned `module-info.java` descriptors) in ascending release order `None (base) < 9 < ... <= view_release`:
     - Base code (`view_release = base_release`) sees only base (`None`) types.
     - Versioned root `N` (`view_release = N`) sees the highest overlay `<= N` (inheriting base and lower `< N` layers while shadowing same-FQN base classes).
     - Downstream modules and `Test` roots see the highest overlay `<= caller_effective_release` (mirroring Spec 0006's binary `select_mrjar_entries` rule).
4. **Preserve Salsa Early-Cutoff & Zero-Config Ergonomics:**
   - Zero `TextRange`s in `FileItemTree`, `ModuleIndex`, `WorkspaceScopeIndex`, or `WorkspaceHierarchyIndex`.
   - Single-module / `db_with(...)` setups with no explicit module graph continue working with zero configuration and zero performance regression.

---

## Non-goals (for this spec)

- Emitting semantic diagnostics for inaccessible unexported JPMS packages or missing `requires` directives (owned by **Spec 0012**, which will reuse this spec's visibility predicates to emit `javac`-familiar *"package X is not visible"* diagnostics with `help: add 'requires ...' to module-info.java`).
- Gradle `testFixtures` (`java-test-fixtures` plugin) cross-module test-fixture sharing (can be added in **Spec 0015** as an optional `RootScope::TestFixtures` / dependency capability).
- Resolving external Maven/Gradle JAR coordinates per-module (currently workspace classpath JARs are shared via `ClasspathIndex`; per-module external JAR scoping belongs in **Spec 0015** alongside local cache resolution).

---

## Relationship to Other Specs

- **Spec 0004 (`0004-lightweight-project-and-workspace-model.md`):** Defines `ModuleId`, `RootScope`, `SourceRoot`, `ModuleDependency`, and `WorkspaceLayout`. This spec extends `SourceRoot` with `release_override: Option<Release>` and wires `WorkspaceLayout`'s module graph into `hir::DefDatabase` and `java-analyzer::GlobalState`.
- **Spec 0005–0008:** All name resolution (`hir::resolve`), type resolution (`hir::ty`), expression inference (`hir::infer`), and hierarchy queries (`hir::hierarchy`) consult the module-scoped lookup layer instead of a flat global map when resolving from a `FileId` context.
- **Spec 0006 (`0006-jdk-and-jar-classpath-indexing.md`):** Binary JEP 238 MR-JAR overlay rules (`select_mrjar_entries`) are mirrored here for source-tree MR-JAR overlays.
- **Position-independent `FileItemTree`:** Originally listed under Spec 0009 in early roadmaps; already implemented and verified on `main` (`crates/hir/src/item_tree.rs`).

---

## Pure Data Model & Algorithms (`crates/hir`)

### 1. Multi-Release `SourceRoot` Discovery (`crates/hir/src/workspace.rs`)

Extend `SourceRoot`:
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRoot {
    pub id: SourceRootId,
    /// Normalized workspace-relative directory path (`"src/main/java"`, `"src/main/java17"`).
    pub rel_path: String,
    pub scope: RootScope,
    /// Optional JEP 238 target release override (`Some(Release(17))` for `src/main/java17`,
    /// `None` for the base source root).
    pub release_override: Option<Release>,
    pub files: Vec<FileId>,
}
```

#### Automatic MR-JAR Path Recognition
When discovering source roots in `workspace.rs` (and Maven `build-helper-maven-plugin` / `maven-compiler-plugin` executions or Gradle `java9`/`java17` source sets in `workspace_config`):
- Recognize directory patterns under a module root (for valid `9 <= N <= Release::LATEST.number()`):
  - `src/main/java<N>` and `src/main/java-<N>` → `(RootScope::Main, Some(Release(N)))`
  - `src/main/versions/<N>/java` and `src/main/versions/<N>` → `(RootScope::Main, Some(Release(N)))`
  - `src/versions/<N>/java` and `src/versions/<N>` → `(RootScope::Main, Some(Release(N)))`
  - `src/test/java<N>` and `src/test/java-<N>` → `(RootScope::Test, Some(Release(N)))`
- Language level precedence for files in a `SourceRoot`:
  1. Per-file shebang (`#!java --source N`)
  2. `SourceRoot.release_override` (`LanguageLevel::new(N, module.language_level.preview)`)
  3. `module.test_language_level` (when `scope == RootScope::Test`)
  4. `module.language_level`

---

### 2. JPMS `module-info.java` Lowering (`crates/hir/src/item_tree.rs`)

Add position-independent JPMS metadata to `FileItemTree` (zero `TextRange`s):
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpmsRequires {
    pub module_name: SmolStr,
    pub is_transitive: bool,
    pub is_static: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpmsExports {
    pub package: String,
    /// Empty for unqualified `exports pkg;`, or target module names for `exports pkg to m1, m2;`.
    pub targets: Vec<SmolStr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpmsModuleSummary {
    pub name: SmolStr,
    pub is_open: bool,
    pub requires: Vec<JpmsRequires>,
    pub exports: Vec<JpmsExports>,
}
```
`FileItemTree` stores `pub jpms_module: Option<JpmsModuleSummary>` (superseding or complementing `module_name: Option<SmolStr>`).

---

### 3. Module Graph & Scope-Partitioned Indexing (`crates/hir/src/symbols.rs`, `lib.rs`)

#### File Origin & Module Graph Input
Introduce a pure, position-independent Salsa input `WorkspaceModuleGraph` (set via `AnalysisHost::set_workspace_module_graph` / `RootDatabase::set_workspace_module_graph`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FileOrigin {
    pub module_id: ModuleId,
    pub scope: RootScope,
    /// `None` for base root, `Some(Release(N))` for JEP 238 overlay root.
    pub release_override: Option<Release>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModuleGraphData {
    pub modules: BTreeMap<ModuleId, ModuleScopeConfig>,
    pub file_origins: BTreeMap<FileId, FileOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleScopeConfig {
    pub id: ModuleId,
    pub base_release: Release,
    pub test_release: Release,
    pub dependencies: Vec<ModuleDependency>,
}
```

#### Partitioned `ModuleIndex`
Instead of a single flat `types_by_fqn` per `ModuleIndex` that clobbers `Main` vs. `Test` vs. `java17` overlays, `ModuleIndex` stores layered slices:
- `main_base: SymbolSlice` (`RootScope::Main`, `release_override: None`)
- `main_overlays: BTreeMap<Release, SymbolSlice>` (`RootScope::Main`, `release_override: Some(N)`)
- `test_base: SymbolSlice` (`RootScope::Test`, `release_override: None`)
- `test_overlays: BTreeMap<Release, SymbolSlice>` (`RootScope::Test`, `release_override: Some(N)`)

*(For compatibility with callers without a `FileId` context — such as `type_hierarchy_subtypes(fqn)` — `WorkspaceIndex` retains an aggregated fallback view while context-sensitive queries use a `VisibleScope` view resolved for the active `FileId`!)*

---

### 4. Visibility & Overlay Resolution Algorithm

Given a lookup originating from `caller_file: FileId`:

1. **Determine Caller Context:**
   - Look up `origin = graph.file_origins.get(&caller_file)` (if `graph` is empty/unset, fall back to the legacy flat `WorkspaceIndex` + full `ClasspathIndex` — O(1) zero-config path).
   - Compute `caller_view_release`:
     - `origin.release_override`, else `test_release` (if `Test`), else `base_release`.
2. **Resolve Within Caller's Own Module (`origin.module_id`):**
   - If `origin.scope == RootScope::Test`:
     - Check `test_overlays` (`<= caller_view_release`, highest release wins) → `test_base` → `main_overlays` (`<= caller_view_release`, highest release wins) → `main_base`.
     - All packages in `origin.module_id` are visible (including non-exported internal packages).
   - If `origin.scope == RootScope::Main`:
     - Check `main_overlays` (`<= caller_view_release`, highest release wins) → `main_base`.
     - `Test` slices are **invisible**.
3. **Resolve Across Reachable Workspace Dependencies:**
   - Compute reachable `ModuleId`s in deterministic declaration order (BFS with `FxHashSet<ModuleId>` cycle guard):
     - Direct dependencies of `origin.module_id` matching `dep.scope == Main` (or `Main | Test` when `origin.scope == Test`).
     - Transitively follow `ModuleDependency::Module { scope: Main, exported: true, .. }` edges (plus JPMS `requires transitive` edges).
   - For each reachable `dep_module_id`:
     - Query **only `Main` slices** of `dep_module_id` (`main_overlays <= caller_view_release`, then `main_base`).
     - **JPMS Export Filter:** If `origin.scope == Main` AND `origin.module_id` has an active `JpmsModuleSummary` (`caller_jpms`) AND `dep_module_id` has an active `JpmsModuleSummary` (`dep_jpms`):
       - Package `pkg` is visible iff `dep_jpms.exports` contains `pkg` with `targets.is_empty()` or `targets.contains(&caller_jpms.name)`.
4. **Resolve Against Binary `ClasspathIndex` (JDK & JARs):**
   - If `origin.scope == Main` AND `origin.module_id` has an active `JpmsModuleSummary` (`caller_jpms`):
     - Compute readable JPMS module names: `"java.base"` + direct `caller_jpms.requires` + transitive `BinaryModuleInfo.requires_transitive` / workspace `JpmsRequires.is_transitive`.
     - A JDK binary type (`BinaryOrigin::Jdk { module_name: Some(m) }`) is visible iff `m` is in the readable set and `m` exports the type's package (`BinaryModuleInfo.exports`).
   - Otherwise (unnamed module or `Test` scope without its own `module-info.java`): all indexed `ClasspathIndex` types remain visible.

---

## LSP & CLI Integration (`crates/ide` & `crates/java-analyzer`)

- **`AnalysisHost` (`crates/ide`):**
  - Add `pub fn set_workspace_module_graph(&mut self, graph: Arc<ModuleGraphData>)` forwarding to `RootDatabase`.
- **`GlobalState` (`crates/java-analyzer/src/server.rs`) & `inspect` (`inspect.rs`):**
  - Build `ModuleGraphData` from the discovered `WorkspaceLayout` whenever workspace files/modules are synchronized.
  - Display `release_override` (e.g. `[java 17 overlay]`) in `java-analyzer inspect` output when versioned MR-JAR source roots are present.

---

## Resolved Design Decisions

1. **Cross-module `Test` visibility:** Cross-module dependencies strictly see the target module's `Main` roots in `v0.1.0` (matching standard Maven/Gradle; `test-jar` / Gradle `testFixtures` deferred to Spec 0015).
2. **MR-JAR source-root configuration:** Rely on automatic path-convention detection (`src/main/java<N>`, `src/main/java-<N>`, `src/[main/]versions/<N>[/java]`, `src/test/java<N>`) in `v0.1.0`, deferring custom `.java-analyzer.toml` per-root release syntax until `usage-config` lands in Spec 0015.
3. **External JAR automatic-module readability (`BinaryOrigin::Jar`):** Enforce strict JPMS `requires` and `exports` across workspace named modules and JDK JPMS modules (`BinaryOrigin::Jdk`), while keeping configured `BinaryOrigin::Jar` entries visible until per-JAR `Automatic-Module-Name` resolution lands in Spec 0015.

---

## Verification & Acceptance Criteria

1. **`Main` vs. `Test` isolation:**
   - `src/main/java` cannot resolve types declared in `src/test/java` (same module or another module); `src/test/java` resolves `src/main/java` types (including package-private/unexported packages of its own module).
2. **Cross-module dependency & transitive `exported` scoping:**
   - `module-a` -> `module-b` (`exported: false`) -> `module-c`: `module-a` resolves `module-b` types, does NOT resolve `module-c` types unless the `b -> c` edge is `exported: true` (`requires transitive` / Gradle `api`).
   - Duplicate FQNs in unrelated sibling modules (`module-x` and `module-y` both declaring `com.example.Config`) resolve to each module's own `Config` without colliding.
3. **JPMS `requires` & `exports` enforcement:**
   - Named module `com.app` with `requires com.lib;` resolves `com.lib.api.PublicService` (`exports com.lib.api;`), cannot resolve `com.lib.internal.Secret` (unexported), and respects qualified `exports com.lib.spi to com.app;`.
   - Named module without `requires java.sql;` resolves `java.lang.String` (`java.base`) but does not resolve `java.sql.Connection`; unnamed module resolves both.
4. **JEP 238 Multi-Release source roots:**
   - Module with base `Release(8)` + `src/main/java` + `src/main/java17`:
     - `src/main/java17` parses Java 17 `record` syntax cleanly with zero syntax or package-mismatch errors.
     - Base `src/main/java` (`Release(8)`) resolves the base `PlatformHelper`, while `src/main/java17` and Java 21 `src/test/java` resolve the `java17` overlay `PlatformHelper`!
5. **Salsa early-cutoff & zero regressions:**
   - Editing a method body in `module-b` does not invalidate `file_item_tree`, `module_index`, or `module-a`'s index; zero `.unwrap()`/`.expect()` in non-test code; `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --all-targets` pass cleanly.
