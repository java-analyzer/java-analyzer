# Spec 0017: Automated Deterministic Conformance Reporting

Status: Accepted

## Motivation

`java-analyzer` already collects rich, machine-readable verification data from three independent sources:

1. **Static analysis & test suite:** `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --all-targets`.
2. **Parser conformance corpus (`mise run conformance-jdk`):** `xtask` writes structured JSON to `target/conformance/report.json` with per-corpus totals, critical findings, unexpected gaps, and `known-divergences.toml` exclusions.
3. **Workspace structure & indexing corpus (`mise run workspace-corpus`):** `java-analyzer inspect --json --show-errors` writes per-repository JSON reports (`target/workspace-corpus/<id>.json`) alongside `target/workspace-corpus/timings.tsv`.

Today, assembling a unified `CONFORMANCE.md` summary (and separating expected test-fixture findings from real regressions) is manual. This spec defines a **100% deterministic, zero-AI** report generator in `xtask` (`cargo xtask report`) paired with a single `mise run conformance-report` entrypoint.

## Goals

1. **Zero AI inference:** Every number, verdict, and finding explanation in `CONFORMANCE.md` comes strictly from structured inputs and version-controlled TOML metadata.
2. **Unified single-command workflow:** Running `mise run conformance-report` executes all verification tiers and regenerates `CONFORMANCE.md` reproducibly.
3. **Signal over noise:** Workspace-corpus findings (parse errors, package mismatches, conflicts) are deterministically classified into **Unexpected** vs **Known / Expected** using `xtask/conformance/workspace-known-findings.toml`, mirroring `known-divergences.toml` from Spec 0003.
4. **Composable & CI-ready:** `cargo xtask report` operates purely on input files (without invoking network or Cargo itself), works even when a subset of inputs is omitted, and supports an optional `--strict` flag for future CI gating.

## Non-goals (for this spec)

- Adding GitHub Actions workflows or publishing release binaries (tracked in [docs/plans/nightly-corpus-and-release-binaries.md](../plans/nightly-corpus-and-release-binaries.md)).
- Modifying `crates/syntax`, `crates/hir`, `crates/ide`, or `crates/java-analyzer`.
- Changing the `target/conformance/report.json` schema (schema v1) or `java-analyzer inspect --json` output schema.

## Relationship to Other Specs

- **Spec 0003** defines `cargo xtask conformance`, `target/conformance/report.json`, and `xtask/conformance/known-divergences.toml`. This spec consumes `report.json` read-only and extends `xtask` with a sibling `report` subcommand.
- **Spec 0004** defines `java-analyzer inspect --json`. This spec consumes its JSON output read-only.

---

## Design

### 1. CLI Interface: `cargo xtask report`

```text
Usage: cargo xtask report [OPTIONS]

Assembles a unified Markdown conformance report from test output, parser conformance
JSON, and workspace-corpus JSON files.

Options:
  --conformance <PATH>       Path to conformance report.json [target/conformance/report.json]
  --workspace-corpus <DIR>   Directory containing timings.tsv and <repo>.json [target/workspace-corpus]
  --test-output <PATH>       Captured stdout from `cargo test --workspace --all-targets`
  --clippy-status <STATUS>   Clippy result: pass | fail | skipped [skipped]
  --known-findings <PATH>    TOML file of known workspace findings [embedded default]
  --revision <TEXT>          Revision string for the report header (e.g. jj change/commit)
  --date <YYYY-MM-DD>        Date string for the report header [current UTC date]
  --out <PATH>               Output Markdown path [target/conformance/CONFORMANCE.md]
  --strict                   Exit 1 if clippy/tests failed or any unexpected findings exist
  -h, --help                 Show this help
```

#### Exit Codes
- **Default (report-only, matching Spec 0003):** Exit `0` whenever the report is written successfully; exit `1` on I/O or malformed input errors; exit `2` on invalid CLI arguments.
- **`--strict` mode:** After writing the report, exit `1` if any of the following hold:
  - `--clippy-status fail`
  - Any failed tests in `--test-output`
  - Any `critical` findings or unexpected `gaps` in `report.json`
  - Any unmatched (`unexpected`) workspace-corpus `parse_error`, `package_mismatch`, or `conflict`.

#### Missing Optional Inputs
- If `--conformance` does not exist, Section 2 is rendered as *Not run (`<path>` not found)*.
- If `--workspace-corpus` does not exist (or has no `timings.tsv`), Section 3 is rendered as *Not run*.
- If `--test-output` is omitted, Section 1 shows only `--clippy-status` (or *Not run*).

---

### 2. Known Workspace Findings (`xtask/conformance/workspace-known-findings.toml`)

Embedded into `xtask` via `include_str!` (overridable with `--known-findings`), parsed with workspace `serde` + `toml`:

```toml
# Known / expected findings in the real-world workspace corpus (`mise run workspace-corpus`).
# Matched entries are classified as "Known / Expected" in CONFORMANCE.md instead of "Unexpected".

[[finding]]
repo = "gson"
kind = "parse_error"
path_prefix = "gson/src/test/java/com/google/gson/functional/Java17RecordTest.java"
reason = "Gson defaults to Java 8 and elevates testRelease to 17 via a JDK-activated Maven profile (deferred in docs/plans/0004-workspace-model-followups.md)."

[[finding]]
repo = "gson"
kind = "parse_error"
path_prefix = "test-graal-native-image/src/test/java/com/google/gson/native_test/Java17RecordReflectionTest.java"
reason = "Gson defaults to Java 8 and elevates testRelease to 17 via a JDK-activated Maven profile (deferred in docs/plans/0004-workspace-model-followups.md)."

[[finding]]
repo = "ant"
kind = "parse_error"
path_prefix = "src/tests/antunit/taskdefs/javac-dir/bad-src/Bad.java"
reason = "Intentional negative test fixture for Ant's javac task failure tests."

[[finding]]
repo = "error-prone"
kind = "package_mismatch"
path_prefix = "core/src/test/java/com/google/errorprone/refaster/testdata/"
reason = "Refaster input/output test fixtures intentionally declare parent package `com.google.errorprone.refaster.testdata`."

[[finding]]
repo = "checkstyle"
kind = "package_mismatch"
path_prefix = "src/it/resources/com/openjdk/checkstyle/test/chapternaming/rulepackagenames/"
reason = "Intentional bad-package-name test fixtures."

[[finding]]
repo = "checkstyle"
kind = "package_mismatch"
path_prefix = "src/xdocs-examples/resources/com/puppycrawl/tools/checkstyle/checks/coding/packagedeclaration/"
reason = "Intentional documentation example fixtures with non-matching package declarations."

[[finding]]
repo = "junit-framework"
kind = "package_mismatch"
path_prefix = "platform-tooling-support-tests/projects/reflection-tests/src/test/java/ReflectionTestCase.java"
reason = "Standalone reflection test fixture placed at the root of src/test/java/."

[[finding]]
repo = "jfx"
kind = "package_mismatch"
path_prefix = "modules/javafx.base/src/main/version-info/VersionInfo.java"
reason = "Generated version-info template stored outside the standard package directory hierarchy."
```

#### Matching Rules
- Paths are normalized to forward slashes (`/`).
- A finding matches an entry iff `finding.repo == entry.repo`, `finding.kind == entry.kind`, and either:
  - `finding.path == entry.path_prefix` (exact file match), or
  - `entry.path_prefix` ends with `/` and `finding.path.starts_with(entry.path_prefix)` (directory prefix match).
- Matched findings sharing the same rule are collapsed in the Markdown breakdown (showing match count + reason), keeping the report concise (e.g., 120 `error-prone` Refaster fixtures render as a single grouped bullet).
- Unmatched findings are listed individually under **Unexpected Findings** with repo, path, and diagnostic message.

---

### 3. `cargo test` Output Parser

Parses standard `cargo test` stdout without requiring nightly `-Z unstable-options --format json`:
- Recognizes suite headers:
  - `Running unittests <src> (target/.../<crate>-<hash>)` → crate name + `Unit tests (<src>)`
  - `Running tests/<name>.rs (target/.../<name>-<hash>)` → integration test name
  - `Doc-tests <crate>` → doc-tests (omitted from table when 0 tests)
- Pairs each header with its subsequent `test result: (ok|FAILED). <P> passed; <F> failed; <I> ignored; ...` summary line.
- Aggregates total passed / failed / ignored across all suites.

---

### 4. `mise.toml` Orchestration (`mise run conformance-report`)

Add a `[tasks.conformance-report]` task in `mise.toml` that:
1. Runs `cargo clippy --workspace --all-targets -- -D warnings` and records `clippy_status=pass|fail`.
2. Runs `cargo test --workspace --all-targets` teeing output to `target/conformance/test-output.txt`.
3. Runs `mise run conformance-jdk` (generating `target/conformance/report.json`).
4. Runs `mise run workspace-corpus` (generating `target/workspace-corpus/*.json` and `timings.tsv`).
5. Resolves the current revision via `jj log -r @- --no-graph -T 'change_id.shortest(8) ++ " (" ++ commit_id.shortest(8) ++ ", " ++ description.first_line() ++ ")"'` (falling back to `"unknown"` if `jj` is unavailable).
6. Invokes `cargo xtask report --conformance target/conformance/report.json --workspace-corpus target/workspace-corpus --test-output target/conformance/test-output.txt --clippy-status "$clippy_status" --revision "$rev" --out target/conformance/CONFORMANCE.md` (forwarding any extra CLI arguments such as `--out CONFORMANCE.md` or `--strict`).

---

### 5. Dependencies & Crate Layering

- Add `serde.workspace = true` and `toml.workspace = true` to `xtask/Cargo.toml` (both are already declared in root `[workspace.dependencies]`; zero new third-party dependencies).
- Keep all report logic inside a new `xtask/src/report.rs` module wired into `xtask/src/cli.rs`.
- Follow `AGENTS.md` Rule 6: no `.unwrap()` / `.expect()` in non-test code (except static embedded TOML validation if appropriate, or return a typed `Result`), deterministic `BTreeMap` ordering, and clean error reporting.

---

### 6. Testing Strategy (TDD)

Add unit and integration tests in `xtask/tests/report.rs`:
1. CLI argument parsing for `cargo xtask report` (defaults, overrides, `--strict`, unknown flags).
2. `cargo test` output parser (unit tests, integration tests, zero-test binaries, failing suites).
3. `report.json` ingestion (clean corpora, known divergences, unexpected gaps, critical findings).
4. Workspace-corpus ingestion (`timings.tsv` ordering, JSON parsing, `workspace-known-findings.toml` exact and prefix matching, grouped known findings vs individual unexpected findings).
5. Graceful handling of missing optional inputs (`--conformance`, `--workspace-corpus`, `--test-output`).
6. Verdict calculation and `--strict` exit code behavior.
7. Deterministic Markdown output snapshot test.

---

## Resolved Decisions

1. **Default output location:** `target/conformance/CONFORMANCE.md` (ignored build artifact), passing `--out CONFORMANCE.md` when updating the tracked repo snapshot.
2. **`--strict` & unexpected `package_mismatch`:** Unexpected `package_mismatch` findings fail `--strict` (exit `1`), while known fixture mismatches stay allowlisted in `workspace-known-findings.toml`.
3. **Tracking `CONFORMANCE.md`:** `CONFORMANCE.md` remains tracked at the repository root and will be refreshed by a separate nightly GitHub Actions workflow when report contents change.
