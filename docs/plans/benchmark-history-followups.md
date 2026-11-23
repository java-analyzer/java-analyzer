# Deferred Follow-Ups: Historical Benchmark Comparisons

Tracked per `AGENTS.md` Rule 5.

## Deferred Scope

1. **Multi-Run Historical Benchmark Comparisons & Regression Diffing**:
   - Deferred until Phase 1–2 features stabilize (around Spec 0016 — Scale, Responsiveness & Editor Polish), since feature additions during Specs 0008–0015 intentionally change the work performed by `java-analyzer`.
   - When implemented, any historical comparison between saved `results.json` snapshots MUST explicitly surface what changed between runs so deltas remain scientifically meaningful:
     - Exact version changes for every benchmarked server (`java-analyzer` commit/rev, `jdtls` milestone/build, `jls` version, `nbls` version)
     - JDK / JVM version and launch flag / configuration changes
     - Host hardware / OS differences
     - Newly added or changed LSP capabilities in the Capability Coverage Matrix (e.g., `completion`, `references`, `rename`, `diagnostics`)
