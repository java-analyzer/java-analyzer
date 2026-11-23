# Java LSP Benchmarks

## Environment & Revisions

- **OS / Arch**: `macos` / `aarch64`
- **CPU**: Apple M4 Pro (12 cores)
- **RAM**: 24 GB
- **Java**: `openjdk version "27.0.1.0.2" 2026-10-21`
- **Revision**: `svkrkzss (83fe82de, Fix bench CLI contract, nbls launcher, and relative paths)`
- **Date**: 2026-11-23
- **Servers**:
  - `java-analyzer`: target/release/java-analyzer lsp
  - `jdtls`: target/bench/servers/jdtls/bin/jdtls -data {cache_dir}
  - `jls`: target/bench/servers/jls/extension/dist/lang_server_mac.sh
  - `nbls`: target/bench/servers/nbls/extension/nbcode/bin/nbcode.sh --userdir {cache_dir} --start-java-language-server

## Matrix 1 — Lifecycle, First Response, Memory & Reliability

| Repo | Tier | Shape | Files | Server | Cold Init | Cold 1st Resp | Cold Ready | Warm Init | Warm 1st Resp | Warm Ready | Peak RSS | Steady RSS | Error Rate | Clean-File Errors |
| :--- | :--- | :--- | ---: | :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `plain-small` | small | plain+single-module | 150 | `java-analyzer` | 17.6ms | 97.7ms | 413.8ms | 4.9ms | 72.5ms | 391.9ms | 219.1 MB | 219.1 MB | 0.0% | 0 |
| `plain-small` | small | plain+single-module | 150 | `jdtls` | 1799.5ms | 2691.6ms | 2718.6ms | 1137.2ms | 1822.1ms | 1842.9ms | 3229.4 MB | 2979.4 MB | 0.0% | 20 |
| `plain-small` | small | plain+single-module | 150 | `jls` | 246.0ms | 448.1ms | 780.0ms | 162.0ms | 350.8ms | 688.9ms | 252.2 MB | 247.7 MB | 0.0% | 0 |
| `plain-small` | small | plain+single-module | 150 | `nbls` | 4596.0ms | 5102.7ms | 6411.1ms | 2763.4ms | 3284.2ms | 4609.0ms | 1064.8 MB | 1051.2 MB | 0.0% | 2 |
| `jackson-core` | small | maven+single-module | 358 | `java-analyzer` | 17.6ms | 667.2ms | 976.6ms | 4.4ms | 184.5ms | 492.6ms | 246.5 MB | 102.4 MB | 0.0% | 0 |
| `jackson-core` | small | maven+single-module | 358 | `jdtls` | 1196.4ms | 19011.1ms | 19036.6ms | 887.7ms | 1844.2ms | 1888.0ms | 2014.9 MB | 1881.1 MB | 0.0% | 0 |
| `jackson-core` | small | maven+single-module | 358 | `jls` | 301.6ms | 664.9ms | 973.9ms | 246.5ms | 495.0ms | 800.1ms | 565.5 MB | 539.2 MB | 0.0% | 0 |
| `jackson-core` | small | maven+single-module | 358 | `nbls` | 2789.2ms | 4804.5ms | 4837.5ms | 2702.1ms | 4507.1ms | 4540.6ms | 1061.9 MB | 1058.9 MB | 0.0% | 0 |
| `gson` | small | maven+multi-module | 249 | `java-analyzer` | 18.9ms | 692.7ms | 1004.1ms | 4.8ms | 181.0ms | 492.2ms | 59.7 MB | 59.7 MB | 0.0% | 0 |
| `gson` | small | maven+multi-module | 249 | `jdtls` | 1199.2ms | 17624.2ms | 17666.9ms | 919.7ms | 1844.0ms | 1884.6ms | 1573.6 MB | 1552.5 MB | 0.0% | 5 |
| `gson` | small | maven+multi-module | 249 | `jls` | 295.1ms | 634.8ms | 2237.0ms | 260.1ms | 514.2ms | 2114.7ms | 590.5 MB | 590.1 MB | 0.0% | 0 |
| `gson` | small | maven+multi-module | 249 | `nbls` | 2888.6ms | 4458.0ms | 4466.8ms | 2613.8ms | 3998.9ms | 4010.8ms | 1259.5 MB | 1255.7 MB | 0.0% | 0 |
| `commons-lang` | medium | maven+single-module | 500 | `java-analyzer` | 18.1ms | 1479.7ms | 1795.5ms | 6.0ms | 297.3ms | 618.0ms | 114.2 MB | 48.1 MB | 0.0% | 0 |
| `commons-lang` | medium | maven+single-module | 500 | `jdtls` | 1198.4ms | 7791.1ms | 7841.1ms | 906.7ms | 1851.5ms | 1891.1ms | 3325.7 MB | 3254.9 MB | 0.0% | 0 |
| `commons-lang` | medium | maven+single-module | 500 | `jls` | 343.9ms | 665.4ms | 976.9ms | 255.9ms | 504.9ms | 816.9ms | 447.8 MB | 431.6 MB | 0.0% | 0 |
| `commons-lang` | medium | maven+single-module | 500 | `nbls` | 2741.2ms | 4088.2ms | 4120.0ms | 2442.7ms | 3961.2ms | 3989.6ms | 1215.2 MB | 1198.1 MB | 0.0% | 0 |
| `ant-ivy` | medium | ant+single-module | 664 | `java-analyzer` | 18.8ms | 1731.2ms | 2037.8ms | 3.4ms | 382.0ms | 689.7ms | 88.6 MB | 29.6 MB | 0.0% | 0 |
| `ant-ivy` | medium | ant+single-module | 664 | `jdtls` | 1055.0ms | 2184.8ms | 2193.7ms | 1099.8ms | 1951.7ms | 1961.7ms | 1647.7 MB | 1647.7 MB | 0.0% | 100 |
| `ant-ivy` | medium | ant+single-module | 664 | `jls` | 478.9ms | 706.0ms | 1731.4ms | 400.7ms | 614.9ms | 1650.8ms | 444.8 MB | 404.1 MB | 0.0% | 0 |
| `ant-ivy` | medium | ant+single-module | 664 | `nbls` | 2593.3ms | 3202.3ms | 3590.9ms | 2481.2ms | 3087.6ms | 3460.9ms | 577.7 MB | 577.7 MB | 0.0% | 46 |
| `gradle-medium` | medium | gradle+multi-module | 407 | `java-analyzer` | 17.0ms | 789.1ms | 1125.7ms | 5.1ms | 250.2ms | 589.2ms | 238.1 MB | 133.0 MB | 0.0% | 0 |
| `gradle-medium` | medium | gradle+multi-module | 407 | `jdtls` | 1036.2ms | 61045.0ms | 177318.3ms | 1409.7ms | 2333.4ms | 2381.0ms | 2967.3 MB | 2716.6 MB | 0.0% | 0 |
| `gradle-medium` | medium | gradle+multi-module | 407 | `jls` | 386.9ms | 711.5ms | 1447.8ms | 335.7ms | 601.4ms | 1337.5ms | 480.0 MB | 475.4 MB | 0.0% | 0 |
| `gradle-medium` | medium | gradle+multi-module | 407 | `nbls` | 2686.9ms | 3464.3ms | 5445.5ms | 4163.3ms | 4867.3ms | 8262.6ms | 1411.2 MB | 1409.3 MB | 0.0% | 0 |
| `tomcat` | large | ant+multi-module | 2633 | `java-analyzer` | 21.3ms | 6740.2ms | 7045.6ms | 4.2ms | 924.3ms | 1231.6ms | 238.8 MB | 56.6 MB | 0.0% | 0 |
| `tomcat` | large | ant+multi-module | 2633 | `jdtls` | 1594.2ms | 23352.7ms | 23354.3ms | 1299.0ms | 2574.0ms | 2576.3ms | 3620.6 MB | 3066.2 MB | 0.0% | 0 |
| `tomcat` | large | ant+multi-module | 2633 | `jls` | 1050.4ms | 1268.7ms | 1574.7ms | 552.4ms | 749.2ms | 1052.5ms | 366.0 MB | 364.8 MB | 0.0% | 0 |
| `tomcat` | large | ant+multi-module | 2633 | `nbls` | 2907.0ms | 3428.1ms | 18323.9ms | 2539.0ms | 3061.9ms | 4313.9ms | 1282.8 MB | 1174.5 MB | 0.0% | 0 |
| `junit-framework` | large | gradle+multi-module | 1462 | `java-analyzer` | 17.7ms | 2782.7ms | 3084.9ms | 4.9ms | 427.4ms | 733.4ms | 252.0 MB | 56.2 MB | 0.0% | 0 |
| `junit-framework` | large | gradle+multi-module | 1462 | `jdtls` | 1217.9ms | 61228.1ms | 167857.3ms | 1552.6ms | 2639.0ms | 2639.8ms | 3822.7 MB | 2699.5 MB | 0.0% | 0 |
| `junit-framework` | large | gradle+multi-module | 1462 | `jls` | 670.6ms | 873.9ms | 1179.4ms | 635.9ms | 829.5ms | 1135.0ms | 453.9 MB | 430.1 MB | 0.0% | 5 |
| `junit-framework` | large | gradle+multi-module | 1462 | `nbls` | 2879.1ms | 3525.5ms | 5904.3ms | 2503.6ms | 3002.3ms | 4775.4ms | 1358.1 MB | 1358.1 MB | 0.0% | 4 |
| `error-prone` | large | maven+multi-module | 1961 | `java-analyzer` | 18.6ms | 3956.5ms | 4262.0ms | 5.5ms | 480.2ms | 792.2ms | 316.5 MB | 77.4 MB | 0.0% | 0 |
| `error-prone` | large | maven+multi-module | 1961 | `jdtls` | 1041.6ms | 18147.2ms | 18175.9ms | 986.2ms | 1957.9ms | 1964.7ms | 3219.8 MB | 2858.4 MB | 0.0% | 36 |
| `error-prone` | large | maven+multi-module | 1961 | `jls` | 553.4ms | 919.5ms | 2663.3ms | 476.1ms | 738.9ms | 2505.2ms | 613.7 MB | 613.0 MB | 0.0% | 2 |
| `error-prone` | large | maven+multi-module | 1961 | `nbls` | 2727.4ms | 4272.2ms | 4278.9ms | 2475.8ms | 4228.8ms | 4236.4ms | 1020.4 MB | 993.0 MB | 0.0% | 0 |

## Matrix 2 — Interactive Latency, Error Rate & Hit Rate

### Navigation & Resolution

| Repo | Server | Operation | 1st Call | p50 | p95 | max | Error Rate | Hit Rate |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `plain-small` | `java-analyzer` | `hover_workspace` | 2.9ms | 0.3ms | 0.3ms | 0.3ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `hover_jdk` | 9.7ms | 3.9ms | 4.3ms | 4.4ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `def_workspace` | 0.1ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `def_jdk` | 5.4ms | 0.4ms | 0.4ms | 0.4ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `type_def` | 0.1ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `doc_symbol_small` | 0.2ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `doc_symbol_large` | 2.2ms | 1.0ms | 1.1ms | 1.2ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `workspace_symbol` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jdtls` | `hover_workspace` | 121.8ms | 1.6ms | 2.0ms | 2.0ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `hover_jdk` | 1967.3ms | 71.9ms | 82.5ms | 88.0ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `def_workspace` | 4.1ms | 1.4ms | 1.6ms | 1.6ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `def_jdk` | 4.1ms | 1.8ms | 2.4ms | 2.9ms | 0.0% | 0.0% |
| `plain-small` | `jdtls` | `type_def` | 8.9ms | 0.2ms | 0.3ms | 0.3ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `doc_symbol_small` | 1.3ms | 0.5ms | 0.5ms | 0.5ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `doc_symbol_large` | 9.7ms | 2.6ms | 3.3ms | 3.4ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `workspace_symbol` | 2.9ms | 0.5ms | 0.7ms | 0.8ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `hover_workspace` | 8.4ms | 0.4ms | 0.7ms | 0.8ms | 0.0% | 0.0% |
| `plain-small` | `jls` | `hover_jdk` | 54.3ms | 7.9ms | 10.2ms | 10.5ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `def_workspace` | 10.0ms | 3.6ms | 5.0ms | 6.1ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `def_jdk` | 31.5ms | 7.1ms | 8.2ms | 8.4ms | 0.0% | 0.0% |
| `plain-small` | `jls` | `type_def` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jls` | `doc_symbol_small` | 3.1ms | 1.0ms | 1.1ms | 1.2ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `doc_symbol_large` | 14.2ms | 3.4ms | 4.2ms | 5.3ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `workspace_symbol` | 14.6ms | 6.1ms | 6.7ms | 7.3ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `hover_workspace` | 23.1ms | 9.0ms | 11.9ms | 12.7ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `hover_jdk` | 228.3ms | 149.0ms | 170.2ms | 173.6ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `def_workspace` | 0.5ms | 4.2ms | 4.7ms | 4.8ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `def_jdk` | 40.4ms | 7.8ms | 8.7ms | 9.3ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `type_def` | 2.0ms | 0.5ms | 0.6ms | 0.7ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `doc_symbol_small` | 1.0ms | 0.3ms | 0.4ms | 0.6ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `doc_symbol_large` | 40.5ms | 8.5ms | 9.3ms | 9.4ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `workspace_symbol` | 1.0ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 0.0% |
| `jackson-core` | `java-analyzer` | `hover_workspace` | 82.5ms | 0.0ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `jackson-core` | `java-analyzer` | `doc_symbol_small` | 0.2ms | 0.2ms | 0.2ms | 0.2ms | 0.0% | 100.0% |
| `jackson-core` | `java-analyzer` | `doc_symbol_large` | 0.8ms | 0.6ms | 0.6ms | 0.6ms | 0.0% | 100.0% |
| `jackson-core` | `java-analyzer` | `workspace_symbol` | N/A | N/A | N/A | N/A | N/A | N/A |
| `jackson-core` | `jdtls` | `hover_workspace` | 2869.0ms | 16.2ms | 23.1ms | 30.2ms | 0.0% | 100.0% |
| `jackson-core` | `jdtls` | `doc_symbol_small` | 2.2ms | 0.9ms | 1.1ms | 1.3ms | 0.0% | 100.0% |
| `jackson-core` | `jdtls` | `doc_symbol_large` | 6.9ms | 2.5ms | 2.9ms | 3.0ms | 0.0% | 100.0% |
| `jackson-core` | `jdtls` | `workspace_symbol` | 10.3ms | 2.6ms | 2.8ms | 2.9ms | 0.0% | 100.0% |
| `jackson-core` | `jls` | `hover_workspace` | 672.7ms | 0.6ms | 0.7ms | 0.8ms | 0.0% | 0.0% |
| `jackson-core` | `jls` | `doc_symbol_small` | 1.5ms | 0.6ms | 0.9ms | 0.9ms | 0.0% | 100.0% |
| `jackson-core` | `jls` | `doc_symbol_large` | 4.8ms | 2.3ms | 3.2ms | 4.7ms | 0.0% | 100.0% |
| `jackson-core` | `jls` | `workspace_symbol` | 170.1ms | 92.0ms | 99.0ms | 105.0ms | 0.0% | 100.0% |
| `jackson-core` | `nbls` | `hover_workspace` | 336.1ms | 58.9ms | 63.6ms | 64.1ms | 0.0% | 100.0% |
| `jackson-core` | `nbls` | `doc_symbol_small` | 9.7ms | 0.6ms | 0.7ms | 0.8ms | 0.0% | 100.0% |
| `jackson-core` | `nbls` | `doc_symbol_large` | 6.2ms | 1.1ms | 1.2ms | 1.2ms | 0.0% | 100.0% |
| `jackson-core` | `nbls` | `workspace_symbol` | 5539.8ms | 41.0ms | 44.2ms | 44.3ms | 0.0% | 100.0% |
| `gson` | `java-analyzer` | `hover_workspace` | 68.2ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `gson` | `java-analyzer` | `doc_symbol_small` | 0.2ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `gson` | `java-analyzer` | `doc_symbol_large` | 0.5ms | 0.4ms | 0.4ms | 0.4ms | 0.0% | 100.0% |
| `gson` | `java-analyzer` | `workspace_symbol` | N/A | N/A | N/A | N/A | N/A | N/A |
| `gson` | `jdtls` | `hover_workspace` | 1991.5ms | 81.9ms | 290.5ms | 510.3ms | 0.0% | 100.0% |
| `gson` | `jdtls` | `doc_symbol_small` | 1.4ms | 0.7ms | 1.4ms | 2.9ms | 0.0% | 100.0% |
| `gson` | `jdtls` | `doc_symbol_large` | 4.8ms | 1.5ms | 2.8ms | 5.0ms | 0.0% | 100.0% |
| `gson` | `jdtls` | `workspace_symbol` | 44.2ms | 5.9ms | 14.5ms | 15.7ms | 0.0% | 100.0% |
| `gson` | `jls` | `hover_workspace` | 22.7ms | 1.2ms | 1.2ms | 1.3ms | 0.0% | 0.0% |
| `gson` | `jls` | `doc_symbol_small` | 1.3ms | 0.5ms | 0.6ms | 0.7ms | 0.0% | 100.0% |
| `gson` | `jls` | `doc_symbol_large` | 5.4ms | 1.8ms | 2.1ms | 2.3ms | 0.0% | 100.0% |
| `gson` | `jls` | `workspace_symbol` | 65.4ms | 28.3ms | 29.8ms | 30.3ms | 0.0% | 100.0% |
| `gson` | `nbls` | `hover_workspace` | 0.4ms | 129.5ms | 201.5ms | 226.4ms | 0.0% | 100.0% |
| `gson` | `nbls` | `doc_symbol_small` | 1.3ms | 0.5ms | 1.5ms | 2.4ms | 0.0% | 100.0% |
| `gson` | `nbls` | `doc_symbol_large` | 151.4ms | 0.8ms | 1.0ms | 1.0ms | 0.0% | 100.0% |
| `gson` | `nbls` | `workspace_symbol` | 9350.4ms | 53.9ms | 60.5ms | 60.7ms | 0.0% | 100.0% |
| `commons-lang` | `java-analyzer` | `hover_workspace` | 167.8ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `commons-lang` | `java-analyzer` | `doc_symbol_small` | 0.1ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `commons-lang` | `java-analyzer` | `doc_symbol_large` | 1.5ms | 1.1ms | 1.2ms | 1.2ms | 0.0% | 100.0% |
| `commons-lang` | `java-analyzer` | `workspace_symbol` | N/A | N/A | N/A | N/A | N/A | N/A |
| `commons-lang` | `jdtls` | `hover_workspace` | 4207.2ms | 61.7ms | 63.8ms | 64.2ms | 0.0% | 100.0% |
| `commons-lang` | `jdtls` | `doc_symbol_small` | 0.9ms | 0.5ms | 0.5ms | 0.5ms | 0.0% | 100.0% |
| `commons-lang` | `jdtls` | `doc_symbol_large` | 12.8ms | 2.9ms | 3.7ms | 3.8ms | 0.0% | 100.0% |
| `commons-lang` | `jdtls` | `workspace_symbol` | 24.5ms | 2.9ms | 3.1ms | 3.1ms | 0.0% | 100.0% |
| `commons-lang` | `jls` | `hover_workspace` | 564.0ms | 3.2ms | 3.4ms | 3.4ms | 0.0% | 0.0% |
| `commons-lang` | `jls` | `doc_symbol_small` | 0.7ms | 0.5ms | 0.7ms | 0.8ms | 0.0% | 100.0% |
| `commons-lang` | `jls` | `doc_symbol_large` | 18.7ms | 7.1ms | 7.7ms | 7.7ms | 0.0% | 100.0% |
| `commons-lang` | `jls` | `workspace_symbol` | 249.2ms | 166.1ms | 181.6ms | 187.8ms | 0.0% | 100.0% |
| `commons-lang` | `nbls` | `hover_workspace` | 710.3ms | 86.3ms | 89.1ms | 89.5ms | 0.0% | 100.0% |
| `commons-lang` | `nbls` | `doc_symbol_small` | 0.8ms | 0.3ms | 0.4ms | 0.4ms | 0.0% | 100.0% |
| `commons-lang` | `nbls` | `doc_symbol_large` | 27.1ms | 2.3ms | 2.4ms | 2.5ms | 0.0% | 100.0% |
| `commons-lang` | `nbls` | `workspace_symbol` | 8116.7ms | 80.5ms | 89.8ms | 91.7ms | 0.0% | 100.0% |
| `ant-ivy` | `java-analyzer` | `hover_workspace` | 132.1ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `ant-ivy` | `java-analyzer` | `doc_symbol_small` | 0.5ms | 0.4ms | 0.4ms | 0.4ms | 0.0% | 100.0% |
| `ant-ivy` | `java-analyzer` | `workspace_symbol` | N/A | N/A | N/A | N/A | N/A | N/A |
| `ant-ivy` | `jdtls` | `hover_workspace` | 2289.8ms | 11.9ms | 13.8ms | 15.1ms | 0.0% | 100.0% |
| `ant-ivy` | `jdtls` | `doc_symbol_small` | 4.4ms | 1.8ms | 2.3ms | 2.5ms | 0.0% | 100.0% |
| `ant-ivy` | `jdtls` | `workspace_symbol` | 6.5ms | 1.2ms | 1.3ms | 1.4ms | 0.0% | 100.0% |
| `ant-ivy` | `jls` | `hover_workspace` | 3.4ms | 0.9ms | 1.0ms | 1.2ms | 0.0% | 0.0% |
| `ant-ivy` | `jls` | `doc_symbol_small` | 2.3ms | 1.3ms | 1.8ms | 1.8ms | 0.0% | 100.0% |
| `ant-ivy` | `jls` | `workspace_symbol` | 227.2ms | 145.1ms | 170.8ms | 174.9ms | 0.0% | 100.0% |
| `ant-ivy` | `nbls` | `hover_workspace` | 317.4ms | 6.0ms | 27.7ms | 27.8ms | 0.0% | 100.0% |
| `ant-ivy` | `nbls` | `doc_symbol_small` | 3.5ms | 1.5ms | 2.5ms | 3.7ms | 0.0% | 100.0% |
| `ant-ivy` | `nbls` | `workspace_symbol` | 0.8ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 0.0% |
| `gradle-medium` | `java-analyzer` | `hover_workspace` | 77.9ms | 0.1ms | 0.1ms | 0.2ms | 0.0% | 100.0% |
| `gradle-medium` | `java-analyzer` | `doc_symbol_small` | 0.4ms | 0.3ms | 0.4ms | 0.4ms | 0.0% | 100.0% |
| `gradle-medium` | `java-analyzer` | `doc_symbol_large` | 12.8ms | 9.9ms | 10.1ms | 10.1ms | 0.0% | 100.0% |
| `gradle-medium` | `java-analyzer` | `workspace_symbol` | N/A | N/A | N/A | N/A | N/A | N/A |
| `gradle-medium` | `jdtls` | `hover_workspace` | 452.2ms | 69.0ms | 82.7ms | 83.1ms | 0.0% | 100.0% |
| `gradle-medium` | `jdtls` | `doc_symbol_small` | 5.2ms | 1.9ms | 2.7ms | 3.0ms | 0.0% | 100.0% |
| `gradle-medium` | `jdtls` | `doc_symbol_large` | 84.5ms | 32.8ms | 45.6ms | 54.4ms | 0.0% | 100.0% |
| `gradle-medium` | `jdtls` | `workspace_symbol` | 53.5ms | 3.5ms | 7.4ms | 10.5ms | 0.0% | 100.0% |
| `gradle-medium` | `jls` | `hover_workspace` | 212.5ms | 18.8ms | 19.9ms | 19.9ms | 0.0% | 0.0% |
| `gradle-medium` | `jls` | `doc_symbol_small` | 2.6ms | 1.4ms | 1.7ms | 1.9ms | 0.0% | 100.0% |
| `gradle-medium` | `jls` | `doc_symbol_large` | 59.2ms | 31.8ms | 34.3ms | 35.2ms | 0.0% | 100.0% |
| `gradle-medium` | `jls` | `workspace_symbol` | 380.3ms | 132.4ms | 137.1ms | 137.7ms | 0.0% | 100.0% |
| `gradle-medium` | `nbls` | `hover_workspace` | 707.6ms | 21.2ms | 61.5ms | 101.1ms | 0.0% | 100.0% |
| `gradle-medium` | `nbls` | `doc_symbol_small` | 4.0ms | 15.1ms | 16.2ms | 16.2ms | 0.0% | 100.0% |
| `gradle-medium` | `nbls` | `doc_symbol_large` | 133.6ms | 81.6ms | 86.3ms | 87.4ms | 0.0% | 100.0% |
| `gradle-medium` | `nbls` | `workspace_symbol` | 3793.4ms | 2.6ms | 3.2ms | 3.3ms | 0.0% | 100.0% |
| `tomcat` | `java-analyzer` | `doc_symbol_small` | 5.3ms | 0.1ms | 0.2ms | 0.2ms | 0.0% | 100.0% |
| `tomcat` | `java-analyzer` | `doc_symbol_large` | 3.5ms | 2.0ms | 2.0ms | 2.1ms | 0.0% | 100.0% |
| `tomcat` | `java-analyzer` | `workspace_symbol` | N/A | N/A | N/A | N/A | N/A | N/A |
| `tomcat` | `jdtls` | `doc_symbol_small` | 206.9ms | 0.6ms | 0.7ms | 0.8ms | 0.0% | 100.0% |
| `tomcat` | `jdtls` | `doc_symbol_large` | 13.3ms | 3.6ms | 4.3ms | 4.3ms | 0.0% | 100.0% |
| `tomcat` | `jdtls` | `workspace_symbol` | 7119.2ms | 0.7ms | 0.8ms | 0.9ms | 0.0% | 100.0% |
| `tomcat` | `jls` | `doc_symbol_small` | 3.2ms | 0.8ms | 1.3ms | 1.5ms | 0.0% | 100.0% |
| `tomcat` | `jls` | `doc_symbol_large` | 40.5ms | 5.9ms | 6.4ms | 6.5ms | 0.0% | 100.0% |
| `tomcat` | `jls` | `workspace_symbol` | 786.5ms | 483.0ms | 525.7ms | 535.8ms | 0.0% | 100.0% |
| `tomcat` | `nbls` | `doc_symbol_small` | 13.5ms | 0.5ms | 0.6ms | 0.7ms | 0.0% | 100.0% |
| `tomcat` | `nbls` | `doc_symbol_large` | 171.1ms | 4.4ms | 63.1ms | 78.0ms | 0.0% | 100.0% |
| `tomcat` | `nbls` | `workspace_symbol` | 727.4ms | 60.3ms | 82.7ms | 94.5ms | 0.0% | 100.0% |
| `junit-framework` | `java-analyzer` | `doc_symbol_small` | 3.2ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `junit-framework` | `java-analyzer` | `doc_symbol_large` | 5.8ms | 1.3ms | 1.3ms | 1.4ms | 0.0% | 100.0% |
| `junit-framework` | `java-analyzer` | `workspace_symbol` | N/A | N/A | N/A | N/A | N/A | N/A |
| `junit-framework` | `jdtls` | `doc_symbol_small` | 44.1ms | 0.5ms | 0.6ms | 0.6ms | 0.0% | 100.0% |
| `junit-framework` | `jdtls` | `doc_symbol_large` | 19.9ms | 3.3ms | 3.8ms | 4.0ms | 0.0% | 100.0% |
| `junit-framework` | `jdtls` | `workspace_symbol` | 11868.8ms | 1.7ms | 1.9ms | 1.9ms | 0.0% | 100.0% |
| `junit-framework` | `jls` | `doc_symbol_small` | 1.9ms | 0.3ms | 0.5ms | 0.6ms | 0.0% | 100.0% |
| `junit-framework` | `jls` | `doc_symbol_large` | 40.8ms | 3.8ms | 4.1ms | 4.4ms | 0.0% | 100.0% |
| `junit-framework` | `jls` | `workspace_symbol` | 717.2ms | 333.9ms | 359.1ms | 360.1ms | 0.0% | 100.0% |
| `junit-framework` | `nbls` | `doc_symbol_small` | 12.7ms | 0.3ms | 0.4ms | 0.5ms | 0.0% | 100.0% |
| `junit-framework` | `nbls` | `doc_symbol_large` | 170.5ms | 2.1ms | 4.7ms | 6.7ms | 0.0% | 100.0% |
| `junit-framework` | `nbls` | `workspace_symbol` | 2780.3ms | 1.6ms | 1.8ms | 1.9ms | 0.0% | 0.0% |
| `error-prone` | `java-analyzer` | `doc_symbol_small` | 1.1ms | 0.6ms | 0.7ms | 0.8ms | 0.0% | 100.0% |
| `error-prone` | `java-analyzer` | `workspace_symbol` | N/A | N/A | N/A | N/A | N/A | N/A |
| `error-prone` | `jdtls` | `doc_symbol_small` | 7.5ms | 2.4ms | 2.9ms | 3.1ms | 0.0% | 100.0% |
| `error-prone` | `jdtls` | `workspace_symbol` | 10812.4ms | 1.6ms | 1.9ms | 2.0ms | 0.0% | 100.0% |
| `error-prone` | `jls` | `doc_symbol_small` | 2.6ms | 1.5ms | 1.8ms | 2.2ms | 0.0% | 100.0% |
| `error-prone` | `jls` | `workspace_symbol` | 807.6ms | 537.2ms | 596.0ms | 609.8ms | 0.0% | 100.0% |
| `error-prone` | `nbls` | `doc_symbol_small` | 4.4ms | 1.4ms | 30.7ms | 93.3ms | 0.0% | 100.0% |
| `error-prone` | `nbls` | `workspace_symbol` | 6605.2ms | 13.6ms | 14.7ms | 15.4ms | 0.0% | 0.0% |

### Type Hierarchies

| Repo | Server | Operation | 1st Call | p50 | p95 | max | Error Rate | Hit Rate |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `plain-small` | `java-analyzer` | `impl_simple` | 0.1ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `impl_deep` | 0.2ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `impl_wide` | 0.5ms | 0.1ms | 0.2ms | 0.2ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `hierarchy_super` | 0.1ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `hierarchy_sub` | 0.1ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `impl_simple` | 23.9ms | 6.9ms | 8.0ms | 8.2ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `impl_deep` | 27.0ms | 12.5ms | 14.6ms | 14.7ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `impl_wide` | 26.2ms | 15.9ms | 26.0ms | 38.2ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `hierarchy_super` | 16.5ms | 10.0ms | 15.4ms | 16.1ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `hierarchy_sub` | 20.6ms | 15.4ms | 21.9ms | 22.5ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `impl_simple` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jls` | `impl_deep` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jls` | `impl_wide` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jls` | `hierarchy_super` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jls` | `hierarchy_sub` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `nbls` | `impl_simple` | 148.0ms | 22.4ms | 27.1ms | 27.3ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `impl_deep` | 20.5ms | 28.1ms | 33.4ms | 37.5ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `impl_wide` | 62.4ms | 100.0ms | 106.7ms | 106.7ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `hierarchy_super` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `nbls` | `hierarchy_sub` | N/A | N/A | N/A | N/A | N/A | N/A |
| `jackson-core` | `java-analyzer` | `impl_simple` | 1.7ms | 0.1ms | 0.1ms | 0.1ms | 0.0% | 100.0% |
| `jackson-core` | `jdtls` | `impl_simple` | 44.3ms | 15.4ms | 23.6ms | 29.0ms | 0.0% | 100.0% |
| `jackson-core` | `jls` | `impl_simple` | N/A | N/A | N/A | N/A | N/A | N/A |
| `jackson-core` | `nbls` | `impl_simple` | 396.1ms | 144.3ms | 151.3ms | 152.7ms | 0.0% | 100.0% |
| `gson` | `java-analyzer` | `impl_wide` | 1.8ms | 0.1ms | 0.2ms | 0.2ms | 0.0% | 100.0% |
| `gson` | `jdtls` | `impl_wide` | 208.0ms | 69.2ms | 196.8ms | 391.0ms | 0.0% | 100.0% |
| `gson` | `jls` | `impl_wide` | N/A | N/A | N/A | N/A | N/A | N/A |
| `gson` | `nbls` | `impl_wide` | 1197.0ms | 580.9ms | 607.2ms | 626.4ms | 0.0% | 100.0% |
| `tomcat` | `java-analyzer` | `impl_wide` | 513.9ms | 0.4ms | 0.4ms | 0.5ms | 0.0% | 100.0% |
| `tomcat` | `java-analyzer` | `hierarchy_sub` | 0.2ms | 0.1ms | 0.2ms | 0.2ms | 0.0% | 100.0% |
| `tomcat` | `jdtls` | `impl_wide` | 260.5ms | 120.6ms | 128.7ms | 130.8ms | 0.0% | 100.0% |
| `tomcat` | `jdtls` | `hierarchy_sub` | 166.2ms | 132.3ms | 155.8ms | 168.2ms | 0.0% | 100.0% |
| `tomcat` | `jls` | `impl_wide` | N/A | N/A | N/A | N/A | N/A | N/A |
| `tomcat` | `jls` | `hierarchy_sub` | N/A | N/A | N/A | N/A | N/A | N/A |
| `tomcat` | `nbls` | `impl_wide` | 1885.0ms | 916.9ms | 995.5ms | 996.5ms | 0.0% | 100.0% |
| `tomcat` | `nbls` | `hierarchy_sub` | N/A | N/A | N/A | N/A | N/A | N/A |
| `junit-framework` | `java-analyzer` | `impl_wide` | 145.0ms | 0.5ms | 0.6ms | 0.6ms | 0.0% | 100.0% |
| `junit-framework` | `jdtls` | `impl_wide` | 2883.6ms | 129.3ms | 133.5ms | 134.4ms | 0.0% | 100.0% |
| `junit-framework` | `jls` | `impl_wide` | N/A | N/A | N/A | N/A | N/A | N/A |
| `junit-framework` | `nbls` | `impl_wide` | 139.7ms | 7.4ms | 8.7ms | 8.9ms | 0.0% | 0.0% |
| `error-prone` | `java-analyzer` | `impl_wide` | 212.0ms | 1.5ms | 1.7ms | 1.9ms | 0.0% | 100.0% |
| `error-prone` | `java-analyzer` | `hierarchy_sub` | 3.3ms | 2.7ms | 2.8ms | 2.9ms | 0.0% | 100.0% |
| `error-prone` | `jdtls` | `impl_wide` | 103.6ms | 16.5ms | 18.6ms | 19.0ms | 0.0% | 100.0% |
| `error-prone` | `jdtls` | `hierarchy_sub` | 44.7ms | 31.8ms | 36.0ms | 39.0ms | 0.0% | 100.0% |
| `error-prone` | `jls` | `impl_wide` | N/A | N/A | N/A | N/A | N/A | N/A |
| `error-prone` | `jls` | `hierarchy_sub` | N/A | N/A | N/A | N/A | N/A | N/A |
| `error-prone` | `nbls` | `impl_wide` | 345.9ms | 26.4ms | 40.6ms | 52.3ms | 0.0% | 0.0% |
| `error-prone` | `nbls` | `hierarchy_sub` | N/A | N/A | N/A | N/A | N/A | N/A |

### Cross-References & Refactoring

| Repo | Server | Operation | 1st Call | p50 | p95 | max | Error Rate | Hit Rate |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `plain-small` | `java-analyzer` | `refs_local` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `refs_cross_file` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `rename_local` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `rename_private` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `rename_public` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `code_action_fix` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `code_action_refactor` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jdtls` | `refs_local` | 8.8ms | 4.0ms | 4.6ms | 5.5ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `refs_cross_file` | 8.0ms | 4.1ms | 5.7ms | 5.7ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `rename_local` | 66.8ms | 4.4ms | 5.9ms | 6.5ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `rename_private` | 27.2ms | 8.6ms | 10.3ms | 11.2ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `rename_public` | 255.1ms | 80.2ms | 94.6ms | 96.0ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `code_action_fix` | 543.1ms | 456.7ms | 512.1ms | 512.7ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `code_action_refactor` | 45.5ms | 28.3ms | 88.3ms | 90.6ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `refs_local` | 7.3ms | 2.9ms | 3.5ms | 4.1ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `refs_cross_file` | 13.5ms | 2.4ms | 2.8ms | 3.3ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `rename_local` | 12.5ms | 2.7ms | 3.6ms | 4.3ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `rename_private` | 7.5ms | 1.6ms | 1.7ms | 1.7ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `rename_public` | 9.0ms | 2.3ms | 3.1ms | 4.4ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `code_action_fix` | 233.4ms | 220.7ms | 227.6ms | 228.3ms | 0.0% | 0.0% |
| `plain-small` | `jls` | `code_action_refactor` | 5.1ms | 2.0ms | 2.7ms | 2.9ms | 0.0% | 0.0% |
| `plain-small` | `nbls` | `refs_local` | 52.8ms | 9.4ms | 11.0ms | 11.7ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `refs_cross_file` | 28.4ms | 18.9ms | 21.3ms | 21.7ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `rename_local` | 62.8ms | 13.7ms | 14.9ms | 15.2ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `rename_private` | 53.4ms | 17.3ms | 20.3ms | 21.1ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `rename_public` | 47.5ms | 22.5ms | 27.9ms | 28.8ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `code_action_fix` | 572.0ms | 523.3ms | 533.7ms | 548.1ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `code_action_refactor` | 13.6ms | 11.2ms | 11.9ms | 11.9ms | 0.0% | 100.0% |

### Autocompletion

| Repo | Server | Operation | 1st Call | p50 | p95 | max | Error Rate | Hit Rate |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `plain-small` | `java-analyzer` | `completion_dot_workspace` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `completion_dot_jdk` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `completion_dot_generic` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `completion_scope` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `completion_auto_import` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `completion_override` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `completion_resolve` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jdtls` | `completion_dot_workspace` | 26.0ms | 9.9ms | 22.5ms | 30.8ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `completion_dot_jdk` | 12.5ms | 10.6ms | 13.2ms | 14.0ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `completion_dot_generic` | 5.9ms | 7.8ms | 14.4ms | 22.8ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `completion_scope` | 21.6ms | 11.9ms | 22.4ms | 22.7ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `completion_auto_import` | 10.4ms | 9.9ms | 34.4ms | 46.6ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `completion_override` | 19.6ms | 11.5ms | 24.4ms | 28.1ms | 0.0% | 0.0% |
| `plain-small` | `jdtls` | `completion_resolve` | 3.3ms | 1.0ms | 1.3ms | 1.5ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `completion_dot_workspace` | 12.1ms | 2.2ms | 3.4ms | 3.9ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `completion_dot_jdk` | 12.7ms | 2.5ms | 3.3ms | 3.8ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `completion_dot_generic` | 4.2ms | 1.8ms | 3.1ms | 4.2ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `completion_scope` | 13.6ms | 4.4ms | 7.1ms | 7.3ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `completion_auto_import` | 9.7ms | 4.8ms | 7.0ms | 7.4ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `completion_override` | 3.1ms | 1.4ms | 1.8ms | 2.0ms | 0.0% | 0.0% |
| `plain-small` | `jls` | `completion_resolve` | 2.5ms | 1.2ms | 1.4ms | 1.5ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `completion_dot_workspace` | 25.4ms | 12.9ms | 15.6ms | 16.8ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `completion_dot_jdk` | 28.6ms | 13.7ms | 16.2ms | 17.7ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `completion_dot_generic` | 13.1ms | 11.0ms | 13.5ms | 13.9ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `completion_scope` | 78.6ms | 19.0ms | 20.9ms | 21.1ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `completion_auto_import` | 22.2ms | 12.6ms | 14.6ms | 15.5ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `completion_override` | 22.0ms | 10.4ms | 11.1ms | 11.4ms | 0.0% | 0.0% |
| `plain-small` | `nbls` | `completion_resolve` | 11.8ms | 29.2ms | 40.3ms | 42.3ms | 0.0% | 100.0% |

### Incremental Editing & Diagnostics

| Repo | Server | Operation | 1st Call | p50 | p95 | max | Error Rate | Hit Rate |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `plain-small` | `java-analyzer` | `edit_body` | 0.4ms | 0.3ms | 0.3ms | 0.3ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `edit_signature` | 0.7ms | 0.4ms | 0.4ms | 0.4ms | 0.0% | 100.0% |
| `plain-small` | `java-analyzer` | `signature_help` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jdtls` | `edit_body` | 207.2ms | 206.2ms | 206.6ms | 206.7ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `edit_signature` | 258.8ms | 207.3ms | 211.7ms | 212.2ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `signature_help` | 3.7ms | 1.6ms | 2.1ms | 2.2ms | 0.0% | 0.0% |
| `plain-small` | `jls` | `edit_body` | 208.6ms | 205.7ms | 206.5ms | 206.7ms | 0.0% | 0.0% |
| `plain-small` | `jls` | `edit_signature` | 8.5ms | 10.3ms | 210.9ms | 211.7ms | 0.0% | 0.0% |
| `plain-small` | `jls` | `signature_help` | 9.3ms | 4.3ms | 7.9ms | 10.5ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `edit_body` | 214.2ms | 212.3ms | 213.4ms | 213.4ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `edit_signature` | 225.3ms | 267.7ms | 282.0ms | 288.0ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `signature_help` | 12.7ms | 11.4ms | 21.9ms | 22.0ms | 0.0% | 100.0% |

### Presentation & Formatting

| Repo | Server | Operation | 1st Call | p50 | p95 | max | Error Rate | Hit Rate |
| :--- | :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: |
| `plain-small` | `java-analyzer` | `semantic_tokens` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `inlay_hints` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `java-analyzer` | `formatting` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jdtls` | `semantic_tokens` | 34.7ms | 27.0ms | 84.4ms | 88.3ms | 0.0% | 100.0% |
| `plain-small` | `jdtls` | `inlay_hints` | 7.8ms | 5.0ms | 6.7ms | 7.1ms | 0.0% | 0.0% |
| `plain-small` | `jdtls` | `formatting` | 5.6ms | 2.0ms | 2.4ms | 2.6ms | 0.0% | 100.0% |
| `plain-small` | `jls` | `semantic_tokens` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jls` | `inlay_hints` | N/A | N/A | N/A | N/A | N/A | N/A |
| `plain-small` | `jls` | `formatting` | 10.0ms | 0.7ms | 1.1ms | 1.2ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `semantic_tokens` | 50.3ms | 21.0ms | 24.3ms | 24.6ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `inlay_hints` | 23.7ms | 5.8ms | 6.3ms | 6.4ms | 0.0% | 100.0% |
| `plain-small` | `nbls` | `formatting` | 14.9ms | 2.2ms | 2.4ms | 2.5ms | 0.0% | 100.0% |
| `commons-lang` | `java-analyzer` | `semantic_tokens` | N/A | N/A | N/A | N/A | N/A | N/A |
| `commons-lang` | `jdtls` | `semantic_tokens` | 119.4ms | 4.2ms | 4.7ms | 4.8ms | 0.0% | 100.0% |
| `commons-lang` | `jls` | `semantic_tokens` | N/A | N/A | N/A | N/A | N/A | N/A |
| `commons-lang` | `nbls` | `semantic_tokens` | 122.3ms | 7.3ms | 8.1ms | 8.2ms | 0.0% | 100.0% |

## Capability Coverage Matrix

| Server | `hover` | `definition` | `typeDefinition` | `implementation` | `typeHierarchy` | `documentSymbol` | `workspaceSymbol` | `references` | `rename` | `codeAction` | `completion` | `completionResolve` | `signatureHelp` | `semanticTokens` | `inlayHint` | `formatting` |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: | :---: |
| `java-analyzer` | supported | supported | supported | supported | supported | supported | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A | N/A |
| `jdtls` | supported | supported | supported | supported | supported | supported | supported | supported | supported | supported | supported | supported | supported | supported | supported | supported |
| `jls` | supported | supported | N/A | N/A | N/A | supported | supported | supported | supported | supported | supported | supported | supported | N/A | N/A | supported |
| `nbls` | supported | supported | supported | supported | N/A | supported | supported | supported | supported | supported | supported | supported | supported | supported | supported | supported |
