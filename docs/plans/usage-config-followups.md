# Plan: `usage-config`, `multicall`, and `#[usage(run)]` Follow-ups

Status: Planned

Tracked per `AGENTS.md` Rule 5. Follow-up scope for adopting `usage-rs` v7's `config` feature (`usage-config`) and `multicall` / `#[usage(run)]` patterns across `java-analyzer` and future Spec 0014 tooling.

## 1. Unified Config Schema via `usage-config` (`features = ["config"]`)

Currently, `usage-rs` is configured in the workspace `Cargo.toml` with `features = ["completions"]` (and `features = ["test"]` in `[dev-dependencies]`), while configuration files and CLI flags are wired separately:
- `.java-analyzer.toml` / `java-analyzer.toml` is parsed manually via `serde` + `toml` DTOs in `crates/java-analyzer/src/workspace_config/toml_config.rs` (`ReleaseValue`, `ClasspathCommand`, `RawDeltaSet`, etc.), separate from `ConfigureArgs` / `InspectArgs` in `crates/java-analyzer/src/cli.rs`.
- Future **Spec 0014** (`javafmt` / `javafmt.toml` formatting configuration) will also need layered resolution across CLI flags, environment variables, project config (`javafmt.toml`), user config, and defaults.

Enabling `features = ["config"]` on `usage` (`usage-config` + `#[derive(usage::Config)]`) allows a single `#[derive]` source of truth to declare:
- **Typed settings & defaults**: `#[derive(usage::Config)]` structs with `#[usage(env = "...", default = ...)]` metadata.
- **Layered resolution**: `usage::config::resolve(Settings::SETTINGS_REGISTRY, Layers::new()...)` merging CLI overrides, environment variables (`EnvLayer`), TOML files (`.java-analyzer.toml` / `javafmt.toml`), and defaults with explicit precedence and provenance.
- **Canonical KDL config schema**: `Settings::spec_kdl()` and `#[usage(settings)]` on `Cli` / `Args` so `java-analyzer.usage.kdl` (and `javafmt.usage.kdl`) advertise both CLI flags and config properties (`prop "..."`) from one definition, keeping docs, completions, and editors synchronized.

### Migration Notes
- **Pre-1.0 clean break (no legacy shims)**: Backwards compatibility for `.java-analyzer.toml` shapes is intentionally ignored until 1.0 readiness — prefer clean, idiomatic `#[derive(usage::Config)]` schemas over legacy untagged DTO shims.
- **Lean release dependencies**: Keep `features = ["config"]` disabled until the config unification work lands so the release binary does not carry unused `usage-config` machinery prematurely.

## 2. `multicall` & `#[usage(run)]` Dispatch for Spec 0014 Formatter Entrypoints

`crates/java-analyzer/src/cli.rs` and `xtask/src/cli.rs` already use `#[usage(run)]` on their subcommand enums (`Command`, `XtaskSubcommand`) with `impl usage::Run` per command struct, eliminating hand-written `match` dispatch tables.

For **Spec 0014** (`java-fmt` / `google-java-format` / `palantir-java-format` style compatibility):
- Use `usage-rs`'s **`multicall`** support (`#[usage(multicall = true)]` / `usage::multicall_applet` / `usage::multicall_basename`) so a single binary can ship BusyBox-style symlinked or hardlinked entrypoints (`java-fmt`, `google-java-format`, `palantir-java-format`, stripping `.exe` on Windows) that inject the invoked basename as the applet subcommand.
- Pair `multicall` with **`#[usage(run)]`** on the formatter subcommand enum so each personality/preset implements `usage::Run` cleanly without duplicating flag parsing or top-level dispatch code, while preserving distinct `.usage.kdl` specs and dynamic shell completions for each entrypoint.
