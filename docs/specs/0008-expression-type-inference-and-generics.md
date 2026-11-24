# Spec 0008: Expression Type Inference & Generics

Status: Accepted

## Motivation

With Specs 0005–0007, `java-analyzer` resolves direct and inherited symbols across workspace sources and binary JDK/JAR types, but type tracking is still erased to raw type names (`String` / `TypeRefTarget`):

- Generic type arguments are discarded, so `List<User> users; users.get(0).getName()` cannot resolve `getName()` (it sees `E`, not `User`).
- Local variable type inference (`var x = ...;`, `for (var u : users)`, `try (var in = ...)`) cannot resolve `x`'s type or any members accessed on `x`.
- Chained expressions involving literals, arrays, casts, ternaries, switch expressions, `new ArrayList<>()`, or generic fluent pipelines (`users.stream().map(u -> u.getName()).toList()`) lose type information mid-chain.
- Untyped lambda parameters (`u -> u.getName()`) have no inferred type, breaking hover, goto-definition, and member lookup inside lambda bodies.
- Overloaded methods and constructors with the same arity (`println(int)` vs `println(String)`, `List.remove(int)` vs `List.remove(Object)`) fall back to declaration order instead of matching argument types.

This spec introduces a structured, cycle-safe Java type representation (`Ty`) and expression type inference engine in `crates/hir` covering `var`, generic substitution across inheritance chains, lambda/method-reference target typing, and type-based overload resolution — unlocking accurate hover, goto-definition, and goto-type-definition across modern generic Java codebases.

## Goals

1. **Structured Java Type IR & Generic Substitution (`crates/hir/src/ty.rs`)**:
   - Represent primitives, `void`, `null`, parameterized/raw class & interface types (`Ty::Class`), arrays (`Ty::Array`), type variables with upper bounds (`Ty::TypeVar`), intersection types (`Ty::Intersection`), wildcards (`?`, `? extends T`, `? super T`), and `Ty::Unknown` for graceful error recovery.
   - Parse type references (from source AST/strings and binary JVM `Signature` attributes) into `Ty` in their declaring scope.
   - Compose `Substitution` maps (`type param -> Ty`) transitively up inheritance chains (both workspace source `extends`/`implements` and binary classfile generic `Signature`s), e.g. `ArrayList<User>` → `List<User>` → `Collection<User>` → `Iterable<User>`.
   - Apply wildcard projection on member access (`List<? extends Number>.get(0)` → `Number`, `List<? super Integer>.get(0)` → `java.lang.Object`).
2. **Preserve Generic Metadata in `FileItemTree` & `ClasspathIndex`**:
   - Store position-independent type-parameter bounds and method/constructor type parameters in `FileItemTree` (`TypeItem`, `MemberItem`) with **zero `TextRange`s** (preserving Salsa early-cutoff).
   - Decode generic superclass, generic superinterfaces, type-parameter bounds, and method type parameters from `.class` `Signature` attributes in `crates/hir/src/classfile.rs` and store them on `BinaryTypeSymbol` / `BinaryMemberSymbol`.
3. **Expression Type Inference (`crates/hir/src/infer.rs`)**:
   - Infer types for all Java expressions: literals, class literals (`Foo.class`), `this`/`super`, variables/fields/record components/enum constants, chained field accesses & method calls, `new` (including diamond `<>`, arrays, and anonymous classes), array indexing (`arr[i]`) & `.length`, unary/binary/assignment/cast/`instanceof` operators, parenthesized expressions, ternary (`?:`), switch expressions (`switch (...) { ... }`), pattern variables, lambdas, and method references.
   - Infer `var` types in local variables, enhanced `for` loops (arrays and `Iterable<E>`), try-with-resources, record patterns, and `(var a, var b) -> ...` lambda parameters.
   - Infer generic method type arguments from explicit type witnesses (`Collections.<String>emptyList()`), argument unification (`List.of("a")`, `Optional.of(user)`, `stream.map(u -> u.getName())`), and expected target types (`List<String> xs = Collections.emptyList()`).
4. **Functional Interfaces, Lambdas & Method References**:
   - Identify Single Abstract Method (SAM) signatures on functional interfaces (accounting for inherited `java.lang.Object` methods and `default`/`static` methods).
   - Propagate target types from assignments, variable initializers, `return` statements, casts, and method-call arguments into lambda parameters and method references.
5. **Type-Based Overload Resolution**:
   - Disambiguate same-arity method and constructor overloads using JLS §15.12.2 phases: (1) strict invocation (exact, primitive widening, reference subtyping), (2) loose invocation (boxing/unboxing), (3) varargs, with a most-specific-candidate tie-breaker and deterministic fallback when argument types are `Ty::Unknown`.
6. **IDE Hover & Navigation Improvements (`crates/ide`)**:
   - Hovering a `var` variable or untyped lambda parameter displays its inferred type (formatted cleanly, e.g. `User` or `List<User>`).
   - Hovering a generic member reference (`users.get(0)`) displays the substituted return/field type alongside the signature.
   - `goto_definition` and `goto_type_definition` work seamlessly through `var`, generics, lambdas, method references, and chained expressions (both source and JDK/JAR binary targets).

## Non-goals (for this spec)

- Full JLS Chapter 18 poly-expression constraint-solver / capture-conversion completeness for pathological nested wildcard inference (we implement practical bidirectional inference + unification covering 95%+ of real-world Java, degrading gracefully to `Ty::Unknown` or upper bounds without panicking).
- Semantic type-mismatch / incompatible-types / unchecked-cast diagnostics (covered by **Spec 0012**; when implemented there, all typechecker errors will follow the `javac`-familiar primary message + `rust-analyzer`-rich spans/related info/hints rule).
- Inlay hints for inferred `var` types or chained expression types (covered by **Spec 0013**).
- Code completion using inferred receiver types (covered by **Spec 0010**, which will build directly on `hir::infer` and `hir::ty`).

---

## Engineering & Project-Skill Constraints

Implementation adheres to `AGENTS.md` Rules 1–8 and `.agents/skills/` (`rust-best-practices`, `ms-rust-guidelines`, `rust-2024-idioms`), with Rule 6 taking precedence:

- **Strict 4-layer boundary (`AGENTS.md` Rule 1 & Rule 6):** All type representation, substitution, inference, and overload resolution live purely in `crates/hir`; `crates/ide` consumes `hir` APIs for hover and navigation; `crates/java-analyzer` depends only on `ide` and requires no new LSP protocol methods.
- **Salsa 0.28 early-cutoff & determinism:** Zero `TextRange`s in `FileItemTree`, `ModuleIndex`, `WorkspaceIndex`, `WorkspaceHierarchyIndex`, or `ClasspathIndex`. Use `BTreeMap` / `BTreeSet` for substitutions and deterministic ordering. Body edits must never invalidate `file_item_tree`, `workspace_index`, or `workspace_hierarchy_index`.
- **Panic-free, cycle-safe, bounded recursion:** Zero `.unwrap()`, `.expect()`, or unchecked indexing in non-test code. Enforce explicit recursion depth limits (`MAX_INFER_DEPTH = 32`, `MAX_SUBST_DEPTH = 32`) and visited-type cycle guards on all hierarchy/substitution/inference walks so malformed or self-referential code (`var x = x;`, `class A<T extends A<T>>`) never stack-overflows or loops.
- **`javac`-familiar, `rust-analyzer`-rich diagnostics:** Although this spec does not emit new semantic diagnostics, the `Ty` display formatting (`display_short`, `display_qualified`) is designed to serve both hover UX and future Spec 0012 `javac`-familiar typechecker messages (e.g. `incompatible types: String cannot be converted to int`, with secondary spans and actionable hints).

---

## Pure Data Model & Algorithms (`crates/hir`)

### 1. Metadata Extensions (`item_tree.rs`, `symbols.rs`, `classpath.rs`, `classfile.rs`)

Add position-independent generic metadata without introducing any `TextRange`s:

- **Source `TypeItem` / `TypeSymbol` (`item_tree.rs`, `symbols.rs`)**:
  - Add `type_param_bounds: BTreeMap<SmolStr, Vec<String>>` (or `Vec<TypeParamDef>` with `name: SmolStr`, `bounds: Vec<String>`) capturing `extends` bounds on class/interface/record type parameters (e.g. `<T extends Number & Comparable<T>>`).
- **Source `MemberItem` / `MemberSymbol`**:
  - Add `type_params: Vec<SmolStr>` and `type_param_bounds: BTreeMap<SmolStr, Vec<String>>` for generic methods and constructors (`<T extends Comparable<T>> T max(T a, T b)`).
- **Binary `BinaryTypeSymbol` & `BinaryMemberSymbol` (`classpath.rs`, `classfile.rs`)**:
  - Extend `classfile.rs` class `Signature` parser to decode:
    - Type-parameter bounds (`type_param_bounds: BTreeMap<SmolStr, Vec<String>>`).
    - Generic superclass reference (`super_class_generic: Option<String>`, e.g. `"java.util.AbstractList<E>"`) and generic interface references (`interfaces_generic: Vec<String>`, e.g. `["java.util.List<E>", "java.util.RandomAccess"]`), falling back to erased `super_class` / `interfaces` when no `Signature` attribute is present.
  - Extend `classfile.rs` method `Signature` parser to decode method-level `type_params: Vec<SmolStr>` and `type_param_bounds: BTreeMap<SmolStr, Vec<String>>` on `BinaryMemberSymbol`.

### 2. Type Representation & Substitution (`crates/hir/src/ty.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PrimitiveType {
    Boolean,
    Byte,
    Short,
    Int,
    Long,
    Char,
    Float,
    Double,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WildcardBound {
    Unbounded,
    Extends(Box<Ty>),
    Super(Box<Ty>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeArg {
    Type(Ty),
    Wildcard(WildcardBound),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    Primitive(PrimitiveType),
    Void,
    Null,
    /// Parameterized or raw class/interface/enum/record/annotation type.
    /// `fqn` is canonical dot-separated (e.g. `"java.util.List"`).
    /// `type_args` is empty for non-generic or raw types.
    Class {
        fqn: String,
        type_args: Vec<TypeArg>,
    },
    Array(Box<Ty>),
    TypeVar {
        name: SmolStr,
        bounds: Vec<Ty>,
    },
    Intersection(Vec<Ty>),
    Unknown,
}

/// Deterministic type-parameter substitution map (`T -> User`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Substitution {
    pub map: BTreeMap<SmolStr, Ty>,
}
```

Key pure operations in `ty.rs`:
- **Parsing**: `parse_type_ref(raw: &str, resolve_type: impl FnMut(&[&str]) -> Option<Ty>) -> Ty` — parses Java type syntax (primitives, `void`, arrays `T[][]`, qualified/nested generic paths `Map.Entry<K, V>`, wildcards `?`, `? extends T`, `? super T`, annotations stripped) into `Ty`.
- **Boxing / Unboxing & Numeric Promotion**:
  - `PrimitiveType::boxed_fqn()` (`int` → `"java.lang.Integer"`, etc.) and `PrimitiveType::from_boxed_fqn(&str)`.
  - `PrimitiveType::widens_to(other)` (`byte -> short -> int -> long -> float -> double`, `char -> int`).
  - `unary_numeric_promotion(&Ty) -> Option<PrimitiveType>` and `binary_numeric_promotion(&Ty, &Ty) -> Option<PrimitiveType>`.
- **Substitution & Supertype Projection**:
  - `Substitution::apply(&self, ty: &Ty) -> Ty` (depth-bounded, handles wildcards: if `T -> ? extends Number`, upper-projects in covariant position when reading a member return/field type).
  - `supertype_substitution(db, sub_fqn, sub_args, target_fqn) -> Option<Substitution>` — walks the inheritance graph from `sub_fqn<sub_args>` to `target_fqn`, composing substitutions at each `extends`/`implements` hop using source `TypeItem.extends`/`implements` or binary `super_class_generic`/`interfaces_generic`.
- **Subtyping & Assignability**:
  - `is_subtype(db, sub: &Ty, sup: &Ty) -> bool` — handles `Null` → any reference type, `Ty::Class` via `linearized_supertypes` + type-arg/wildcard compatibility, `Array` covariance (`String[] <: Object[]`, `T[] <: Object`, `Cloneable`, `Serializable`), `TypeVar` via upper bounds, `Intersection`, and `Primitive` widening.
  - `is_assignable(db, from: &Ty, to: &Ty) -> bool` — `is_subtype` plus primitive boxing/unboxing.
  - `least_upper_bound(db, a: &Ty, b: &Ty) -> Ty` — for ternary and switch expression branches.
- **Formatting**:
  - `Ty::display_short(&self) -> String` — e.g. `List<User>`, `Map<String, ? extends Number>`, `int[]` (used for concise hover signatures).
  - `Ty::display_qualified(&self) -> String` — e.g. `java.util.List<com.example.User>`.

### 3. Expression Type Inference (`crates/hir/src/infer.rs`)

Provide a pure, cycle-safe inference engine used by `resolve.rs` and `ide`:

- **Entry points**:
  - `infer_expr_ty(db, file_id, expr_node, expected: Option<&Ty>) -> Ty`
  - `infer_local_ty(db, file_id, local_decl_or_binding_node) -> Ty` (handles explicit types, `var`, enhanced-for element type, resource `var`, pattern variables, and untyped/`var` lambda parameters via enclosing target type).
  - `find_functional_interface_sam(db, ty: &Ty) -> Option<SamSignature>` — finds the unique abstract non-`Object` method of an interface type, with its parameter types and return type substituted using `ty`'s type arguments.
- **Expression rules**:
  1. **Literals**: `INT_NUMBER` → `int`, `LONG_NUMBER` → `long`, `FLOAT_NUMBER` → `float`, `DOUBLE_NUMBER` → `double`, `CHAR_LITERAL` → `char`, `STRING_LITERAL` / `TEXT_BLOCK` → `java.lang.String`, `TRUE_KW`/`FALSE_KW` → `boolean`, `NULL_KW` → `Ty::Null`.
  2. **Class literal** (`T.class`): `java.lang.Class<boxed(T)>`.
  3. **`this` / `super` / `I.super`**: enclosing class `Ty` with its own type parameters, or substituted supertype `Ty`.
  4. **Name references (`NAME_REF_EXPR`)**: resolve local/param/field/enum-constant/type; if local is `var` or untyped lambda param, infer its `Ty` (guarded against self-reference cycles like `var x = x;`).
  5. **Field access (`FIELD_ACCESS_EXPR`)**:
     - If receiver is `Ty::Array(_)` and field is `length` → `Primitive(Int)`.
     - Otherwise infer receiver `Ty`, look up field on receiver (or supertype), compute `supertype_substitution` from receiver to declaring owner, and substitute field's declared `Ty`.
  6. **Method call (`METHOD_CALL_EXPR` / `CALL_EXPR`)**:
     - Infer receiver `Ty` (or implicit `this` / static import owner).
     - Resolve best overload using argument types (see §4).
     - Build substitution:
       - Start with `supertype_substitution` from receiver `Ty` to declaring owner.
       - Bind explicit method type arguments if present (`obj.<String>foo(...)`).
       - Unify remaining method type parameters against actual argument `Ty`s (for lambda arguments, propagate partially-substituted SAM parameter types first, then unify lambda return type!).
       - Fall back to unifying return type against `expected: Option<&Ty>` (for target-typed calls like `Collections.emptyList()`), then type-parameter upper bounds (`java.lang.Object`).
     - Return `subst.apply(&return_ty)`.
  7. **Object & array creation (`NEW_EXPR`)**:
     - Array creation (`new T[n]`, `new T[] { ... }`) → `Ty::Array(...)`.
     - Explicit type args (`new ArrayList<User>()`) → `Ty::Class { fqn, type_args }`.
     - Diamond (`new ArrayList<>()`): infer type args from `expected` (projected to `fqn` via supertype relationship if `expected` is e.g. `List<User>`!) or from constructor arguments.
  8. **Array access (`ARRAY_ACCESS_EXPR`)**: unwrap `Ty::Array(elem)` → `*elem`.
  9. **Operators (`UNARY_EXPR`, `POSTFIX_EXPR`, `BINARY_EXPR`, `ASSIGN_EXPR`, `CAST_EXPR`, `INSTANCEOF_EXPR`, `PAREN_EXPR`)**: standard JLS typing rules (including `String` concatenation with `+`, numeric promotion, and boolean operators).
  10. **Ternary (`TERNARY_EXPR`) & Switch expression (`SWITCH_EXPR`)**: collect branch expression / `yield` types, apply numeric promotion or `least_upper_bound`, guided by `expected`.
  11. **Lambdas (`LAMBDA_EXPR`) & Method references (`METHOD_REF_EXPR`)**:
      - Derive target type from enclosing context (variable initializer, assignment, `return`, cast, or enclosing call's candidate parameter type).
      - Resolve SAM via `find_functional_interface_sam` to bind untyped/`var` lambda parameters and validate/resolve method reference target.

### 4. Type-Based Overload Resolution (`crates/hir/src/resolve.rs`)

Upgrade `select_callable_from_iter` (methods and constructors) to accept inferred argument types `arg_tys: &[Ty]`:

1. Filter candidates by name/kind and arity compatibility (fixed arity or varargs).
2. If only one candidate matches, return it immediately (fast path — zero extra inference work!).
3. When multiple candidates match:
   - Score each candidate by JLS §15.12.2 phase:
     - **Phase 1 (Strict):** every argument matches its substituted parameter type via exact match, primitive widening, or reference subtyping (no boxing, no varargs).
     - **Phase 2 (Loose):** every argument matches via `is_assignable` (allowing boxing/unboxing, no varargs).
     - **Phase 3 (Varargs):** fixed prefix matches loosely and trailing arguments match varargs component type (or array type).
     - **Phase 4 (Fallback):** arity matches, but one or more argument types are `Ty::Unknown` or incompatible (preserves resilient navigation on incomplete/broken code).
   - Pick the lowest (best) phase with at least one candidate.
   - Within that phase, prefer the **most specific** candidate (candidate `A` is more specific than `B` if every parameter of `A` is a subtype / narrower primitive of `B`'s corresponding parameter). Break remaining ties deterministically by declaration order.

### 5. `ResolvedSymbol` & `resolve_type_of_symbol_at_offset` Upgrades

- When resolving a member on a parameterized receiver (`users.get(0)`), substitute the member's return/field `type_ref` and parameter types in `ResolvedSymbol::detail` (or expose the substituted `Ty` alongside it) so hover displays the concrete instantiated signature.
- When resolving a `var` local variable or untyped lambda parameter, populate `ResolvedSymbol::detail` with `SymbolDetail::TypeRef(inferred_ty.display_short())` instead of `"var"` or `None`.
- Upgrade `resolve_type_of_symbol_at_offset` (`textDocument/typeDefinition`) to use `infer_expr_ty` / `infer_local_ty` and unwrap `Ty::Array` / `Ty::Class` to the underlying workspace or binary type target (so `typeDefinition` on `var u = users.get(0);` jumps directly to `User`!).

---

## IDE Integration (`crates/ide`)

- **`hover`**:
  - `var x = List.of(new User());` → hover on `x` renders the inferred type (`List<User> x`) instead of `var x`.
  - `users.stream().map(u -> u.getName())` → hover on `u` renders `User u`; hover on `getName` renders `String getName()`.
  - `users.get(0)` → hover on `get` renders the substituted return type (`User get(int index)`).
- **`goto_definition` & `goto_type_definition`**:
  - Automatically leverage the upgraded `hir::resolve_at_offset` and `hir::resolve_type_of_symbol_at_offset`.
  - No new LSP capabilities or protocol handlers needed in `crates/java-analyzer`.

---

## Resolved Design Decisions

1. **Hover display for generic-substituted calls:** Hover displays the substituted signature directly (e.g. `User get(int index)`).
2. **Binary generic supertype & method type-parameter metadata:** Extended `BinaryTypeSymbol` (`super_class_generic`, `interfaces_generic`, `type_param_bounds`) and `BinaryMemberSymbol` (`type_params`, `type_param_bounds`) so generic substitution works end-to-end across JDK/JAR hierarchies (e.g. `ArrayList<User>` → `List<User>` → `Iterable<User>`).
3. **Project Valhalla Value-Type readiness (HIR-only):** Added position-independent `TypeIdentity` (`Identity` | `Value` | `Unspecified`) on `TypeItem`/`TypeSymbol`/`BinaryTypeSymbol`, use-site `Nullability` (`Unspecified` | `NonNull` | `Nullable`) on `Ty::Class`/`Ty::TypeVar`/`Ty::Array`, and centralized `Ty` predicates (`is_primitive`, `is_nullable`, `is_value_type`, `boxed`) so JEP 401/402 value classes, null-restricted types (`Point!`/`Point?`), and universal generics (`List<int>`/`List<Point!>`) slot in cleanly without breaking existing Java 8–25 semantics. Preview `crates/syntax` support for `value` modifiers and `!`/`?` type suffixes is deferred until Java 28 preview support.

---

## Verification & Acceptance Criteria

1. **TDD & Verification Gate (`AGENTS.md` Rules 2 & 7):** Failing unit and integration tests written first; `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --all-targets` pass cleanly.
2. **Salsa early-cutoff:** Editing a method body does not invalidate `file_item_tree`, `workspace_index`, or `workspace_hierarchy_index`.
3. **Cycle & malformed-code safety:** Self-referential `var x = x;`, recursive bounds `class A<T extends A<T>>`, and incomplete expressions never panic or infinite-loop.
4. **Generics & inheritance substitution:**
   - `List<User> users; users.get(0).getName()` resolves `getName()` to `User.getName`.
   - `class UserList extends ArrayList<User> {}` → `new UserList().get(0).getName()` and `for (var u : new UserList()) { u.getName(); }` resolve `u` as `User`.
   - Wildcards: `List<? extends User>` resolves `.get(0)` to `User`; `List<? super User>` resolves `.get(0)` to `Object`.
5. **`var` & expression typing:**
   - `var` locals, enhanced `for (var x : arr)`, `for (var x : list)`, `try (var r = ...)`, diamond `new ArrayList<>()`, array indexing `arr[0].foo()`, ternary `cond ? a : b`, switch expressions, and string concatenation `"x" + 1` all infer expected types.
6. **Lambdas & method references:**
   - `users.stream().map(u -> u.getName()).toList()` infers `u: User`, resolves `u.getName()`, and infers the pipeline result as `List<String>`.
   - `users.forEach(User::ping)` resolves `ping` and infers functional target types.
7. **Type-based overload resolution:**
   - Same-arity overloads (`foo(int)` vs `foo(String)`, `foo(Number)` vs `foo(Integer)`, `foo(Object)` vs `foo(String)`) resolve to the most specific matching overload across primitives, boxing, subtyping, and varargs.
8. **LSP end-to-end (`crates/java-analyzer/tests/`):**
   - Hover, `textDocument/definition`, and `textDocument/typeDefinition` verified on `var`, generic chains, lambdas, and overloaded methods against both workspace and binary JDK types.
