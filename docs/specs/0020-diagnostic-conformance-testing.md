# Spec 0020: Diagnostic Message & UX Conformance Testing

Status: Accepted

## Motivation

Spec 0003 verifies whether the parser accepts valid Java and rejects invalid Java, explicitly declaring diagnostic wording and UX quality out of scope. Meanwhile, `AGENTS.md` Rule 6 establishes a clear bar for diagnostics across syntax and future semantic analysis (Spec 0012):

- **Syntax and typechecker errors** must use familiar `javac`-like primary messages enriched with `rustc`/`rust-analyzer`-grade usability (precise spans, secondary `DiagnosticRelatedInformation` locations, and actionable `help:`/`note:` hints).
- **Extra IDE inspections** (nullability, unused symbols, custom lints) may use custom wording while preserving the same rich UX.

Without automated conformance testing for diagnostics:
- Parser and typechecker messages silently drift into internal implementation jargon (e.g. `expected SEMI` instead of `';' expected`).
- Spans regress onto imprecise ranges, whole lines, or zero-width EOF offsets.
- Error recovery cascades produce noisy follow-on diagnostics after a single root-cause mistake.
- Semantic diagnostics introduced in Spec 0012 can silently diverge from `javac`'s classification on real-world `openjdk-langtools` negative test cases.

This spec defines a **two-tier diagnostic conformance framework** (mirroring Spec 0003) that tests diagnostic accuracy, `javac` category parity, span precision, cascade noise, and UX quality — starting with syntax diagnostics and expanding cleanly as Spec 0012 semantic diagnostics land.

---

## Goals

1. **Test all four UX dimensions deterministically (Tier 1):** Assert diagnostic code, exact primary span, `javac`-familiar primary message, and rich `rustc`/`rust-analyzer` metadata (secondary `DiagnosticRelatedInformation` spans and `help:`/`note:` hints) via readable, hermetic snapshot fixtures in `cargo test`.
2. **Measure `javac` parity on real negative corpora (Tier 2):** Reuse `openjdk-langtools` (`@compile/fail/ref=*.out`) and optional `javac -XDrawDiagnostics` oracle runs in `xtask` (`cargo xtask diag-conformance`) to track detection, span accuracy, `compiler.err.*` code parity, and cascade-noise ratio without blocking the build.
3. **Automated UX quality invariants:** Automatically flag regressions such as leaked internal `SyntaxKind` names (`SEMI`, `L_PAREN`, `IDENT`), Rust `Debug` formatting in user-facing messages, empty non-EOF spans, or missing secondary locations on multi-site errors (e.g. duplicate symbol, incompatible override, type mismatch).
4. **Phased rollout:**
   - **Phase A (Syntax & Workspace Diagnostics):** Immediate coverage for `crates/syntax` and existing `crates/ide` diagnostics.
   - **Phase B (Semantic & Typechecker Diagnostics):** Plugs directly into Spec 0012 diagnostics using the same snapshot harness and `diag_mapping.toml` table.
5. **Deterministic reporting:** Emit `target/conformance/diagnostics.json` consumable as an optional section in `cargo xtask report` (`CONFORMANCE.md`, Spec 0017).

---

## Non-goals (for this spec)

- Byte-for-byte string equality with `javac` stdout (our UX intentionally enriches `javac` primary messages with tighter spans, secondary labels, and `help:`/`note:` hints, and may improve notoriously opaque `javac` wording where explicitly documented).
- Localization / i18n of diagnostic messages (English only).
- Implementing new semantic diagnostics themselves (owned by **Spec 0012**).
- Quick-fix / code-action execution testing (owned by future code-action specs, though `help:` text can be asserted here).

---

## Relationship to Other Specs

- **Spec 0003 (`0003-java-parser-conformance-testing.md`):** Reuses the pinned `openjdk-langtools` corpus cache (`target/conformance/corpus/`) and `known-divergences.toml` pattern, extending conformance from accept/reject to diagnostic quality and `compiler.err.*` parity.
- **Spec 0012 (Semantic Diagnostics & Type Checking, future):** Primary consumer of Tier 1 snapshot fixtures and Tier 2 semantic `compiler.err.*` parity checks.
- **Spec 0017 (`0017-automated-conformance-reporting.md`):** `cargo xtask report` gains an optional `--diag-conformance <PATH>` input to render a Diagnostic Conformance summary section in `CONFORMANCE.md`.

---

## Two-Tier Architecture

| Tier | Location | Runs in | Build gate |
|------|----------|---------|------------|
| **1. Diagnostic UX Snapshots** | `crates/ide/test_data/diagnostics/{syntax,semantic,inspections}/*.java` + `*.snap` | `cargo test` | **Yes, fails the build** |
| **2. Corpus Diagnostic Conformance** | `cargo xtask diag-conformance` | on demand / scheduled CI | **No, report only** |

### Tier 1: Rendered Caret Snapshot Fixtures (Gating)

Each test fixture is a standalone `.java` file (or multi-file fixture using `//- /path/File.java` markers) paired with a deterministic `rustc`-style rendered snapshot (`.snap`, updatable with `UPDATE_EXPECT=1`).

Rendering diagnostics in a `rustc`-like ASCII format makes **span precision, primary wording, secondary labels, and hints immediately legible in code review**:

```text
error[syntax/expected-token]: ';' expected
  --> Main.java:3:20
   |
 3 |         int x = 1 + 2
   |                      ^ expected ';' here
   |
   = help: add a semicolon at the end of the statement
```

```text
error[type/incompatible-types]: incompatible types: String cannot be converted to int
  --> Main.java:4:17
   |
 4 |         int x = getName();
   |                 ^^^^^^^^^ expected `int`, found `String`
   |
  note: `getName()` returns `String` here
  --> Main.java:2:12
   |
 2 |     String getName() { return "hi"; }
   |            ^^^^^^^
```

#### Automated Tier 1 UX Invariants (Checked on Every Fixture)
Every diagnostic produced in Tier 1 tests is checked against a small suite of structural UX assertions:
1. **No leaked internals:** Primary message and hints must not contain raw `SyntaxKind` identifiers (`SEMI`, `L_PAREN`, `R_BRACE`, `IDENT`, `EOF`, `WHITESPACE`) or Rust debug syntax (`Some(`, `Ty::`, `SmolStr`).
2. **Clean punctuation & casing:** No trailing period on single-sentence primary messages (matching `javac`/`rustc`), non-empty message, valid stable diagnostic code (`<category>/<slug>`).
3. **Span hygiene:** Primary range is valid UTF-8, within file bounds, and non-empty unless pointing at true end-of-file.
4. **Multi-site completeness:** Error codes tagged `requires_related = true` (e.g., duplicate declaration, incompatible override, ambiguous overload, type mismatch from a call) must attach at least one `DiagnosticRelatedInformation` entry.

---

### Tier 2: Corpus Diagnostic Conformance (`cargo xtask diag-conformance`)

A report-only subcommand in `xtask` (using `usage-rs` CLI conventions) that evaluates `java-analyzer` diagnostics against `openjdk-langtools` negative tests (`@compile/fail/ref=*.out`) and, optionally, a live `javac -XDrawDiagnostics` oracle.

#### 1. Code Mapping Registry (`xtask/conformance/diag_mapping.toml`)
Maps `javac` diagnostic keys (`compiler.err.*` / `compiler.warn.*`) to `java-analyzer` diagnostic codes, phase (`syntax` vs `semantic`), and expected primary-message patterns:

```toml
[[mapping]]
javac_key = "compiler.err.expected"
phase = "syntax"
ja_code = "syntax/expected-token"
message_pattern = "expected"
notes = "e.g. `';' expected`, `')' expected`"

[[mapping]]
javac_key = "compiler.err.prob.found.req"
phase = "semantic"
ja_code = "type/incompatible-types"
message_pattern = "incompatible types:"
requires_related = true
```

#### 2. Four Scored Conformance Metrics
Rather than brittle exact-text diffing against `javac`, Tier 2 computes four orthogonal metrics per corpus & phase:

| Metric | Definition | Why it matters |
|--------|------------|----------------|
| **Line / Span Match Rate** | % of expected `javac` error lines where `java-analyzer` emits a diagnostic on the exact line (and ±1 line tolerance reported separately) | Ensures users see red squiggles in the right place |
| **Category / Code Parity** | % of matched diagnostics whose `ja_code` maps to the expected `javac_key` in `diag_mapping.toml` | Ensures we diagnose the *right* error, not an accidental parse failure |
| **UX Lint Pass Rate** | % of emitted diagnostics passing all UX quality invariants (no leaked `SyntaxKind`, non-empty span, clean wording, related info where required) | Prevents regression into cryptic compiler internals |
| **Cascade Noise Ratio** | Mean ratio of `emitted_diagnostics / expected_javac_diagnostics` on single-error negative fixtures (ideal = `1.00`) | Quantifies error-recovery quality and prevents error-avalanche UX |

#### 3. JSON Output & `CONFORMANCE.md` Integration
- Emits `target/conformance/diagnostics.json` (schema v1) with per-phase totals, per-`javac_key` breakdown, top unmapped/mismatched `javac_key`s, worst cascade-noise files, and UX lint violations.
- `cargo xtask report --diag-conformance target/conformance/diagnostics.json` renders a concise summary table in `CONFORMANCE.md` (omitted gracefully when the file is absent).

---

## Phased Rollout Plan

1. **Phase A (Can land independently before Spec 0012):**
   - Introduce stable diagnostic codes + optional `related_information` / `help` fields on `ide::Diagnostic`.
   - Build the Tier 1 `rustc`-style snapshot renderer & UX invariant checker in `crates/ide`.
   - Seed Tier 1 syntax diagnostic snapshots covering common parse errors (`';' expected`, unclosed braces/strings, illegal start of expression, version-gated syntax).
   - Add `cargo xtask diag-conformance --phase syntax` against `openjdk-langtools` parser-phase `.out` fixtures.
2. **Phase B (Lands alongside Spec 0012 — Semantic Diagnostics):**
   - Add `semantic` entries to `xtask/conformance/diag_mapping.toml`.
   - Require every new Spec 0012 diagnostic to ship with Tier 1 caret snapshots (primary span + related info + `help:` hint).
   - Enable `--phase all` in `cargo xtask diag-conformance` and wire into `mise run conformance-report`.

---

## Resolved Design Decisions

1. **Tier 1 snapshot format:** `rustc`-style rendered ASCII caret snapshots (`.snap` files with `UPDATE_EXPECT=1`).
2. **Message parity vs. improved wording:** Keep familiar `javac` primary wording by default for syntax and typechecker errors, while allowing explicit, documented overrides in `xtask/conformance/diag_mapping.toml` where we deliberately improve notoriously cryptic `javac` messages (e.g. `illegal start of expression`).
3. **Timing / Phasing:** Schedule **Phase A** (syntax diagnostic UX snapshots + `cargo xtask diag-conformance --phase syntax`) ahead of **Spec 0012 (Semantic Diagnostics)** so diagnostic UX infrastructure is ready when semantic diagnostics begin; **Phase B** lands alongside Spec 0012.

---

## Verification & Acceptance Criteria (When Implemented)

1. `cargo test --workspace --all-targets` runs all Tier 1 diagnostic UX snapshots and enforces all UX invariants (zero leaked `SyntaxKind` names, valid spans, required related info).
2. `cargo xtask diag-conformance` runs cleanly against cached `openjdk-langtools`, emits `target/conformance/diagnostics.json`, exits `0` in default report-only mode, and skips gracefully when corpora are absent.
3. `cargo xtask report --diag-conformance ...` renders the diagnostic conformance summary deterministically in `CONFORMANCE.md`.
4. `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings.
