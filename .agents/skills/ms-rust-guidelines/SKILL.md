---
name: ms-rust-guidelines
description: >
  Curated Microsoft Pragmatic Rust Guidelines for performance, universal idioms,
  crate boundary interoperability, and resilience. Consult when optimizing hot paths,
  designing cross-crate APIs, structuring tests/telemetry, or auditing static state.
license: MIT
metadata:
  source: microsoft/rust-guidelines (via lx-industries/ms-rust-skill)
  compliance_date: "2026-10-09"
---

# Microsoft Pragmatic Rust Guidelines (Curated)

<!-- The Pragmatic Rust Guidelines are copyrighted (c) by Microsoft Corporation and licensed under the MIT license. -->

Curated subset of Microsoft's Pragmatic Rust Guidelines tailored for `java-analyzer`.

> **Project precedence:** `AGENTS.md` Rule 6 takes precedence on any conflict. Ignore async-specific subsections (`M-ASYNC-STACK-SIZE`, `M-YIELD-POINTS`, and the `Futures` section of `M-TYPES-SEND`), as `java-analyzer` is a synchronous/threaded LSP.

## Which Guideline to Read and When

Load **only** the guideline file(s) relevant to the current task:

### 1. [`08_performance_guidelines.md`](08_performance_guidelines.md)
Use when optimizing hot paths (lexer, parser, CST/HIR indexing, Salsa queries), reducing indirection (`M-AVOID-INDIRECTION`), choosing boxed slices/strings (`M-BOX-DST`), fast hashers (`M-FAST-HASHER`), pre-sizing/shrinking collections (`M-INITIAL-CAPACITY`, `M-SHRINK-TO-FIT`), reusing allocations (`M-MEM-REUSE`), or controlling telemetry overhead (`M-LOG-OVERHEAD`).

### 2. [`11_universal_guidelines.md`](11_universal_guidelines.md)
Use for general Rust style, naming, and maintainability: documenting magic values (`M-DOCUMENTED-MAGIC`), preferring `#[expect]` over `#[allow]` (`M-LINT-OVERRIDE-EXPECT`), structured logging (`M-LOG-STRUCTURED`), `Debug`/`Display` conventions (`M-PUBLIC-DEBUG`, `M-PUBLIC-DISPLAY`), free vs. associated functions (`M-REGULAR-FN`), and clear naming (`M-SHORT-NAMES`, `M-WEASEL-WORDS`).

### 3. [`13_libraries_interoperability_guidelines.md`](13_libraries_interoperability_guidelines.md)
Use when designing crate boundaries (`syntax` → `hir` → `ide`): avoiding leaked third-party types (`M-DONT-LEAK-TYPES`), accepting `impl AsRef<T>` (`M-IMPL-ASREF`) and `impl RangeBounds<T>` (`M-IMPL-RANGEBOUNDS`), keeping core crates sans-IO (`M-IMPL-IO`), and ensuring shared/snapshot types remain `Send` (`M-TYPES-SEND`).

### 4. [`14_libraries_resilience_guidelines.md`](14_libraries_resilience_guidelines.md)
Use when avoiding global/thread-local mutable state (`M-AVOID-STATICS`, essential for Salsa determinism), validating builders in `.build()` (`M-BUILD-RESULT`), structuring integration tests (`M-INTEGRATION-TESTS`), emitting telemetry via `tracing` instead of `println!` (`M-LOG-NOT-PRINT`), avoiding glob re-exports (`M-NO-GLOB-REEXPORTS`), enforcing newtype invariants (`M-STRONG-TYPES-GUARD`), and feature-gating test utilities (`M-TEST-UTIL`).
