# Java Analyzer Conformance & Verification Report

**Date:** 2026-11-22  
**Workspace Revision:** `kqwywxpr (de61b2e4, feature(testing): Create conformance test suit runner)`  
**Overall Verdict:** **PASS** — 0 test failures, 0 Clippy warnings, 0 critical findings, and **0 unexpected findings** across **35,052 Java files**.

---

## 1. Static Analysis & Workspace Test Suite

Verified with `cargo clippy --workspace --all-targets -- -D warnings` (pass — 0 warnings) and `cargo test --workspace --all-targets`.

| Crate | Test Suite | Passed | Failed | Ignored |
|---|---|---:|---:|---:|
| `hir` | Unit tests (src/lib.rs) | 28 | 0 | 0 |
| `ide` | Unit tests (src/lib.rs) | 18 | 0 | 0 |
| `java-analyzer` | Unit tests (src/lib.rs) | 47 | 0 | 0 |
| `java-analyzer` | `lsp_diagnostics` | 5 | 0 | 0 |
| `java-analyzer` | `lsp_document_symbols` | 3 | 0 | 0 |
| `java-analyzer` | `lsp_goto_definition` | 5 | 0 | 0 |
| `java-analyzer` | `lsp_hover` | 4 | 0 | 0 |
| `java-analyzer` | `workspace_fixtures` | 7 | 0 | 0 |
| `syntax` | Unit tests (src/lib.rs) | 14 | 0 | 0 |
| `syntax` | `lexer_fixtures` | 1 | 0 | 0 |
| `syntax` | `parser_fixtures` | 3 | 0 | 0 |
| `syntax` | `parser_robustness` | 7 | 0 | 0 |
| `xtask` | `conformance` | 19 | 0 | 0 |
| `xtask` | `report` | 9 | 0 | 0 |
| **Total** | | **170** | **0** | **0** |

---

## 2. Parser Conformance Corpus (`mise run conformance-jdk`)

Executed across 5 corpora (**15,035 Java files**, **15,030 conforming**).

| Corpus | Kind | Files | Conforming | Rate | Critical | Notes |
|---|---|---:|---:|---:|---:|---|
| `commons-lang` | valid | 249 | 249 | 100.0% | 0 | Clean |
| `google-java-format` | valid | 48 | 48 | 100.0% | 0 | Clean |
| `guava` | valid | 630 | 630 | 100.0% | 0 | Clean |
| `jackson-core` | valid | 135 | 135 | 100.0% | 0 | Clean |
| `jdk-27-ga` | valid | 13,973 | 13,968 | 100.0%* | 0 | 5 expected exclusion(s) |
| **Total** | | **15,035** | **15,030** | **100.0%*** | **0** | **0 unexpected gaps** |

\* Excluding documented entries in `xtask/conformance/known-divergences.toml`:
- `jdk-27-ga` / `jdk.charsets/share/classes/sun/nio/cs/ext/IBM33722.java`: Generated string concatenation chain exceeds the 1024-link MAX_CHAIN robustness limit (Spec 0002).
- `jdk-27-ga` / `jdk.localedata/share/classes/sun/text/resources/ext/CollationData_ja.java`: Generated string concatenation chain exceeds the 1024-link MAX_CHAIN robustness limit (Spec 0002).
- `jdk-27-ga` / `jdk.localedata/share/classes/sun/text/resources/ext/CollationData_ko.java`: Generated string concatenation chain exceeds the 1024-link MAX_CHAIN robustness limit (Spec 0002).
- `jdk-27-ga` / `jdk.localedata/share/classes/sun/text/resources/ext/CollationData_zh.java`: Generated string concatenation chain exceeds the 1024-link MAX_CHAIN robustness limit (Spec 0002).
- `jdk-27-ga` / `jdk.localedata/share/classes/sun/text/resources/ext/CollationData_zh_TW.java`: Generated string concatenation chain exceeds the 1024-link MAX_CHAIN robustness limit (Spec 0002).

---

## 3. Workspace Structure & Indexing Corpus (`mise run workspace-corpus`)

Executed `java-analyzer inspect --json --show-errors` across 12 repositories (**20,017 Java files**, **31,846 indexed types**, **0 conflicts**, **45,274 ms** total).

| Repository | Build | Release | Modules | Files | Types | Parse Errors | Pkg Mismatches | Conflicts | Time (ms) |
|---|---|---|---:|---:|---:|---:|---:|---:|---:|
| `commons-lang` | maven | Java 8 | 1 | 645 | 1,214 | 0 | 0 | 0 | 1,894 |
| `gson` | maven | Java 8 | 7 | 268 | 736 | 2 | 0 | 0 | 763 |
| `junit-framework` | gradle | Java 27 | 37 | 1,809 | 3,904 | 0 | 1 | 0 | 3,923 |
| `ant-ivy` | ant | Java 8 | 17 | 667 | 865 | 0 | 0 | 0 | 1,925 |
| `ant` | ant | Java 8 | 12 | 1,338 | 1,820 | 1 | 0 | 0 | 3,374 |
| `tomcat` | ant | Java 25 | 8 | 3,281 | 5,801 | 0 | 0 | 0 | 8,357 |
| `bach` | none | Java 27 | 4 | 27 | 32 | 0 | 0 | 0 | 80 |
| `jbang-catalog` | none | Java 27 | 3 | 14 | 24 | 0 | 0 | 0 | 57 |
| `checkstyle` | maven | Java 21 | 1 | 3,705 | 5,728 | 0 | 5 | 0 | 6,279 |
| `jackson-core` | maven | Java 27 | 2 | 469 | 631 | 0 | 0 | 0 | 1,015 |
| `error-prone` | maven | Java 27 | 8 | 2,101 | 2,849 | 0 | 120 | 0 | 4,522 |
| `jfx` | gradle | Java 27 | 59 | 5,693 | 8,242 | 0 | 1 | 0 | 13,085 |
| **Total** | | | **159** | **20,017** | **31,846** | **3** | **127** | **0** | **45,274** |

### Unexpected Findings

None.

### Known / Expected Findings

- **gson** (`parse_error`, `gson/src/test/java/com/google/gson/functional/Java17RecordTest.java`): Gson defaults to Java 8 and elevates testRelease to 17 via a JDK-activated Maven profile (deferred in docs/plans/0004-workspace-model-followups.md).
- **gson** (`parse_error`, `test-graal-native-image/src/test/java/com/google/gson/native_test/Java17RecordReflectionTest.java`): Gson defaults to Java 8 and elevates testRelease to 17 via a JDK-activated Maven profile (deferred in docs/plans/0004-workspace-model-followups.md).
- **ant** (`parse_error`, `src/tests/antunit/taskdefs/javac-dir/bad-src/Bad.java`): Intentional negative test fixture for Ant's javac task failure tests.
- **error-prone** (`package_mismatch`, 120 matches under `core/src/test/java/com/google/errorprone/refaster/testdata/`): Refaster input/output test fixtures intentionally declare parent package `com.google.errorprone.refaster.testdata`.
- **checkstyle** (`package_mismatch`, 3 matches under `src/it/resources/com/openjdk/checkstyle/test/chapternaming/rulepackagenames/`): Intentional bad-package-name test fixtures.
- **checkstyle** (`package_mismatch`, 2 matches under `src/xdocs-examples/resources/com/puppycrawl/tools/checkstyle/checks/coding/packagedeclaration/`): Intentional documentation example fixtures with non-matching package declarations.
- **junit-framework** (`package_mismatch`, `platform-tooling-support-tests/projects/reflection-tests/src/test/java/ReflectionTestCase.java`): Standalone reflection test fixture placed at the root of src/test/java/.
- **jfx** (`package_mismatch`, `modules/javafx.base/src/main/version-info/VersionInfo.java`): Generated version-info template stored outside the standard package directory hierarchy.
