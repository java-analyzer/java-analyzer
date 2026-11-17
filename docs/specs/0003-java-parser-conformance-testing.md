# Spec 0003: Java Parser Conformance Testing

Status: Accepted

## Motivation

Spec 0002 defines the lexer, the CST and the grammar milestones, and its test strategy is built
on hand-written fixtures. Fixtures only prove that the parser agrees with what we *thought* the
language was. They say nothing about how much of real Java the parser accepts, or whether it
agrees with `javac` and the JLS.

There is no freely available official conformance suite (Oracle's JCK/TCK is proprietary), so
conformance has to be assessed from several open corpora and from `javac` itself as an oracle.
This spec defines how, and keeps that assessment from ever blocking the build.

It also makes the **language level** a first-class parser input (see "Language level"), which
amends decisions in spec 0002 (see "Relationship to other specs").

## Goals

1. **Measurable completeness and compliance**: report what share of valid Java the parser
   accepts and what share of invalid Java it rejects, per corpus and per language level.
2. **Two tiers with different gates**: deterministic fixtures gate the build; everything else is
   a report-only feedback loop.
3. **Language-level support**: parse according to the declared language level, from the oldest
   officially supported LTS up to the latest release, including preview features.
4. **Findings become fixtures**: every defect found by the corpus is reduced to a minimal Tier 1
   fixture before it is fixed (TDD).

## Non-goals (for this spec)

- Semantic conformance (name resolution, type checking). That belongs to `hir`.
- Matching `javac` diagnostic messages. Only accept/reject and, optionally, error position are
  compared.
- Redistributing third-party test sources.
- Incremental reparse equivalence, and error-recovery quality metrics. Both are candidates for a
  later spec.

## Relationship to other specs

- **Spec 0002** is the source of truth for tokens, syntax kinds, the CST, error recovery, the
  text-fixture format and the baseline invariants (roundtrip, no panic, non-empty `ERROR`).
  This spec reuses them and does not redefine them.
- This spec **amends spec 0002** in three places, listed here so they can be applied to 0002
  deliberately:
  1. Decision 2 ("Language level: no gating") is replaced by the rules in "Language level".
     Tree *structure* stays version-agnostic as 0002 requires; what changes is contextual
     keyword interpretation and the addition of feature-availability diagnostics.
  2. The non-goal "Preview or experimental language features" is lifted.
  3. `parse(text: &str) -> Parse` gains a language-level parameter.
- **Test strategy item 6** of spec 0002 (opt-in real-world smoke test) is generalised by
  Tier 2 below; the JDK `src.zip` run becomes the `jdk-src` source.
- **Spec 0001** phases apply unchanged: this spec is Phase 1 for conformance tooling, and
  Tier 1 fixtures are Phase 2.

## Two tiers

| Tier | Location | Runs in | Build gate |
|------|----------|---------|------------|
| **1. Fixtures** | `crates/syntax/test_data/parser/{ok,err}/*.txt`, unit tests | `cargo test` | **Yes, fails the build** |
| **2. Conformance corpus** | `xtask` (`cargo xtask conformance`) | on demand / scheduled CI | **No, report only** |

### Tier 1: fixtures (gating)

- Uses the text-fixture format, harness and `UPDATE_EXPECT=1` workflow from spec 0002
  ("Test strategy"). Nothing new is introduced for the format.
- Hermetic, deterministic and fast: no network, no JDK, no filesystem outside the repo.
- Cases under `parser/ok/` must produce zero errors; cases under `parser/err/` must produce the
  errors in their expected section.
- The invariants from spec 0002 hold for every case.
- The language level of a case is given in the separator options, next to the existing
  `--- trivia` style options: `--- release=17`, `--- release=21 preview`. With no option, the
  case runs at the latest release without preview.

### Tier 2: conformance corpus (report only)

A feedback loop that reports completeness and compliance. It **never fails the build**.

- The exit code is `0` regardless of conformance results. It is non-zero only if the tool
  itself is broken (bad arguments, unwritable output directory, internal bug).
- Missing prerequisites (no JDK, corpus not fetched, no network) cause the affected source to be
  **skipped** with a reason in the report, not an error.
- It is not part of `cargo test` and not a default workspace member (see "Repository layout").
- The numbers are expected to be low until the milestones of spec 0002 (M2 to M4) land; the
  baseline exists to show progress, not to judge it.

## Corpus sources

Each source is pinned (git tag, commit or release version) in `xtask/conformance/sources.toml`
and fetched into a cache directory (default `target/conformance/corpus/`).

| Id | Source | Kind | Expectation derivation |
|----|--------|------|------------------------|
| `openjdk-langtools` | OpenJDK `test/langtools/tools/javac` | valid + invalid | jtreg header: `@compile` is valid. `@compile/fail/ref=*.out` is invalid **only if** the `.out` file contains only parser-phase `compiler.err.*` codes (see "Parser-phase error allowlist"). Everything else is skipped. |
| `jdk-src` | JDK `src.zip` | valid | Must parse clean. |
| `libs` | Guava, commons-lang, Jackson, Spring (pinned) | valid | Must parse clean. |
| `tree-sitter-java` | `test/corpus` snippets | valid | Source extracted from corpus files; expected trees ignored. |
| `grammars-v4` | `java/java/examples` | valid | Must parse clean. |
| `javaparser` | `javaparser-core-testing` resources | valid + invalid | By directory/name convention. |
| `google-java-format` | `core/src/test/resources/.../testdata` | valid | Must parse clean (`*.input`). |

New sources are added by editing `sources.toml` and adding an adapter in `xtask`. The report
schema does not change.

Licensing: corpora are **fetched at run time** and cached under `target/`; they are never
committed. Only derived artifacts are committed (`baseline.json`, `known-divergences.toml`).
Tier 1 reductions are written by us, not copied.

### Parser-phase error allowlist

`javac` reports many semantic errors for files that are syntactically valid. A
`@compile/fail` case is classed as *syntactically invalid* only if its expected diagnostics are
exclusively from an allowlist maintained in `xtask/conformance/parser_errors.txt`, for example:

`compiler.err.expected`, `compiler.err.expected2`, `compiler.err.expected3`,
`compiler.err.illegal.start.of.expr`, `compiler.err.illegal.start.of.type`,
`compiler.err.illegal.start.of.stmt`, `compiler.err.premature.eof`, `compiler.err.not.stmt`,
`compiler.err.illegal.char`, `compiler.err.unclosed.str.lit`, `compiler.err.unclosed.comment`,
`compiler.err.orphaned`.

The list is seeded from `javac`'s parser and refined as the report reveals misclassified cases.

## Differential mode (`javac`)

Requires a JDK on `PATH` / `JAVA_HOME`; skipped otherwise.

- A small helper (`xtask/conformance/javac-oracle/`) uses `com.sun.source.util.JavacTask#parse()`
  for syntax-only parsing (no attribution) and emits per-file JSON:
  `{ accepted: bool, errors: [{line, col, code}] }`.
- The runner compares the result with our parser's. Divergence categories:
  - `we-reject-javac-accepts` (completeness gap)
  - `we-accept-javac-rejects` (compliance gap)
  - `both-reject-position-differs` (informational)
- `javac` is deliberately lenient in places (for example modifier validity is deferred to later
  phases). Where `javac` and the JLS disagree, the JLS wins, and the case is recorded in
  `xtask/conformance/known-divergences.toml` with a justification. Known gaps in spec 0002 (for
  example deferred Unicode escapes) are recorded there too, so they are not counted as
  regressions.

## Language level

### Requirement

The parser MUST support every Java language level from the supported floor to the latest
release, including preview features, selected by the *declared* language level of the file's
module. The level is an input to parsing, not an afterthought.

```rust
/// Declared language level of a compilation unit.
pub struct LanguageLevel {
    pub release: Release,   // supported floor ..= latest
    pub preview: bool,      // --enable-preview; meaningful only at the newest feature release
}
pub fn parse(text: &str, level: LanguageLevel) -> Parse;
```

- The declared level comes from the build system (Maven `maven.compiler.release` / `source`,
  Gradle toolchain / `sourceCompatibility`, `--release`, `--enable-preview`) or LSP settings,
  falling back to the latest release.
- Resolving it involves IO, so it happens in the LSP crate and reaches the `syntax` crate as a
  `salsa` input. The `syntax` crate stays IO-free (spec 0001).
- Files in one workspace may have different levels (multi-module builds), so the level is
  per-file, not global.

### Supported floor

The floor is **the oldest LTS release that is still officially supported**: still receiving
security updates from Oracle or Eclipse Adoptium (Temurin). The vendor-neutral reference is the
Adoptium support roadmap / endoflife.date.

- As of 2026-11 the floor is **Java 8** (Temurin, Corretto and Zulu support it until
  2030-12-31). Java 11 and 17 are above the floor and fully covered.
- Declared levels below the floor (for example `-source 1.7`) are **clamped to the floor**, and
  the LSP surfaces a notice (`declared language level 7 is below the minimum supported level 8;
  analysing as 8`). They are not parsed with pre-floor semantics.
- Features already standard at the floor (lambdas, default methods, type annotations, and so on)
  are always available and are **not** in the feature table. Only features introduced after the
  floor, and floor-relative contextual-keyword behaviour (for example `_` as an identifier at
  level 8), are tracked.
- **Raising the floor is a deliberate, documented change** made when the floor LTS leaves
  support. Fixtures and corpus tags specific to the dropped levels are removed in the same
  change. Lowering the floor is not planned.

### Feature table

Level-dependent behaviour is **data-driven** from one `const` table in `crates/syntax`
(`language_level.rs`), exposed through the public API so that `xtask` reads the same data and
tests and implementation cannot drift. One entry per language feature:

| Field | Meaning |
|-------|---------|
| `id` | for example `text-blocks`, `switch-expressions`, `record-patterns`, `unnamed-variables` |
| `jep` | originating JEP, where applicable |
| `preview_in` | release(s) where it was a preview feature (may repeat across releases) |
| `stable_in` | release where it became standard (absent if still preview or withdrawn) |
| `withdrawn_in` | release where it was removed, if ever |

A feature is **available** at a level if `release >= stable_in`, or if `preview` is set and
`release` is within one of its preview windows.

### Parsing strategy

To keep the IDE useful when the declared level is wrong or stale (very common in practice), the
parser is **permissive in structure but strict in meaning**:

1. **One grammar, a superset of all levels.** The parser builds the tree for any syntax it
   recognises even if the declared level predates it, so completion, navigation and
   highlighting keep working. This is the version-agnostic structure spec 0002 requires.
2. **Level changes interpretation only where the same text means different things**, that is,
   contextual keywords and ambiguities: `_` (identifier at 8, error at 9 to 20, unnamed
   variable/pattern when the feature is available), `var` (a type name below 10), `yield`
   (identifier below 14), `record` (identifier below 16), `sealed` / `permits` / `non-sealed`
   (identifiers below 17), `when` (guard in switch patterns). The lexer and parser consult the
   level for these.
3. **Unavailable features produce diagnostics, not parse failures.** A feature used at a level
   where it is unavailable, or a preview feature without `preview`, gets a syntax-phase
   diagnostic attached to the offending node, in the style of spec 0002: short, lowercase,
   actionable. For example `text blocks require language level 15` and
   `records are a preview feature in java 14; enable preview`. The tree is unaffected.
4. **Removed features** (withdrawn after preview) are diagnosed in the same way, not dropped.

This mirrors how rust-analyzer treats editions and feature gates.

### Corpus tagging

Each corpus file is tagged with an expected language level, taken in order from:

1. the jtreg `@compile ... --release N` / `-source N` / `--enable-preview` options
2. the directory convention of the source
3. the lowest release at which `javac` accepts the file (differential mode)
4. otherwise the latest release

### Level-specific conformance checks

In addition to the per-file checks below, the runner performs a **level matrix** in differential
mode (a `javac --release M` sweep, with `javac` as the oracle):

- **Gating**: a file using feature F, introduced at release N, must be accepted at levels >= N
  and carry a feature-unavailable diagnostic at levels < N.
- **Contextual keywords**: corpus files that use those words as identifiers (older code) must
  parse them as identifiers at exactly the levels where `javac` does.
- **Preview**: files marked `--enable-preview` must parse clean with `preview` on, and report a
  preview diagnostic with it off.

The report is broken down per level, and per feature where the feature table identifies one.

### Tier 1 requirements

Every row of the feature table has at least one `ok` and one `err` (feature-unavailable) fixture
case, covering the boundary levels (`stable_in - 1`, `stable_in`, and the edges of each preview
window).

## Checks performed per file (Tier 2)

| Check | Applies to | Severity in report |
|-------|-----------|--------------------|
| No panic / no hang (per-file timeout, default 5 s) | all | **Critical** |
| Lossless roundtrip | all | **Critical** |
| Valid file parses without errors | valid | completeness gap |
| Invalid file produces at least one error | invalid | compliance gap |
| Agrees with `javac` accept/reject | differential | per category above |

"Critical" means listed first in the report and flagged as a correctness bug. It still does not
fail the run. The roundtrip and no-panic invariants are the same ones spec 0002 enforces in
Tier 1; Tier 2 checks them on inputs we did not write.

Fuzzing (`cargo-fuzz` targets for lexer and parser, seeded from the corpus) is a separate,
optional command, `cargo xtask fuzz`, complementing the seeded property tests of spec 0002.
Crashes it finds are reduced to Tier 1 fixtures.

## Report

`cargo xtask conformance [--source <id>]... [--release <N>] [--no-fetch] [--update-baseline]`

Output goes to `target/conformance/`:

- `report.json`: machine-readable, schema-versioned (`"schema": 1`).
- `report.md`: human summary.
- `failures/<source>/<category>.txt`: file lists per failure category, for triage.

`report.md` contains, per source and per release level:

- totals: files seen and skipped (with reasons)
- completeness: valid files accepted / valid files
- compliance: invalid files rejected / invalid files
- critical findings (panics, timeouts, roundtrip failures), listed first
- `javac` divergence counts by category
- delta against `baseline.json` (newly passing, newly failing)

`--update-baseline` rewrites `baseline.json`. It is a deliberate, reviewed action and is never
automatic.

## Repository layout

```
xtask/                        # outside crates/, so not matched by members = ["crates/*"]
  conformance/
    sources.toml
    parser_errors.txt
    known-divergences.toml
    baseline.json
    javac-oracle/             # Java helper for differential mode
```

`Cargo.toml` uses `members = ["crates/*"]`. `xtask` is added as an explicit member and left out
of `default-members`, so `cargo test` and `cargo build` at the root neither build nor run it.

`xtask` performs IO (network, filesystem, process spawning). That is allowed because it is
development tooling, not one of the four product layers of the architecture rule. It may depend
on `syntax` through its public API only.

CI is optional: a scheduled or manual workflow runs `cargo xtask conformance` with
`continue-on-error: true` and uploads `target/conformance/` as an artifact. It never blocks
merges.

## Impacted code

- `crates/syntax`: `LanguageLevel`, `Release` and the feature table (`language_level.rs`);
  `parse` gains a level parameter; the lexer/parser consult the level for contextual keywords;
  feature-availability diagnostics in `Parse.errors`; fixture harness learns the `release=` and
  `preview` options.
- `crates/hir`: the declared level becomes a per-file `salsa` input; the `parse` query takes it.
- `crates/java-analyzer`: resolves the declared level from the build system / settings, and
  shows the below-floor notice.
- `xtask/`: new, as described above.
- `docs/specs/0002-java-syntax-subset.md`: amended as listed in "Relationship to other specs".

## Acceptance criteria for the first `xtask` implementation

1. `cargo test` at the workspace root neither fetches anything nor needs a JDK.
2. `cargo xtask conformance` exits `0` with no JDK and no network, reporting every source as
   skipped.
3. Given a local directory of `.java` files, the runner writes `report.json` and `report.md`,
   with panics and roundtrip failures caught and reported without aborting the run.
4. The jtreg header parser and the allowlist classification have unit tests inside `xtask`.
5. Entries in `known-divergences.toml` are excluded from the "unexpected" divergence counts.

## Decisions

1. **Report-only Tier 2**: nothing beyond Tier 1 fixtures may fail the build.
2. **Language level is a parser input from the start**, per-file, resolved outside `syntax`.
3. **Floor = oldest officially supported LTS** (Java 8 as of 2026-11), clamped below, raised
   deliberately.
4. **Structure is version-agnostic, interpretation and diagnostics are level-aware.**
5. **Corpora are fetched, never committed.**
6. **`javac` is the practical oracle, the JLS the tie-breaker.**
7. **Fixture format is spec 0002's**; level is expressed through separator options.

## Open questions

- Pin policy: track JDK LTS tags only, or also the latest feature release?
- Preview windows as new JDKs ship: proposal is a release-process checklist item to update the
  feature table and re-baseline.
- Is one `report.md` per run enough, or do we want history (for example a committed trend file)?
- Should the language-level rules move into their own spec? They are a parser feature rather
  than a testing concern, and are kept here only because conformance is what motivates them.

## Immediate next steps

1. Review and accept this spec; apply the amendments to spec 0002.
2. Decide the open questions above.
3. Then, test-first: `LanguageLevel` and the feature table in `syntax`, Tier 1 gating fixtures,
   and finally the `xtask` runner skeleton.
