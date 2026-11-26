# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-11-26

### Added

- **Java Parser & Lossless CST (`crates/syntax`, Specs 0001–0003):** Hand-written scanner and error-resilient parser built on `rowan` supporting Java 8 through Java 27 preview features, validated by OpenJDK and library conformance testing (`cargo xtask conformance`).
- **Lightweight Workspace & Multi-Release Support (`crates/hir`, `crates/java-analyzer`, Specs 0004, 0009):** Zero-config autodiscovery across Maven, Gradle, Ant, Bazel, Mill, SBT, Elide, JBang, and plain Java layouts, `.java-analyzer.toml` configuration via `java-analyzer inspect` / `java-analyzer configure`, and JEP 238 multi-release source-root discovery and overlay resolution.
- **Incremental Semantic Engine (`crates/hir`, Specs 0005–0009, 0021):** `salsa`-powered symbol indexing, name resolution, `Main`/`Test` root scoping, JPMS `module-info.java` readability and package export enforcement, JDK (`jmod`/source) and JAR binary classpath indexing, type hierarchy/inheritance, generics and expression type inference, and Phase 1 JSpecify 1.0 / third-party nullability and null-restricted type tracking.
- **LSP Server (`crates/java-analyzer`):** Language Server Protocol server over stdio providing syntax and workspace diagnostics, go to definition, go to type definition, go to implementation, hover (with on-demand Javadoc and nullability suffixes), document symbols, and type hierarchy (`prepare`, `supertypes`, `subtypes`).
- **CLI & Shell Completions (`crates/java-analyzer`, Spec 0022):** `usage-rs`-powered CLI with canonical KDL usage spec and dynamic shell completions (`bash`, `zsh`, `fish`, `elvish`, `nushell`, `powershell`).
- **Quality, Conformance & Benchmarking Tooling (`xtask`, Specs 0017, 0018, 0020 Phase A):** Diagnostic UX snapshot and `javac` parity conformance harness (`diag-conformance`), unified `CONFORMANCE.md` reporting (`report`), and reproducible multi-server LSP benchmark suite (`bench`).
- **Editor Clients & Release Pipeline (`editors/`, `.github/workflows/`, Spec 0022):** Editor integrations for Zed (`editors/zed`), VS Code (`editors/vscode`), Neovim (`editors/nvim`), and Helix (`editors/helix`), `cargo xtask release-prep` / `dist` packaging, `cargo-binstall` and `mise` metadata, and GitHub Actions CI, cross-platform release, and nightly conformance workflows.
