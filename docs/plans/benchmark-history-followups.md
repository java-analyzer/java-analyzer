# Deferred Follow-Ups: Benchmark Suite & Historical Comparisons

Tracked per `AGENTS.md` Rule 5.

## Deferred Scope

1. **Multi-Run Historical Benchmark Comparisons & Regression Diffing**:
   - Deferred until Phase 1–2 features stabilize (around Spec 0016 — Scale, Responsiveness & Editor Polish), since feature additions during Specs 0008–0015 intentionally change the work performed by `java-analyzer`.
   - When implemented, any historical comparison between saved `results.json` snapshots MUST explicitly surface what changed between runs so deltas remain scientifically meaningful:
     - Exact version changes for every benchmarked server (`java-analyzer` commit/rev, `jdtls` milestone/build, `jls` version, `nbls` version)
     - JDK / JVM version and launch flag / configuration changes
     - Host hardware / OS differences
     - Newly added or changed LSP capabilities in the Capability Coverage Matrix (e.g., `completion`, `references`, `rename`, `diagnostics`)

2. **OS Disk / Page-Cache Clearing Before Cold Starts (`--clear-disk-cache`)**:
   - Before each Phase 1 cold start, flush the OS filesystem page cache so every server reads source files, JDK modules, and JARs from a genuinely cold disk state rather than inheriting warm OS buffers from the preceding server:
     - **macOS:** `sync` + `purge` (via user-authorized elevation; fail loudly with an actionable hint if unavailable rather than silently running with warm OS cache).
     - **Linux:** `sync` + writing `3` to `/proc/sys/vm/drop_caches`.
   - Record `disk_cache_cleared: bool` in `results.json` (`EnvironmentInfo`) and the report header so cold-OS-cache runs are explicitly labeled.

3. **Apples-to-Apples Fairness Invariants**:
   - **Clean Workspace Isolation (`--isolate-workspace`):** Run each server against a pristine copy of each repository (copy-on-write clone or cleanup of server-generated artifacts like `.project`, `.classpath`, `.settings/`, `.gradle/`, `.nb-gradle/`, `build/`, `target/`) so one server's side effects never accelerate or break the next server.
   - **Offline Dependency Prefetch (`--prefetch-deps`):** Pre-populate Maven/Gradle local caches (`~/.m2`, `~/.gradle`) before timed runs so cold starts measure LSP project import & indexing rather than Maven Central download variance, while keeping each server's own index/workspace cache 100% cold.
   - **Identical Runtime Environment:** Pin identical `JAVA_HOME`, `LC_ALL=C`, and CPU/power conditions across all servers, and record active `JAVA_HOME` and server JVM flags (`-Xmx`, etc.) in `results.json`.
   - **Strict Capability & Hit-Rate Parity:** Highlight apples-to-apples comparisons over the **intersection** of repos, tiers, shapes, and operations where all compared servers both advertise the capability and achieve valid semantic hits (`hit_rate > 0%`), preventing fast-but-empty or unimplemented responses from skewing comparisons.
   - **Multi-Trial Interleaved Runs (`--trials <N>`):** Rotate/interleave server execution order across trials and report medians to cancel thermal throttling and OS scheduling jitter.
