# Post-1.0 Changelog & Commit Grouping Transition

## Context

During the pre-`1.0.0` era (`v0.1.0` → `v0.6.0` → `v1.0.0`), `cliff.toml` operates in **"Road to 1.0" capability-pillar mode**:
- `feat(<scope>):` commits are grouped into capability-progress sections (`LSP & IDE Capabilities`, `Java Language & Semantic Support`, `Workspace, CLI & Formatter`, `Editor Integrations`, `Conformance & Performance`) so adopters can track progress toward 1.0 feature completeness.
- `Spec: <NNNN>` and `Highlight: <summary>` commit trailers surface completed roadmap specifications and milestone callouts at the top of each Draft GitHub Release, while `Changelog: ignore` suppresses same-cycle unreleased fixups.
- Pre-`v0.1.0` imperative-verb fallback rules at the bottom of `cliff.toml` cover the initial 78 commits prior to adopting Conventional Commits.

## Follow-ups

1. **After `v0.1.0` is tagged:**
   - Remove the `Pre-v0.1.0 imperative-subject fallback mappings` block at the bottom of `cliff.toml` (all commits after `v0.1.0` use Conventional Commits).
2. **At `v1.0.0` Stabilization (`Spec 0022` / Phase 6):**
   - Switch `cliff.toml` `commit_parsers` from capability-pillar sections to standard SemVer maintenance sections (`Breaking Changes`, `Added`, `Fixed`, `Performance`), while preserving `**scope**:` bold prefixes, `Highlight:` callouts, and `Changelog: ignore`.
   - Move `conformance` and `bench` scopes (`feat(conformance|bench):`) to `skip = true` so internal harness additions no longer appear in end-user SemVer release notes.
