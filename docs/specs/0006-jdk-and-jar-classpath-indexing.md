# Spec 0006: JDK & JAR Binary Classpath Indexing

Status: Proposed

## Motivation

With Spec 0004 (workspace layout & JDK discovery) and Spec 0005 (cross-file source symbol indexing & name resolution), `java-analyzer` resolves symbols declared across workspace `.java` files. However, real Java code constantly references standard library types (`Object`, `String`, `List`, `Map`, `Optional`, `System.out`, `Objects.requireNonNull`) and external `.jar` dependencies. Without binary classpath indexing:

- Hovering or invoking go-to-definition on JDK/JAR types falls back to generic identifier text or returns `None`.
- Member access on standard-library-typed receivers (e.g. `String s; s.length()`) cannot resolve.
- **Spec 0007 (Type Hierarchy & Inheritance)** cannot resolve `extends Object`, `implements List<T>`, `implements AutoCloseable`, or other binary supertypes.

This spec defines how `java-analyzer` indexes compiled `.class` files from the active JDK (`jmods/*.jmod` / JDK 8 `rt.jar`) and local `.jar` / class-directory dependencies (including **JEP 238 Multi-Release JARs**), exposes them through `hir`'s pure `DefDatabase`, upgrades semantic `hover` and member resolution, and supports on-demand go-to-definition via JDK `src.zip` or synthesized read-only `.java` stubs — **with zero running JVM required**.

## Goals

1. **Zero-JVM binary indexing using existing Rust crates**:
   - Use an existing pure-Rust `.class` parser crate (`cafebabe` with `parse_bytecode(false)` preferred for minimal overhead; `ristretto_classfile` if needed for JDK 25–27 major versions `69–71`) inside `crates/hir` (pure `&[u8]` parsing, zero IO).
   - Use the pure-Rust `zip` crate (`default-features = false, features = ["deflate"]`) in `crates/java-analyzer` for reading `.jar`, `.jmod`, and `src.zip` archives.
2. **Strict 4-layer architecture**:
   - All filesystem IO (reading `$JAVA_HOME/jmods/*.jmod`, `jre/lib/rt.jar`, `.jar` files, class directories, `lib/src.zip`, and writing on-demand cached stub/source files) lives **exclusively in `crates/java-analyzer`**.
   - `crates/hir` remains 100% pure and IO-free, receiving an `Arc<ClasspathIndex>` via a Salsa input on `DefDatabase`.
3. **JEP 238 Multi-Release JARs**:
   - Detect `Multi-Release: true` in `META-INF/MANIFEST.MF` and overlay `META-INF/versions/<N>/` entries for `9 <= N <= active_release` in ascending order of `N` on top of base `.class` entries (including versioned `module-info.class`), ignoring `N > active_release`.
4. **Implicit `java.lang.*` & `import module` resolution**:
   - Resolve unqualified `java.lang.*` types automatically (after workspace single-type imports and same-package types, per JLS §6.3 / §7.5).
   - Resolve `import module <M>;` (JEP 511) using exported packages parsed from each JDK/JAR `module-info.class` (including `requires transitive` re-exports).
5. **Semantic hover & on-demand go-to-definition for binary symbols**:
   - `ide::hover` renders rich Java signatures (with generics, parameter names when available, and module/JAR origin) for resolved binary types and members.
   - `textDocument/definition` on a binary symbol materializes the real `.java` source from `$JAVA_HOME/lib/src.zip` (when available) or synthesizes a clean read-only `.java` stub file into a local cache directory, returning a standard `file://` `Location` that works out of the box in Zed, VSCode, and Neovim.
6. **Step 1 indexing scope**:
   - Index the resolved JDK (`probe.resolved_jdk`) and local filesystem `.jar` / class-directory `ModuleDependency::Path` entries.

## Non-goals (for this spec)

- Resolving Maven/Gradle/Ivy/Git coordinates from `~/.m2`, `~/.gradle`, or remote repositories (deferred to **Spec 0015**).
- Reading OpenJDK `lib/modules` (`jimage`) for slim JRE distributions that omit `jmods/` (deferred to a follow-up; `jmods/*.jmod` covers standard JDK 9–27 installations and `rt.jar` covers JDK 8).
- Full inheritance traversal (`extends`/`implements` member lookup) and generic type-argument substitution (covered by **Spec 0007** and **Spec 0008**; this spec extracts and stores `super_class`, `interfaces`, `permitted_subclasses`, and raw/generic signatures so Spec 0007/0008 can consume them directly).
- Bytecode decompilation of method bodies (only public/protected API signatures and stubs are needed).

---

## Pure Data Model in `crates/hir`

### `crates/hir/src/classpath.rs` & `crates/hir/src/classfile.rs`

```rust
/// Where a binary type originated (used for hover display and on-demand source/stub lookup).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BinaryOrigin {
    /// A JPMS JDK module (e.g. `module_name = Some("java.base")`, `archive_path = ".../jmods/java.base.jmod"`),
    /// or JDK 8 `rt.jar` (`module_name = None`).
    Jdk {
        module_name: Option<String>,
    },
    /// A local `.jar` archive or compiled classes directory.
    Jar {
        /// Display name / normalized path (e.g. `"lib/junit-4.13.2.jar"`).
        label: String,
    },
}

/// A compiled Java type parsed from a `.class` file (no source `FileId` / `TextRange`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryTypeSymbol {
    /// Simple name (e.g. `"String"`, `"Entry"`).
    pub name: String,
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
    pub type_params: Vec<String>,
    /// Superclass canonical FQN (e.g. `Some("java.lang.Object")`; `None` for `java.lang.Object`).
    pub super_class: Option<String>,
    /// Directly implemented/extended interface FQNs.
    pub interfaces: Vec<String>,
    /// Sealed `permits` FQNs from `PermittedSubclasses` attribute (Java 17+).
    pub permitted_subclasses: Vec<String>,
    /// Public/protected fields, methods, constructors, enum constants, and record components.
    pub members: Vec<BinaryMemberSymbol>,
    /// Simple names of directly nested public/protected types.
    pub nested_types: Vec<String>,
    pub origin: BinaryOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryMemberSymbol {
    pub owner_type: String,
    pub name: String,
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
    pub name: String,
    /// Unqualified exported packages (e.g. `["java.lang", "java.util", ...]`).
    pub exports: Vec<String>,
    /// `requires transitive` target module names.
    pub requires_transitive: Vec<String>,
}

/// Pure, deterministic index of all binary JDK and JAR types visible to the workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClasspathIndex {
    /// Canonical FQN (`"java.lang.String"`, `"java.util.Map.Entry"`) -> `BinaryTypeSymbol`.
    pub types_by_fqn: BTreeMap<String, BinaryTypeSymbol>,
    /// Package name (`"java.lang"`) -> sorted top-level type FQNs.
    pub types_by_package: BTreeMap<String, Vec<String>>,
    /// Package name -> (simple name -> FQN) for O(log N) lookups.
    pub package_type_map: BTreeMap<String, BTreeMap<String, String>>,
    /// JPMS module name (`"java.base"`) -> `BinaryModuleInfo`.
    pub modules: BTreeMap<String, BinaryModuleInfo>,
}
```

### Pure `.class` Parsing (`crates/hir/src/classfile.rs`)

- `parse_class_bytes(bytes: &[u8], origin: BinaryOrigin) -> Result<ParsedClassfile, ClassfileError>` where `ParsedClassfile` is either `Type(BinaryTypeSymbol)` or `Module(BinaryModuleInfo)`.
- Skips bytecode (`Code` attribute) completely for speed.
- Filters out:
  - Synthetic/bridge members (`ACC_SYNTHETIC`, `ACC_BRIDGE`) and anonymous/local classes (`$1`, `$1Local`).
  - Non-public/non-protected top-level & member declarations (package-private/private implementation details are excluded from the external binary API index).
- Decodes:
  - **JVM descriptors** (`Ljava/lang/String;`, `[I`, `(Ljava/lang/String;I)Z`) into Java type strings (`String`, `int[]`, etc.).
  - **JVM generic `Signature` attributes** (`<K:Ljava/lang/Object;V:Ljava/lang/Object;>Ljava/lang/Object;`) into readable type parameters, supertypes, field types, and method signatures.
  - **`Record` attribute** into `MemberKind::RecordComponent` entries and `TypeKind::Record`.
  - **`InnerClasses` attribute** to distinguish real nested types (`java.util.Map.Entry`) from `$` in identifiers and capture `is_static`.
  - **`MethodParameters` attribute** for parameter names when compiled with `-parameters`, falling back to `arg0`, `arg1`, ...

### Pure JEP 238 Multi-Release Overlay (`crates/hir/src/classpath.rs`)

```rust
/// Selects the effective `.class` entry bytes for a JAR given `is_multi_release` and `target_release`.
///
/// Entries are `(zip_entry_path, bytes)`:
/// - Base entries live at `<pkg>/<Name>.class` (and `module-info.class`).
/// - When `is_multi_release == true`, entries under `META-INF/versions/<N>/<rel>` with
///   `9 <= N <= target_release.number()` override base entries in ascending order of `N`.
/// - Entries with `N > target_release.number()` are ignored.
pub fn select_mrjar_entries<'a>(
    entries: &'a [(&'a str, &'a [u8])],
    is_multi_release: bool,
    target_release: Release,
) -> BTreeMap<String, &'a [u8]>
```

---

## Salsa & Resolution Integration (`crates/hir`)

### `DefDatabase` (`crates/hir/src/lib.rs`)

Add one Salsa input to `DefDatabase`:

```rust
#[salsa::query_group(DefDatabaseStorage)]
pub trait DefDatabase: SourceDatabase {
    #[salsa::input]
    fn workspace_files(&self) -> Arc<Vec<FileId>>;

    /// Indexed JDK and external JAR binary symbols. Defaults to an empty `ClasspathIndex`
    /// in `RootDatabase::default()`.
    #[salsa::input]
    fn classpath_index(&self) -> Arc<ClasspathIndex>;

    fn file_symbols(&self, file_id: FileId) -> Arc<FileSymbols>;
    fn workspace_index(&self) -> Arc<WorkspaceIndex>;
}
```

### Updated Name Resolution Precedence (`crates/hir/src/resolve.rs`)

Workspace source symbols always take precedence over binary classpath symbols (matching `javac`'s `-sourcepath` / source-over-binary rule). For type names:

1. Lexical local types & enclosing type parameters / nested types (existing Spec 0005 rules).
2. Single-type imports (`import a.b.Foo;`): check `workspace_index`, then `classpath_index`.
3. Same-package types: check `workspace_index`, then `classpath_index`.
4. **Implicit `java.lang.*`** (for simple unqualified type names): check `classpath_index` in package `"java.lang"`.
5. Type-import-on-demand (`import a.b.*;`): check `workspace_index`, then `classpath_index`.
6. **Module imports (`import module M;`, JEP 511)**: look up `M` in `classpath_index.modules` (following `requires_transitive` transitively, e.g. `java.se` → `java.base`, `java.sql`, ...) and check each exported package in `classpath_index`.
7. Qualified type paths (`java.util.List`, `java.util.Map.Entry`, including inside `import` declarations): check `workspace_index`, then `classpath_index`.
8. Static imports (`import static java.util.Objects.requireNonNull;`, `import static java.lang.Math.*;`) and member access (`System.out`, `s.length()`, `List.of(...)`, `String::valueOf`): check workspace types first, then `classpath_index` types (with the same arity/varargs overload preference).

`ResolvedSymbol` carries an optional `binary_target: Option<BinaryTarget>` (containing `origin: BinaryOrigin`, `internal_name: String`, `qualified_name: String`, `member_name: Option<String>`) alongside `file_id: Option<FileId>` so `ide` and `java-analyzer` know when a resolved symbol lives in a JDK module or JAR.

---

## IDE & LSP Behavior (`crates/ide` & `crates/java-analyzer`)

### `crates/ide`
- **`ide::hover`**:
  - When `ResolvedSymbol` is a binary type or member, format fenced `java` Markdown showing the declaration signature plus a subtle origin line (e.g. `*from JDK module \`java.base\`*` or `*from \`junit-4.13.2.jar\`*`).
- **`ide::goto_definition`**:
  - Extend `NavigationTarget` (or return `NavigationTarget::Source { file_id, full_range, focus_range }` vs. `NavigationTarget::Binary(BinaryTarget)`).

### `crates/java-analyzer`
1. **JDK & JAR Loader (`crates/java-analyzer/src/classpath_loader.rs`)**:
   - **JDK 9–27**: Scan `$JAVA_HOME/jmods/*.jmod` (strip the 4-byte `JM\x01\x00` header, open as ZIP). Parse `classes/module-info.class` first to get the module's exported packages; index public `.class` entries in exported packages (keeping indexing fast — skipping `jdk.internal.*` and non-exported internal packages).
   - **JDK 8**: Open `$JAVA_HOME/jre/lib/rt.jar` (or `$JAVA_HOME/lib/rt.jar`), indexing public `.class` entries outside `sun.*` / `com.sun.*`.
   - **Workspace local JARs & class directories**: For every `ModuleDependency::Path { path, .. }` across `probe.layout.modules`:
     - If `.jar`: read `META-INF/MANIFEST.MF` for `Multi-Release: true`, apply `select_mrjar_entries` at `probe.resolved_level.release`, and parse `.class` entries.
     - If directory: walk `.class` files and parse them.
2. **On-Demand Source / Stub Materializer (`crates/java-analyzer/src/classpath_loader.rs`)**:
   - When `textDocument/definition` resolves to `NavigationTarget::Binary(target)`:
     1. For `BinaryOrigin::Jdk { module_name }`: if `$JAVA_HOME/lib/src.zip` (or `$JAVA_HOME/src.zip`) exists, look up `<module>/<top_level_path>.java` (JDK 9+) or `<top_level_path>.java` (JDK 8), extract it into a deterministic cache path under `<temp_or_cache>/java-analyzer/jdk-src/<jdk-hash>/...`, parse it with `syntax::parse` to locate the exact type/member `TextRange`, and return `Location { uri: file://..., range }`!
     2. Otherwise (JAR without sources, or JDK without `src.zip`): synthesize a clean, valid `.java` stub file (package, class/interface/enum/record/annotation header with `extends`/`implements`, fields, constructors, methods with `throw new UnsupportedOperationException();` or `;` for interfaces/abstract), write it to `<temp_or_cache>/java-analyzer/stubs/<origin-hash>/<path>.java`, locate the exact type/member range, and return `Location { uri: file://..., range }`!
3. **`server.rs` & `cli.rs`**:
   - In `GlobalState::set_workspace_root`, load `ClasspathIndex` from `probe` and call `self.db.set_classpath_index(Arc::new(classpath_index))`.
   - In `java-analyzer inspect`, display binary classpath stats (`JDK/JAR binary types` and `JDK modules` counts) in both human and `--json` output.

---

## Acceptance Criteria

1. Pure-Rust `.class` parsing in `crates/hir` parses classes, interfaces, enums, records, annotations, generic `Signature`s, `MethodParameters`, `InnerClasses`, `PermittedSubclasses`, and `module-info.class` from raw bytes with zero IO (including JDK 27 classfiles).
2. JEP 238 Multi-Release JAR overlay selects the highest `META-INF/versions/<N>/` entry with `9 <= N <= active_release`, ignoring versions above `active_release`.
3. Workspace `.java` files resolve `String`, `Object`, `System.out`, `List`, `Map.Entry`, `Objects.requireNonNull`, and `import module java.base;` via `DefDatabase::classpath_index`.
4. `ide::hover` shows rich Java signatures and origin for JDK and JAR types/members (e.g. `String`, `s.length()`, `System.out`).
5. `textDocument/definition` on JDK types/members opens the real source from `$JAVA_HOME/lib/src.zip` (when present) or a synthesized `.java` stub file via a standard `file://` `Location`.
6. `java-analyzer inspect` reports binary classpath stats alongside workspace source stats, and indexing the local JDK completes quickly (target: < 500ms in release mode).
7. Workspace source types always shadow identically named binary types, and `RootDatabase::default()` continues to work without panicking when `classpath_index` is not explicitly set.
