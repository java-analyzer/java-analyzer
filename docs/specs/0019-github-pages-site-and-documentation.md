# Spec 0019: Statically Generated GitHub Pages Site & Documentation

Status: Accepted

## Motivation

A developer hearing about `java-analyzer` for the first time — or clicking a link shared by a colleague — currently lands on a raw GitHub repository where installation instructions, editor setup, benchmarks, and design notes are scattered across standalone Markdown files. That creates unnecessary friction at the exact moment someone is deciding whether to try a new Java LSP:

- **No clear front door for adoption:** Newcomers need an immediate, honest overview of what `java-analyzer` is, why they would consider it over heavyweight JVM-based Java LSPs, and how to start using it in their editor.
- **No structured end-user documentation:** Once installed, users need well-organized documentation that separates hands-on tutorials, task-focused how-to guides, accurate CLI/config/LSP references, and conceptual explanations (following [Diátaxis](https://diataxis.fr/)).
- **No unified home for contributors and live reports:** Contributors and power users should be able to browse the roadmap, design specifications, parser conformance results, and interactive benchmark visualizations on the web — published automatically from canonical repository files with zero manual duplication.

This spec defines the architecture, information design, and publishing pipeline for a statically generated project website and documentation portal deployed to GitHub Pages. It is intended to be implemented once the initial round of core LSP functionality is largely complete, documenting what users can actually use at launch (with an honest pointer to the Roadmap for what comes next, and zero speculative promises).

---

## Goals

1. **Dedicated First-Glance Landing Page (`index.mdx`)**:
   - Standalone splash structure (separate from the Diátaxis docs sidebar), geared toward developers interested in or stumbling upon a Java Language Server Protocol implementation. Three things must be unmistakable at first glance:
     1. **What it is**: A fast, lightweight Java Language Server written in Rust (`rowan` lossless CST + `salsa` incremental queries), with primary CTA (**Start Using It**) and secondary CTA (**See Benchmarks** / **GitHub**).
     2. **Why you would consider it**: Native speed, low memory footprint (backed by real benchmark data and an embedded light/dark SVG chart), zero-config workspace discovery across plain folders and standard Java build tools (Maven, Gradle, Ant+Ivy, JPMS), modern Java grammar support, and an honest summary of shipped capabilities.
     3. **How you can start using it**: Copyable install command and tabbed editor setup snippets (**Zed**, **VSCode**, **Neovim**, **Helix**, **Emacs**), leading directly into the First Project Tutorial and Diátaxis documentation entry cards.
2. **Diátaxis End-User Documentation ([diataxis.fr](https://diataxis.fr/))**:
   - Strictly grounded in shipped, verifiable capabilities (never speculative future features):
     - **Tutorials** *(learning-oriented)*: Hands-on, step-by-step lessons (e.g. installing `java-analyzer` and analyzing a first zero-config Java project; working with a multi-module build-tool workspace).
     - **How-to Guides** *(task-oriented)*: Focused practical recipes — configuring editors (Zed, VSCode, Neovim, Helix, Emacs), customizing source roots/JDKs/dependencies with `.java-analyzer.toml`, and diagnosing workspace layout with `java-analyzer inspect` / `configure`.
     - **Reference** *(information-oriented)*: Authoritative technical lookup — `.java-analyzer.toml` schema, CLI commands & flags, shipped LSP capabilities & editor settings matrix, Java parser conformance (`CONFORMANCE.md`), and LSP benchmarks (`BENCHMARKS.md`, `charts/*.svg`, and hosted interactive Vega-Lite report at `/benchmarks/report/`).
     - **Explanation** *(understanding-oriented)*: Conceptual deep-dives — why a native Rust Java LSP, how zero-config workspace & binary classpath discovery work, and the 4-layer `rowan` + `salsa` architecture.
3. **Dedicated Contributors Section**:
   - Clearly separated from end-user docs: contributor onboarding & workflow (`mise`, `jj`, TDD, verification gate), running `xtask` conformance & benchmark suites, the auto-synced **Roadmap** (`docs/ROADMAP.md`), and the browsable **Specifications** library (`docs/specs/*.md`).
4. **Single Source of Truth (No Drifting Copies)**:
   - Canonical repo documents (`docs/ROADMAP.md`, `docs/specs/*.md`, `CONFORMANCE.md`, `BENCHMARKS.md`, `charts/*.svg`, and `results.json`) remain authoritative in Git and are ingested/transformed automatically at site build time into gitignored build inputs.
5. **Zero-Friction Local Dev & CI Deployment**:
   - Local `mise` tasks (`site-install`, `site-sync`, `site-dev`, `site-build`, `site-check`) for live preview, static production build, and broken-link/asset validation.
   - Canonical public URL at **`https://java-analyzer.github.io/`** (`SITE_URL=https://java-analyzer.github.io`, `SITE_BASE=/`) hosted under the `java-analyzer` GitHub organization (`java-analyzer/java-analyzer.github.io`), while keeping `SITE_URL` / `SITE_BASE` env-configurable so subpath previews and future custom domains work without structural changes.
   - GitHub Actions workflow (`.github/workflows/pages.yml`) that validates builds on PRs and publishes the built static site automatically on pushes to `main`.
6. **Simple, Maintainable Web Stack**:
   - Standardize on **Astro + Starlight** defaults (built-in Pagefind search, light/dark toggle, keyboard navigation, Shiki syntax highlighting, and `<Tabs>`/`<Card>` components) plus a single Mermaid plugin for `ROADMAP.md` and architecture diagrams.
   - Strictly isolated from the Rust Cargo workspace (`site/` directory) so `cargo test` and `cargo clippy` remain fast and unaffected.

---

## Non-goals (for this spec)

- Dynamic server-side rendering, databases, or user accounts.
- Analytics and tracking (deferred until after launch when needed to measure usage and optimize SEO; static SEO basics like `<meta>` tags, canonical URLs, and `sitemap.xml` are included out of the box via Astro + Starlight).
- Replacing `README.md`, `BENCHMARKS.md`, or `CONFORMANCE.md` in the repository root (those remain first-class for GitHub repository visitors).
- Publishing `cargo doc` rustdoc API pages (`/api/`) in v1 (deferred as a follow-up CI step).
- Versioned multi-release docs (e.g. `v0.1` vs `v0.2` switcher) until `java-analyzer` begins cutting stable semver releases.
- Writing speculative documentation for unimplemented features.

---

## Resolved Design Decisions

1. **Static Site Generator:** **Astro + Starlight** (with Starlight's out-of-the-box defaults including Pagefind search, light/dark toggle, keyboard navigation, and Shiki highlighting, plus a Mermaid plugin). Reuses `node = "lts"` already pinned in `mise.toml`.
2. **Hosting URL / Domain:** **`https://java-analyzer.github.io/`** under the `java-analyzer` GitHub organization (`java-analyzer/java-analyzer.github.io`, `SITE_BASE=/`), with `SITE_URL` and `SITE_BASE` kept environment-driven so subpath previews or a future custom domain require zero structural changes.
3. **Interactive Vega-Lite Benchmark Report:** **Included** — check in `results.json` so CI regenerates and publishes the interactive Vega-Lite `report.html` at `/benchmarks/report/` alongside `BENCHMARKS.md` and `charts/*.svg`.
4. **Rustdoc (`/api/`):** **Deferred** — out of scope for v1; can be added as a follow-up step in the same Pages workflow later.

---

## Architecture & Site Structure

### 1. Directory Layout (`site/`, isolated from Cargo workspace)

```text
site/
├── package.json                  # Astro + Starlight + Mermaid plugin
├── astro.config.mjs              # Env-driven SITE_URL/SITE_BASE, Starlight sidebar
├── scripts/
│   └── sync-docs.mjs             # Deterministic pre-build sync from canonical repo files
├── public/
│   ├── favicon.svg               # Project icon
│   ├── charts/                   # Synced (gitignored) from root charts/*.svg
│   └── benchmarks/report/        # Synced (gitignored) interactive Vega-Lite HTML report
└── src/
    ├── styles/                   # Minimal accessible overrides (e.g. wide benchmark tables)
    └── content/docs/
        ├── index.mdx             # Splash landing page (What · Why · How to start)
        ├── tutorials/            # Diátaxis: Learning-oriented
        │   ├── first-project.md
        │   └── multi-module.md
        ├── how-to/               # Diátaxis: Task-oriented
        │   ├── editors.md
        │   ├── configure-workspace.md
        │   └── troubleshoot.md
        ├── reference/            # Diátaxis: Information-oriented
        │   ├── configuration.md
        │   ├── cli.md
        │   ├── lsp-capabilities.md
        │   ├── conformance.md    # Synced (gitignored) from CONFORMANCE.md
        │   └── benchmarks.md     # Synced (gitignored) from BENCHMARKS.md
        ├── explanation/          # Diátaxis: Understanding-oriented
        │   ├── why-java-analyzer.md
        │   ├── workspace-model.md
        │   └── architecture.md
        └── contributors/         # Additional section for contributors
            ├── getting-started.md
            ├── benchmarking.md
            ├── roadmap.md        # Synced (gitignored) from docs/ROADMAP.md
            └── specs/            # Synced (gitignored) from docs/specs/*.md + generated index
```

Synced files (`src/content/docs/contributors/specs/`, `src/content/docs/contributors/roadmap.md`, `src/content/docs/reference/benchmarks.md`, `src/content/docs/reference/conformance.md`, `public/charts/`, `public/benchmarks/`) are **generated at build time and gitignored** so the repo never carries duplicate copies.

### 2. Content Sync & Link-Rewriting Contract (`site/scripts/sync-docs.mjs`)

The pre-build sync script must be deterministic, idempotent, and zero-dependency (Node ESM `node:fs`/`node:path`):

1. **Frontmatter injection**: Extract the leading `# Title` from synced Markdown files (`docs/specs/*.md`, `docs/ROADMAP.md`, `BENCHMARKS.md`, `CONFORMANCE.md`) into YAML frontmatter (`title`, `description`, and sidebar status badge for specs), stripping the duplicate H1.
2. **GitHub Alert compatibility**: Convert GitHub-flavored admonitions (`> [!NOTE]`, `> [!IMPORTANT]`, `> [!WARNING]`, `> [!TIP]`) into Starlight aside syntax (`:::note`, `:::caution`, `:::tip`).
3. **Link & asset rewriting**:
   - Rewrite `charts/<name>.svg` references in `BENCHMARKS.md` to respect `SITE_BASE`.
   - Rewrite inter-spec links (`0004-*.md`, `docs/specs/...`) to site routes and deferred `plans/*.md` links to GitHub blob URLs.
4. **Interactive Benchmark Report**:
   - Regenerate (via `cargo xtask bench-plot --out html` from checked-in `results.json`) or copy `report.html` into `site/public/benchmarks/report/index.html` and link it prominently from `reference/benchmarks.md`.

### 3. `mise.toml` Tasks

- `mise run site-install` — `npm --prefix site ci`
- `mise run site-sync` — `node site/scripts/sync-docs.mjs`
- `mise run site-dev` — run `site-sync` and start Astro dev server
- `mise run site-build` — run `site-sync` and build production static output in `site/dist/` (defaults to `SITE_URL=https://java-analyzer.github.io` and `SITE_BASE=/`)
- `mise run site-check` — build and verify `site/dist/index.html`, all expected routes, SVG assets, and zero broken internal links (checking both root `SITE_BASE=/` and subpath `SITE_BASE=/java-analyzer`)

### 4. GitHub Actions Deployment (`.github/workflows/pages.yml`)

- **Target**: Publishes `site/dist/` to **`https://java-analyzer.github.io/`** (via `java-analyzer/java-analyzer.github.io` Pages deployment, or direct `actions/deploy-pages` if hosted from the org Pages repo).
- **Triggers**: `push` to `main`, `pull_request` targeting `main` (build + link check only, no deploy), and `workflow_dispatch`.
- **Concurrency**: `group: pages`, `cancel-in-progress: false`.
- **Steps**: Checkout → setup Node LTS (matching `mise.toml` with npm cache) → `npm --prefix site ci` → generate interactive benchmark report (`cargo xtask bench-plot --out html` from checked-in `results.json`) → `SITE_URL=https://java-analyzer.github.io` `SITE_BASE=/` sync & build → link/asset check → publish to `java-analyzer.github.io` on `main`.

---

## Verification & Acceptance Criteria (When Implemented)

1. **Local build & smoke test (`AGENTS.md` Rule 7)**:
   - `mise run site-build` and `mise run site-check` succeed cleanly from a fresh checkout and produce `site/dist/index.html` plus all expected sections (`tutorials`, `how-to`, `reference`, `explanation`, `contributors`) and `/benchmarks/report/index.html` with zero broken internal links or missing SVG assets (including under a non-root `SITE_BASE`).
2. **Single source of truth & idempotency**:
   - Running `mise run site-sync` twice produces byte-identical output and `jj status` shows zero untracked generated files.
   - Updating a spec in `docs/specs/` or `BENCHMARKS.md` / `CONFORMANCE.md` automatically reflects in `site/dist/` without editing files under `site/src/content/docs/`.
3. **Workspace isolation**:
   - `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace --all-targets` continue to pass with zero regressions.
4. **Honesty & visual sanity**:
   - Landing page communicates **what it is**, **why consider it**, and **how to start using it** at first glance, documents only shipped capabilities, and renders cleanly across desktop/mobile viewports and light/dark themes.
