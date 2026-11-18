# Spec 0004: Lightweight Workspace, Source Root Inference, and Pluggable Project Model

Status: Accepted

## Motivation

Existing Java language servers (Eclipse JDT.LS, IntelliJ/Fleet, NetBeans) treat a Maven
(`pom.xml`), Gradle (`build.gradle[.kts]`), or Bazel build definition as a prerequisite for
project analysis. Anything outside those three formats—a directory of `.java` files built by a
shell script, `mise`, `just`, or Apache Ant + Ivy (`build.xml` / `ivy.xml`); a single-file Java
script (`#!/usr/bin/env java`, JBang, or a JEP 512 compact source file); or a project managed by
newer build tools (`elide`, `mill`, `sbt`, Coursier)—either fails to load or drops into a degraded
"non-project file" mode without cross-file resolution.

Unlike Rust (where files do not declare their crate path and require `Cargo.toml` or
`rust-project.json` to define the module tree), **Java source and resource trees carry strong
structural signals on disk**:

- `package-info.java` and regular `.java` files declare `package a.b.c;`, which—combined with the
  file's path `<root>/a/b/c/Foo.java`—identifies `<root>` as a Java `sources` root and allows
  mismatched `package` declarations to be diagnosed immediately.
- `module-info.java` declares both the JPMS module name and its explicit `requires` and `exports`
  edges.
- `META-INF/` (and `META-INF/MANIFEST.MF` or `META-INF/services/...`) inside `<root>/META-INF` is a
  strong signal that `<root>` is a `resources` root, complementing conventional sibling folders
  (`src/main/resources`, `src/test/resources`, `resources/`).
- Files without a `package` declaration belong to the unnamed package of their directory, or are
  compact source files / single-file scripts (JEP 330, JEP 512).

This spec defines how `java-analyzer` models every compilation unit group as a **Module** (named
via `module-info.java` or addressed by its workspace-relative path when unnamed), infers `sources`
and `resources` roots (`main` and `test`) and **compile-time** inter-module dependencies
automatically, supports rich compile-time dependency coordinates (module names, relative paths,
Maven/Ivy coordinates, and GitHub/GitLab repository references), introspects recognized build
manifests (including Ant `build.xml` and Ivy `ivy.xml`), and provides a lightweight
readline/widget-based `java-analyzer configure` CLI and LSP quick-fix code actions to resolve
ambiguities without duplicating external configuration.

## Goals

1. **Zero-config baseline**: any directory containing `.java` files works immediately—with
   inferred modules, `main`/`test` Java `sources` and `resources` roots (via `package-info.java`,
   `package`, `module-info.java`, and `META-INF/` signals), inter-module `requires` dependency
   edges, and ambient JDK discovery—without requiring any build-tool manifest or config file.
2. **Unified Module model**:
   - **Named JPMS modules** (with `module-info.java`) carry their declared JPMS module name (e.g.
     `"com.example.core"`) plus their workspace-relative path.
   - **Locally inferred unnamed modules** (without `module-info.java`) are identified directly by
     their normalized workspace-relative directory path (e.g. `"."` for the workspace root, or
     `"services/core"`, `"scripts"`), avoiding synthetic name collisions.
3. **Compile-time dependencies only**: as an LSP, `java-analyzer` models **only compile-time
   dependencies** required for type resolution, navigation, and diagnostics (`main` compile-time
   and `test` compile-time scopes). Runtime-only, deployment, packaging, assembly, or container
   dependencies are out of scope.
4. **Strict 3-tier precedence**:
   - **Tier 1 (`.java-analyzer.toml` / `java-analyzer.toml`)**: instant, authoritative override
     and `{ include, exclude }` delta over detected module paths, source/resource roots, and
     compile-time module dependencies.
   - **Tier 2 (Recognised project manifests & version files)**: immediately override zero-config
     inference when present (`maven`, `gradle`, `ant` + `ivy`, `bazel`, `mill`, `sbt`, `elide`,
     `jbang`, plus `.java-version`, `.tool-versions`, `.sdkmanrc`, `mise.toml`).
   - **Tier 3 (Zero-config source inference)**: initial inference from `module-info.java`,
     `package-info.java`, `package` declarations, `META-INF/`, `#!` shebang lines, and ambient
     `JAVA_HOME`.
5. **Detect all specs, surface conflicts as LSP diagnostics, and disambiguate at the source**:
   collect all project manifests and version declarations in a workspace; surface any unresolved
   ambiguities as LSP diagnostics (with optional Quick-Fix `WorkspaceEdit` code actions); and in
   `java-analyzer configure`, offer to remove the ambiguity in the external files first (default)
   or record a minimal disambiguation entry in `.java-analyzer.toml` (fallback).
6. **Never hardcode what can be uniquely inferred**: `.java-analyzer.toml` is a minimal overlay.
   Anything uniquely declared in an external manifest/version file or uniquely inferred from
   source is omitted from `.java-analyzer.toml`.
7. **Read-only configuration detection**: project discovery and analysis treat
   `.java-analyzer.toml` and project manifests as strictly read-only. Mutations only happen via
   explicit user action (`java-analyzer configure` on the CLI, or user-invoked LSP Quick-Fix
   `WorkspaceEdit` code actions).
8. **First-class single-file and compact source Java**: support `#!` shebang lines (JEP 330),
   `import module` declarations (JEP 511), and compact source files with top-level methods/fields
   (JEP 512).

## Non-goals (for this spec)

- Modeling runtime-only, deployment, assembly, fat-jar, or packaging dependencies.
- Executing full project builds or compiling bytecode.
- Performing network IO inside `hir` or `syntax` (all IO and external coordinate resolution live
  in `crates/java-analyzer` per Spec 0001).
- Mutating workspace files as a side effect of background project discovery.

## Relationship to other specs

- **Spec 0001**: preserves the strict 4-layer architecture (`syntax` $\to$ `hir` $\to$ `ide` $\to$
  `java-analyzer`). Source-root, resource-root, package-consistency checking, and module-graph
  inference are pure functions in `hir`; filesystem scanning, manifest introspection, and the
  `configure` CLI live exclusively in `java-analyzer`.
- **Spec 0002 & Spec 0003** are amended with three syntax additions needed for lightweight Java
  programs:
  1. **Shebang lines (`#!...`, JEP 330)**: lexed as `SHEBANG` when `#!` appears at byte 0 of a
     source file, accepted at the start of `SOURCE_FILE`.
  2. **Module import declarations (`import module M;`, JEP 476/494/511)**: accepted in
     `IMPORT_DECL` and gated by the `module-imports` feature (preview in Java 23–24, stable in
     Java 25).
  3. **Compact source files and instance `main` methods (JEP 445/477/495/512)**: top-level
     fields, methods, and initializers directly inside `SOURCE_FILE`, gated by the
     `compact-source-files` feature (preview in Java 21–24, stable in Java 25).

## The Module Model

Every workspace consists of one or more **Modules** (`ModuleData` in `hir`):

```rust
pub struct ModuleData {
    pub id: ModuleId,
    /// Normalized workspace-relative directory path of this module (`"."` for the workspace root,
    /// or e.g. `"services/core"`, `"scripts"`). Always unique within a workspace and used as the
    /// canonical reference for unnamed modules.
    pub rel_path: String,
    /// Declared JPMS module name when backed by `module-info.java` (e.g. `Some("com.example.core")`),
    /// or `None` for an unnamed module.
    pub module_name: Option<String>,
    pub language_level: LanguageLevel,
    /// Java source roots (`Main` and `Test`) belonging to this module.
    pub sources: Vec<SourceRoot>,
    /// Resource roots (`Main` and `Test`) belonging to this module.
    pub resources: Vec<ResourceRoot>,
    /// Directed compile-time dependencies of this module.
    pub dependencies: Vec<ModuleDependency>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RootScope {
    Main,
    Test,
}

/// Compile-time dependency of a module. Deployment/runtime-only dependencies are not modeled.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModuleDependency {
    /// Another module in the workspace (resolved to `ModuleId`) or a named JPMS/JDK module.
    Module {
        target: ModuleTarget,
        scope: RootScope,
        /// True for `requires transitive` or Gradle `api` dependencies re-exported to dependents.
        exported: bool,
    },
    /// Workspace-relative or filesystem path to another local module directory, `.jar`, or
    /// class directory (`"./shared"`, `"lib/custom.jar"`).
    Path {
        path: String,
        scope: RootScope,
    },
    /// Maven / Ivy coordinate (`group:artifact:version[:classifier]` or `<dependency org="..." name="..." rev="..."/>`).
    Maven {
        group: String,
        artifact: String,
        version: String,
        classifier: Option<String>,
        scope: RootScope,
    },
    /// GitHub, GitLab, or Git repository reference (`github:owner/repo#ref`, `gitlab:owner/repo#ref`).
    Git {
        repo: String,
        reference: Option<String>,
        scope: RootScope,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModuleTarget {
    Workspace(ModuleId),
    External(String),
}
```

### Module identification & addressing

- **Unnamed modules (`module_name: None`)**: identified in the internal model, CLI, and
  `.java-analyzer.toml` by their normalized workspace-relative path `rel_path` (`"."` for the
  workspace root, `"services/core"`, `"tools/cli"`).
- **Named JPMS modules (`module_name: Some(name)`)**: carry both their `rel_path` and their JPMS
  `module_name` (from `module-info.java`). They can be referenced by either `module_name` (e.g.
  `requires com.example.core` in `module-info.java` or `[modules."com.example.core"]`) or by their
  `rel_path` (`"services/core"`).

### `main` and `test` scopes inside a Module

Rather than splitting a module into separate `foo` and `foo-test` modules, `main` and `test` roots
live inside the **same `ModuleData`** tagged with `RootScope::Main` or `RootScope::Test`:

- Code in `RootScope::Main` sees `Main` sources/resources and `RootScope::Main` compile-time
  dependencies.
- Code in `RootScope::Test` sees both `Main` and `Test` sources/resources of its own module plus
  both `Main` and `Test` compile-time dependencies.
- A source or resource root whose path ends with `/test`, `/tests`, `/src/test/java`, or
  `/src/test/resources` (or equals `"test"` / `"tests"`) is classified as `RootScope::Test`; all
  others default to `RootScope::Main`.

### Separate `sources` and `resources` roots (including `META-INF/` signal)

Each module tracks both Java `sources` roots and non-Java `resources` roots:

1. **Conventional sibling resource folders**: when a module has a source root at
   `<mod>/src/main/java`, `<mod>/src/test/java`, or `<mod>/src`, sibling directories
   `<mod>/src/main/resources`, `<mod>/src/test/resources`, or `<mod>/resources` are attached as
   `ResourceRoot`s if present.
2. **`META-INF/` / `META-INF/MANIFEST.MF` indicator signal**: any non-ignored directory `<dir>`
   containing `META-INF/MANIFEST.MF`, `META-INF/services/`, or a `META-INF/` subdirectory is a
   strong indicator that `<dir>` is a `ResourceRoot` and is attached to its enclosing module
   (excluding build output directories such as `target/`, `build/`, `out/`, or `dist/`).

## Three-tier configuration precedence

Every workspace and module property is resolved through three tiers, from highest to lowest
precedence:

| Tier | Source | Role |
|------|--------|------|
| **1. Explicit Override** | `.java-analyzer.toml` or `java-analyzer.toml` | **Instant override & delta.** Scalar fields (`build`, `jdk`, `release`, `preview`) win unconditionally; `{ include, exclude }` on workspace modules and per-module `sources`, `resources`, and `dependencies` explicitly add to or remove from detected values. |
| **2. Project Manifests & Version Specs** | `pom.xml`, `build.gradle[.kts]`, `build.xml` + `ivy.xml` (Ant/Ivy), `MODULE.bazel`/`BUILD.bazel`, `build.mill`/`build.sc`, `build.sbt`, `elide.pkl`/`elide.yaml`, `jbang.properties`, `.java-version`, `.tool-versions`, `.sdkmanrc`, `mise.toml` | **Authoritative external specs.** Immediately override Tier 3 zero-config inference. If multiple Tier 2 specs disagree on the same property, a conflict diagnostic is surfaced unless Tier 1 overrides it. |
| **3. Zero-Config Inference** | `module-info.java`, `package-info.java`, `.java` `package` headers, `META-INF/`, `#!` shebang `--source N`, `//DEPS`, ambient `JAVA_HOME` / `PATH` | **Initial baseline & fallback.** Used whenever Tier 1 and Tier 2 do not specify a property. |

## Tier 3: Zero-config source root, package consistency, and module inference

Given `.java` files (with their parsed `SOURCE_FILE` trees) and discovered `META-INF/` / resource
directory paths, `hir` infers modules, source/resource roots, package consistency errors, and
inter-module dependencies deterministically with zero IO:

1. **Module roots & source roots from `module-info.java`**:
   - Each `<source_root>/module-info.java` with `module <name> { ... }` creates a named JPMS
     module (`module_name = Some("<name>")`).
   - If `<source_root>` ends with `/src/main/java`, `/src/test/java`, or `/src`, the module's
     `rel_path` is the parent directory `<mod_dir>` (or `"."` if at the workspace root); otherwise
     `rel_path` is `<source_root>`.
2. **Package-inferred source roots & `package-info.java` authority**:
   - **`package-info.java` is authoritative for its directory**: if `<dir>/package-info.java`
     declares `package a.b.c;`, then:
     - `a.b.c` is the authoritative package name for `<dir>`, and stripping `a/b/c` from `<dir>`
       establishes the enclosing `SourceRoot`.
     - Any sibling `.java` file in `<dir>` whose `package` declaration is missing or differs from
       `a.b.c` is flagged with a **compile error diagnostic** (`declared package '<actual>' does
       not match package-info.java package 'a.b.c'`).
   - **When `package-info.java` is absent**:
     - Source roots are inferred by stripping declared `package a.b.c;` suffixes from `.java` file
       paths (using `module-info.java`, build manifests, or directory consensus when a single file
       is misdeclared).
     - Once a `SourceRoot` `<root>` is established, the canonical expected package for any file at
       `<root>/a/b/c/Foo.java` is `a.b.c` (derived from its relative path to `<root>`).
     - Any `.java` file under `<root>` whose declared `package` disagrees with its relative path
       from `<root>` is flagged with a **compile error diagnostic** (`declared package '<actual>'
       does not match expected package 'a.b.c'`).
3. **Unnamed-package & script directories**:
   - Files with no `package` declaration in a directory that is not inside a packaged subtree
     (plain classes in the unnamed package, JEP 512 compact source files, or `#!` scripts) infer
     their immediate parent directory as an unnamed-package `SourceRoot`.
4. **Resource root inference (`META-INF/` & sibling conventions)**:
   - Any directory containing `META-INF/` (`META-INF/MANIFEST.MF`, `META-INF/services/`, etc.) or
     matching `<mod_dir>/src/{main,test}/resources` or `<mod_dir>/resources` is attached as a
     `ResourceRoot` (`Main` or `Test`) on the enclosing module.
5. **Automatic compile-time inter-module dependency detection**:
   - From every `module-info.java`, `requires [transitive] [static] <target>;` directives are
     extracted (`transitive` maps to `exported: true`).
   - If `<target>` matches `module_name` of another module in the workspace, a directed
     `ModuleDependency::Module { target: ModuleTarget::Workspace(id), ... }` edge is added.
   - Leading `//DEPS <coord>` comments in single-file scripts are parsed into `ModuleDependency`
     entries (`Maven`, `Git`, `Path`, or `Module`).

## Tier 2: Recognised project manifests and version specifications

When scanning a workspace, `java-analyzer` collects **all** recognised build manifests, task
runners, and version declarations rather than stopping at the first match:

### Project manifests (Compile-time introspection only)

| Kind | Marker files | Extracted compile-time metadata |
|------|--------------|---------------------------------|
| `maven` | `pom.xml` | `<maven.compiler.release>`, `<maven.compiler.source>`, `<java.version>`, `<modules>`, `src/{main,test}/{java,resources}`, compile/provided/test `<dependency>` entries |
| `gradle` | `build.gradle`, `build.gradle.kts`, `settings.gradle`, `settings.gradle.kts` | `JavaLanguageVersion.of(N)`, `sourceCompatibility`, subprojects, `api`/`implementation`/`compileOnly`/`testImplementation` dependencies |
| `ant` (+ `ivy`) | `build.xml`, `ivy.xml`, `ivysettings.xml` | Ant `<property>` bindings, `<javac srcdir="..." classpath="..." classpathref="..." source="..." target="..." release="..."/>`, `<path>` / `<fileset dir="..." includes="**/*.jar"/>` compile-time JARs; paired Ivy `ivy.xml` `<dependency org="..." name="..." rev="..." conf="..."/>` mapped to `ModuleDependency::Maven` (`Main` for compile/default/provided `conf`, `Test` for test `conf`, ignoring runtime/deploy-only `conf`s). `build.xml` + `ivy.xml` are treated as a single complementary `ant` project, never a conflict. |
| `bazel` | `MODULE.bazel`, `WORKSPACE`, `WORKSPACE.bazel`, `BUILD.bazel` | `.bazelrc` `--java_language_version`, `--java_runtime_version`, `java_library` / `java_binary` `srcs` & `deps` |
| `mill` | `build.mill`, `build.mill.scala`, `build.sc` | `javacOptions` (`--release`, `-source`), `ivyDeps` / `compileIvyDeps` / `moduleDeps` |
| `sbt` | `build.sbt` | `javacOptions` (`-release`, `-source`), `libraryDependencies`, standard `src/{main,test}/{java,resources}` roots |
| `elide` | `elide.pkl`, `elide.yaml`, `elide.json` | target JVM release, source/resource roots, compile-time dependencies |
| `jbang` | `jbang.properties`, `//DEPS` in `.java` | `//JAVA <N>`, `//JAVAC_OPTIONS`, `//DEPS` compile-time coordinates |

A standalone `ivy.xml` (even without `build.xml`) is also recognized for compile-time dependency
extraction on top of inferred source roots.

### Version and toolchain specifications

| Source | Format example | Extracted value |
|--------|----------------|-----------------|
| `.java-version` | `21` or `21.0.2-tem` | `Release(21)` + vendor/version hint |
| `.sdkmanrc` | `java=21.0.2-tem` | `Release(21)` + vendor/version hint |
| `.tool-versions` (`asdf` / `mise`) | `java temurin-21.0.2+13.0.LTS` | `Release(21)` + vendor/version hint |
| `mise.toml` / `.mise.toml` | `java = "21"` under `[tools]` | `Release(21)` + vendor/version hint |
| Project manifest | `<maven.compiler.release>21</maven.compiler.release>`, `JavaLanguageVersion.of(21)`, `<javac release="21">`, `--java_language_version=21` | `Release(21)` (and `preview = true` when `--enable-preview` is configured) |

## Tier 1: Explicit override file (`.java-analyzer.toml` / `java-analyzer.toml`)

Both `.java-analyzer.toml` and `java-analyzer.toml` at the workspace root are supported:

- Project configuration detection treats the config file as a **read-only, authoritative source**.
- When writing (`java-analyzer configure` or a user-accepted LSP Quick-Fix `WorkspaceEdit`), if
  either file already exists it is updated in place; otherwise `.java-analyzer.toml` is used as the
  default filename.

```toml
# Workspace-level overrides (all optional)
build = "gradle"                              # "maven" | "gradle" | "ant" | "bazel" | "mill" | "sbt" | "elide" | "jbang" | "custom" | "none"
jdk = "/Library/Java/JavaVirtualMachines/temurin-21.jdk/Contents/Home"
release = 21
preview = false
classpath_command = ["mise", "run", "classpath"]

# Workspace-level module path adjustments (add/remove module directories from discovery)
include = ["tools/extra-module"]
exclude = ["crates/syntax/test_data", "vendor/legacy"]

# Per-module delta overrides (keyed by relative module path like "." or "services/core",
# or by JPMS module name like "com.example.app")
[modules."services/core"]
release = 25
preview = true
sources      = { include = ["build/generated/sources/ann"], exclude = ["src/main/java/deprecated"] }
resources    = { include = ["assets"],                      exclude = ["src/main/resources/bak"] }
dependencies = {
    include = [
        "./shared/utils",                     # relative path to another local module or jar
        "com.example.api",                    # JPMS module name
        "com.google.guava:guava:33.4.0-jre",  # Maven / Ivy coordinate
        "github:owner/repo#v1.2.0",           # GitHub/GitLab repository reference
    ],
    exclude = ["com.example.legacy"]
}
```

### `include` / `exclude` delta semantics

At every level (workspace module discovery, per-module `sources`, per-module `resources`, and
per-module `dependencies`), configuration is expressed as an `{ include = [...], exclude = [...] }`
delta applied to the auto-detected set:

$$
\text{Effective Set} = (\text{Detected Set} \setminus \text{exclude}) \cup \text{include}
$$

### Minimal-overlay rule (Never hardcode uniquely inferred state)

`java-analyzer configure` MUST NOT write any key to `.java-analyzer.toml` whose value is already
uniquely determined by Tier 2 or Tier 3:

- Auto-detected modules, their `sources` roots, their `resources` roots (`META-INF/` and sibling
  conventions), and their `module-info.java` / manifest / `ivy.xml` `dependencies` are **never**
  written into `.java-analyzer.toml`; only non-empty `include` / `exclude` deltas or explicit
  per-module overrides are persisted.
- If `release` (and `preview`) are uniquely determined by `.java-version`, `.tool-versions`,
  `mise.toml`, `pom.xml`, `build.gradle[.kts]`, or `build.xml` without conflict, `release` is
  omitted.
- If only one primary build tool is present (e.g., `pom.xml` or `build.xml` + `ivy.xml`), `build`
  is omitted.
- If the resulting overlay has zero keys (everything is uniquely inferred and no manual override
  or `include`/`exclude` delta was requested), `java-analyzer configure` prints the inferred
  summary and writes no file.

## Conflict detection, LSP diagnostics, and disambiguation

A conflict or ambiguity occurs when Tier 2 specs or source declarations disagree and Tier 1 does
not override the disputed property:

1. **`BuildToolConflict`**: more than one primary build-tool manifest is present at the workspace
   root (for example, both `pom.xml` and `build.gradle.kts`, or both `pom.xml` and `build.xml`; note
   that `build.xml` + `ivy.xml` is a single `ant` project and does not conflict).
2. **`ReleaseConflict`**: multiple external specs declare different major Java releases (for
   example, `.java-version` specifies `17` while `pom.xml` specifies
   `<maven.compiler.release>21</maven.compiler.release>`).
3. **`PackageMismatch`**: a `.java` file's `package` declaration disagrees with its directory's
   authoritative `package-info.java` or with its relative path from the enclosing `SourceRoot`.

### Surfacing ambiguities in the LSP

- Every unresolved ambiguity (`BuildToolConflict`, `ReleaseConflict`, `PackageMismatch`) is
  surfaced as an **LSP diagnostic** (`publishDiagnostics`) on the relevant file(s) so the user
  immediately sees the cause in the editor.
- The LSP server may attach **Quick-Fix Code Actions** (`textDocument/codeAction` returning a
  `WorkspaceEdit`) to those diagnostics:
  1. Reconcile the conflicting external spec file or `package` declaration (preferred default fix).
  2. Add the minimal disambiguating entry (`build = "..."` or `release = N`) to
     `.java-analyzer.toml`.
- Background project detection itself remains strictly read-only.

### Disambiguation order of preference in `java-analyzer configure`

When `java-analyzer configure` encounters a conflict, it presents the conflicting sources (file
path and declared value) and offers two resolution paths in priority order:

1. **Primary (Default) — Reconcile the external specifications**:
   Remove the ambiguity at the source so the workspace's own files agree and no
   `.java-analyzer.toml` is needed—for example, updating `.java-version` / `.tool-versions` to
   match the release declared in `pom.xml` / `build.gradle.kts`.
2. **Secondary (Fallback) — Disambiguate in `.java-analyzer.toml`**:
   When both conflicting files must intentionally remain as-is, write **only** the disambiguating
   key (`build = "gradle"` or `release = 21`) to `.java-analyzer.toml`.

## CLI & Interactive Readline TUI: `java-analyzer configure`

### Readline-heavy TUI design

When invoked on an interactive terminal (TTY) without `-y` / `--yes`, `java-analyzer configure`
uses a **lightweight, inline readline-heavy TUI** (preserving terminal scrollback rather than
taking over the alternate screen):

- **Single-select & multi-select choice widgets**: arrow-key / `j`/`k` / spacebar selection with
  live fuzzy filtering for choosing among discovered JDKs, resolving `BuildToolConflict` or
  `ReleaseConflict` options (with *"Reconcile external spec files"* pre-selected as the default
  recommended choice), and toggling module/source roots.
- **Readline input with Tab autocompletion**: path completion for `--jdk`, `sources`, `resources`,
  and local `.jar`/module paths, plus completion over discovered workspace module names and
  installed JDK versions.
- **Non-interactive & scriptable flags**: every interactive widget has a 1-to-1 CLI flag for
  non-interactive/CI usage:

```text
Usage:
  java-analyzer [lsp]                  Start the LSP server over stdio (default)
  java-analyzer configure [OPTIONS] [DIR]

Options:
  --print, --dry-run                   Show inferred modules, roots, specs, and conflicts without writing
  --reconcile                          Resolve version conflicts by updating external version files to match the chosen release
  -y, --yes                            Accept default non-conflicting configuration without interactive prompts
  --jdk <PATH>                         Explicit JDK home override
  --release <N>                        Explicit Java language level release override
  --preview                            Enable preview language features
  --build <KIND>                       Explicit build tool (`maven`, `gradle`, `ant`, `bazel`, `mill`, `sbt`, `elide`, `jbang`, `custom`, `none`)
  --include <DIR>                      Explicitly include an additional workspace module path (repeatable)
  --exclude <DIR>                      Explicitly exclude a workspace path from detection (repeatable)
  --module <PATH_OR_NAME>              Target module (relative path like `.` or `services/core`, or JPMS name) for module flags
  --include-source <DIR>               Add a source root to the target (or `.`) module
  --exclude-source <DIR>               Remove a source root from the target (or `.`) module
  --include-resource <DIR>             Add a resource root to the target (or `.`) module
  --exclude-resource <DIR>             Remove a resource root from the target (or `.`) module
  --include-dep <SPEC>                 Add a compile-time dependency (path, module, `group:artifact:version`, or `github:owner/repo#ref`)
  --exclude-dep <SPEC>                 Remove a compile-time dependency from the target (or `.`) module
  --classpath-cmd <CMD>                Shell/tool command that prints the compile-time classpath
  -h, --help                           Show help
```

## Single-file and compact source Java syntax (JEP 330, JEP 511, JEP 512)

To support script-style and lightweight `.java` files:

1. **Shebang lines (`SHEBANG`, JEP 330)**:
   - If a file starts with `#!` at byte offset `0`, the lexer emits a `SHEBANG` token covering
     up to (but excluding) the first line terminator.
   - `source_file` consumes an optional leading `SHEBANG` token before package/import/top-level
     declarations.
2. **Module imports (`import module M;`, JEP 476 / 494 / 511)**:
   - `import_decl` accepts contextual `module` after `IMPORT_KW` (`import module java.base;`).
   - Added to `FEATURES` as `module-imports` (`jep: Some(511)`, preview in `23, 24`, stable in
     `25`).
3. **Compact source files and instance `main` methods (JEP 445 / 477 / 495 / 512)**:
   - `top_level_item` in `grammar.rs` accepts field, method, and initializer declarations directly
     inside `SOURCE_FILE` when no enclosing class header is present.
   - Added to `FEATURES` as `compact-source-files` (`jep: Some(512)`, preview in `21, 22, 23, 24`,
     stable in `25`).
   - `ide::document_symbols` includes top-level methods and fields of compact source files in the
     document outline.

## Impacted code

- `docs/specs/0004-lightweight-project-and-workspace-model.md`: this spec.
- `crates/syntax`: `SHEBANG` token in `syntax_kind.rs` and `lexer.rs`; `import module` and
  top-level compact-source members in `grammar.rs`; `MODULE_IMPORTS` and `COMPACT_SOURCE_FILES` in
  `language_level.rs`.
- `crates/hir`: new `workspace.rs` module with `FileHeader`, `ModuleHeader`, `RootScope`,
  `SourceRoot`, `ResourceRoot`, `ModuleDependency` (enum: `Module`, `Path`, `Maven`, `Git`),
  `ModuleData`, `PackageMismatch`, `WorkspaceLayout`, `extract_file_header`, and
  `infer_workspace_layout` (enforcing `package-info.java` authority and relative-path package
  consistency, inferring `resources` from `META-INF/` and sibling conventions, and wiring
  compile-time inter-module dependencies).
- `crates/ide`: top-level member outline support in `document_symbols` and package-mismatch /
  workspace-conflict diagnostics.
- `crates/java-analyzer`: `workspace_config.rs` (read-only 3-tier resolution, multi-spec probing
  including Ant `build.xml` + Ivy `ivy.xml`, conflict detection, external spec reconciliation,
  `include`/`exclude` filtering, and minimal-overlay serialization) and `cli.rs`
  (`java-analyzer [lsp]` and the readline/widget-based `java-analyzer configure` TUI).

## Acceptance criteria

1. A directory of `.java` files with no build manifest infers its modules (referenced by relative
   path `rel_path` when unnamed, or by JPMS `module_name` when `module-info.java` is present),
   `main`/`test` `sources` roots, `resources` roots (including via `META-INF/` and
   `META-INF/MANIFEST.MF` signals), and compile-time inter-module `requires` dependency edges.
2. `package-info.java` is authoritative for its directory's package name; any `.java` file whose
   declared `package` disagrees with `package-info.java` (or, when absent, with its relative path
   from the enclosing `SourceRoot`) produces a compile error diagnostic.
3. `ModuleDependency` is a typed enum supporting `Module`, `Path`, `Maven`
   (`group:artifact:version`), and `Git` (`github:owner/repo#ref`, `gitlab:owner/repo#ref`)
   compile-time dependencies.
4. Ant (`build.xml`) and Ivy (`ivy.xml`) manifests are introspected as a complementary pair for
   `<javac srcdir="..." classpath="..." source="..." target="..." release="..."/>`, `<fileset>`
   JARs, and `<dependency org="..." name="..." rev="..." conf="..."/>` compile/test coordinates
   alongside Maven, Gradle, Bazel, Mill, sbt, Elide, and JBang.
5. Workspace-level and per-module `include` / `exclude` deltas for `sources`, `resources`, and
   `dependencies` explicitly add to or remove from the detected sets without requiring the
   detected items to be repeated.
6. Single-file scripts with `#!...`, `import module java.base;`, and top-level `void main() {}`
   parse losslessly, enforce feature gates at `< 25`, and appear in `ide::document_symbols`.
7. When `.java-analyzer.toml` or `java-analyzer.toml` is present, its fields immediately override
   all lower tiers.
8. When a single recognised project manifest (`pom.xml`, `build.gradle.kts`, `build.xml` +
   `ivy.xml`, etc.) or version file is present without conflict, it immediately overrides
   zero-config inference, and `java-analyzer configure` writes **no** redundant keys to
   `.java-analyzer.toml`.
9. When conflicting manifests or version specs exist, `WorkspaceProbe` reports the exact conflict
   (surfaced as diagnostics in the LSP); `java-analyzer configure` offers external spec
   reconciliation first and minimal `.java-analyzer.toml` disambiguation second via its readline
   choice/autocomplete TUI (or CLI flags).

## Decisions

1. **Compile-time scope only**: only compile-time (`Main` and `Test`) dependencies are modeled.
2. **Unnamed modules are addressed by workspace-relative path (`rel_path`)**: `"."` for the
   workspace root or e.g. `"services/core"`, while named modules also carry their JPMS
   `module_name`.
3. **`package-info.java` is authoritative**: package mismatches against `package-info.java` or the
   source-root relative path are compile error diagnostics.
4. **`META-INF/` is a first-class resource root signal**: any non-build-output directory containing
   `META-INF/` (`META-INF/MANIFEST.MF`, `META-INF/services/`) is inferred as a `ResourceRoot`.
5. **Ant (`build.xml`) and Ivy (`ivy.xml`) pair together**: `ivy.xml` enriches `ant` compile/test
   dependencies without triggering a build-tool conflict.
6. **`.java-analyzer.toml` is a read-only authoritative override during detection**: only explicit
   user actions (`java-analyzer configure` or LSP Quick-Fix `WorkspaceEdit` code actions) write to
   it, and only minimal non-inferred deltas are stored.
