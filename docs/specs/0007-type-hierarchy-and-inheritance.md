# Spec 0007: Type Hierarchy, Inheritance & `super` Resolution

Status: Accepted

## Motivation

With Spec 0005 (cross-file source symbols) and Spec 0006 (JDK & JAR binary classpath indexing), `java-analyzer` resolves direct members declared on a type. However, real Java code relies heavily on inheritance and polymorphism:

- Calling inherited methods or reading inherited fields (`this.toString()`, `list.size()`, `sub.baseMethod()`, `MyException.getMessage()`).
- Accessing superclass members and constructors via `super.member`, `Interface.super.defaultMethod()`, and `super(...)`.
- Navigating from an interface or abstract base class/method to its concrete implementations (`textDocument/implementation`), or from a variable/field/expression to its declared type (`textDocument/typeDefinition`).
- Exploring a type's supertypes and subtypes (`textDocument/prepareTypeHierarchy`, `typeHierarchy/supertypes`, `typeHierarchy/subtypes`).

This spec defines how `hir` resolves `extends`, `implements`, and `permits` clauses across both workspace sources and binary JDK/JAR types, builds a Salsa-tracked, cycle-safe type hierarchy, extends member and `super` resolution, and exposes `implementation`, `typeDefinition`, and LSP 3.17 Type Hierarchy through `ide` and `java-analyzer`.

## Goals

1. **Position-independent inheritance metadata in `FileItemTree`**:
   - Extract `extends`, `implements`, `permits` type references and `is_abstract` / `is_final` / `is_default` / `has_override` flags into `TypeItem` and `MemberItem` with **zero `TextRange`s**, preserving Salsa early-cutoff across method body edits.
2. **Unified, cycle-safe type hierarchy across source and binary types**:
   - Resolve `extends`, `implements`, and `permits` clauses in the declaring file's scope across `WorkspaceIndex` and `ClasspathIndex` (stripping generic type arguments for hierarchy edges).
   - Apply JLS implicit supertypes (`class` → `java.lang.Object`, `enum` → `java.lang.Enum`, `record` → `java.lang.Record`, `@interface` → `java.lang.annotation.Annotation`).
   - Guarantee cycle safety on malformed cyclic inheritance (`class A extends B {} class B extends A {}`).
   - Provide a Salsa-tracked, position-independent `WorkspaceHierarchyIndex` for fast subtype/implementation lookups.
3. **Inherited member lookup & `super` resolution**:
   - Resolve inherited fields, methods, and nested types along the linearized supertype chain (nearest superclass first, then interfaces BFS, `java.lang.Object` last), excluding non-inherited interface `static` methods (JLS §8.4.8) and constructors.
   - Resolve `super.field`, `super.method(...)`, `Interface.super.method(...)`, and `super(...)` constructor calls.
   - Match overriding methods (`@Override` and signature/arity matching) across the hierarchy.
4. **LSP navigation features**:
   - `textDocument/implementation` for types (workspace subtypes) and methods (workspace overrides).
   - `textDocument/typeDefinition` for variables, parameters, fields, record components, and typed expressions (navigating to either workspace source or materialized JDK/JAR binary types via Spec 0006).
   - LSP 3.17 Type Hierarchy (`textDocument/prepareTypeHierarchy`, `typeHierarchy/supertypes`, `typeHierarchy/subtypes`).

## Non-goals (for this spec)

- Generic type-argument substitution (`List<User>.get(0)` → `User`) and `var` expression type inference (covered by **Spec 0008**).
- Semantic diagnostics for invalid `@Override`, unimplemented abstract methods, or cyclic inheritance errors (covered by **Spec 0012**; this spec only ensures safe resolution without panics or infinite loops).
- Full binary subtype scanning across all 5,000+ JDK classes for `textDocument/implementation` (subtype queries index workspace types plus `permitted_subclasses`; scanning all JDK classes for `implements Serializable` would flood editors with noise).

---

## Engineering & Project-Skill Constraints

Implementation adheres to `AGENTS.md` Rules 1–8 and `.agents/skills/` (`rust-best-practices`, `ms-rust-guidelines`, `rust-2024-idioms`), with Rule 6 taking precedence:

- **Strict 4-layer boundary (`AGENTS.md` Rule 1 & Rule 6):** All hierarchy resolution lives purely in `crates/hir`; `crates/ide` exposes `goto_implementation`, `goto_type_definition`, and `type_hierarchy` using `NavigationTarget::Source | Binary`; `crates/java-analyzer` depends only on `ide` and handles LSP JSON/URI conversion.
- **Salsa 0.28 early-cutoff & determinism:** Zero `TextRange`s in `FileItemTree`, `ModuleIndex`, `WorkspaceIndex`, or `WorkspaceHierarchyIndex`; return `Arc<T>` directly from `#[salsa::tracked]` queries; use deterministic `BTreeMap` / `BTreeSet` collections; drop `Analysis` snapshots before mutating `AnalysisHost`.
- **Panic-free & Rust 2024 idioms:** Zero `.unwrap()`, `.expect()`, or unchecked indexing in non-test code; use `SmolStr`, borrowing, `#[must_use]`, and Rust 2024 `if let` chains / `let ... else`.
- **`javac`-familiar, `rust-analyzer`-rich diagnostics:** Any syntax or typechecker error surfaced by `java-analyzer` (including future Spec 0012 hierarchy diagnostics such as `cyclic inheritance involving ...`, `method does not override or implement a method from a supertype`, or `... is not abstract and does not override abstract method ...`) should use a familiar `javac`-like primary message enriched with `rustc`/`rust-analyzer`-grade usability: precise primary spans, secondary `DiagnosticRelatedInformation` locations (e.g. the supertype declaration, conflicting signature, or cycle edge), and actionable `help:` / `note:` hints. Extra IDE inspections (nullability analysis, unused symbols, custom lints) may use `java-analyzer`'s own wording with the same rich UX.

---

## Pure Data Model & Queries (`crates/hir`)

### 1. `FileItemTree` & `TypeSymbol` Extensions (`crates/hir/src/item_tree.rs`, `symbols.rs`)

Add position-independent supertype and modifier fields to `TypeItem` (and mirrored on `TypeSymbol`):

```rust
pub struct TypeItem {
    // ... existing fields ...
    /// Raw type references from `extends` clause (e.g. `["Base", "com.foo.Iface<T>"]`).
    pub extends: Vec<String>,
    /// Raw type references from `implements` clause.
    pub implements: Vec<String>,
    /// Raw type references from `permits` clause (sealed types).
    pub permits: Vec<String>,
    pub is_abstract: bool,
    pub is_final: bool,
}

pub struct MemberItem {
    // ... existing fields ...
    pub is_default: bool,
    pub is_abstract: bool,
    pub has_override: bool,
}
```

### 2. Hierarchy Graph & Salsa Query (`crates/hir/src/hierarchy.rs`, `crates/hir/src/lib.rs`)

```rust
/// Resolved direct supertypes for a canonical type FQN.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirectSupertypes {
    /// Direct superclass FQN (`None` for `java.lang.Object` and interfaces).
    pub super_class: Option<String>,
    /// Directly implemented/extended interface FQNs (deduplicated, declaration order).
    pub interfaces: Vec<String>,
    /// Directly permitted subtype FQNs (sealed types).
    pub permits: Vec<String>,
}

/// Position-independent reverse index of workspace subtype relationships.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceHierarchyIndex {
    /// Supertype FQN -> sorted list of direct workspace subtype FQNs.
    pub direct_subtypes: BTreeMap<String, Vec<String>>,
    /// Workspace type FQN -> resolved `DirectSupertypes`.
    pub supertypes: BTreeMap<String, DirectSupertypes>,
}
```

Add `fn workspace_hierarchy_index(&self) -> Arc<WorkspaceHierarchyIndex>;` to `DefDatabase`, backed by a `#[salsa::tracked]` query depending on `WorkspaceModules` and `ClasspathInput` (so method-body edits hit Salsa early-cutoff and never recompute the hierarchy!).

Key pure functions in `crates/hir/src/hierarchy.rs`:
- `direct_supertypes(db: &dyn DefDatabase, fqn: &str) -> DirectSupertypes` — checks `workspace_hierarchy_index` first, then `classpath_index` (normalizing `java.lang.Object` / `Enum` / `Record` / `Annotation` implicit supertypes).
- `linearized_supertypes(db: &dyn DefDatabase, fqn: &str) -> Vec<String>` — cycle-safe (`FxHashSet<String>` visited guard) linearization: superclass chain first, then interface BFS, `java.lang.Object` last (excluding `fqn` itself).
- `direct_subtypes(db: &dyn DefDatabase, fqn: &str) -> Vec<String>` — O(log N) lookup in `workspace_hierarchy_index.direct_subtypes` (plus `permitted_subclasses` when present in workspace).
- `all_subtypes(db: &dyn DefDatabase, fqn: &str) -> Vec<String>` — cycle-safe transitive closure of `direct_subtypes` in deterministic sorted order.
- `find_overridden_methods(db: &dyn DefDatabase, owner_fqn: &str, method_name: &str, param_count: usize) -> Vec<MethodTarget>` — walks `linearized_supertypes` for matching non-static, non-constructor methods.
- `find_overriding_methods(db: &dyn DefDatabase, owner_fqn: &str, method_name: &str, param_count: usize) -> Vec<(FileId, MemberId)>` — walks `all_subtypes(db, owner_fqn)` for workspace methods overriding `(method_name, param_count)`.

### 3. Inherited Member & `super` Resolution (`crates/hir/src/resolve.rs`)

- **Inherited member lookup:** When resolving a member on a type `T` (receiver expression, `this.m`, or unqualified `m` inside `T`):
  1. Check `T`'s direct members (workspace or binary).
  2. Walk `linearized_supertypes(db, T)`:
     - Check fields, methods (excluding `static` methods on interfaces per JLS §8.4.8), enum constants, record components, and nested types.
     - Preserve arity / varargs overload preference across the hierarchy (preferring the most specific overriding declaration).
- **`super` expression & qualified `I.super`:**
  - `super` alone inside class `C` resolves to `C`'s `super_class` (defaulting to `java.lang.Object`).
  - `super.field` / `super.method(...)` resolves starting from `C`'s `super_class` (skipping `C`'s own members).
  - `I.super.method(...)` resolves `I` in `C`'s superinterfaces and looks up `method` starting from `I` (supporting interface `default` method disambiguation).
  - `super(...)` inside a constructor body resolves to the matching constructor of `C`'s `super_class` (by arity/varargs).
- **Declaration-site type extraction for `typeDefinition`:**
  - Expose `resolve_type_of_symbol_at_offset(db, file_id, offset) -> Option<ResolvedSymbol>` resolving the declared type of a local variable, parameter, field, record component, method return type, or expression to its `ResolvedSymbol` (workspace or binary).

---

## IDE & LSP Integration (`crates/ide` & `crates/java-analyzer`)

### `crates/ide`
- `ide::hover`: When hovering a method that overrides a superclass or superinterface method, append a brief `*overrides \`Owner.method(...)\`*` note below the signature.
- `Analysis::goto_implementation(&self, file_id: FileId, offset: u32) -> Cancellable<Vec<NavigationTarget>>`:
  - On a type declaration or type reference → all workspace subtypes (`all_subtypes`).
  - On a method declaration or call site → all workspace overriding methods (`find_overriding_methods`).
- `Analysis::goto_type_definition(&self, file_id: FileId, offset: u32) -> Cancellable<Option<NavigationTarget>>`:
  - Resolves the symbol's declared type via `hir::resolve_type_of_symbol_at_offset` and returns `NavigationTarget::Source` or `NavigationTarget::Binary`.
- `Analysis::prepare_type_hierarchy(&self, file_id: FileId, offset: u32) -> Cancellable<Option<TypeHierarchyNode>>`
- `Analysis::type_hierarchy_supertypes(&self, fqn: &str) -> Cancellable<Vec<TypeHierarchyNode>>`
- `Analysis::type_hierarchy_subtypes(&self, fqn: &str) -> Cancellable<Vec<TypeHierarchyNode>>`
  - `TypeHierarchyNode` carries `name: SmolStr`, `qualified_name: String`, `kind: SymbolKind`, `target: NavigationTarget`.

### `crates/java-analyzer`
- Advertise in `capabilities()`:
  - `implementation_provider: Some(ImplementationProviderCapability::Simple(true))`
  - `type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true))`
  - `type_hierarchy_provider: Some(TypeHierarchyServerCapability::Simple(true))`
- Handle in `server.rs` (running on background worker threads via O(1) `GlobalStateSnapshot`):
  - `textDocument/implementation` → `GotoImplementationResponse::Array(locations)`
  - `textDocument/typeDefinition` → `GotoTypeDefinitionResponse::Scalar(location)` (reusing Spec 0006's `materialize_binary_location` for JDK/JAR types!)
  - `textDocument/prepareTypeHierarchy`, `typeHierarchy/supertypes`, `typeHierarchy/subtypes` (storing canonical FQN in `TypeHierarchyItem::data` for stateless follow-up queries).

---

## Verification & Acceptance Criteria

1. **TDD & Verification Gate (`AGENTS.md` Rules 2 & 7):** Failing tests written first; `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --all-targets` pass cleanly.
2. **Salsa early-cutoff:** Editing a method body does not invalidate `workspace_hierarchy_index`.
3. **Cycle safety:** Cyclic `extends`/`implements` declarations never panic or infinite-loop.
4. **Cross-source & binary inheritance:** Workspace classes extending/implementing workspace types or JDK/JAR types (`extends Exception`, `implements List<String>`, `implements AutoCloseable`) resolve inherited methods (`getMessage()`, `size()`, `close()`, `toString()`).
5. **`super` resolution:** `super.field`, `super.method()`, `Interface.super.defaultMethod()`, and `super(...)` constructors resolve accurately (and skip subclass overrides).
6. **LSP features:** Integration tests verify `textDocument/implementation`, `textDocument/typeDefinition` (including jumping to JDK binary types), and `prepareTypeHierarchy` / `supertypes` / `subtypes`.
