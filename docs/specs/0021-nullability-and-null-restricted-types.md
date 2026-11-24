# Spec 0021: Nullability & Null-Restricted Types

Status: Accepted

## Motivation

`java-analyzer`'s type IR (`crates/hir/src/ty.rs`, introduced in Spec 0008) already carries a use-site `Nullability` slot (`Unspecified` | `NonNull` | `Nullable`) on `Ty::Class`, `Ty::TypeVar`, and `Ty::Array`, plus null-restricted subtyping and LUB rules (`null` ⊄ `T!`, `T! <: T?`, `LUB(T!, null) == T?`).

Modern Java ecosystems (Spring Framework 7, Guava, JUnit, Gradle, and Kotlin-interop codebases) already annotate nullability today using **JSpecify 1.0** (`org.jspecify.annotations.*`) and legacy nullability annotations (`jakarta.annotation.*`, `javax.annotation.*`, `org.jetbrains.annotations.*`, etc.), well ahead of Java 28+ Project Valhalla null-restricted type syntax (`Point!`, `Point?`).

This spec defines a **source-agnostic nullability & null-restricted type model** in `crates/hir` and `crates/ide` that:
1. Treats **JSpecify 1.0** as the primary annotation source today, alongside a curated compatibility table for widely used third-party nullability annotations.
2. Preserves **provenance (`NullabilitySource`)** so hover and future Spec 0012 diagnostics can report *why* a type is non-null or nullable (e.g. `@NullMarked`, `@Nullable`, third-party annotation, expression inference, or future Java 28+ `!`/`?` syntax).
3. Strictly follows **JLS §9.7.4** for type-use vs. declaration-site annotation placement (including arrays and nested types).
4. Scales seamlessly to **Java 28+ Valhalla null-restricted types** (`T!`, `T?`) without redesigning the HIR or IDE layers.

---

## Goals

1. **Source-Agnostic Nullability Model with Provenance (`crates/hir`):**
   - Represent lexical defaulting scopes via a generic `NullnessScope` (`Inherited` | `NonNullByDefault` | `Unspecified`) paired with `NullabilitySource` (`JSpecify` | `ThirdPartyAnnotation` | `LanguageSyntax` | `Inferred`).
   - Keep all scope and annotation metadata **position-independent** (zero `TextRange`s in `FileItemTree`, `WorkspaceIndex`, and `ClasspathIndex`) so Salsa early-cutoff is preserved.
2. **JLS §9.7.4-Compliant Source Extraction (`item_tree.rs`, `symbols.rs`):**
   - Extract scope defaults from `module-info.java`, `package-info.java`, type declarations, and member declarations, resolving annotation names via file imports.
   - Follow JLS §9.7.4 for type-use annotations (`@Nullable String[]` annotates the element `String`; `String @Nullable []` annotates the array; `Outer.@Nullable Inner` annotates `Inner`) and declaration-site annotations (legacy non-`TYPE_USE` annotations on fields/methods/parameters).
3. **Binary `.class` Annotation & Type-Annotation Decoding (`classfile.rs`, `classpath.rs`):**
   - Upgrade `ristretto_classfile` from `0.31` to `0.35` (matching `rustc 1.95.0` MSRV, Java 27 classfile support).
   - Decode **both** `RuntimeVisibleAnnotations` / `RuntimeInvisibleAnnotations` (covering `RUNTIME`-retention JSpecify and `CLASS`-retention JetBrains annotations) and **both** `RuntimeVisibleTypeAnnotations` / `RuntimeInvisibleTypeAnnotations` (JVMS §4.7.20 `TargetType` + `TypePath` for nested generic arguments and array dimensions).
4. **JSpecify 1.0 Type Defaulting, Substitution & Inference (`ty.rs`, `infer.rs`):**
   - Resolve effective lexical scope (`member → inner type → outer type → package-info → module-info`).
   - Apply JSpecify 1.0 defaulting rules (including parametric unannotated `TypeVar`, implicit non-null `Object` upper bound in `NonNullByDefault` scopes, wildcard bound inheritance, and unforced local-variable root types).
   - Propagate nullability through `Substitution::apply` (`T` parametric, `@Nullable T` nullable projection, `@NonNull T` non-null projection) and expression inference (literals, `new`, `this`, `null`, ternary/switch LUB, method/field accesses).
5. **Concise IDE Hover UX (`crates/ide`):**
   - Display nullability using concise suffix syntax (`User?`, `User!`, matching IntelliJ/Kotlin/Valhalla conventions; `Unspecified` remains plain `User` with zero noise on unannotated code).
   - Optionally surface provenance in hover notes when helpful (e.g., inferred via `@NullMarked`).
6. **Phased Rollout:**
   - **Phase 1:** Source & binary extraction (`ristretto_classfile 0.35`), scope resolution, JSpecify `Ty` defaulting/substitution/inference, and concise hover display.
   - **Phase 2 (paired with Spec 0012 & Spec 0020):** Null-safety diagnostics (`nullability/nullable-dereference`, `nullability/nullable-return`, `nullability/nullable-argument`, `nullability/incompatible-override`) with `DiagnosticRelatedInformation` pointing to the originating `@Nullable` / `@NullMarked` declaration and Tier 1 caret snapshots.

---

## Non-goals (for this spec)

- Java 28+ preview parser syntax for `!`/`?` type suffixes or `value class` modifiers (deferred until Java 28 preview support in `crates/syntax`; the HIR model already supports them via `NullabilitySource::LanguageSyntax`).
- Full dataflow / path-sensitive null-narrowing analysis (`if (x != null) { x.foo(); }`) in Phase 1 (belongs in Phase 2 / Spec 0012).
- Custom user-configured annotation FQN lists in LSP settings (can be added later as a workspace config extension).

---

## Relationship to Other Specs

- **Spec 0008 (`0008-expression-type-inference-and-generics.md`):** Already provides `Nullability` on `Ty::Class`, `Ty::TypeVar`, and `Ty::Array`, plus `TypeIdentity` and null-restricted subtyping/LUB. This spec populates `Nullability` from real source and binary annotations and adds scope defaulting & provenance.
- **Spec 0012 (Semantic Diagnostics, future) & Spec 0020 (`0020-diagnostic-conformance-testing.md`):** Phase 2 null-safety diagnostics will use Spec 0012's diagnostic pipeline and Spec 0020's Tier 1 caret snapshot harness.

---

## Pure Data Model & Algorithms (`crates/hir`)

### 1. Source-Agnostic Scope & Provenance Types

```rust
/// Where a nullability constraint or defaulting scope originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NullabilitySource {
    /// `org.jspecify.annotations.{Nullable, NonNull, NullMarked, NullUnmarked}`.
    JSpecify,
    /// Recognized third-party annotations (`jakarta.annotation.*`, `javax.annotation.*`,
    /// `org.jetbrains.annotations.*`, `org.checkerframework.checker.nullness.qual.*`,
    /// `edu.umd.cs.findbugs.annotations.*`, `org.springframework.lang.*`).
    ThirdPartyAnnotation,
    /// Built-in Java language semantics (Java 28+ `T!`/`T?` syntax, primitives, literals, `new`, `this`).
    LanguageSyntax,
    /// Derived via expression inference, LUB, or generic substitution.
    Inferred,
}

/// Lexical nullability defaulting scope on a module, package, type, or member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NullnessScope {
    /// Inherit from enclosing scope (`member → inner type → outer type → package → module`).
    #[default]
    Inherited,
    /// Unannotated type uses in scope default to `Nullability::NonNull` (e.g. `@NullMarked`).
    NonNullByDefault(NullabilitySource),
    /// Explicitly resets scope default to `Nullability::Unspecified` (e.g. `@NullUnmarked`).
    Unspecified(NullabilitySource),
}
```

### 2. Recognized Annotation Table (JSpecify-First + Curated Aliases)

| Role | Canonical (JSpecify 1.0) | Curated Third-Party Aliases |
|---|---|---|
| **Nullable** | `org.jspecify.annotations.Nullable` | `jakarta.annotation.Nullable`, `javax.annotation.Nullable`, `org.jetbrains.annotations.Nullable`, `org.checkerframework.checker.nullness.qual.Nullable`, `edu.umd.cs.findbugs.annotations.Nullable`, `org.springframework.lang.Nullable` |
| **NonNull** | `org.jspecify.annotations.NonNull` | `jakarta.annotation.Nonnull`, `javax.annotation.Nonnull`, `org.jetbrains.annotations.NotNull`, `org.checkerframework.checker.nullness.qual.NonNull`, `edu.umd.cs.findbugs.annotations.NonNull`, `org.springframework.lang.NonNull` |
| **NonNullByDefault scope** | `org.jspecify.annotations.NullMarked` | `org.springframework.lang.NonNullApi` (package-level) |
| **Unspecified scope** | `org.jspecify.annotations.NullUnmarked` | — |

### 3. Position-Independent Index Extensions (Zero `TextRange`s)

- **`FileItemTree` (`item_tree.rs`):**
  - `pub nullness_scope: NullnessScope` (populated from `package-info.java` / `module-info.java` annotations).
  - `TypeItem.nullness_scope: NullnessScope`, `MemberItem.nullness_scope: NullnessScope`.
  - Declaration-site nullability on `MemberItem` and `ParamSummary` for annotations attached to `MODIFIER_LIST`, while type-use annotations stay attached to their exact type position per **JLS §9.7.4** (`@Nullable String[]` → element `String?`, `String @Nullable []` → array `[]?`, `Outer.@Nullable Inner` → `Inner?`).
- **`WorkspaceIndex` (`symbols.rs`):**
  - `pub package_nullness: BTreeMap<String, NullnessScope>`
  - `pub module_nullness: BTreeMap<SmolStr, NullnessScope>`
- **`ClasspathIndex` & `classfile.rs`:**
  - Upgrade `ristretto_classfile = "0.35"` in root `Cargo.toml`.
  - Add `nullness_scope: NullnessScope` to `BinaryTypeSymbol`, `BinaryMemberSymbol`, `BinaryModuleInfo`, and `package_nullness: BTreeMap<String, NullnessScope>` to `ClasspathIndex`.
  - Decode `RuntimeVisibleAnnotations` + `RuntimeInvisibleAnnotations` (scope + declaration-site fallback) and `RuntimeVisibleTypeAnnotations` + `RuntimeInvisibleTypeAnnotations` (JVMS §4.7.20 `TargetType` + `TypePath` for nested generic args, wildcards, inner types, and array dimensions).

### 4. Scope Resolution & Defaulting Rules (`ty.rs`, `infer.rs`)

- `effective_nullness_scope(db, owner) -> NullnessScope`: walks `member → inner type → outer types → package → module`, returning the first non-`Inherited` scope (or `NullnessScope::Inherited` when unmarked).
- When `NonNullByDefault(_)` is active:
  - Unannotated class/interface/array type uses in return types, field types, parameter types, type arguments, and explicit bounds default to `Nullability::NonNull`.
  - **JLS / JSpecify exceptions:**
    - Unannotated `TypeVar` usages (`T`) stay parametric (`Nullability::Unspecified` on the `TypeVar`), preserving the substituted type argument's nullability verbatim; `@Nullable T` projects to `Nullable` and `@NonNull T` projects to `NonNull`.
    - Unbounded type-parameter declarations (`class Box<T>`) receive implicit upper bound `java.lang.Object!` (`NonNull`); `<T extends @Nullable Object>` allows nullable arguments.
    - Unbounded wildcards (`?`) inherit the corresponding type parameter's upper bound nullability.
    - Local variable root types (`String s = ...`, `var s = ...`) are not forced `NonNull` by scope; they infer nullability from their initializer.
    - `catch` parameters and `extends`/`implements` root types are implicitly `NonNull`.

---

## IDE Integration (`crates/ide`)

- **Phase 1 (Hover):**
  - Concise suffix formatting via `Ty::display_short()` (`User?` for nullable, `User!` for non-null, `User` for unspecified — matching IntelliJ/Kotlin/Valhalla style).
  - Unannotated legacy code remains 100% `Unspecified` (`User`), preserving clean hover output on existing Java 8–25 codebases.
- **Phase 2 (Null-Safety Diagnostics — paired with Spec 0012 & Spec 0020):**
  - Emit actionable null-safety diagnostics with secondary `DiagnosticRelatedInformation` pointing to the `@Nullable` or `@NullMarked` declaration site and `help:` hints.

---

## Verification & Acceptance Criteria

1. **Source extraction & JLS §9.7.4 placement:**
   - `@NullMarked` / `@NullUnmarked` nesting across `module-info.java`, `package-info.java`, outer class, inner class, and method.
   - `@Nullable String[]` resolves to `String?[]!` (in `@NullMarked`) vs `String @Nullable []` resolving to `String![]?`.
   - `Outer.@Nullable Inner` annotates `Inner`, not `Outer`.
2. **Binary `.class` decoding (`ristretto_classfile 0.35`):**
   - Synthetic `.class` tests for both `RuntimeVisible*` (JSpecify) and `RuntimeInvisible*` (JetBrains `CLASS`-retention) annotations and `TypePath`-nested generic type annotations (`List<@Nullable String>`, `Map<String, @Nullable List<@NonNull User>>`).
3. **JSpecify substitution & inference:**
   - Parametric `T`, `@Nullable T`, `@NonNull T`, `<T>` vs `<T extends @Nullable Object>`, local `var`, and ternary LUB (`flag ? user : null` → `User?`).
4. **Salsa early-cutoff & zero-panic discipline:**
   - Body edits do not invalidate `file_item_tree`, `workspace_index`, or `classpath_index`; zero `.unwrap()`/`.expect()` in non-test code.
5. **Verification gate:**
   - `cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace --all-targets` pass cleanly.
