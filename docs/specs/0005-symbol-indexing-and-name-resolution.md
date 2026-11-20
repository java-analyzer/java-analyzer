# Spec 0005: Symbol Indexing, Name Resolution, and Go to Definition

Status: Accepted

## Motivation

`java-analyzer` parses Java source files into lossless `rowan` CSTs (`syntax`), infers workspace
modules and source roots (`hir::workspace`), and serves syntax diagnostics, document symbols, and
placeholder hover over LSP (`ide` + `java-analyzer`). However, every feature is currently
single-file and purely syntactic:

- `hir` has no semantic symbol table or cross-file index.
- `ide::hover` only distinguishes the `class` keyword from raw `IDENT` tokens.
- `textDocument/definition` is not implemented.
- `GlobalState` in `java-analyzer` only loads files when opened via `didOpen` and blanks them out on
  `didClose`, preventing resolution across closed workspace files.

This spec defines the first semantic analysis layer in `hir`, the editor features built on top of it
in `ide`, and workspace file loading plus `textDocument/definition` in `java-analyzer`.

## Goals

1. **Incremental symbol indexing in `hir`**: introduce a Salsa `DefDatabase` query group (extending
   `SourceDatabase`) with `workspace_files`, per-file `file_symbols`, and an aggregated
   `workspace_index` supporting fast lookup by FQN and package name.
2. **Salsa early-cutoff**: represent `FileSymbols` with `PartialEq + Eq` and exclude method,
   constructor, and initializer bodies so downstream workspace queries are not invalidated by
   signature-preserving edits.
3. **Accurate Java name resolution in `hir`**: resolve identifiers at any byte offset across
   lexical local scopes, enclosing/nested types, Java import precedence (single-type, same-package,
   on-demand, static, qualified paths), and basic member access (`Qualifier.member`).
4. **Editor navigation & semantic hover in `ide`**: implement `goto_definition` returning
   `NavigationTarget` and upgrade `hover` to display resolved symbol kind, qualified name, and
   signature/type.
5. **Workspace-backed LSP server**: preload workspace `.java` files into `RootDatabase`, keep open
   buffers authoritative over disk, reload disk text on `didClose`, and serve
   `textDocument/definition`.

## Non-goals (for this spec)

- Full JLS type inference (generics instantiation, diamond `<>`, `var` RHS inference, lambda target
  typing, overload resolution by argument types).
- Indexing external JDK / JAR bytecode (`.class` / `.jmod` / `jrt:` files); unresolved external
  types (such as `java.lang.String`) gracefully return `None` from resolution and fall back in
  hover.
- Supertype / interface inheritance member lookup (`extends` / `implements` traversal).
- File-watcher (`workspace/didChangeWatchedFiles`) incremental disk reloading.

## Architecture & Layering

Preserves the strict 4-layer architecture from Spec 0001:

- `crates/syntax`: unchanged; `hir` consumes existing `SyntaxKind` nodes and tokens.
- `crates/hir`: new `DefDatabase` query group (`workspace_files`, `file_symbols`,
  `workspace_index`), symbol data structures, and `resolve_at_offset` — pure, zero IO.
- `crates/ide`: `NavigationTarget`, `goto_definition`, and upgraded `hover` depending on
  `&dyn DefDatabase`.
- `crates/java-analyzer`: all filesystem IO (loading workspace `.java` files on workspace init and
  reloading disk text on `didClose`), bidirectional `Url <-> FileId` tracking, and LSP
  `textDocument/definition` wiring.

## 1. Salsa Incremental Queries in `hir`

### `DefDatabase` and `RootDatabase`

Add a new query group `DefDatabase` in `crates/hir` extending `SourceDatabase` and register
`DefDatabaseStorage` on `RootDatabase`:

```rust
#[salsa::query_group(DefDatabaseStorage)]
pub trait DefDatabase: SourceDatabase {
    /// Ordered list of all `.java` files currently in the workspace.
    #[salsa::input]
    fn workspace_files(&self) -> Arc<Vec<FileId>>;

    /// Declared package, imports, types, and members extracted from a single file.
    fn file_symbols(&self, file_id: FileId) -> Arc<FileSymbols>;

    /// Aggregated workspace-wide lookup index over `workspace_files`.
    fn workspace_index(&self) -> Arc<WorkspaceIndex>;
}

#[salsa::database(SourceDatabaseStorage, DefDatabaseStorage)]
#[derive(Default)]
pub struct RootDatabase {
    storage: salsa::Storage<Self>,
}
```

> **Note**: Any test or caller constructing a `RootDatabase` and invoking `DefDatabase` queries must
> initialize `workspace_files` (e.g., `db.set_workspace_files(Arc::new(vec![file_id]))`).
> `GlobalState::new()` must initialize `workspace_files` to an empty `Arc<Vec<FileId>>`.

### Per-file Symbol Model (`FileSymbols`)

`file_symbols(db, file_id)` walks `db.syntax_tree(file_id)` and extracts structural declarations
without descending into method/constructor/initializer bodies (ignoring local classes and local
variables). All types derive `Debug, Clone, PartialEq, Eq` for Salsa memoization and early-cutoff.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSymbols {
    pub file_id: FileId,
    /// Declared package name (e.g. `Some("com.example")`), or `None` for the unnamed package.
    pub package: Option<String>,
    pub imports: Vec<ImportItem>,
    /// All top-level and nested type declarations in this file (flattened or linked via `enclosing`).
    pub types: Vec<TypeSymbol>,
    /// Top-level fields/methods in JEP 512 compact source files (direct children of `SOURCE_FILE`).
    pub top_level_members: Vec<MemberSymbol>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    /// `import a.b.Foo;`
    SingleType,
    /// `import a.b.*;`
    TypeOnDemand,
    /// `import static a.b.Foo.BAR;`
    StaticSingle,
    /// `import static a.b.Foo.*;`
    StaticOnDemand,
    /// `import module java.base;`
    Module,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportItem {
    pub kind: ImportKind,
    /// Dotted segments of the `PATH` node (e.g. `["a", "b", "Foo"]`).
    pub path: Vec<String>,
    pub range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TypeKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeSymbol {
    pub file_id: FileId,
    /// Simple name (e.g. `"Inner"`).
    pub name: String,
    /// Canonical dot-separated FQN (e.g. `"com.example.Outer.Inner"`, or `"Outer.Inner"` in unnamed package).
    pub qualified_name: String,
    /// Enclosing type's `qualified_name`, if nested.
    pub enclosing_type: Option<String>,
    pub kind: TypeKind,
    pub is_static: bool,
    pub type_params: Vec<String>,
    /// Direct members of this type (fields, methods, constructors, enum constants, record components).
    pub members: Vec<MemberSymbol>,
    /// Names of directly nested types (resolved via `qualified_name` = `"{self.qualified_name}.{nested}"`).
    pub nested_types: Vec<String>,
    pub full_range: TextRange,
    pub selection_range: TextRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MemberKind {
    Field,
    Method,
    Constructor,
    EnumConstant,
    RecordComponent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSymbol {
    pub file_id: FileId,
    /// Enclosing type's `qualified_name` (`None` only for JEP 512 compact-source top-level members).
    pub owner_type: Option<String>,
    pub name: String,
    pub kind: MemberKind,
    pub is_static: bool,
    /// Collapsed source text of the declared type (field type, method return type, record component type; `None` for constructors/enum constants).
    pub type_ref: Option<String>,
    /// Parameter list summary for methods/constructors (e.g. `"(int a, String b)"`) and arity for overload disambiguation.
    pub params: Vec<ParamSummary>,
    pub full_range: TextRange,
    pub selection_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSummary {
    pub name: Option<String>,
    pub type_ref: String,
    pub is_varargs: bool,
}
```

#### CST Extraction Rules (matching `crates/syntax/src/syntax_kind.rs`)

- **Package**: `PACKAGE_DECL` $\to$ `PATH` $\to$ join `NAME_REF` children with `"."`.
- **Imports**: `IMPORT_DECL` $\to$:
  - `has_static`: direct child token `STATIC_KW`.
  - `is_module`: direct child token `IDENT` with text `"module"` (contextual keyword, not a keyword token).
  - `is_wildcard`: direct child token `STAR` (preceded by `DOT`; not inside `PATH`).
  - `path`: `NAME_REF` children of the `PATH` child node.
- **Type declarations**: `CLASS_DECL`, `INTERFACE_DECL`, `ENUM_DECL`, `RECORD_DECL`,
  `ANNOTATION_TYPE_DECL` (recursing into `CLASS_BODY` and `ENUM_BODY` for nested types).
  - `selection_range`: text range of the `NAME` child; `full_range`: text range of the declaration node.
  - Declarations without a `NAME` (half-typed code) are skipped.
  - `is_static`: `true` if `MODIFIER_LIST` contains `STATIC_KW`, or if implicitly static in Java
    (nested `INTERFACE_DECL`, `ENUM_DECL`, `RECORD_DECL`, `ANNOTATION_TYPE_DECL`, or any type nested
    inside an `INTERFACE_DECL` / `ANNOTATION_TYPE_DECL`).
- **Members**:
  - `FIELD_DECL`: one `MemberSymbol` per `VARIABLE_DECLARATOR` child with a `NAME`. `full_range` is
    the `VARIABLE_DECLARATOR` range (matching `document_symbols`), `selection_range` is its `NAME`.
    `is_static` is `true` if `MODIFIER_LIST` has `STATIC_KW` or the owner is an `Interface` /
    `Annotation`.
  - `METHOD_DECL` / `CONSTRUCTOR_DECL`: `NAME` child, `PARAM_LIST` $\to$ `PARAM` children, return
    type node (`PATH_TYPE | PRIMITIVE_TYPE | ARRAY_TYPE`, where `void` is `PRIMITIVE_TYPE`).
  - `RECORD_COMPONENT` (inside `RECORD_COMPONENT_LIST`): `MemberKind::RecordComponent`, `type_ref`
    from its type child, `selection_range` from its `NAME` child.
  - `ENUM_CONSTANT` (inside `ENUM_BODY`): `MemberKind::EnumConstant`, `is_static = true`,
    `selection_range` from its `NAME` child.

### Workspace Index (`WorkspaceIndex`)

`workspace_index(db)` aggregates `db.file_symbols(file_id)` for every `file_id` in
`db.workspace_files()`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceIndex {
    /// Canonical FQN (`"com.example.Outer.Inner"`, or `"Foo"` in unnamed package) -> `TypeSymbol`.
    pub types_by_fqn: BTreeMap<String, TypeSymbol>,
    /// Package name (`""` for unnamed package, `"com.example"` otherwise) -> top-level type FQNs in that package.
    pub types_by_package: BTreeMap<String, Vec<String>>,
    /// Package name -> simple name -> top-level type FQN for O(1) same-package & wildcard lookup.
    pub package_type_map: BTreeMap<String, BTreeMap<String, String>>,
}
```

Using `BTreeMap` guarantees deterministic iteration order and structural `Eq` for Salsa.

## 2. Name Resolution in `hir`

Expose a pure resolution entry point in `crates/hir`:

```rust
pub fn resolve_at_offset(
    db: &dyn DefDatabase,
    file_id: FileId,
    offset: u32,
) -> Option<ResolvedSymbol>
```

### `ResolvedSymbol`

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedSymbol {
    Type(TypeSymbol),
    Member(MemberSymbol),
    Local(LocalSymbol),
    TypeParam(TypeParamSymbol),
    Package { name: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LocalKind {
    LocalVar,
    Param,
    LambdaParam,
    ForEachVar,
    CatchParam,
    ResourceVar,
    PatternVar,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalSymbol {
    pub file_id: FileId,
    pub name: String,
    pub kind: LocalKind,
    /// Collapsed declared type text if explicit (e.g. `Some("Foo")`), `None` for untyped lambda params or `var`.
    pub type_ref: Option<String>,
    pub full_range: TextRange,
    pub selection_range: TextRange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParamSymbol {
    pub file_id: FileId,
    pub name: String,
    pub bounds: Vec<String>,
    pub full_range: TextRange,
    pub selection_range: TextRange,
}
```

Helper methods on `ResolvedSymbol`:

- `pub fn location(&self) -> Option<(FileId, TextRange, TextRange)>` returning
  `(file_id, full_range, selection_range)` (`None` for `ResolvedSymbol::Package`).

### Token Selection at `offset`

1. Look up the token at `offset` using `syntax_tree.token_at_offset(offset.into()).right_biased()`.
2. Only `IDENT` tokens are candidates for symbol resolution (keywords such as `class` return `None`
   so `hover` can fall back to keyword docs).
3. Inspect the parent node of the `IDENT` token:
   - **`NAME` (declaration site)**: resolve directly to the declared symbol (type, member, local,
     parameter, type parameter) so `hover` displays full semantic info on declarations and
     `goto_definition` lands on the declaration itself.
   - **`NAME_REF` (reference site)**: dispatch based on syntactic context (below).

### Syntactic Context Dispatch for `NAME_REF`

1. **Inside `IMPORT_DECL` $\to$ `PATH`**:
   - Collect the prefix of `NAME_REF` segments in `PATH` up to and including the target `NAME_REF`.
   - For `SingleType`, `TypeOnDemand`, and `StaticOnDemand`: try resolving the prefix as a type FQN
     in `workspace_index`; if not a type, check if it is a known package prefix and return
     `ResolvedSymbol::Package`.
   - For `StaticSingle` (`import static a.b.Foo.BAR;`): if the cursor is on the final segment `BAR`,
     resolve `a.b.Foo` as a type and look up static member/nested type `BAR` on it; for earlier
     segments, resolve as type or package.
2. **Inside `PACKAGE_DECL` $\to$ `PATH`**:
   - Return `ResolvedSymbol::Package` for the prefix up to the cursor.
3. **Inside `PATH_TYPE` or `ANNOTATION` $\to$ `PATH`**:
   - Collect the `NAME_REF` segments up to and including the cursor.
   - Resolve the segment chain as a type reference (see *Type & Import Resolution* below).
   - Note: `PATH_TYPE` holds all segments of `a.b.Outer<T>.Inner<U>` as direct `NAME_REF` children
     (interleaved with `TYPE_ARGS`), so taking segments up to the cursor naturally resolves `Outer`
     when clicking `Outer` and `Outer.Inner` when clicking `Inner`.
   - In `NEW_EXPR` (`new Foo(args)`), `Foo` is a `PATH_TYPE` and resolves to `TypeSymbol` `Foo`.
4. **Member access (`FIELD_ACCESS_EXPR`, `METHOD_CALL_EXPR`, `METHOD_REF_EXPR`)**:
   - When the target `NAME_REF` is the member name (the `NAME_REF` directly following `DOT` or
     `COLON2`, not part of the receiver):
     - First, check if the entire dotted receiver + member chain is a qualified type path
       (e.g., `com.example.Foo` inside `com.example.Foo.BAR` or `Outer.Inner.x`). If the chain
       through the cursor matches a type FQN in `workspace_index`, return `ResolvedSymbol::Type`.
     - Otherwise, resolve the receiver's type `T` (see *Receiver Type Resolution* below) and look up
       `member` on `T` (nested types, fields, methods, enum constants, record components).
     - For `METHOD_CALL_EXPR`, prefer methods matching the argument count of `ARG_LIST` (falling
       back to the first method with the matching name).
5. **Unqualified expression (`NAME_REF_EXPR`, including callee of `CALL_EXPR`)**:
   - If the `NAME_REF_EXPR` is the callee of a `CALL_EXPR` (`foo(args)`):
     1. Enclosing type(s) methods (innermost to outermost, plus compact-source top-level methods),
        preferring matching argument count.
     2. Static imports (`StaticSingle` then `StaticOnDemand`) matching method name.
   - Otherwise (variable, field, constant, or type qualifier like `Foo` in `Foo.BAR`):
     1. **Lexical local scopes** (innermost to outermost; see below).
     2. **Enclosing type(s) members** (fields, record components, enum constants, nested types;
        innermost to outermost, plus compact-source top-level fields).
     3. **Static imports** (`StaticSingle` then `StaticOnDemand`) for static fields, enum constants,
        or nested types.
     4. **Type & import resolution** (allows `Foo` in `Foo.BAR` or `Foo::bar` to resolve to type
        `Foo`).

### Lexical Local Scope Resolution

Walk ancestor nodes from the `NAME_REF` upward until reaching the enclosing member boundary
(`METHOD_DECL`, `CONSTRUCTOR_DECL`, `INITIALIZER`, or `SOURCE_FILE`), checking each scope in
innermost-to-outermost order so inner declarations shadow outer ones:

| Enclosing AST Node | Bindings in Scope |
|--------------------|-------------------|
| `BLOCK` | Preceding `LOCAL_VAR_DECL` statements (every `VARIABLE_DECLARATOR` with a `NAME` whose declaration ends before the cursor and does not contain the cursor in its own initializer). Also local type declarations (`CLASS_DECL`, `INTERFACE_DECL`, `ENUM_DECL`, `RECORD_DECL`) declared before the cursor. |
| `FOR_STMT` | `LOCAL_VAR_DECL` child in the `for (...)` header (visible in condition, update, and body). |
| `FOREACH_STMT` | Direct `NAME` child + type child of `FOREACH_STMT` (visible in body, not in the iterable expression). |
| `CATCH_CLAUSE` | Direct `NAME` child + first type child of `CATCH_CLAUSE` (visible in catch `BLOCK`). |
| `TRY_STMT` | `RESOURCE` children inside `RESOURCE_LIST` declared before the cursor (direct `NAME` child + type child; visible in subsequent resources and try `BLOCK`). |
| `LAMBDA_EXPR` | `PARAM` children inside `LAMBDA_PARAMS` (note: untyped params have no type child). |
| `METHOD_DECL` / `CONSTRUCTOR_DECL` | `PARAM` children inside `PARAM_LIST`, plus `TYPE_PARAM` children inside `TYPE_PARAMS`. |
| `INSTANCEOF_EXPR` / `IF_STMT` / `SWITCH_CASE` | Pattern variables (`TYPE_PATTERN` direct `NAME` child, including nested inside `RECORD_PATTERN`) in `SWITCH_CASE` labels (visible in the case guard and body) and `if (x instanceof Foo f)` (visible in the then-branch and RHS of `&&`). |

Unnamed bindings (`UNDERSCORE_KW`, Java 22+) are never bound as resolvable names.

### Type & Import Resolution Precedence

To resolve a type segment chain `segs = [s0, s1, ..., sk]` at a site inside `file_id`:

1. **Single segment `[name]`**:
   1. Enclosing `TYPE_PARAM`s (from enclosing method/constructor and enclosing type declarations,
      innermost to outermost).
   2. Enclosing type declarations themselves, their nested types, and local types in scope (walking
      innermost to outermost).
   3. Single-type imports (`ImportKind::SingleType` where `path.last() == Some(name)`) resolved in
      `workspace_index.types_by_fqn`.
   4. Static single imports (`ImportKind::StaticSingle` where `path.last() == Some(name)`) if the
      target is a nested type.
   5. Same-package top-level types (`workspace_index.package_type_map[current_package][name]`).
   6. Type-import-on-demand (`ImportKind::TypeOnDemand`, looking up `"{pkg}.{name}"` in
      `workspace_index.types_by_fqn`).
   7. Static-import-on-demand (`ImportKind::StaticOnDemand`, looking up nested type `"{type}.{name}"`
      in `workspace_index.types_by_fqn`).
2. **Multi-segment `[s0, s1, ..., sk]`**:
   - First check if `segs.join(".")` directly matches a type FQN in `workspace_index.types_by_fqn`
     (handles fully-qualified types like `com.example.Foo` and `com.example.Outer.Inner`).
   - Otherwise, resolve `s0` as a single-segment type to `base_fqn`, then walk `s1..=sk` as nested
     types `"{fqn}.{seg}"` in `workspace_index.types_by_fqn` (handles `Outer.Inner` via import or
     same-package).

### Receiver Type Resolution (Basic Member Access)

To determine the workspace `TypeSymbol` of a receiver expression `recv` in `recv.member` or
`recv.method(...)`:

1. `THIS_EXPR`: innermost enclosing `TypeSymbol`.
2. `NAME_REF_EXPR`: resolve the identifier at its `NAME_REF`:
   - `ResolvedSymbol::Type(t)` $\to$ `t`.
   - `ResolvedSymbol::Local(l)` with `type_ref: Some(ty)` $\to$ strip generics/arrays from `ty` and
     resolve as a type in the file's scope.
   - `ResolvedSymbol::Member(m)` (`Field` or `RecordComponent`) with `type_ref: Some(ty)` $\to$ strip
     generics/arrays from `ty` and resolve as a type in the owner type's file scope.
   - `ResolvedSymbol::Member(m)` (`EnumConstant`) $\to$ owner `TypeSymbol`.
3. `FIELD_ACCESS_EXPR`:
   - First check if the dotted chain is a qualified type FQN in `workspace_index`.
   - Otherwise resolve the field access itself to a `ResolvedSymbol` and map its `type_ref` / `Type`
     as above (supports chains like `a.b.c`).
4. `NEW_EXPR`: resolve its `PATH_TYPE` child as a type.
5. `PAREN_EXPR`: unwrap inner expression and recurse.

## 3. `ide` Features

### `NavigationTarget` and `goto_definition`

Add to `crates/ide/src/lib.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationTarget {
    pub file_id: FileId,
    /// Range of the entire declaration.
    pub full_range: TextRange,
    /// Range of the declaration's identifier (`NAME`), used as cursor destination.
    pub focus_range: TextRange,
}

pub fn goto_definition(
    db: &dyn DefDatabase,
    file_id: FileId,
    offset: u32,
) -> Option<NavigationTarget>
```

`goto_definition` calls `hir::resolve_at_offset(db, file_id, offset)` and converts
`symbol.location()` into `NavigationTarget { file_id, full_range, focus_range }`.

### Upgraded `hover`

Update `ide::hover` signature from `&dyn SourceDatabase` to `&dyn DefDatabase`:

```rust
pub fn hover(db: &dyn DefDatabase, file_id: FileId, offset: u32) -> Option<HoverResult>
```

Behavior:

1. Preserve `right_biased` token lookup at `offset` (so whitespace after a token returns `None`).
2. If `hir::resolve_at_offset(db, file_id, offset)` returns `Some(sym)`, format rich Markdown:
   - `Type(t)`: ```` ```java\n<kind> <qualified_name>\n``` ```` (e.g. `class com.example.Foo`).
   - `Member(m)`:
     - Field / RecordComponent: ```` ```java\n<type> <owner>.<name>\n``` ````
     - Method: ```` ```java\n<return_type> <owner>.<name>(<params>)\n``` ````
     - Constructor: ```` ```java\n<owner>(<params>)\n``` ````
     - EnumConstant: ```` ```java\n<owner>.<name>\n``` ````
   - `Local(l)`: ```` ```java\n<type_or_var> <name>\n``` ```` (annotated with local/parameter kind).
   - `TypeParam(tp)`: ```` ```java\ntype parameter <name>\n``` ```` (with `extends ...` if bounded).
   - `Package { name }`: ```` ```java\npackage <name>\n``` ````.
3. If unresolved, fall back to the existing behavior:
   - `CLASS_KW` $\to$ `"Keyword `class` declares a new class"`
   - `IDENT` $\to$ `format!("Identifier `{}`", token.text())`
   - Anything else $\to$ `None`.

## 4. `java-analyzer` LSP Server

### Workspace File Loading & Lifecycle in `GlobalState`

Update `GlobalState` in `crates/java-analyzer/src/server.rs`:

```rust
pub struct GlobalState {
    db: RootDatabase,
    files: HashMap<Url, FileId>,
    file_urls: HashMap<FileId, Url>,
    open_files: HashSet<Url>,
    next_file_id: u32,
    workspace_probe: Option<WorkspaceProbe>,
}
```

Lifecycle rules:

1. **Initialization (`GlobalState::new`)**: initializes `db.set_workspace_files(Arc::new(Vec::new()))`.
2. **Workspace load (`set_workspace_root` / `with_workspace`)**:
   - Runs `probe_workspace(workspace_dir)`.
   - Iterates over `probe.file_paths.keys()` (sorted normalized relative paths):
     - Resolves the absolute path `workspace_dir.join(rel_path)` and converts to `Url::from_file_path`.
     - Allocates a stable `FileId` if not already in `self.files`, recording both `files` and
       `file_urls`.
     - If the URI is **not** in `self.open_files`, reads disk text with `fs::read_to_string`,
       computes language level (`parse_shebang_level` or `probe.language_level_for_path`), and sets
       `file_language_level` + `file_text` on `self.db`.
   - Calls `sync_workspace_files()` to update `self.db.set_workspace_files(...)` with a sorted,
     deduplicated `Vec<FileId>`.
3. **`DidOpen` / `DidChange` (`set_text`)**:
   - Inserts `uri` into `self.open_files`.
   - Allocates a `FileId` if new (updating `files`, `file_urls`, and `sync_workspace_files()`).
   - Updates `file_language_level` and `file_text` in `self.db` with the live editor buffer.
4. **`DidClose` (`close`)**:
   - Removes `uri` from `self.open_files`.
   - If `uri` maps to an existing file on disk (`fs::read_to_string` succeeds), reloads the disk text
     and language level into `self.db` **without** removing the `FileId` from `files`, `file_urls`,
     or `workspace_files`. Cross-file resolution into the closed file continues to work.
   - If the file no longer exists on disk, removes it from `files` and `file_urls`, updates
     `sync_workspace_files()`, and blanks its `file_text`.
   - `handle_notification` still publishes an empty diagnostics list for the closed URI so the
     client clears diagnostics on closed editors.

### `textDocument/definition` Handler & Capability

1. In `capabilities()`:
   - Set `definition_provider: Some(OneOf::Left(true))`.
2. In `handle_request()`:
   - Handle `GotoDefinition::METHOD` (`lsp_types::request::GotoDefinition`).
3. In `GlobalState::goto_definition(&self, params: GotoDefinitionParams) -> Option<GotoDefinitionResponse>`:
   - Resolve `file_id` from `params.text_document_position_params.text_document.uri`.
   - Convert LSP `Position` to byte `offset` via `position_to_offset`.
   - Call `ide::goto_definition(&self.db, file_id, offset)?`.
   - Look up target `uri` via `self.file_urls.get(&target.file_id)?` and target text via
     `self.db.file_text(target.file_id)`.
   - Convert `target.focus_range` via `text_range_to_range` and return
     `Some(GotoDefinitionResponse::Scalar(Location { uri: uri.clone(), range }))`.

## 5. Acceptance Criteria & Test Plan

### `crates/hir` Unit Tests

1. **`file_symbols` extraction**:
   - Extracts package, all 5 `ImportKind` variants (`SingleType`, `TypeOnDemand`, `StaticSingle`,
     `StaticOnDemand`, `Module`), top-level and nested classes/interfaces/enums/records/annotations,
     fields, methods, constructors, enum constants, record components, and compact-source top-level
     members with exact `full_range` and `selection_range`.
   - Ignores local classes and local variables inside method bodies.
2. **`workspace_index` & Salsa incremental behavior**:
   - Indexes types across multiple files by FQN and package name (including nested types
     `a.b.Outer.Inner` and unnamed-package types).
   - Editing a file updates `workspace_index` incrementally; editing only a method body without
     changing signatures/ranges preserves `FileSymbols` equality.
3. **Name resolution (`resolve_at_offset`)**:
   - Lexical locals: `LOCAL_VAR_DECL`, `PARAM`, `LAMBDA_PARAMS` (typed & untyped), `FOREACH_STMT`,
     `CATCH_CLAUSE`, `RESOURCE`, `TYPE_PATTERN`, `TYPE_PARAM`, block shadowing, and forward-reference
     exclusion.
   - Enclosing type & nested type members (fields, methods, record components, enum constants) and
     `this.member`.
   - Type & import precedence: enclosing/nested type shadows single-type import, which shadows
     same-package type, which shadows wildcard import (`*`).
   - Qualified type paths (`a.b.Foo`, `Outer.Inner`) in type positions, `new` expressions,
     expressions (`a.b.Foo.BAR`), and inside `import` declarations themselves.
   - Static imports (`import static a.b.Foo.BAR;` and `import static a.b.Foo.*;`) for fields,
     methods, and enum constants.
   - Basic member access (`Qualifier.member` / `Qualifier.method()`): static access on a resolved
     type name, instance access on a local/param/field whose declared type resolves to a workspace
     type, and overload arity preference.

### `crates/ide` Unit Tests

1. `goto_definition` within a single file (local var, param, field, method, nested class) and across
   multiple files (via single-type import, same-package, wildcard import, static import, and
   `obj.field` / `obj.method()`).
2. Upgraded `hover` returns qualified kind/name/signature Markdown for resolved types, members,
   locals, and type parameters, while preserving fallback behavior for `class` keyword and
   unresolved identifiers (e.g., `String`).

### `crates/java-analyzer` Integration Tests (`crates/java-analyzer/tests/lsp_goto_definition.rs`)

1. `capabilities()` advertises `definition_provider: Some(OneOf::Left(true))`.
2. Opening a multi-file workspace preloads `.java` files on disk so `textDocument/definition` jumps
   from an open file (`App.java`) to a closed file (`Model.java`) across packages via import and
   within the same package.
3. Editing `Model.java` in memory via `didOpen` + `didChange` immediately updates cross-file
   `textDocument/definition` and `textDocument/hover` targets without writing to disk.
4. Closing `Model.java` via `didClose` reloads its on-disk content (and clears its published
   diagnostics) so `textDocument/definition` from `App.java` into `Model.java` continues to succeed.
5. `textDocument/hover` over a resolved symbol returns semantic Markdown containing its qualified
   name/signature.

## Decisions

1. **`new Foo(...)` resolves `Foo` to `TypeSymbol`**: `Foo` in `NEW_EXPR` is parsed as `PATH_TYPE`
   and resolves uniformly to `ResolvedSymbol::Type(Foo)` rather than guessing constructors without
   type checking.
2. **Overload disambiguation by arity**: when multiple methods share a name on a type, call sites
   (`CALL_EXPR` / `METHOD_CALL_EXPR`) prefer matching `ARG_LIST` expression count before falling back
   to the first declaration.
3. **`GotoDefinitionResponse::Scalar(Location)` using `focus_range`**: returns the identifier range
   (`selection_range`) for maximum LSP client compatibility.
4. **Disk-backed fallback on `didClose`**: closing a workspace file reloads disk text rather than
   blanking the file, preserving cross-file semantic resolution across the workspace.
