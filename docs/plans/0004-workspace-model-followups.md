# Plan: Spec 0004 Follow-ups (Workspace & Project Model)

Status: Planned

Follow-up items from [Spec 0004](../specs/0004-lightweight-project-and-workspace-model.md) deferred from the initial implementation to be handled in later iterations.

## 1. LSP Quick-Fix Code Actions (`textDocument/codeAction`)
- Advertise `code_action_provider` in `crates/java-analyzer/src/server.rs`.
- Attach `WorkspaceEdit` Quick-Fix code actions to workspace diagnostics:
  - **`PackageMismatch`**: update the file's `package` declaration to match the authoritative `package-info.java` or enclosing `SourceRoot` relative path.
  - **`ReleaseConflict`**:
    1. *(Default / Preferred)* Reconcile conflicting external version files (`.java-version`, `.sdkmanrc`, `.tool-versions`, `pom.xml`, etc.) to a single release.
    2. *(Fallback)* Add a minimal `release = N` override to `.java-analyzer.toml`.
  - **`BuildToolConflict`**: add a minimal `build = "..."` disambiguation entry to `.java-analyzer.toml`.

## 2. Additional Tier 2 Manifest Introspectors
Currently `pom.xml`, `build.gradle[.kts]`, `build.xml`, and `ivy.xml` are introspected for release levels, source roots, and compile-time dependencies. Extend `crates/java-analyzer/src/workspace_config.rs` to extract compile-time metadata from the remaining detected build specs:
- **Bazel** (`MODULE.bazel`, `BUILD.bazel`): `java_library` / `java_binary` `srcs` and `deps` (beyond `.bazelrc` `--java_language_version`).
- **Mill** (`build.mill`, `build.sc`): `javacOptions`, `ivyDeps`, `compileIvyDeps`, `moduleDeps`.
- **sbt** (`build.sbt`): `javacOptions`, `libraryDependencies`.
- **Elide** (`elide.pkl`, `elide.yaml`, `elide.json`): target JVM release, source/resource roots, compile-time dependencies.
- **JBang** (`jbang.properties`, `//JAVA`, `//JAVAC_OPTIONS`): project-level release/options alongside per-file `//DEPS`.

## 3. Readline & Choice-Widget TUI for `java-analyzer configure`
Upgrade the interactive session in `crates/java-analyzer/src/cli.rs` from numbered line prompts to an inline, scrollback-preserving readline/widget TUI:
- Arrow-key / `j`/`k` / spacebar single-select and multi-select widgets with live fuzzy filtering (for JDK selection, conflict resolution, and module/source toggles).
- Readline input with Tab autocompletion for filesystem paths (`--jdk`, `sources`, `resources`, local `.jar`s), discovered workspace module names, and installed JDK versions.

## 4. Real-World Project Inference Gaps
Remaining deferred items discovered via `mise run workspace-corpus` across 12 public repositories:

### Ant + Ivy (`apache/ant-ivy`, `apache/ant`, `apache/tomcat`)
- **Deduplicate manifest-declared vs. inferred source roots**: When `build.xml` declares `<javac srcdir="test/custom-classpath"/>` or `<javac srcdir="java"/>`, attach inferred files to `.`'s source root rather than also creating a duplicate standalone module.

### Maven (`commons-lang`, `google/gson`)
- **External parent POM & BOM resolution**: Resolve parent POMs (e.g. `org.apache.commons:commons-parent`) and `<scope>import</scope>` BOMs from `~/.m2/repository` so inherited properties (`${commons.jmh.version}`) and managed dependency versions (`*`) expand fully *(tracked with Spec 0015)*.
- **JDK-activated `<profile>` properties**: Evaluate `<profile><activation><jdk>...</jdk></activation>` blocks in `pom.xml` (e.g. Gson's JDK 17 profile setting `<maven.compiler.testRelease>17</maven.compiler.testRelease>` for `Java17RecordTest.java`).
- **Workspace sibling resolution for Maven dependencies**: Match submodule `<dependency>` coordinates whose `groupId`/`artifactId` match another module's `pom.xml` in the same workspace into `ModuleDependency::Module { target: ModuleTarget::Workspace(id), ... }` instead of `ModuleDependency::Maven`.

### JBang (`jbangdev/jbang-catalog`)
- **Versionless BOM-managed `//DEPS` coordinates**: Parse 2-part coordinates (`group:artifact`, e.g. `io.quarkus:quarkus-picocli`) as `ModuleDependency::Maven` with version `"*"` instead of falling through to `ModuleTarget::External`.
- **Brace-aware GAV splitting**: Avoid splitting on `:` inside `${prop:default}` fallback expressions (e.g. `io.quarkus:quarkus-bom:${quarkus.version:3.5.0}@pom`).


