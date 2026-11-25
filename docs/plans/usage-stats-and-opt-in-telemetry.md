# Plan: Download Analytics & Transparent Opt-In Feature Telemetry

Status: Planned

Tracked per `AGENTS.md` Rule 5. Captures the phased approach to measuring `java-analyzer` and `java-fmt` adoption, platform breakdown, and real-world LSP feature usage without compromising open-source trust or user privacy.

---

## Stage 1: Passive, Zero-Telemetry Distribution Stats (`v0.1.0` – `v0.2.0`)

Requires **zero network calls from `java-analyzer` itself** and zero user data collection:

1. **GitHub Releases Asset Counters (`v0.1.0+`, Spec 0022):**
   - Because `mise use -g github:java-analyzer/java-analyzer`, `cargo binstall java-analyzer`, the Zed extension, and the universal auto-upgrading VS Code `.vsix` all download platform archives directly from GitHub Releases, querying `/repos/java-analyzer/java-analyzer/releases` yields exact per-version and per-platform (`darwin-arm64`, `linux-musl-x64`, `windows-x64`, `.vsix`, `nightly`) download counts.
   - Optional maintainer helper (`cargo xtask release-stats` or `xh` + `jq` `mise` task) to summarize release downloads by version, channel (`stable` vs. `nightly`), and OS/arch.
2. **Public Registry Counters (`v0.2.0+`):**
   - Open VSX, VS Code Marketplace, crates.io (`java-analyzer`, `java-fmt`), and Homebrew tap / GitHub traffic APIs provide independent install and upgrade trends.
3. **Cookieless Documentation Analytics (`v0.2.0+`, post-Spec 0019 launch):**
   - Privacy-first, cookieless pageview analytics (e.g. Cloudflare Web Analytics, GoatCounter, or Plausible — no cookies, no cross-site tracking, no GDPR banner required) on the Astro/Starlight docs portal to see which editor guides and tutorials developers actually read.

---

## Stage 2: Go-Style Transparent, Strictly Opt-In Feature Telemetry (Future Spec, `v0.4.0+`)

Inspired by [Go Transparent Telemetry](https://go.dev/doc/telemetry) — designed so privacy-conscious developers trust it completely while contributors get actionable signal on feature usage, real-world latencies, and crashes.

### Core Design Invariants
1. **Strictly Opt-In (`off` by default):**
   - Never transmits anything unless explicitly enabled by the user (`java-analyzer telemetry on` or `telemetry.enabled = true` in user-level config / editor settings).
   - Zero nagging modal popups.
2. **Zero Code, Paths, Symbols, or Identifiers:**
   - Never records file paths, package/class/method names, repository names, usernames, IP addresses, or error message strings containing user identifiers.
   - Records **only** coarse, bounded schema counters and latency histograms aggregated over a weekly window:
     - **Environment bucket:** `java-analyzer` version, OS, CPU arch, editor client name (`clientInfo.name`), active Java `Release` (`8`..`27`), detected `BuildToolKind`, and coarse workspace file-count tier (`<100`, `100-1k`, `1k-5k`, `5k+`).
     - **Feature usage counters:** count of LSP requests handled per method (`hover`, `gotoDefinition`, `gotoTypeDefinition`, `gotoImplementation`, `completion`, `references`, `rename`, `formatting`, `codeAction`, `inlayHint`) and `java-fmt` style preset (`oracle`, `google`, `palantir`).
     - **Performance & stability buckets:** coarse p50/p95 latency buckets (e.g. `<5ms`, `5-25ms`, `25-100ms`, `>100ms`), cold-ready time bucket, and caught Salsa cancellation / panic / parse-error counts.
3. **100% Local-First & Inspectable Before Upload:**
   - Counters accumulate locally in a plain JSON file under the user's state/cache directory.
   - Subcommands (`java-analyzer telemetry status | preview | on | off | clear`) let any user inspect the **exact JSON payload** (`preview`) before a single byte is ever uploaded.
4. **Lightweight Collection & Public Aggregates:**
   - A tiny, stateless ingestion endpoint (e.g. Cloudflare Worker + D1/R2 or GitHub-hosted public aggregate JSON) stores weekly counter summaries and can publish anonymized aggregate charts openly on the project website (`/telemetry/`).
