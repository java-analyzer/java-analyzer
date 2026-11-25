# Spec 0022: Release Process, Packaging & Publishing

Status: Accepted

## Motivation

`java-analyzer` is approaching its **`v0.1.0`** milestone (Phase 1 Core Semantic Engine complete once Spec 0009 lands), yet today the repository has:
- No `.github/workflows/` CI gate on pull requests or `main`.
- No prebuilt cross-platform binary releases, forcing every user and editor client (`editors/zed`, `editors/vscode`, `editors/nvim`, `editors/helix`) to install a Rust toolchain and compile from source (`cargo install --path crates/java-analyzer`).
- Manual version strings duplicated across `Cargo.toml`, `editors/vscode/package.json`, `editors/zed/extension.toml`, and `crates/java-analyzer/java-analyzer.usage.kdl`.
- Legacy repository URLs pointing at `luolong/java-analyzer` ahead of moving to the canonical **`java-analyzer/java-analyzer`** GitHub organization.

This spec defines a reproducible, end-to-end **CI, release engineering, binary packaging, auto-upgrading editor client distribution, and nightly corpus verification pipeline** under **`https://github.com/java-analyzer/java-analyzer`**, starting with a soft launch at **`v0.1.0`**, public registry submissions at **`v0.2.0`**, and `java-fmt` multi-binary artifacts joining in **`v0.5.0`**.

---

## Goals

1. **Canonical Organization Metadata & Single-Command Release Prep (`cargo xtask release-prep <VERSION>`):**
   - Standardize all repository URLs and metadata across `Cargo.toml`, `editors/vscode/package.json`, `editors/zed/extension.toml`, and docs on **`https://github.com/java-analyzer/java-analyzer`**.
   - Atomically synchronize version strings across root `Cargo.toml`, `editors/vscode/package.json`, `editors/zed/extension.toml`, and `crates/java-analyzer/java-analyzer.usage.kdl` (via `Cli::to_kdl()`), verified in CI via `--check`.
   - Maintain `CHANGELOG.md` with per-release milestone notes.
2. **Continuous Integration Gate (`.github/workflows/ci.yml`):**
   - Gate every push to `main` and pull request on `cargo xtask release-prep --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets`, Zed extension host tests (`editors/zed`), and VS Code extension TypeScript compilation (`editors/vscode`).
3. **Cross-Platform Native Binary Releases & Rolling Nightlies (`.github/workflows/release.yml`):**
   - Build LTO-optimized, stripped native archives on every `v*.*.*` tag (and rolling `nightly` prereleases) across all 5 primary targets:
     - `aarch64-apple-darwin`, `x86_64-apple-darwin`
     - `x86_64-unknown-linux-musl` (static, zero `glibc` coupling), `aarch64-unknown-linux-musl`
     - `x86_64-pc-windows-msvc`
   - Bundle each archive with `java-analyzer` (plus `java-fmt` and multicall drop-in links starting in `v0.5.0`), `LICENSE-MIT`, `LICENSE-APACHE`, `README.md`, and pre-generated `bash`/`zsh`/`fish` completion scripts.
   - Publish `SHA256SUMS` and GitHub Artifact Attestations (`actions/attest-build-provenance`).
4. **Zero-Friction CLI Installation (`v0.1.0` Soft Launch → `v0.2.0` Public Registries):**
   - **`mise` (`v0.1.0`):** Standard asset naming enables immediate `mise use -g github:java-analyzer/java-analyzer` with zero external registry PR.
   - **`cargo-binstall` (`v0.1.0`):** Add `[package.metadata.binstall]` to `crates/java-analyzer/Cargo.toml` pointing at `https://github.com/java-analyzer/java-analyzer/releases/download/...`.
   - **Public registries (`v0.2.0`):** Submit to Open VSX, VS Code Marketplace, Zed extension registry (`zed-industries/extensions`), Homebrew tap (`java-analyzer/homebrew-tap`), and crates.io once Completion, References, and the Spec 0019 docs site are live.
5. **Auto-Downloading & Auto-Upgrading Universal Editor Extensions (`editors/zed` & `editors/vscode`):**
   - **Zed (`editors/zed`):** Automatically download and cache the matching platform binary from `java-analyzer/java-analyzer` GitHub Releases (`zed::latest_github_release` + `zed::download_file`) when not overridden by `lsp.java-analyzer.binary.path` or `PATH`, automatically picking up new releases.
   - **VS Code / Cursor / VSCodium (`editors/vscode`):** Ship a **single lightweight universal `.vsix`** attached to every GitHub Release that automatically downloads the matching platform binary from `java-analyzer/java-analyzer` GitHub Releases into `context.globalStorageUri`, verifies its SHA-256 digest against `SHA256SUMS`, checks for newer releases on startup (`javaAnalyzer.updates.channel`: `"stable"` | `"nightly"` | `"off"`), and seamlessly upgrades & restarts the server when a new release is published.
   - **Neovim & Helix (`editors/nvim`, `editors/helix`):** Consume `java-analyzer` from `PATH` (`mise`, `cargo-binstall`, Homebrew, or release tarball), with `mason.nvim` registry submission planned for `v0.2.0`.
6. **Scheduled Nightly Real-World Corpus Verification (`.github/workflows/nightly.yml`):**
   - Runs `mise run conformance-report --strict` (OpenJDK parser conformance, `diag-conformance`, and 12-repo `workspace-corpus`) nightly, uploads `CONFORMANCE.md` + JSON artifacts, and promotes the rolling `nightly` prerelease tag only when verification passes.
7. **Passive Download & Adoption Visibility (Zero Client Telemetry in `v0.1.0`):**
   - Because `mise`, `cargo-binstall`, `editors/zed`, and the universal auto-upgrading `editors/vscode` `.vsix` all fetch platform archives from GitHub Releases, GitHub's Releases API (`download_count` per asset and release tag) provides immediate, zero-telemetry visibility into installs, upgrades, and OS/architecture breakdown from `v0.1.0` onward (with broader ecosystem stats and future opt-in feature telemetry tracked in [usage-stats-and-opt-in-telemetry.md](../plans/usage-stats-and-opt-in-telemetry.md)).

---

## Resolved Design Decisions

1. **GitHub Organization:** Repository canonical home is **`https://github.com/java-analyzer/java-analyzer`** (migrating from `luolong/java-analyzer` prior to `v0.1.0`).
2. **Linux Binary ABI:** Static **`musl`** (`x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`) so pure-Rust release binaries run on any Linux distro or container without `glibc` version coupling.
3. **VS Code Extension Delivery:** **Single universal `.vsix` with automatic GitHub Release download & auto-upgrade** (verifying `SHA256SUMS`, caching in `context.globalStorageUri`, supporting `stable`/`nightly`/`off` update channels and a `Java Analyzer: Check for Updates` command).
4. **Registry Publishing Cadence:** **Soft launch at `v0.1.0`** (GitHub Releases + `mise` `github:` backend + `cargo-binstall` + attached universal `.vsix` + Zed GitHub Release auto-download), followed by **public registry submissions at `v0.2.0`** (Open VSX, VS Code Marketplace, Zed extension registry, Homebrew tap, crates.io).

---

## Non-goals (for this spec)

- macOS Apple Developer ID notarization / Windows Authenticode EV code-signing certificates in `v0.1.0` (standard open-source tarballs/zips with `SHA256SUMS` and GitHub SLSA provenance attestations are used initially; OS code signing can be layered in pre-`v1.0.0`).
- Submitting to centralized third-party registries prior to **`v0.2.0`**.
- Linux distro-native `.deb` / `.rpm` packages in `v0.1.0` (static `musl` archives + `mise` + `cargo-binstall` cover Linux cleanly).

---

## Relationship to Other Specs

- **Spec 0009 (`0009-module-scoping-and-mrjar-source-roots.md`):** Final semantic feature gate before tagging **`v0.1.0`**.
- **Spec 0014 (`java-fmt`):** Release archive layout and `[profile.release]` are designed so `java-fmt`, `google-java-format`, and `palantir-java-format` slot into the exact same build matrix in **`v0.5.0`**.
- **Spec 0017 (`0017-automated-conformance-reporting.md`) & Spec 0020 (`0020-diagnostic-conformance-testing.md`):** Executed in `.github/workflows/nightly.yml` to gate rolling `nightly` binary promotion.
- **Spec 0019 (`0019-github-pages-site-and-documentation.md`):** Landing-page install commands (`mise use -g github:java-analyzer/java-analyzer`, `cargo binstall java-analyzer`, editor tabs) align 1-to-1 with this spec.

---

## Architecture & Pipeline Design

### 1. Cargo Release Profile & Binstall Metadata

Root `Cargo.toml`:
```toml
[workspace.package]
repository = "https://github.com/java-analyzer/java-analyzer"

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
```

`crates/java-analyzer/Cargo.toml`:
```toml
[package.metadata.binstall]
pkg-url = "{ repo }/releases/download/v{ version }/{ name }-v{ version }-{ target }.{ archive-format }"
pkg-fmt = "tgz"
bin-dir = "{ name }-v{ version }-{ target }/{ bin }{ binary-ext }"

[package.metadata.binstall.overrides.x86_64-pc-windows-msvc]
pkg-fmt = "zip"
```

---

### 2. Version Synchronization (`cargo xtask release-prep`)

Add `cargo xtask release-prep [<VERSION>] [--check]` (`usage-rs`):
- Validates `<VERSION>` is valid SemVer (`X.Y.Z` or `X.Y.Z-rc.N`).
- Synchronizes (or checks in `--check` mode without modifying files):
  1. Root `Cargo.toml` (`[workspace.package].version`)
  2. `editors/vscode/package.json` (`"version"`)
  3. `editors/zed/extension.toml` (`version`)
  4. `crates/java-analyzer/java-analyzer.usage.kdl` (regenerated so its embedded `version` matches)
- Verifies `CHANGELOG.md` contains a `## [<VERSION>]` section when preparing a release.

---

### 3. Archive Naming & Layout Contract

| Platform / Arch | Rust Target Triple | Archive Filename |
|---|---|---|
| macOS Apple Silicon | `aarch64-apple-darwin` | `java-analyzer-v<VER>-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `x86_64-apple-darwin` | `java-analyzer-v<VER>-x86_64-apple-darwin.tar.gz` |
| Linux x86_64 (static) | `x86_64-unknown-linux-musl` | `java-analyzer-v<VER>-x86_64-unknown-linux-musl.tar.gz` |
| Linux ARM64 (static) | `aarch64-unknown-linux-musl` | `java-analyzer-v<VER>-aarch64-unknown-linux-musl.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `java-analyzer-v<VER>-x86_64-pc-windows-msvc.zip` |

*(For the rolling `nightly` prerelease, `<VER>` is `nightly`, e.g. `java-analyzer-nightly-aarch64-apple-darwin.tar.gz`, accompanied by a `BUILD_INFO.json` recording the commit SHA and build timestamp so clients detect new nightly builds cleanly.)*

Each archive unpacks a single top-level directory:
```text
java-analyzer-v0.1.0-aarch64-apple-darwin/
├── java-analyzer              # (.exe on Windows)
├── README.md
├── LICENSE-APACHE
├── LICENSE-MIT
└── completions/
    ├── java-analyzer.bash
    ├── _java-analyzer
    └── java-analyzer.fish
```
Companion checksum manifest on every release: `SHA256SUMS`.

---

### 4. Auto-Upgrading Universal Editor Extensions

#### Zed (`editors/zed/src/lib.rs`)
1. Explicit user setting `lsp.java-analyzer.binary.path` wins if set.
2. `worktree.which("java-analyzer")` on `PATH` is used if present.
3. Otherwise queries `zed::latest_github_release("java-analyzer/java-analyzer", ReleaseOptions { require_assets: true, pre_release: false })`, maps `(Os, Architecture)` to the matching asset, downloads & extracts into `java-analyzer-<version>/`, marks executable, and removes older cached directories.

#### VS Code (`editors/vscode/src/extension.ts`)
Shipped as a single universal `java-analyzer-<VER>.vsix`:
1. **Binary resolution order:**
   - Explicit non-empty `javaAnalyzer.server.path` setting (when customized by the user) → run directly.
   - Managed binary cached under `context.globalStorageUri/server/<tag>/java-analyzer[.exe]`.
   - Fallback to `java-analyzer` on `PATH` if offline on first launch.
2. **Automatic download & background auto-upgrade:**
   - Setting `javaAnalyzer.updates.channel`: `"stable"` (default, queries `/releases/latest`), `"nightly"` (queries `/releases/tags/nightly` and compares `BUILD_INFO.json` / asset digest), or `"off"` (disables network update checks).
   - On first activation with no cached server, downloads the platform archive + `SHA256SUMS` with a VS Code progress notification, verifies the SHA-256 digest before unpacking, sets `0o755` on Unix, and starts the server.
   - On subsequent startups (throttled, e.g. at most once per 6 hours unless manually triggered via **`Java Analyzer: Check for Server Updates`**), checks GitHub Releases asynchronously in the background without delaying LSP startup; when a newer version is available, downloads & verifies it in the background, swaps the active binary (or prompts/notifies cleanly), and prunes previous cached versions.

---

### 5. GitHub Actions Workflows (`.github/workflows/`)

1. **`ci.yml`** (`pull_request`, `push` to `main`):
   - Runs `cargo xtask release-prep --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets`, `cd editors/zed && cargo test`, and `cd editors/vscode && npm ci && npm run compile`.
2. **`release.yml`** (`push` tags `v*.*.*`, plus reusable `workflow_call` for `nightly`):
   - Builds the 5 target binaries, runs native smoke tests (`--version`, `completion zsh`), packages archives + `SHA256SUMS`, generates SLSA provenance attestations (`actions/attest-build-provenance`), packages the universal `java-analyzer-<VER>.vsix`, and publishes the GitHub Release (with conditional `OVSX_PAT` / `VSCE_PAT` publish steps ready for `v0.2.0`).
3. **`nightly.yml`** (`schedule: cron '0 3 * * *'` + `workflow_dispatch`):
   - Executes `mise run conformance-report --strict` against OpenJDK + the 12-repo `workspace-corpus`.
   - Uploads `CONFORMANCE.md` and `target/workspace-corpus/*.json` as workflow artifacts, and promotes the rolling `nightly` prerelease via `release.yml` when green.

---

## Verification & Acceptance Criteria (When Implemented)

1. `cargo xtask release-prep --check` passes in CI and unit tests verify version synchronization across `Cargo.toml`, `package.json`, `extension.toml`, and `java-analyzer.usage.kdl`.
2. `.github/workflows/{ci,release,nightly}.yml` pass YAML validation (`yq '.' .github/workflows/*.yml > /dev/null`).
3. Local archive packaging command (`cargo xtask dist` / `mise run dist`) builds a complete host archive, smoke-tests the unpacked binary and completions, and emits valid `SHA256SUMS` (`AGENTS.md` Rule 7).
4. `editors/zed` unit tests verify asset selection & fallback order; `editors/vscode` unit tests verify platform-to-asset mapping, SHA-256 verification, and version comparison logic.
