# Spec 0006: JDK & JAR Binary Classpath Indexing

Status: Accepted

## Motivation

With Spec 0004 (workspace layout & JDK discovery) and Spec 0005 (cross-file source symbol indexing & name resolution), `java-analyzer` resolves symbols declared across workspace `.java` files. However, real Java code constantly references standard library types (`Object`, `String`, `List`, `Map`, `Optional`, `System.out`, `Objects.requireNonNull`) and external `.jar` dependencies. Without binary classpath indexing:

- Hovering or invoking go-to-definition on JDK/JAR types falls back to generic identifier text or returns `None`.
- Member access on standard-library-typed receivers (e.g. `String s; s.length()`) cannot resolve.
- **Spec 0007 (Type Hierarchy & Inheritance)** cannot resolve `extends Object`, `implements List<T>`, `implements AutoCloseable`, or other binary supertypes.

This spec defines how `java-analyzer` indexes compiled `.class` files from the active JDK (`jmods/*.jmod` / JDK 8 `rt.jar`) and local `.jar` / class-directory dependencies (including **JEP 238 Multi-Release JARs**), exposes them through `hir`'s pure `DefDatabase`, upgrades semantic `hover` and member resolution, and supports on-demand go-to-definition via JDK `src.zip` or synthesized read-only `.java` stubs — **with zero running JVM required**.

## Goals

1. **Zero-JVM binary indexing using well-maintained workspace crates (`Build vs. Buy`)**:
   - Declare shared dependencies in root `[workspace.dependencies]` (`Cargo.toml`).
   - Use **`ristretto_classfile`** in `crates/hir` (pure `&[u8]` parsing, zero IO) because it explicitly supports Java 8 through Java 27 (classfile major versions `52–71`), parses `Module`, `Record`, `PermittedSubclasses`, `InnerClasses`, `Signature`, and `MethodParameters`, and supports pure-Rust `.class` serialization for JVM-free TDD unit tests. (`cafebabe 0.9.0` stops at Java 21 / major `65`.)
   - Use **`zip`** (`default-features = false, features = ["deflate"]`) in `crates/java-analyzer` for reading `.jar`, `.jmod`, and `src.zip` archives, and a small pure-Rust `jimage` reader (`crates/java-analyzer/src/jimage.rs`) for OpenJDK `lib/modules` (supporting both jimage v1.0 and v1.1, including JEP 493 JDK 24–27 runtimes that omit `jmods/`).
2. **Strict 4-layer architecture & sans-IO core (`AGENTS.md` Rule 1 & Rule 6, `M-DONT-LEAK-TYPES`, `M-IMPL-IO`)**:
   - All filesystem IO (reading `$JAVA_HOME/jmods/*.jmod`, `jre/lib/rt.jar`, `.jar` files, class directories, `lib/src.zip`, and writing on-demand cached stub/source files) lives **exclusively in `crates/java-analyzer`**.
   - `crates/hir` remains 100% pure and IO-free, receiving an `Arc<ClasspathIndex>` via a Salsa 0.28 input on `RootDatabase` and never leaking `ristretto_classfile` types outside `crates/hir::classfile`.
   - `crates/java-analyzer` depends **only on `crates/ide`** (never directly on `hir` or `syntax`). Pure classpath types, stub synthesis (`ide::synthesize_binary_stub`), and source symbol location (`ide::locate_binary_symbol_in_source`, which calls `syntax::parse` inside `ide`) are exposed cleanly through `crates/ide` without glob re-exports (`M-NO-GLOB-REEXPORTS`).
   - `crates/syntax` stays pure and Salsa-free.
3. **JEP 238 Multi-Release JARs**:
   - Detect `Multi-Release: true` in `META-INF/MANIFEST.MF` and overlay `META-INF/versions/<N>/` entries for `9 <= N <= active_release` in ascending order of `N` on top of base `.class` entries (including versioned `module-info.class`), ignoring `N > active_release`.
4. **Implicit `java.lang.*` & `import module` resolution**:
   - Resolve unqualified `java.lang.*` types automatically (after workspace single-type imports and same-package types, per JLS §6.3 / §7.5).
   - Resolve `import module <M>;` (JEP 511) using exported packages parsed from each JDK/JAR `module-info.class` (including `requires transitive` re-exports, cycle-safe).
5. **Semantic hover (with on-demand Javadoc) & on-demand go-to-definition for binary symbols**:
   - `ide::hover` renders rich Java signatures (with generics, parameter names when available, and module/JAR origin) for resolved binary types and members, enriched on-demand with Javadoc extracted from `$JAVA_HOME/lib/src.zip`, companion `*-sources.jar` (`/** ... */`), or companion `*-javadoc.jar` HTML when available.
   - `textDocument/definition` on a binary symbol materializes the real `.java` source from `$JAVA_HOME/lib/src.zip` (for JDK types) or a companion `*-sources.jar` (for JAR types), falling back to a synthesized read-only `.java` stub file in the OS cache directory, locates the exact declaration range via `ide::locate_binary_symbol_in_source`, and returns a standard `file://` `Location` that works out of the box in Zed, VSCode, and Neovim.
6. **Step 1 indexing scope**:
   - Index the resolved JDK (`probe.resolved_jdk`) and local filesystem `.jar` / class-directory `ModuleDependency::Path` entries.

## Non-goals (for this spec)

- Resolving Maven/Gradle/Ivy/Git coordinates from `~/.m2`, `~/.gradle`, or remote repositories (deferred to **Spec 0015**).
- Full inheritance traversal (`extends`/`implements` member lookup) and generic type-argument substitution (covered by **Spec 0007** and **Spec 0008**; this spec extracts and stores `super_class`, `interfaces`, `permitted_subclasses`, and raw/generic signatures so Spec 0007/0008 can consume them directly).
- Bytecode decompilation of method bodies (only public/protected API signatures and stubs are needed).

---

## Engineering & Project-Skill Constraints

All implementation code for this spec must adhere to `AGENTS.md` Rules 1–8 and the curated Rust skills in `.agents/skills/` (`rust-best-practices`, `ms-rust-guidelines`, `rust-2024-idioms`), with Rule 6 taking precedence on any conflict:

- **Panic-free & observable (`AGENTS.md` Rule 6, `obs-structured-fields`, `M-LOG-NOT-PRINT`):** Zero `.unwrap()`, `.expect()`, or unchecked indexing in non-test code. Malformed `.class` files, corrupt `.jar`/`.jmod` archives, or missing `src.zip` entries must never panic the parser or LSP server — return typed `Result`s in `hir` and log recoverable archive/classfile failures in `java-analyzer` via structured `tracing::warn!(path = %..., error = %..., "...")` (never `eprintln!`). `hir` and `ide` never install a tracing subscriber (`obs-library-facade`).
- **Salsa determinism & early-cutoff (`AGENTS.md` Rule 6, `M-AVOID-STATICS`):** Never store `TextRange`s in `ClasspathIndex`; use deterministic `BTreeMap` collections; avoid global mutable statics (`M-AVOID-STATICS`); always drop any `Analysis` snapshot before mutating `AnalysisHost`/`RootDatabase` on the same thread.
- **Memory & allocation discipline (`AGENTS.md` Rule 6, `mem-compact-string`, `M-INITIAL-CAPACITY`, `M-SHRINK-TO-FIT`):** Use `SmolStr` (matching `TypeSymbol`/`MemberSymbol` in `crates/hir/src/symbols.rs`) for short identifiers (`name`, `type_params`, `nested_types`, `module_name`, `member_name`); prefer borrowing (`&str`, `&[u8]`) over `.clone()` and `String`/`Vec` allocations; pre-size collections when counts are known; keep `GlobalState::snapshot()` O(1) using `Arc<ClasspathIndex>` and `Option<Arc<PathBuf>>`.
- **Rust 2024 & API hygiene (`rust-2024-idioms`, `M-LINT-OVERRIDE-EXPECT`):** Use Rust 2024 idioms (`if let` chains, `let ... else`, exhaustive `match`), tight visibility (`pub(crate)` / private by default), `#[must_use]` on pure constructors/queries, `TryFrom` over narrowing `as` casts (`num-cast-try-from`), and `#[expect(...)]` with justification if a lint override is ever needed. All types stored in `RootDatabase` and `GlobalStateSnapshot` must be `Send + Sync` (`M-TYPES-SEND`).

---

## Pure Data Model in `crates/hir`

### `crates/hir/src/classpath.rs` & `crates/hir/src/classfile.rs`

```rust
use std::collections::BTreeMap;
use smol_str::SmolStr;

/// Where a binary type originated (used for hover display and on-demand source/stub lookup).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BinaryOrigin {
    /// A JPMS JDK module (e.g. `module_name = Some("java.base")`),
    /// or JDK 8 `rt.jar` (`module_name = None`).
    Jdk {
        module_name: Option<SmolStr>,
    },
    /// A local `.jar` archive or compiled classes directory.
    Jar {
        /// Display name / normalized path (e.g. `"lib/junit-4.13.2.jar"`).
        label: String,
    },
}

/// Identifies a resolved binary type or member for `hover` and `goto_definition`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BinaryTarget {
    pub origin: BinaryOrigin,
    /// JVM internal binary name (e.g. `"java/util/Map$Entry"`).
    pub internal_name: String,
    /// Canonical dot-separated FQN (e.g. `"java.util.Map.Entry"`).
    pub qualified_name: String,
    /// Member simple name when targeting a field, method, constructor, enum constant, or record component.
    pub member_name: Option<SmolStr>,
}

/// A compiled Java type parsed from a `.class` file (no source `FileId` / `TextRange`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryTypeSymbol {
    /// Simple name (e.g. `"String"`, `"Entry"`).
    pub name: SmolStr,
    /// Canonical dot-separated FQN (e.g. `"java.lang.String"`, `"java.util.Map.Entry"`).
    pub qualified_name: String,
    /// JVM internal binary name (e.g. `"java/util/Map$Entry"`), used for `src.zip` & stub lookup.
    pub internal_name: String,
    /// Package name (`""` for unnamed package, `"java.lang"` otherwise).
    pub package: String,
    /// Enclosing type's canonical FQN if nested (e.g. `Some("java.util.Map")`).
    pub enclosing_type: Option<String>,
    pub kind: TypeKind,
    pub is_public: bool,
    pub is_static: bool,
    pub is_final: bool,
    pub is_abstract: bool,
    /// Type parameters from generic `Signature` attribute (e.g. `["E"]`, `["K", "V"]`).
    pub type_params: Vec<SmolStr>,
    /// Superclass canonical FQN (e.g. `Some("java.lang.Object")`; `None` for `java.lang.Object`).
    pub super_class: Option<String>,
    /// Directly implemented/extended interface FQNs.
    pub interfaces: Vec<String>,
    /// Sealed `permits` FQNs from `PermittedSubclasses` attribute (Java 17+).
    pub permitted_subclasses: Vec<String>,
    /// Public/protected fields, methods, constructors, enum constants, and record components.
    pub members: Vec<BinaryMemberSymbol>,
    /// Simple names of directly nested public/protected types.
    pub nested_types: Vec<SmolStr>,
    pub origin: BinaryOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryMemberSymbol {
    pub owner_type: String,
    pub name: SmolStr,
    pub kind: MemberKind,
    pub is_public: bool,
    pub is_static: bool,
    /// Human-readable return or field type (decoded from generic `Signature` if present, else JVM descriptor).
    pub type_ref: Option<String>,
    /// Method/constructor parameters (using `MethodParameters` names when present, else `arg0`, `arg1`, ...).
    pub params: Vec<ParamSummary>,
    pub is_varargs: bool,
}

/// Parsed JPMS module exports from `module-info.class` (used for `import module M;`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BinaryModuleInfo {
    pub name: SmolStr,
    /// Unqualified exported packages (e.g. `["java.lang", "java.util", ...]`).
    pub exports: Vec<String>,
    /// `requires transitive` target module names.
    pub requires_transitive: Vec<SmolStr>,
}

/// Pure, deterministic index of all binary JDK and JAR types visible to the workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClasspathIndex {
    /// Canonical FQN (`"java.lang.String"`, `"java.util.Map.Entry"`) -> `BinaryTypeSymbol`.
    pub types_by_fqn: BTreeMap<String, BinaryTypeSymbol>,
    /// Package name (`"java.lang"`) -> sorted top-level type FQNs.
    pub types_by_package: BTreeMap<String, Vec<String>>,
    /// Package name -> (simple name -> FQN) for O(log N) lookups.
    pub package_type_map: BTreeMap<String, BTreeMap<SmolStr, String>>,
    /// JPMS module name (`"java.base"`) -> `BinaryModuleInfo`.
    pub modules: BTreeMap<SmolStr, BinaryModuleInfo>,
}
```

### Pure `.class` Parsing (`crates/hir/src/classfile.rs`)

- `parse_class_bytes(bytes: &[u8], origin: BinaryOrigin) -> Result<Option<ParsedClassfile>, ClassfileError>` where `ParsedClassfile` is either `Type(BinaryTypeSymbol)` or `Module(BinaryModuleInfo)`.
- Returns `Ok(None)` for filtered-out entries:
  - Synthetic/bridge members (`ACC_SYNTHETIC`, `ACC_BRIDGE`) and anonymous/local classes (`$1`, `$1Local`, or `InnerClasses` entries with no outer class).
  - Non-public/non-protected top-level & member declarations (package-private/private implementation details are excluded from the external binary API index).
- Decodes:
  - **JVM descriptors** (`Ljava/lang/String;`, `[I`, `(Ljava/lang/String;I)Z`) into Java type strings (`String`, `int[]`, etc.).
  - **JVM generic `Signature` attributes** (`<K:Ljava/lang/Object;V:Ljava/lang/Object;>Ljava/lang/Object;`) into readable type parameters, supertypes, field types, and method signatures, falling back gracefully to raw descriptors if a signature is malformed.
  - **`Record` attribute** into `MemberKind::RecordComponent` entries and `TypeKind::Record`.
  - **`InnerClasses` attribute** to distinguish real nested types (`java.util.Map.Entry`) from `$` in identifiers and capture `is_static`.
  - **`PermittedSubclasses` attribute** into `permitted_subclasses`.
  - **`MethodParameters` attribute** for parameter names when compiled with `-parameters`, falling back to `arg0`, `arg1`, ...

### Pure JEP 238 Multi-Release Overlay (`crates/hir/src/classpath.rs`)

```rust
/// Parses `META-INF/MANIFEST.MF` and returns `true` iff `Multi-Release: true` is present.
#[must_use]
pub fn parse_manifest_multi_release(manifest_text: &str) -> bool;

/// Selects the effective `.class` entry bytes for a JAR given `is_multi_release` and `target_release`.
///
/// Entries are `(zip_entry_path, bytes)`:
/// - Base entries live at `<pkg>/<Name>.class` (and `module-info.class`).
/// - When `is_multi_release == true`, entries under `META-INF/versions/<N>/<rel>` with
///   `9 <= N <= target_release.number()` override base entries in ascending order of `N`.
/// - Entries with `N > target_release.number()` are ignored.
#[must_use]
pub fn select_mrjar_entries<'a>(
    entries: &'a [(&'a str, &'a [u8])],
    is_multi_release: bool,
    target_release: Release,
) -> BTreeMap<String, &'a [u8]>;
```

---

## Salsa 0.28 & Resolution Integration (`crates/hir`)

### `DefDatabase` & `RootDatabase` (`crates/hir/src/lib.rs`)

Use Salsa 0.28's `#[salsa::input]` struct pattern (matching `SourceFile`, `ModuleFiles`, and `WorkspaceModules`):

```rust
#[salsa::input]
pub(crate) struct ClasspathInput {
    pub(crate) index: Arc<ClasspathIndex>,
}
```

- Add `classpath_input: Option<ClasspathInput>` to `RootDatabase` (`None` in `RootDatabase::default()`, returning `Arc::new(ClasspathIndex::default())` without panicking).
- Add `RootDatabase::set_classpath_index(&mut self, index: Arc<ClasspathIndex>)` using `salsa::Setter` when `classpath_input` already exists (only updating when `*existing.index(self) != *index` to preserve early-cutoff).
- Extend `DefDatabase`:
  ```rust
  pub trait DefDatabase: SourceDatabase {
      // ... existing queries ...

      /// Indexed JDK and external JAR binary symbols.
      fn classpath_index(&self) -> Arc<ClasspathIndex>;
  }
  ```

### Updated Name Resolution Precedence (`crates/hir/src/resolve.rs`)

Workspace source symbols always take precedence over binary classpath symbols (matching `javac`'s `-sourcepath` / source-over-binary rule). For type names:

1. Lexical local types & enclosing type parameters / nested types (existing Spec 0005 rules).
2. Single-type imports (`import a.b.Foo;`): check `workspace_index`, then `classpath_index`.
3. Static single imports resolving to a nested type: check `workspace_index`, then `classpath_index`.
4. Same-package types: check `workspace_index`, then `classpath_index`.
5. **Implicit `java.lang.*`** (for simple unqualified type names): check `classpath_index` in package `"java.lang"`.
6. Type-import-on-demand (`import a.b.*;`): check `workspace_index`, then `classpath_index`.
7. Static-import-on-demand (`import static a.b.Outer.*;`): check `workspace_index`, then `classpath_index`.
8. **Module imports (`import module M;`, JEP 511)**: look up `M` in `classpath_index.modules` (following `requires_transitive` transitively with cycle detection, e.g. `java.se` → `java.base`, `java.sql`, ...) and check each exported package in `classpath_index`.
9. Qualified type paths (`java.util.List`, `java.util.Map.Entry`, including inside `import` declarations) and package prefixes in `import` declarations: check `workspace_index`, then `classpath_index`.
10. Static imports (`import static java.util.Objects.requireNonNull;`, `import static java.lang.Math.*;`) and member access (`System.out`, `s.length()`, `List.of(...)`, `String::valueOf`): check workspace types first, then `classpath_index` types (with the same arity/varargs overload preference).

`ResolvedSymbol` carries `pub binary_target: Option<BinaryTarget>` alongside `file_id: Option<FileId>` so `ide` and `java-analyzer` know when a resolved symbol lives in a JDK module or JAR.

---

## IDE & LSP Behavior (`crates/ide` & `crates/java-analyzer`)

### `crates/ide`
- **Explicit re-exports (`M-NO-GLOB-REEXPORTS`):** Re-export `BinaryMemberSymbol`, `BinaryModuleInfo`, `BinaryOrigin`, `BinaryTarget`, `BinaryTypeSymbol`, `ClasspathIndex`, `ParsedClassfile`, `parse_class_bytes`, `parse_manifest_multi_release`, and `select_mrjar_entries` so `java-analyzer` stays strictly layered on `ide` alone.
- **`AnalysisHost` & `Analysis`:** Add `AnalysisHost::set_classpath_index(&mut self, index: Arc<ClasspathIndex>)` and `Analysis::classpath_index_stats(&self) -> Cancellable<(usize, usize)>` (binary types, binary modules).
- **`ide::hover`:** When `ResolvedSymbol::binary_target` is `Some(target)`, format fenced `java` Markdown showing the declaration signature plus a subtle origin line (e.g. `*from JDK module \`java.base\`*` or `*from \`junit-4.13.2.jar\`*`), and attach `binary_target: Option<BinaryTarget>` on `HoverResult` so `java-analyzer` can enrich hover with on-demand Javadoc without adding IO to `ide`.
- **`ide::goto_definition`:** Upgrade `NavigationTarget` to an enum:
  - `NavigationTarget::Source { file_id, full_range, focus_range }`
  - `NavigationTarget::Binary(BinaryTarget)`
- **`crates/ide/src/binary.rs`:**
  - `synthesize_binary_stub(index: &ClasspathIndex, target: &BinaryTarget) -> Option<String>`: generates a clean, syntactically valid `.java` stub (package, modifiers, type parameters, `extends`/`implements`/`permits`, record components, enum constants, fields, constructors, methods with `throw new UnsupportedOperationException();` or `;` for interfaces/abstract, plus nested types).
  - `locate_binary_symbol_in_source(source: &str, target: &BinaryTarget) -> Option<TextRange>`: parses `source` via `syntax::parse` (inside `ide`, never in `java-analyzer`) to locate the exact type or member `NAME` range in an extracted `src.zip`/`-sources.jar` file or synthesized stub.
  - `extract_javadoc_from_source(source: &str, target: &BinaryTarget) -> Option<String>`: extracts the leading `/** ... */` doc comment on the target type/member via `syntax::parse` and converts Javadoc tags (`{@code ...}`, `{@link ...}`, `@param`, `@return`, `@throws`, `<p>`, `<code>`) into clean Markdown.
  - `extract_javadoc_from_html(html: &str, target: &BinaryTarget) -> Option<String>`: extracts the class or member description block from a standard `javadoc`-generated HTML page inside a `*-javadoc.jar` and converts common HTML tags to clean Markdown.

### `crates/java-analyzer`
1. **JDK & JAR Loader (`crates/java-analyzer/src/classpath_loader.rs`)**:
   - **JDK 9–27 (`jmods/*.jmod` or `lib/modules` `jimage`)**:
     - If `$JAVA_HOME/jmods/` exists, scan `*.jmod` in parallel via `std::thread::scope` (strip the 4-byte `JM\x01\x00` header, open as ZIP via `Cursor<&[u8]>`).
     - Otherwise, if `$JAVA_HOME/lib/modules` exists (including JEP 493 linkable runtimes without `jmods/`), open it via the pure-Rust `jimage` reader (supporting jimage v1.0 and v1.1).
     - In both cases, parse `module-info.class` first to obtain each module's exported packages, then index public `.class` entries in exported packages (skipping `jdk.internal.*` and non-exported internal packages). Log any corrupt/unreadable archive or classfile via structured `tracing::warn!`.
   - **JDK 8**: Open `$JAVA_HOME/jre/lib/rt.jar` (or `$JAVA_HOME/lib/rt.jar`), indexing public `.class` entries outside `sun.*` / `com.sun.*`.
   - **Workspace local JARs & class directories**: For every `ModuleDependency::Path { path, .. }` across `probe.layout.modules`:
     - If `.jar`: read `META-INF/MANIFEST.MF` via `ide::parse_manifest_multi_release`, apply `ide::select_mrjar_entries` at `probe.resolved_level.release`, and parse `.class` entries via `ide::parse_class_bytes`.
     - **Companion archive discovery:** For each `<dir>/<stem>.jar`, probe for sibling source archives (`<stem>-sources.jar`, `<stem>-src.jar`, `<stem>-source.jar`) and Javadoc archives (`<stem>-javadoc.jar`, `<stem>-docs.jar`), recording them in a `JarCompanions` map for on-demand lookup (zero startup cost).
     - If directory: recursively walk `.class` files and parse them.
2. **On-Demand Source / Stub Materializer & Javadoc Lookup (`crates/java-analyzer/src/classpath_loader.rs`)**:
   - Cache root prefers `$XDG_CACHE_HOME/java-analyzer`, falling back to `~/Library/Caches/java-analyzer` (macOS) / `~/.cache/java-analyzer` (Unix) / `%LOCALAPPDATA%\java-analyzer` (Windows), and `std::env::temp_dir().join("java-analyzer")` only if no user cache/home directory is set.
   - **On-demand `textDocument/definition` for `NavigationTarget::Binary(target)`:**
     1. For `BinaryOrigin::Jdk { module_name }`: key the cache by JDK version (parsed from `$JAVA_HOME/release` `JAVA_VERSION="..."`, falling back to the JDK directory name, e.g. `jdk-25.0.1`). If `$JAVA_HOME/lib/src.zip` (or `$JAVA_HOME/src.zip`) exists, look up `<module>/<top_level_path>.java` (JDK 9+) or `<top_level_path>.java` (JDK 8), extract it into `<cache_root>/jdk-src/<jdk-version>/...`, locate the exact type/member range via `ide::locate_binary_symbol_in_source`, and return `Location { uri: file://..., range }`.
     2. For `BinaryOrigin::Jar { label }`: if a companion `*-sources.jar` was discovered, extract `<top_level_path>.java` into `<cache_root>/jar-src/<jar-key>/...`, locate the exact range via `ide::locate_binary_symbol_in_source`, and return `Location { uri: file://..., range }`.
     3. Fallback (JAR without sources, or JDK without `src.zip`): call `ide::synthesize_binary_stub`, write it to `<cache_root>/stubs/<jdk-version-or-jar-key>/<path>.java`, locate the exact type/member range via `ide::locate_binary_symbol_in_source`, and return `Location { uri: file://..., range }`.
   - **On-demand `textDocument/hover` Javadoc enrichment:**
     1. Try source `/** ... */` doc comment from `$JAVA_HOME/lib/src.zip` (JDK) or companion `*-sources.jar` (JAR) via `ide::extract_javadoc_from_source`.
     2. Fallback to companion `*-javadoc.jar` HTML (`<pkg_path>/<Type>.html`) via `ide::extract_javadoc_from_html`.
     3. Format the extracted Markdown cleanly between the fenced Java signature and the origin line.
3. **`server.rs` & CLI Separation of Concerns**:
   - **`java-analyzer lsp` (default):** Full semantic engine — in `GlobalState::apply_workspace_probe`, loads `ClasspathIndex` and `JarCompanions` from `probe`, stores `Arc<ClasspathIndex>`, `Arc<JarCompanions>`, and `Option<Arc<PathBuf>>` on `GlobalState` and `GlobalStateSnapshot` (preserving O(1) `snapshot()`), and calls `self.host.set_classpath_index(Arc::clone(&index))` to power type resolution, hover (with on-demand Javadoc), and jump-to-definition.
   - **`java-analyzer inspect`:** Read-only view of autodiscovered workspace configuration (build/version specs, resolved JDK path, modules, source/resource/multi-release roots, package mismatches, and conflicts); intentionally does **not** index binary JDK/JAR bytecode.
   - **`java-analyzer configure`:** Disambiguates workspace conflicts and writes minimal `.java-analyzer.toml` deltas for layout, JDK/release pinning, and dependencies; intentionally does **not** index binary JDK/JAR bytecode.

---

## Verification & Acceptance Criteria

1. **TDD & Verification Gate (`AGENTS.md` Rules 2 & 7):** Write failing tests first for each layer (`hir`, `ide`, `java-analyzer`); `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --all-targets` pass cleanly.
2. **Pure-Rust `.class` parsing (Java 8–27):** `crates/hir` parses classes, interfaces, enums, records, annotations, generic `Signature`s, `MethodParameters`, `InnerClasses`, `PermittedSubclasses`, and `module-info.class` from raw bytes with zero IO (including Java 27 major version `71`), tested with synthetic `.class` files built via `ristretto_classfile` (no JVM required).
3. **JEP 238 Multi-Release JAR overlay:** Selects the highest `META-INF/versions/<N>/` entry with `9 <= N <= active_release`, ignoring versions above `active_release`.
4. **Binary symbol resolution:** Workspace `.java` files resolve `String`, `Object`, `System.out`, `List`, `Map.Entry`, `Objects.requireNonNull`, `String::valueOf`, and `import module java.base;` via `DefDatabase::classpath_index`.
5. **Semantic hover:** `ide::hover` shows rich Java signatures and origin for JDK and JAR types/members (e.g. `String`, `s.length()`, `System.out`).
6. **On-demand go-to-definition:** `textDocument/definition` on JDK/JAR types/members opens the real source from `$JAVA_HOME/lib/src.zip` (when present) or a synthesized `.java` stub file via a standard `file://` `Location`, without `java-analyzer` depending on `syntax` or `hir`.
7. **Performance:** Indexing the local JDK completes quickly (target: `< 500ms` in release mode), and `java-analyzer inspect` stays fast without peeking into binary modules.
8. **Shadowing & safety:** Workspace source types always shadow identically named binary types; malformed archives/classfiles log via `tracing::warn!` without panicking; `RootDatabase::default()` continues to work without panicking when `classpath_index` is not explicitly set.
