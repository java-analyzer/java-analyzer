---
name: rust-2024-idioms
description: >
  Curated atomic Rust rules (current for Rust 2024 edition) covering pattern matching
  (if-let chains, let-else), memory/allocation optimization, API/trait/type design,
  collections, Serde, numeric safety, performance patterns, and tracing observability.
license: MIT
metadata:
  author: leonardomso (curated subset)
  version: "1.5.1"
---

# Rust 2024 Idioms & Micro-Rules (Curated)

Curated subset of 62 high-signal rules from `leonardomso/rust-skills`. Open only the specific rule file(s) in `rules/` relevant to the code you are writing or reviewing.

> **Project precedence:** `AGENTS.md` Rule 6 takes precedence on any conflict.

## Quick Reference

### 1. Pattern Matching & Rust 2024 (`pat-`)
- [`pat-if-let-chains`](rules/pat-if-let-chains.md) - Use `if let` chains to combine pattern bindings and conditions (Rust 2024)
- [`pat-let-else`](rules/pat-let-else.md) - Use `let ... else` for early-return pattern extraction
- [`pat-matches-macro`](rules/pat-matches-macro.md) - Use `matches!()` for boolean pattern tests
- [`pat-exhaustive-enum`](rules/pat-exhaustive-enum.md) - Match owned enums exhaustively; avoid catch-all `_` that hides new variants
- [`pat-at-bindings`](rules/pat-at-bindings.md) - Use `@` bindings to capture a value while matching it against a pattern

### 2. Memory & Allocation Optimization (`mem-`)
- [`mem-with-capacity`](rules/mem-with-capacity.md) - Use `with_capacity()` when size is known
- [`mem-box-large-variant`](rules/mem-box-large-variant.md) - Box large enum variants to reduce overall enum size
- [`mem-boxed-slice`](rules/mem-boxed-slice.md) - Use `Box<[T]>` instead of `Vec<T>` for fixed-size heap data
- [`mem-clone-from`](rules/mem-clone-from.md) - Use `clone_from()` to reuse allocations when repeatedly cloning
- [`mem-reuse-collections`](rules/mem-reuse-collections.md) - Clear and reuse collections instead of creating new ones in loops
- [`mem-avoid-format`](rules/mem-avoid-format.md) - Avoid `format!()` when string literals work
- [`mem-write-over-format`](rules/mem-write-over-format.md) - Use `write!()` into existing buffers instead of `format!()` allocations
- [`mem-compact-string`](rules/mem-compact-string.md) - Use compact string types (`SmolStr`) for memory-constrained string storage
- [`mem-smaller-integers`](rules/mem-smaller-integers.md) - Use appropriately-sized integers to reduce memory footprint
- [`mem-assert-type-size`](rules/mem-assert-type-size.md) - Use static assertions to guard against accidental type size growth
- [`mem-take-replace`](rules/mem-take-replace.md) - Use `mem::take` / `mem::replace` to move a value out of a `&mut` without cloning
- [`mem-drop-order`](rules/mem-drop-order.md) - Know and control drop order: struct fields drop top-to-bottom, locals in reverse

### 3. API Design, Traits & Type Safety (`api-`, `trait-`, `type-`, `conv-`, `closure-`)
- [`api-newtype-safety`](rules/api-newtype-safety.md) - Use newtypes to prevent mixing semantically different values
- [`api-parse-dont-validate`](rules/api-parse-dont-validate.md) - Parse into validated types at boundaries
- [`api-must-use`](rules/api-must-use.md) - Mark types and functions with `#[must_use]` when ignoring results is likely a bug
- [`api-from-not-into`](rules/api-from-not-into.md) - Implement `From<T>`, not `Into<U>`
- [`api-impl-asref`](rules/api-impl-asref.md) - Use `AsRef<T>` when you only need to borrow the inner data
- [`api-impl-into`](rules/api-impl-into.md) - Accept `impl Into<T>` for flexible APIs, implement `From<T>` for conversions
- [`api-impl-fromiterator`](rules/api-impl-fromiterator.md) - Implement `FromIterator`, `Extend`, and `IntoIterator` for collection types
- [`api-sealed-trait`](rules/api-sealed-trait.md) - Use sealed traits to prevent external implementations while allowing use
- [`api-extension-trait`](rules/api-extension-trait.md) - Use extension traits to add methods to external types
- [`trait-associated-type-vs-generic`](rules/trait-associated-type-vs-generic.md) - Associated type for a single output type; generic parameter for multiple input types
- [`trait-default-methods`](rules/trait-default-methods.md) - Define a trait in terms of a few required methods plus defaulted ones
- [`trait-dyn-vs-generic`](rules/trait-dyn-vs-generic.md) - Choose static dispatch (`impl Trait`/generics) vs dynamic dispatch (`dyn Trait`) deliberately
- [`trait-object-safety`](rules/trait-object-safety.md) - Keep a trait dyn-compatible (object-safe) when you need `dyn Trait`
- [`type-newtype-ids`](rules/type-newtype-ids.md) - Wrap IDs in newtypes
- [`type-enum-states`](rules/type-enum-states.md) - Use enums for mutually exclusive states
- [`type-no-stringly`](rules/type-no-stringly.md) - Avoid stringly-typed APIs; use enums, newtypes, or validated types
- [`type-display-vs-debug`](rules/type-display-vs-debug.md) - Use `Display` for user-facing output and `Debug` for diagnostics
- [`conv-tryfrom-fallible`](rules/conv-tryfrom-fallible.md) - Implement `TryFrom` for fallible conversions
- [`conv-fromstr-parsing`](rules/conv-fromstr-parsing.md) - Implement `FromStr` to enable `str::parse`
- [`closure-fn-trait-bounds`](rules/closure-fn-trait-bounds.md) - Require the least restrictive `Fn` trait (`FnOnce` ⊇ `FnMut` ⊇ `Fn`)
- [`closure-impl-fn-return`](rules/closure-impl-fn-return.md) - Return closures as `impl Fn`/`FnMut`/`FnOnce`, not `Box<dyn Fn>`
- [`closure-disjoint-capture`](rules/closure-disjoint-capture.md) - Lean on disjoint closure field captures

### 4. Collections, Serde & Numeric Safety (`coll-`, `serde-`, `num-`)
- [`coll-map-choice`](rules/coll-map-choice.md) - Pick map by access pattern (`FxHashMap`, `BTreeMap`, `IndexMap`)
- [`coll-seq-choice`](rules/coll-seq-choice.md) - Default to `Vec`; use `VecDeque` for queues; avoid `LinkedList`
- [`coll-set-membership`](rules/coll-set-membership.md) - Use `HashSet`/`BTreeSet` for membership tests and dedup, not `Vec::contains`
- [`serde-rename-all`](rules/serde-rename-all.md) - Match external naming conventions with `#[serde(rename_all = ...)]`
- [`serde-default-compat`](rules/serde-default-compat.md) - Use `#[serde(default)]` for optional and backward-compatible fields
- [`serde-skip-empty`](rules/serde-skip-empty.md) - Omit empty fields with `skip_serializing_if`
- [`serde-enum-representation`](rules/serde-enum-representation.md) - Choose enum tagging deliberately
- [`serde-deny-unknown-fields`](rules/serde-deny-unknown-fields.md) - Reject unexpected keys with `#[serde(deny_unknown_fields)]`
- [`serde-try-from-validate`](rules/serde-try-from-validate.md) - Validate while deserializing with `#[serde(try_from = "Raw")]`
- [`num-overflow-explicit`](rules/num-overflow-explicit.md) - Handle integer overflow explicitly (`checked_`/`saturating_`/`wrapping_`)
- [`num-cast-try-from`](rules/num-cast-try-from.md) - Avoid `as` for narrowing casts; use `From` for widening and `TryFrom` for narrowing
- [`num-nonzero`](rules/num-nonzero.md) - Use `NonZero*` types to forbid zero and unlock niche optimization

### 5. Performance & Observability (`perf-`, `obs-`)
- [`perf-iter-over-index`](rules/perf-iter-over-index.md) - Prefer iterators over manual indexing
- [`perf-iter-lazy`](rules/perf-iter-lazy.md) - Keep iterators lazy, collect only when needed
- [`perf-collect-once`](rules/perf-collect-once.md) - Don't collect intermediate iterators
- [`perf-entry-api`](rules/perf-entry-api.md) - Use entry API for map insert-or-update
- [`perf-drain-reuse`](rules/perf-drain-reuse.md) - Use `drain` to reuse allocations
- [`perf-extend-batch`](rules/perf-extend-batch.md) - Use `extend` for batch insertions
- [`perf-io-buffering`](rules/perf-io-buffering.md) - Wrap `Read`/`Write` in `BufReader`/`BufWriter` for many small operations
- [`obs-tracing-over-log`](rules/obs-tracing-over-log.md) - Use `tracing` for structured diagnostics instead of `println!` or bare `log`
- [`obs-library-facade`](rules/obs-library-facade.md) - Libraries emit through `tracing` and never install a subscriber
- [`obs-structured-fields`](rules/obs-structured-fields.md) - Record structured key-value fields rather than string interpolation
- [`obs-error-chain`](rules/obs-error-chain.md) - Log errors with their full source chain, and log each error once
