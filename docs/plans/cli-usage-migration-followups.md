# Deferred Follow-Ups: `jdx/usage` CLI Migration

Tracked per `AGENTS.md` Rule 5.

## Deferred Scope

1. **Migrate `crates/java-analyzer` CLI from `clap` to `usage-rs`**:
   - Once `xtask` + `mise.toml` are cleanly running on `usage-rs` v7, evaluate migrating `crates/java-analyzer/src/cli.rs` (`lsp`, `inspect`, `configure`) from `clap` to `usage-rs` so the entire workspace shares a single CLI crate and `java-analyzer` can export a canonical `.usage.kdl` schema for shell completions and editor integrations.
   - Remove `clap` from root `[workspace.dependencies]` once no crate in the workspace depends on it.
