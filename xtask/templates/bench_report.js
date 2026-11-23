    (function () {
      const dataEl = document.getElementById('bench-data');
      const report = dataEl ? JSON.parse(dataEl.textContent || '{}') : { servers: [], results: [] };
      const results = Array.isArray(report.results) ? report.results : [];
      const metaScheme = document.querySelector('meta[name="color-scheme"]');
      const THEME_KEY = 'java-bench-theme';

      const OP_CATEGORIES = {
        hover_workspace: 'Navigation & Resolution',
        hover_jdk: 'Navigation & Resolution',
        def_workspace: 'Navigation & Resolution',
        def_jdk: 'Navigation & Resolution',
        type_def: 'Navigation & Resolution',
        doc_symbol_small: 'Navigation & Resolution',
        doc_symbol_large: 'Navigation & Resolution',
        workspace_symbol: 'Navigation & Resolution',
        impl_simple: 'Type Hierarchies',
        impl_deep: 'Type Hierarchies',
        impl_wide: 'Type Hierarchies',
        hierarchy_super: 'Type Hierarchies',
        hierarchy_sub: 'Type Hierarchies',
        refs_local: 'Cross-References & Refactoring',
        refs_cross_file: 'Cross-References & Refactoring',
        rename_local: 'Cross-References & Refactoring',
        rename_private: 'Cross-References & Refactoring',
        rename_public: 'Cross-References & Refactoring',
        code_action_fix: 'Cross-References & Refactoring',
        code_action_refactor: 'Cross-References & Refactoring',
        completion_dot_workspace: 'Autocompletion',
        completion_dot_jdk: 'Autocompletion',
        completion_dot_generic: 'Autocompletion',
        completion_scope: 'Autocompletion',
        completion_auto_import: 'Autocompletion',
        completion_override: 'Autocompletion',
        completion_resolve: 'Autocompletion',
        edit_body: 'Incremental Editing & Diagnostics',
        edit_signature: 'Incremental Editing & Diagnostics',
        signature_help: 'Incremental Editing & Diagnostics',
        semantic_tokens: 'Presentation & Formatting',
        inlay_hints: 'Presentation & Formatting',
        formatting: 'Presentation & Formatting'
      };

      const state = {
        scale: 'log',
        phase: 'both',
        tier: 'all',
        shape: 'all',
        opCategory: 'all',
        servers: new Set((report.servers && report.servers.length ? report.servers : results.map((r) => r.server))),
        baseline: null
      };

      function getSavedTheme() {
        try {
          const val = localStorage.getItem(THEME_KEY);
          return val === 'light' || val === 'dark' ? val : 'system';
        } catch {
          return 'system';
        }
      }

      function isResolvedDark() {
        const content = metaScheme ? metaScheme.getAttribute('content') : 'light dark';
        if (content === 'dark') return true;
        if (content === 'light') return false;
        return window.matchMedia && window.matchMedia('(prefers-color-scheme: dark)').matches;
      }

      function applyTheme(choice, persist, shouldRender = true) {
        if (metaScheme) {
          metaScheme.setAttribute('content', choice === 'system' ? 'light dark' : choice);
        }
        if (persist) {
          try {
            if (choice === 'system') localStorage.removeItem(THEME_KEY);
            else localStorage.setItem(THEME_KEY, choice);
          } catch {}
        }
        document.querySelectorAll('.theme-btn').forEach((btn) => {
          btn.setAttribute('aria-pressed', String(btn.dataset.themeChoice === choice));
        });
        if (shouldRender) renderCharts();
      }

      document.querySelectorAll('.theme-btn').forEach((btn) => {
        btn.addEventListener('click', () => applyTheme(btn.dataset.themeChoice || 'system', true));
      });
      window.addEventListener('storage', (e) => {
        if (e.key === THEME_KEY) applyTheme(getSavedTheme(), false);
      });
      if (window.matchMedia) {
        window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
          if (getSavedTheme() === 'system') renderCharts();
        });
      }

      // Build server filter chips
      const serverFiltersEl = document.getElementById('server-filters');
      if (serverFiltersEl) {
        Array.from(state.servers).forEach((srv) => {
          const label = document.createElement('label');
          label.className = 'chip-label';
          const input = document.createElement('input');
          input.type = 'checkbox';
          input.checked = true;
          input.value = srv;
          input.addEventListener('change', () => {
            if (input.checked) state.servers.add(srv);
            else state.servers.delete(srv);
            applyFiltersAndRender();
          });
          label.appendChild(input);
          label.appendChild(document.createTextNode(srv));
          serverFiltersEl.appendChild(label);
        });
      }

      document.querySelectorAll('.scale-btn').forEach((btn) => {
        btn.addEventListener('click', () => {
          state.scale = btn.dataset.scale || 'log';
          document.querySelectorAll('.scale-btn').forEach((b) => {
            b.setAttribute('aria-pressed', String(b === btn));
          });
          renderCharts();
        });
      });

      document.querySelectorAll('.phase-btn').forEach((btn) => {
        btn.addEventListener('click', () => {
          state.phase = btn.dataset.phase || 'both';
          document.querySelectorAll('.phase-btn').forEach((b) => {
            b.setAttribute('aria-pressed', String(b === btn));
          });
          renderCharts();
        });
      });

      const tierEl = document.getElementById('filter-tier');
      const shapeEl = document.getElementById('filter-shape');
      const opCatEl = document.getElementById('filter-op-cat');
      if (tierEl) tierEl.addEventListener('change', () => { state.tier = tierEl.value; applyFiltersAndRender(); });
      if (shapeEl) shapeEl.addEventListener('change', () => { state.shape = shapeEl.value; applyFiltersAndRender(); });
      if (opCatEl) opCatEl.addEventListener('change', () => { state.opCategory = opCatEl.value; applyFiltersAndRender(); });

      // Baseline comparison file upload
      const baselineInput = document.getElementById('baseline-input');
      const clearBaselineBtn = document.getElementById('clear-baseline');
      const baselineBanner = document.getElementById('baseline-banner');

      function indexBaseline(baseReport) {
        const byPair = new Map();
        const list = Array.isArray(baseReport?.results) ? baseReport.results : [];
        for (const r of list) {
          byPair.set(`${r.repo}\0${r.server}`, r);
        }
        return { raw: baseReport, byPair };
      }

      if (baselineInput) {
        baselineInput.addEventListener('change', async () => {
          const file = baselineInput.files && baselineInput.files[0];
          if (!file) return;
          try {
            const parsed = JSON.parse(await file.text());
            if (!parsed || !Array.isArray(parsed.results)) {
              throw new Error('Expected schema-v1 BenchReport JSON with a results array');
            }
            state.baseline = indexBaseline(parsed);
            if (clearBaselineBtn) clearBaselineBtn.hidden = false;
            if (baselineBanner) {
              baselineBanner.hidden = false;
              const rev = parsed.environment?.revision || file.name;
              const commonCount = activeScopeResults().length;
              baselineBanner.textContent = `Comparing intersection of ${commonCount} common repo×server pairs against baseline: ${rev}`;
            }
            applyFiltersAndRender();
          } catch (err) {
            if (baselineBanner) {
              baselineBanner.hidden = false;
              baselineBanner.textContent = `Failed to load baseline JSON: ${err.message}`;
            }
          }
        });
      }
      if (clearBaselineBtn) {
        clearBaselineBtn.addEventListener('click', () => {
          state.baseline = null;
          if (baselineInput) baselineInput.value = '';
          clearBaselineBtn.hidden = true;
          if (baselineBanner) baselineBanner.hidden = true;
          applyFiltersAndRender();
        });
      }

      function activeScopeResults() {
        if (!state.baseline) return results;
        return results.filter((r) => state.baseline.byPair.has(`${r.repo}\0${r.server}`));
      }

      const CANONICAL_TIERS = ['small', 'medium', 'large'];
      const CANONICAL_SHAPES = ['plain', 'maven', 'gradle', 'ant', 'single-module', 'multi-module'];
      const CANONICAL_CATS = [
        'Navigation & Resolution',
        'Type Hierarchies',
        'Cross-References & Refactoring',
        'Autocompletion',
        'Incremental Editing & Diagnostics',
        'Presentation & Formatting'
      ];

      function rebuildSelectOptions(selectEl, containerEl, allLabel, values, stateKey) {
        if (!selectEl) return;
        const prev = state[stateKey];
        selectEl.textContent = '';
        const allOpt = document.createElement('option');
        allOpt.value = 'all';
        allOpt.textContent = allLabel;
        selectEl.appendChild(allOpt);
        for (const v of values) {
          const opt = document.createElement('option');
          opt.value = v;
          opt.textContent = v;
          selectEl.appendChild(opt);
        }
        if (prev !== 'all' && values.includes(prev)) {
          selectEl.value = prev;
        } else {
          state[stateKey] = 'all';
          selectEl.value = 'all';
        }
        if (containerEl) containerEl.hidden = values.length <= 1;
      }

      function syncFilterOptionsToScope() {
        const scoped = activeScopeResults();
        const tierSet = new Set(scoped.map((r) => r.tier));
        const shapeSet = new Set();
        const catSet = new Set();
        const serverSet = new Set();
        for (const r of scoped) {
          serverSet.add(r.server);
          for (const s of r.shapes || []) shapeSet.add(s);
          const base = state.baseline ? state.baseline.byPair.get(`${r.repo}\0${r.server}`) : null;
          for (const [opId, st] of Object.entries(r.ops || {})) {
            if (!st || st.status !== 'supported') continue;
            if (base) {
              const baseOp = base.ops?.[opId];
              if (!baseOp || baseOp.status !== 'supported') continue;
            }
            catSet.add(OP_CATEGORIES[opId] || 'Other');
          }
        }
        rebuildSelectOptions(
          tierEl,
          document.getElementById('control-tier'),
          state.baseline ? 'All common tiers' : 'All executed tiers',
          CANONICAL_TIERS.filter((t) => tierSet.has(t)),
          'tier'
        );
        rebuildSelectOptions(
          shapeEl,
          document.getElementById('control-shape'),
          state.baseline ? 'All common shapes' : 'All executed shapes',
          CANONICAL_SHAPES.filter((s) => shapeSet.has(s)),
          'shape'
        );
        rebuildSelectOptions(
          opCatEl,
          document.getElementById('control-op-cat'),
          state.baseline ? 'All common categories' : 'All executed categories',
          CANONICAL_CATS.filter((c) => catSet.has(c)),
          'opCategory'
        );
        if (serverFiltersEl) {
          serverFiltersEl.querySelectorAll('label.chip-label').forEach((lbl) => {
            const inp = lbl.querySelector('input[type="checkbox"]');
            if (inp) lbl.hidden = !serverSet.has(inp.value);
          });
        }
      }

      function matchesRow(dataset, checkCategory) {
        if (state.baseline && !state.baseline.byPair.has(`${dataset.repo}\0${dataset.server}`)) {
          return false;
        }
        if (state.tier !== 'all' && dataset.tier !== state.tier) return false;
        if (state.shape !== 'all') {
          const shapes = (dataset.shapes || '').split('+');
          if (!shapes.includes(state.shape)) return false;
        }
        if (!state.servers.has(dataset.server)) return false;
        if (checkCategory && state.opCategory !== 'all' && dataset.category !== state.opCategory) return false;
        return true;
      }

      function computeDeltaPct(current, baseline) {
        if (!Number.isFinite(current) || !Number.isFinite(baseline) || baseline <= 0) return null;
        return ((current - baseline) / baseline) * 100;
      }

      function updateTableDeltas() {
        document.querySelectorAll('.delta-slot').forEach((slot) => { slot.textContent = ''; });
        if (!state.baseline) return;

        document.querySelectorAll('#lifecycle-table tbody tr').forEach((tr) => {
          const base = state.baseline.byPair.get(`${tr.dataset.repo}\0${tr.dataset.server}`);
          if (!base) return;
          tr.querySelectorAll('td[data-metric]').forEach((td) => {
            const metric = td.dataset.metric;
            const cur = Number(td.dataset.value);
            const prev = Number(base.lifecycle?.[metric]);
            renderDeltaBadge(td.querySelector('.delta-slot'), computeDeltaPct(cur, prev));
          });
        });

        document.querySelectorAll('#ops-table tbody tr').forEach((tr) => {
          const base = state.baseline.byPair.get(`${tr.dataset.repo}\0${tr.dataset.server}`);
          const baseOp = base?.ops?.[tr.dataset.op];
          if (!baseOp || baseOp.status !== 'supported') return;
          tr.querySelectorAll('td[data-metric]').forEach((td) => {
            const metric = td.dataset.metric;
            const cur = Number(td.dataset.value);
            const prev = Number(baseOp[metric]);
            renderDeltaBadge(td.querySelector('.delta-slot'), computeDeltaPct(cur, prev));
          });
        });
      }

      function renderDeltaBadge(slot, delta) {
        if (!slot || delta === null || !Number.isFinite(delta)) return;
        const badge = document.createElement('span');
        const cls = delta <= -1 ? 'delta-good' : delta >= 1 ? 'delta-bad' : 'delta-flat';
        badge.className = `delta-badge ${cls}`;
        const sign = delta > 0 ? '+' : '';
        badge.textContent = `${sign}${delta.toFixed(1)}%`;
        slot.appendChild(badge);
      }

      function filteredResults() {
        return activeScopeResults().filter((r) => {
          if (state.tier !== 'all' && r.tier !== state.tier) return false;
          if (state.shape !== 'all' && !(r.shapes || []).includes(state.shape)) return false;
          if (!state.servers.has(r.server)) return false;
          return true;
        });
      }

      function applyFiltersAndRender() {
        syncFilterOptionsToScope();
        document.querySelectorAll('#lifecycle-table tbody tr').forEach((tr) => {
          tr.hidden = !matchesRow(tr.dataset, false);
        });
        document.querySelectorAll('#ops-table tbody tr').forEach((tr) => {
          tr.hidden = !matchesRow(tr.dataset, true);
        });
        const scopedServers = new Set(activeScopeResults().map((r) => r.server));
        document.querySelectorAll('#caps-table tbody tr').forEach((tr) => {
          tr.hidden = !scopedServers.has(tr.dataset.server) || !state.servers.has(tr.dataset.server);
        });
        updateTableDeltas();
        renderCharts();
      }

      function scaleConfig() {
        return state.scale === 'log'
          ? { type: 'log', base: 10, nice: true }
          : { type: 'linear', zero: true, nice: true };
      }

      function posVal(v) {
        if (v === null || v === undefined || !Number.isFinite(v)) return null;
        return state.scale === 'log' ? Math.max(v, 0.01) : v;
      }

      function renderCharts() {
        const offlineNote = document.getElementById('vega-offline-note');
        if (typeof window.vegaEmbed !== 'function') {
          if (offlineNote) offlineNote.hidden = false;
          return;
        }
        if (offlineNote) offlineNote.hidden = true;

        const active = filteredResults();
        const dark = isResolvedDark();
        const embedOpts = {
          theme: dark ? 'dark' : undefined,
          renderer: 'svg',
          actions: { export: true, source: false, compiled: false, editor: false }
        };

        const phases = state.phase === 'both' ? ['Cold', 'Warm'] : [state.phase === 'cold' ? 'Cold' : 'Warm'];

        // 1. First Response
        const firstRows = [];
        const readyRows = [];
        const memRows = [];
        const opsRows = [];

        for (const r of active) {
          const base = state.baseline ? state.baseline.byPair.get(`${r.repo}\0${r.server}`) : null;
          for (const ph of phases) {
            const firstKey = ph === 'Cold' ? 'cold_first_response_ms' : 'warm_first_response_ms';
            const readyKey = ph === 'Cold' ? 'cold_ready_ms' : 'warm_ready_ms';

            const firstRaw = r.lifecycle?.[firstKey];
            const firstPlot = posVal(firstRaw);
            if (firstPlot !== null) {
              const baseVal = base?.lifecycle?.[firstKey];
              const d = computeDeltaPct(firstRaw, baseVal);
              firstRows.push({
                repo: r.repo,
                server: r.server,
                phase: ph,
                series: `${r.server} (${ph})`,
                value: firstPlot,
                raw_ms: Number(firstRaw.toFixed(2)),
                delta_pct: d === null ? '—' : `${d > 0 ? '+' : ''}${d.toFixed(1)}%`
              });
            }

            const readyRaw = r.lifecycle?.[readyKey];
            const readyPlot = posVal(readyRaw);
            if (readyPlot !== null) {
              const baseVal = base?.lifecycle?.[readyKey];
              const d = computeDeltaPct(readyRaw, baseVal);
              readyRows.push({
                repo: r.repo,
                server: r.server,
                phase: ph,
                series: `${r.server} (${ph})`,
                value: readyPlot,
                raw_ms: Number(readyRaw.toFixed(2)),
                delta_pct: d === null ? '—' : `${d > 0 ? '+' : ''}${d.toFixed(1)}%`
              });
            }
          }

          for (const [label, key] of [['Steady RSS', 'steady_rss_mb'], ['Peak RSS', 'peak_rss_mb']]) {
            const mb = r.lifecycle?.[key];
            if (Number.isFinite(mb)) {
              const d = computeDeltaPct(mb, base?.lifecycle?.[key]);
              memRows.push({
                repo: r.repo,
                server: r.server,
                metric: label,
                series: `${r.server} (${label})`,
                mb: Number(mb.toFixed(2)),
                delta_pct: d === null ? '—' : `${d > 0 ? '+' : ''}${d.toFixed(1)}%`
              });
            }
          }

          for (const [opId, st] of Object.entries(r.ops || {})) {
            if (!st || st.status !== 'supported') continue;
            const cat = OP_CATEGORIES[opId] || 'Other';
            if (state.opCategory !== 'all' && cat !== state.opCategory) continue;
            const p50Plot = posVal(st.p50_ms);
            if (p50Plot === null) continue;
            const baseOp = base?.ops?.[opId];
            if (base && (!baseOp || baseOp.status !== 'supported')) continue;
            const d = baseOp && baseOp.status === 'supported' ? computeDeltaPct(st.p50_ms, baseOp.p50_ms) : null;
            opsRows.push({
              repo: r.repo,
              server: r.server,
              op: opId,
              category: cat,
              p50_plot: p50Plot,
              first_ms: st.first_ms === null ? null : Number(st.first_ms.toFixed(2)),
              p50_ms: Number(st.p50_ms.toFixed(3)),
              p95_ms: st.p95_ms === null ? null : Number(st.p95_ms.toFixed(3)),
              max_ms: st.max_ms === null ? null : Number(st.max_ms.toFixed(3)),
              err_pct: Number((st.error_rate_pct || 0).toFixed(1)),
              hit_pct: Number((st.hit_rate_pct || 0).toFixed(1)),
              delta_pct: d === null ? '—' : `${d > 0 ? '+' : ''}${d.toFixed(1)}%`
            });
          }
        }

        const firstSpec = {
          $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
          background: 'transparent',
          width: 'container',
          height: 240,
          data: { values: firstRows },
          mark: { type: 'bar', tooltip: true, cornerRadiusTopLeft: 3, cornerRadiusTopRight: 3 },
          encoding: {
            x: { field: 'repo', type: 'nominal', title: 'Repository' },
            xOffset: { field: 'series', type: 'nominal' },
            y: { field: 'value', type: 'quantitative', title: 'First Response (ms)', scale: scaleConfig() },
            color: { field: 'server', type: 'nominal', title: 'Server' },
            opacity: { field: 'phase', type: 'nominal', scale: { domain: ['Cold', 'Warm'], range: [1.0, 0.55] } },
            tooltip: [
              { field: 'repo', title: 'Repo' },
              { field: 'server', title: 'Server' },
              { field: 'phase', title: 'Phase' },
              { field: 'raw_ms', title: 'First Resp (ms)' },
              { field: 'delta_pct', title: 'vs Baseline' }
            ]
          }
        };

        const readySpec = {
          $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
          background: 'transparent',
          width: 'container',
          height: 240,
          data: { values: readyRows },
          mark: { type: 'bar', tooltip: true, cornerRadiusTopLeft: 3, cornerRadiusTopRight: 3 },
          encoding: {
            x: { field: 'repo', type: 'nominal', title: 'Repository' },
            xOffset: { field: 'series', type: 'nominal' },
            y: { field: 'value', type: 'quantitative', title: 'Ready Time (ms)', scale: scaleConfig() },
            color: { field: 'server', type: 'nominal', title: 'Server' },
            opacity: { field: 'phase', type: 'nominal', scale: { domain: ['Cold', 'Warm'], range: [1.0, 0.55] } },
            tooltip: [
              { field: 'repo', title: 'Repo' },
              { field: 'server', title: 'Server' },
              { field: 'phase', title: 'Phase' },
              { field: 'raw_ms', title: 'Ready (ms)' },
              { field: 'delta_pct', title: 'vs Baseline' }
            ]
          }
        };

        const memSpec = {
          $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
          background: 'transparent',
          width: 'container',
          height: 240,
          data: { values: memRows },
          mark: { type: 'bar', tooltip: true, cornerRadiusTopLeft: 3, cornerRadiusTopRight: 3 },
          encoding: {
            x: { field: 'repo', type: 'nominal', title: 'Repository' },
            xOffset: { field: 'series', type: 'nominal' },
            y: { field: 'mb', type: 'quantitative', title: 'RSS (MB)', scale: { zero: true } },
            color: { field: 'server', type: 'nominal', title: 'Server' },
            opacity: { field: 'metric', type: 'nominal', scale: { domain: ['Steady RSS', 'Peak RSS'], range: [1.0, 0.5] } },
            tooltip: [
              { field: 'repo', title: 'Repo' },
              { field: 'server', title: 'Server' },
              { field: 'metric', title: 'Metric' },
              { field: 'mb', title: 'RSS (MB)' },
              { field: 'delta_pct', title: 'vs Baseline' }
            ]
          }
        };

        const opsSpec = {
          $schema: 'https://vega.github.io/schema/vega-lite/v5.json',
          background: 'transparent',
          data: { values: opsRows },
          facet: { row: { field: 'repo', type: 'nominal', title: 'Repository' } },
          spec: {
            width: { step: 34 },
            height: 190,
            mark: { type: 'bar', tooltip: true, cornerRadiusTopLeft: 3, cornerRadiusTopRight: 3 },
            encoding: {
              x: { field: 'op', type: 'nominal', title: 'Operation', axis: { labelAngle: -35 } },
              xOffset: { field: 'server', type: 'nominal' },
              y: { field: 'p50_plot', type: 'quantitative', title: 'p50 (ms)', scale: scaleConfig() },
              color: { field: 'server', type: 'nominal', title: 'Server' },
              tooltip: [
                { field: 'repo', title: 'Repo' },
                { field: 'server', title: 'Server' },
                { field: 'op', title: 'Operation' },
                { field: 'first_ms', title: '1st (ms)' },
                { field: 'p50_ms', title: 'p50 (ms)' },
                { field: 'p95_ms', title: 'p95 (ms)' },
                { field: 'max_ms', title: 'max (ms)' },
                { field: 'err_pct', title: 'Err%' },
                { field: 'hit_pct', title: 'Hit%' },
                { field: 'delta_pct', title: 'p50 vs Baseline' }
              ]
            }
          }
        };

        const embedSafe = (id, spec) => {
          const host = document.getElementById(id);
          if (!host) return;
          const card = host.closest('.chart-card');
          if (!spec.data.values.length) {
            if (card) card.hidden = true;
            host.textContent = '';
            return;
          }
          if (card) card.hidden = false;
          window.vegaEmbed(host, spec, embedOpts).catch(() => {
            if (offlineNote) offlineNote.hidden = false;
          });
        };

        embedSafe('vl-first-response', firstSpec);
        embedSafe('vl-ready-time', readySpec);
        embedSafe('vl-memory', memSpec);
        embedSafe('vl-ops', opsSpec);
      }

      // Sortable tables (Invoker Commands API with feature-detected fallback)
      const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

      function sortTable(table, th) {
        const headerRow = th.parentElement;
        if (!headerRow) return;
        const colIdx = Array.from(headerRow.children).indexOf(th);
        if (colIdx < 0) return;
        const colType = th.dataset.type || 'text';
        const current = th.getAttribute('aria-sort');
        const nextDir = current === 'ascending' ? 'descending' : 'ascending';

        for (const other of headerRow.children) {
          if (other !== th) other.removeAttribute('aria-sort');
        }
        th.setAttribute('aria-sort', nextDir);

        const tbody = table.tBodies[0];
        if (!tbody) return;
        const rows = Array.from(tbody.rows);
        rows.sort((a, b) => {
          const cellA = a.cells[colIdx];
          const cellB = b.cells[colIdx];
          if (colType === 'number') {
            const rawA = cellA && cellA.dataset.value !== undefined ? cellA.dataset.value : '';
            const rawB = cellB && cellB.dataset.value !== undefined ? cellB.dataset.value : '';
            const numA = rawA === '' ? NaN : Number(rawA);
            const numB = rawB === '' ? NaN : Number(rawB);
            const validA = Number.isFinite(numA);
            const validB = Number.isFinite(numB);
            if (!validA && !validB) return 0;
            if (!validA) return 1;
            if (!validB) return -1;
            const diff = numA - numB;
            return nextDir === 'ascending' ? diff : -diff;
          }
          const textA = (cellA ? cellA.textContent : '').trim();
          const textB = (cellB ? cellB.textContent : '').trim();
          const cmp = collator.compare(textA, textB);
          return nextDir === 'ascending' ? cmp : -cmp;
        });

        for (const row of rows) {
          if (tbody.moveBefore) tbody.moveBefore(row, null);
          else tbody.appendChild(row);
        }

        const statusEl = document.getElementById('sort-status');
        if (statusEl) {
          statusEl.textContent = `Sorted by ${th.textContent.trim()}, ${nextDir}`;
        }
      }

      document.querySelectorAll('table').forEach((table) => {
        table.addEventListener('command', (e) => {
          if (e.command === '--sort' && e.source) {
            const th = e.source.closest('th');
            if (th) sortTable(table, th);
          }
        });
      });

      if (!('commandForElement' in HTMLButtonElement.prototype)) {
        document.addEventListener('click', (e) => {
          const btn = e.target.closest('button[command="--sort"]');
          if (!btn) return;
          const targetId = btn.getAttribute('commandfor');
          const table = targetId ? document.getElementById(targetId) : btn.closest('table');
          const th = btn.closest('th');
          if (table && th) sortTable(table, th);
        });
      }

      applyTheme(getSavedTheme(), false, false);
      applyFiltersAndRender();
    })();
