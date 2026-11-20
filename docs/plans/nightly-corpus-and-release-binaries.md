# Plan: Release Binaries & Nightly Real-World Corpus Verification Job

Status: Planned

## Goal
Run an automated nightly CI job against a prescribed corpus of real-world public Java repositories to catch regressions in workspace structure auto-detection (`java-analyzer inspect --json`), parser health, and indexing performance.

## Prerequisite: Publish `java-analyzer` Binaries
Before the nightly job (and editor clients) can consume prebuilt binaries without compiling from source on every run:
- Add a GitHub Actions release workflow building and publishing `java-analyzer` binaries (and SHA-256 checksums) for:
  - `aarch64-apple-darwin` / `x86_64-apple-darwin`
  - `x86_64-unknown-linux-gnu` (or `musl`) / `aarch64-unknown-linux-gnu`
  - `x86_64-pc-windows-msvc`
- Publish both tagged releases and rolling `nightly` prerelease artifacts (also reusable by the Zed, VSCode, and Neovim clients).

## Nightly Real-World Corpus Job
- **Corpus manifest** (e.g. `xtask/conformance/workspace-corpus.toml`): lists prescribed public Java repositories, shallow-clone refs, and expected structural assertions:
  - `apache/commons-lang` (Maven single-module)
  - `google/gson` (Maven multi-module + JPMS)
  - `junit-team/junit-framework` (Gradle Kotlin DSL + JPMS + compact source files)
  - `apache/ant-ivy` (Ant + Ivy pair)
  - `apache/ant` (Ant + `lib/` JARs)
  - `sormuras/bach` (Zero-config pure JPMS)
  - `jbangdev/jbang-catalog` (JBang single-file scripts)
- **Scheduled GitHub Actions workflow** (`schedule: cron` + `workflow_dispatch`):
  1. Downloads the latest published `java-analyzer` binary (or builds `--release` as fallback).
  2. Shallow-clones each corpus repository.
  3. Runs `java-analyzer inspect --json --show-errors <repo>`.
  4. Validates per-repo expectations:
     - Active build tool and expected conflicts (`0`).
     - Expected modules, `main`/`test` `sources` & `resources` roots, and JPMS cross-module edges.
     - Allowed parse-error budget (e.g. `0` for `commons-lang`, `junit-framework`, `ant-ivy`, `bach`, `jbang-catalog`; explicit allowlist for intentional broken fixtures like Ant's `Bad.java`).
     - Zero unexpected `PackageMismatch` diagnostics.
     - Wall-clock timing summary to flag indexing performance regressions.
  5. Uploads the raw JSON inspection reports as workflow artifacts and opens/updates a summary on failure.
