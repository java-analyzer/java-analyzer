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
