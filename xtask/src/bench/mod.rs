//! Pure-Rust Java LSP benchmark harness (`cargo xtask bench`, Spec 0018).

pub mod client;
pub mod probes;
pub mod report;
pub mod rss;
pub mod synth;

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde_json::{Value, json};

use crate::report::current_utc_date;

use self::{
    client::{LspClient, ServerCaps, path_to_file_uri},
    probes::{
        BenchManifest, CompletionProbeSpec, EditProbeSpec, OpCategory, ProbeSpec, RepoSpec,
        Shape, Tier, resolve_anchor_utf16, resolve_cursor_after_utf16,
    },
    report::{
        BenchReport, EnvironmentInfo, LifecycleStats, OpStats, REPORT_SCHEMA_VERSION,
        RepoServerResult, format_markdown, format_stdout_tables,
    },
    rss::RssSampler,
    synth::generate_plain_small,
};

/// Server definition parsed from `--server <NAME=CMD>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerSpec {
    pub name: String,
    pub cmd: String,
}

impl ServerSpec {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let Some((name, cmd)) = spec.split_once('=') else {
            return Err(format!(
                "--server expects `<NAME=CMD>`, got `{spec}`"
            ));
        };
        let name = name.trim();
        let cmd = cmd.trim();
        if name.is_empty() || cmd.is_empty() {
            return Err(format!(
                "--server expects non-empty name and command, got `{spec}`"
            ));
        }
        Ok(Self {
            name: name.to_string(),
            cmd: cmd.to_string(),
        })
    }
}

impl std::str::FromStr for ServerSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Parsed CLI options for `cargo xtask bench`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchArgs {
    pub servers: Vec<ServerSpec>,
    pub repos: Vec<(String, PathBuf)>,
    pub tiers: Vec<Tier>,
    pub shapes: Vec<Shape>,
    pub ops: Vec<OpCategory>,
    pub probes: Option<PathBuf>,
    pub warmup: usize,
    pub iterations: usize,
    pub ready_timeout_secs: u64,
    pub request_timeout_secs: u64,
    pub skip_warm_restart: bool,
    pub revision: Option<String>,
    pub json_out: PathBuf,
    pub md_out: PathBuf,
}

impl Default for BenchArgs {
    fn default() -> Self {
        Self {
            servers: Vec::new(),
            repos: Vec::new(),
            tiers: Vec::new(),
            shapes: Vec::new(),
            ops: Vec::new(),
            probes: None,
            warmup: 5,
            iterations: 15,
            ready_timeout_secs: 180,
            request_timeout_secs: 30,
            skip_warm_restart: false,
            revision: None,
            json_out: PathBuf::from("target/bench/results.json"),
            md_out: PathBuf::from("target/bench/BENCHMARKS.md"),
        }
    }
}

/// Parsed CLI options for `cargo xtask bench-gen-plain`.
#[derive(Debug, Clone, PartialEq, Eq, usage::Args)]
#[usage(crate = usage)]
pub struct GenPlainArgs {
    /// Destination directory
    #[usage(long, value_name = "DIR", default = "target/bench/corpus/plain-small")]
    pub out: PathBuf,
}

impl Default for GenPlainArgs {
    fn default() -> Self {
        Self {
            out: PathBuf::from("target/bench/corpus/plain-small"),
        }
    }
}

/// Parses `--repo <ID=PATH>`.
pub fn parse_repo_arg(spec: &str) -> Result<(String, PathBuf), String> {
    let Some((id, path)) = spec.split_once('=') else {
        return Err(format!("--repo expects `<ID=PATH>`, got `{spec}`"));
    };
    let id = id.trim();
    let path = path.trim();
    if id.is_empty() || path.is_empty() {
        return Err(format!(
            "--repo expects non-empty id and path, got `{spec}`"
        ));
    }
    Ok((id.to_string(), PathBuf::from(path)))
}

/// Runs `cargo xtask bench-gen-plain` and returns the exit code.
pub fn run_gen_plain(args: &GenPlainArgs) -> i32 {
    match generate_plain_small(&args.out) {
        Ok(summary) => {
            println!(
                "generated plain-small: {} files ({} lines) in {}",
                summary.total_files,
                summary.total_lines,
                summary.root.display()
            );
            0
        }
        Err(e) => {
            eprintln!("error: failed to generate plain-small: {e}");
            1
        }
    }
}

/// Runs `cargo xtask bench` and returns the exit code.
pub fn run(args: &BenchArgs) -> i32 {
    match run_inner(args) {
        Ok(report) => {
            let stdout_tables = format_stdout_tables(&report);
            print!("{stdout_tables}");
            println!(
                "wrote JSON report to {} and Markdown report to {}",
                args.json_out.display(),
                args.md_out.display()
            );
            0
        }
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}

fn run_inner(args: &BenchArgs) -> Result<BenchReport, String> {
    let manifest = match &args.probes {
        Some(path) => {
            let text = fs::read_to_string(path)
                .map_err(|e| format!("failed to read probes {}: {e}", path.display()))?;
            BenchManifest::from_toml_str(&text)?
        }
        None => BenchManifest::embedded()?,
    };

    let servers = resolve_servers(args)?;
    let repos = resolve_repos(args, &manifest)?;
    if repos.is_empty() {
        return Err("no benchmark repositories matched the selected filters".to_string());
    }

    let active_categories: BTreeSet<OpCategory> = if args.ops.is_empty() {
        OpCategory::ALL.into_iter().collect()
    } else {
        args.ops.iter().copied().collect()
    };

    let mut results = Vec::new();
    for (repo_spec, repo_dir) in &repos {
        let file_count = count_java_files(repo_dir);
        for server in &servers {
            eprintln!(
                "[bench] running `{}` on `{}` ({} files)...",
                server.name, repo_spec.id, file_count
            );
            let res = run_single_pair(
                args,
                repo_spec,
                repo_dir,
                file_count,
                server,
                &active_categories,
            )?;
            results.push(res);
        }
    }

    let report = BenchReport {
        schema_version: REPORT_SCHEMA_VERSION,
        environment: collect_environment(&servers, args.revision.as_deref()),
        servers: servers.iter().map(|s| s.name.clone()).collect(),
        results,
    };

    write_report_files(&report, &args.json_out, &args.md_out)?;
    Ok(report)
}

fn resolve_servers(args: &BenchArgs) -> Result<Vec<ServerSpec>, String> {
    if !args.servers.is_empty() {
        return Ok(args.servers.clone());
    }
    let release_bin = Path::new("target/release/java-analyzer");
    if release_bin.is_file() {
        return Ok(vec![ServerSpec {
            name: "java-analyzer".to_string(),
            cmd: format!("{} lsp", release_bin.display()),
        }]);
    }
    let debug_bin = Path::new("target/debug/java-analyzer");
    if debug_bin.is_file() {
        return Ok(vec![ServerSpec {
            name: "java-analyzer".to_string(),
            cmd: format!("{} lsp", debug_bin.display()),
        }]);
    }
    Err("no --server provided and neither target/release/java-analyzer nor target/debug/java-analyzer exists".to_string())
}

fn resolve_repos(
    args: &BenchArgs,
    manifest: &BenchManifest,
) -> Result<Vec<(RepoSpec, PathBuf)>, String> {
    let by_id: BTreeMap<&str, &RepoSpec> = manifest
        .repos
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect();

    if !args.repos.is_empty() {
        let mut out = Vec::with_capacity(args.repos.len());
        for (id, path) in &args.repos {
            let spec = match by_id.get(id.as_str()) {
                Some(&existing) => existing.clone(),
                None => RepoSpec {
                    id: id.clone(),
                    tier: Tier::Small,
                    shapes: vec![Shape::Plain],
                    url: String::new(),
                    approx_files: 0,
                    ready_probe: None,
                    clean_files: Vec::new(),
                    probes: BTreeMap::new(),
                    edits: BTreeMap::new(),
                    completions: BTreeMap::new(),
                },
            };
            if !args.tiers.is_empty() && !args.tiers.contains(&spec.tier) {
                continue;
            }
            if !args.shapes.is_empty()
                && !args.shapes.iter().any(|wanted| spec.shapes.contains(wanted))
            {
                continue;
            }
            if !path.is_dir() {
                return Err(format!(
                    "repository `{id}` path does not exist: {}",
                    path.display()
                ));
            }
            out.push((spec, path.clone()));
        }
        return Ok(out);
    }

    let mut out = Vec::new();
    for spec in &manifest.repos {
        if !args.tiers.is_empty() && !args.tiers.contains(&spec.tier) {
            continue;
        }
        if !args.shapes.is_empty()
            && !args.shapes.iter().any(|wanted| spec.shapes.contains(wanted))
        {
            continue;
        }

        let dir = PathBuf::from("target/bench/corpus").join(&spec.id);
        if spec.id == "plain-small" {
            if !dir.is_dir() || count_java_files(&dir) == 0 {
                generate_plain_small(&dir).map_err(|e| {
                    format!("failed to generate plain-small at {}: {e}", dir.display())
                })?;
            }
            out.push((spec.clone(), dir));
        } else if dir.is_dir() {
            out.push((spec.clone(), dir));
        } else {
            eprintln!(
                "[bench] skipping `{}` (not found at {}; run `mise run bench-fetch` or pass `--repo`)",
                spec.id,
                dir.display()
            );
        }
    }
    Ok(out)
}

struct CacheDirGuard {
    path: PathBuf,
}

impl CacheDirGuard {
    fn create_fresh(repo_id: &str, server_name: &str) -> Result<Self, String> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let safe_server: String = server_name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .collect();
        let path = std::env::temp_dir().join(format!(
            "java-analyzer-bench-{repo_id}-{safe_server}-{}-{stamp}",
            std::process::id()
        ));
        if path.exists() {
            let _ = fs::remove_dir_all(&path);
        }
        fs::create_dir_all(&path)
            .map_err(|e| format!("failed to create cache dir {}: {e}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for CacheDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run_single_pair(
    args: &BenchArgs,
    repo: &RepoSpec,
    repo_dir: &Path,
    file_count: usize,
    server: &ServerSpec,
    active_categories: &BTreeSet<OpCategory>,
) -> Result<RepoServerResult, String> {
    let cache_guard = CacheDirGuard::create_fresh(&repo.id, &server.name)?;
    let ready_timeout = Duration::from_secs(args.ready_timeout_secs);
    let request_timeout = Duration::from_secs(args.request_timeout_secs);

    let file_cache = load_probe_files(repo, repo_dir);
    let initial_probe = pick_initial_probe_file(repo, repo_dir, &file_cache);

    // Phase 1: Cold Start & Cold First Response
    let cold_spawn = Instant::now();
    let mut client = LspClient::spawn(&server.cmd, repo_dir, &cache_guard.path)?;
    let rss_sampler = RssSampler::start(client.pid());

    let caps = client.initialize(repo_dir, ready_timeout)?;
    let cold_init_ms = cold_spawn.elapsed().as_secs_f64() * 1000.0;

    let cold_first_response_ms =
        measure_first_response(&mut client, &caps, initial_probe.as_ref(), cold_spawn, request_timeout);

    let cold_ready_ms =
        wait_until_ready(&mut client, &caps, repo, repo_dir, &file_cache, cold_spawn, ready_timeout, request_timeout);
    eprintln!(
        "[bench]   cold: init={:.0}ms, 1st={}, ready={}",
        cold_init_ms,
        cold_first_response_ms.map_or_else(|| "N/A".to_string(), |v| format!("{v:.0}ms")),
        cold_ready_ms.map_or_else(|| "timeout".to_string(), |v| format!("{v:.0}ms")),
    );

    // Open all clean & probe files so diagnostics and operations have live buffers.
    let mut open_uris = BTreeMap::new();
    for (rel, text) in &file_cache {
        let full = repo_dir.join(rel);
        if let Ok(uri) = client.ensure_open(&full, text) {
            open_uris.insert(rel.clone(), uri);
        }
    }

    // Build ordered execution plan of interactive operations.
    let plan = build_op_plan(repo, &caps, active_categories);
    let mut first_ms_map: BTreeMap<String, f64> = BTreeMap::new();
    let mut op_acc: BTreeMap<String, OpAccumulator> = BTreeMap::new();

    // Phase 2: Unwarmed First-Call capture (pass 1) + Warmup passes.
    for op in &plan {
        if !op.supported {
            continue;
        }
        let outcome = execute_op(&mut client, &caps, repo_dir, &file_cache, &open_uris, op, request_timeout);
        if outcome.error {
            // Still record unwarmed timing if desired, or only record elapsed.
            first_ms_map.insert(op.id.clone(), outcome.elapsed_ms);
        } else {
            first_ms_map.insert(op.id.clone(), outcome.elapsed_ms);
        }
    }

    for _ in 0..args.warmup {
        for op in &plan {
            if !op.supported {
                continue;
            }
            let _ = execute_op(&mut client, &caps, repo_dir, &file_cache, &open_uris, op, request_timeout);
        }
    }

    // Phase 3: Measured steady-state runs.
    rss_sampler.mark_steady_start();
    for _ in 0..args.iterations {
        for op in &plan {
            if !op.supported {
                continue;
            }
            let outcome = execute_op(&mut client, &caps, repo_dir, &file_cache, &open_uris, op, request_timeout);
            let acc = op_acc.entry(op.id.clone()).or_default();
            acc.total_requests += 1;
            if outcome.error {
                acc.errors += 1;
            } else {
                acc.samples_ms.push(outcome.elapsed_ms);
                if outcome.hit {
                    acc.hits += 1;
                }
            }
        }
    }

    // Count false-positive ERROR diagnostics on untouched clean files.
    let clean_file_errors = count_clean_file_errors(&client, repo, &open_uris);

    let rss_summary = rss_sampler.finish();
    client.shutdown(request_timeout);

    // Phase 4: Warm Restart reusing the same cache directory.
    let (warm_init_ms, warm_first_response_ms, warm_ready_ms) = if args.skip_warm_restart {
        (None, None, None)
    } else {
        let warm_spawn = Instant::now();
        let mut warm_client = LspClient::spawn(&server.cmd, repo_dir, &cache_guard.path)?;
        let warm_caps = warm_client.initialize(repo_dir, ready_timeout)?;
        let w_init = warm_spawn.elapsed().as_secs_f64() * 1000.0;
        let w_first = measure_first_response(
            &mut warm_client,
            &warm_caps,
            initial_probe.as_ref(),
            warm_spawn,
            request_timeout,
        );
        let w_ready = wait_until_ready(
            &mut warm_client,
            &warm_caps,
            repo,
            repo_dir,
            &file_cache,
            warm_spawn,
            ready_timeout,
            request_timeout,
        );
        warm_client.shutdown(request_timeout);
        (Some(w_init), w_first, w_ready)
    };

    let mut ops = BTreeMap::new();
    let mut total_requests = 0usize;
    let mut total_errors = 0usize;
    for op in &plan {
        if !op.supported {
            ops.insert(op.id.clone(), OpStats::unsupported());
            continue;
        }
        let acc = op_acc.remove(&op.id).unwrap_or_default();
        total_requests += acc.total_requests;
        total_errors += acc.errors;
        let first = first_ms_map.get(&op.id).copied();
        ops.insert(
            op.id.clone(),
            OpStats::from_samples(
                first,
                &acc.samples_ms,
                acc.total_requests,
                acc.errors,
                acc.hits,
            ),
        );
    }

    Ok(RepoServerResult {
        repo: repo.id.clone(),
        tier: repo.tier,
        shapes: repo.shapes.clone(),
        files: file_count,
        server: server.name.clone(),
        capabilities: caps.to_coverage_map(),
        lifecycle: LifecycleStats {
            cold_init_ms,
            cold_first_response_ms,
            cold_ready_ms,
            warm_init_ms,
            warm_first_response_ms,
            warm_ready_ms,
            peak_rss_mb: rss_summary.peak_rss_mb,
            steady_rss_mb: rss_summary.steady_rss_mb,
            clean_file_errors,
        },
        total_requests,
        total_errors,
        ops,
    })
}

#[derive(Default)]
struct OpAccumulator {
    samples_ms: Vec<f64>,
    total_requests: usize,
    errors: usize,
    hits: usize,
}

struct OpOutcome {
    elapsed_ms: f64,
    error: bool,
    hit: bool,
}

#[derive(Clone)]
enum OpKind {
    Probe(ProbeSpec),
    Edit(EditProbeSpec),
    Completion(CompletionProbeSpec),
}

struct PlannedOp {
    id: String,
    supported: bool,
    kind: OpKind,
}

const ORDERED_OPS: [(&str, OpCategory); 33] = [
    ("hover_workspace", OpCategory::Nav),
    ("hover_jdk", OpCategory::Nav),
    ("def_workspace", OpCategory::Nav),
    ("def_jdk", OpCategory::Nav),
    ("type_def", OpCategory::Nav),
    ("doc_symbol_small", OpCategory::Nav),
    ("doc_symbol_large", OpCategory::Nav),
    ("workspace_symbol", OpCategory::Nav),
    ("impl_simple", OpCategory::Hierarchy),
    ("impl_deep", OpCategory::Hierarchy),
    ("impl_wide", OpCategory::Hierarchy),
    ("hierarchy_super", OpCategory::Hierarchy),
    ("hierarchy_sub", OpCategory::Hierarchy),
    ("refs_local", OpCategory::Refactor),
    ("refs_cross_file", OpCategory::Refactor),
    ("rename_local", OpCategory::Refactor),
    ("rename_private", OpCategory::Refactor),
    ("rename_public", OpCategory::Refactor),
    ("code_action_fix", OpCategory::Refactor),
    ("code_action_refactor", OpCategory::Refactor),
    ("completion_dot_workspace", OpCategory::Completion),
    ("completion_dot_jdk", OpCategory::Completion),
    ("completion_dot_generic", OpCategory::Completion),
    ("completion_scope", OpCategory::Completion),
    ("completion_auto_import", OpCategory::Completion),
    ("completion_override", OpCategory::Completion),
    ("completion_resolve", OpCategory::Completion),
    ("edit_body", OpCategory::Edit),
    ("edit_signature", OpCategory::Edit),
    ("signature_help", OpCategory::Edit),
    ("semantic_tokens", OpCategory::Present),
    ("inlay_hints", OpCategory::Present),
    ("formatting", OpCategory::Present),
];

fn op_supported_by_caps(op_id: &str, caps: &ServerCaps) -> bool {
    match op_id {
        "hover_workspace" | "hover_jdk" => caps.hover,
        "def_workspace" | "def_jdk" => caps.definition,
        "type_def" => caps.type_definition,
        "doc_symbol_small" | "doc_symbol_large" => caps.document_symbol,
        "workspace_symbol" => caps.workspace_symbol,
        "impl_simple" | "impl_deep" | "impl_wide" => caps.implementation,
        "hierarchy_super" | "hierarchy_sub" => caps.type_hierarchy,
        "refs_local" | "refs_cross_file" => caps.references,
        "rename_local" | "rename_private" | "rename_public" => caps.rename,
        "code_action_fix" | "code_action_refactor" => caps.code_action,
        "completion_dot_workspace"
        | "completion_dot_jdk"
        | "completion_dot_generic"
        | "completion_scope"
        | "completion_auto_import"
        | "completion_override" => caps.completion,
        "completion_resolve" => caps.completion && caps.completion_resolve,
        "edit_body" | "edit_signature" => caps.hover || caps.document_symbol,
        "signature_help" => caps.signature_help,
        "semantic_tokens" => caps.semantic_tokens,
        "inlay_hints" => caps.inlay_hints,
        "formatting" => caps.formatting,
        _ => false,
    }
}

fn build_op_plan(
    repo: &RepoSpec,
    caps: &ServerCaps,
    active_categories: &BTreeSet<OpCategory>,
) -> Vec<PlannedOp> {
    let mut plan = Vec::new();
    for (op_id, category) in ORDERED_OPS {
        if !active_categories.contains(&category) {
            continue;
        }
        let kind = if let Some(spec) = repo.probes.get(op_id) {
            OpKind::Probe(spec.clone())
        } else if let Some(spec) = repo.edits.get(op_id) {
            OpKind::Edit(spec.clone())
        } else if let Some(spec) = repo.completions.get(op_id) {
            OpKind::Completion(spec.clone())
        } else {
            continue;
        };
        plan.push(PlannedOp {
            id: op_id.to_string(),
            supported: op_supported_by_caps(op_id, caps),
            kind,
        });
    }
    plan
}

fn load_probe_files(repo: &RepoSpec, repo_dir: &Path) -> BTreeMap<String, String> {
    let mut rels = BTreeSet::new();
    for rel in &repo.clean_files {
        rels.insert(rel.clone());
    }
    for spec in repo.probes.values() {
        rels.insert(spec.file.clone());
    }
    for spec in repo.edits.values() {
        rels.insert(spec.file.clone());
    }
    for spec in repo.completions.values() {
        rels.insert(spec.file.clone());
    }

    let mut cache = BTreeMap::new();
    for rel in rels {
        let full = repo_dir.join(&rel);
        if let Ok(text) = fs::read_to_string(&full) {
            cache.insert(rel, text);
        }
    }
    cache
}

fn pick_initial_probe_file(
    repo: &RepoSpec,
    repo_dir: &Path,
    file_cache: &BTreeMap<String, String>,
) -> Option<(PathBuf, String)> {
    if let Some(ready_id) = &repo.ready_probe
        && let Some(p) = repo.probes.get(ready_id)
        && let Some(text) = file_cache.get(&p.file)
    {
        return Some((repo_dir.join(&p.file), text.clone()));
    }
    if let Some(first_clean) = repo.clean_files.first()
        && let Some(text) = file_cache.get(first_clean)
    {
        return Some((repo_dir.join(first_clean), text.clone()));
    }
    file_cache
        .iter()
        .next()
        .map(|(rel, text)| (repo_dir.join(rel), text.clone()))
}

fn measure_first_response(
    client: &mut LspClient,
    caps: &ServerCaps,
    initial_probe: Option<&(PathBuf, String)>,
    spawn_instant: Instant,
    timeout: Duration,
) -> Option<f64> {
    let (path, text) = initial_probe?;
    let uri = client.ensure_open(path, text).ok()?;
    if caps.document_symbol {
        let resp = client.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
            timeout,
        );
        if let Ok(val) = resp
            && !is_empty_result(&val)
        {
            return Some(spawn_instant.elapsed().as_secs_f64() * 1000.0);
        }
    }
    if caps.hover {
        let resp = client.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": 0, "character": 0 }
            }),
            timeout,
        );
        if resp.is_ok() {
            return Some(spawn_instant.elapsed().as_secs_f64() * 1000.0);
        }
    }
    Some(spawn_instant.elapsed().as_secs_f64() * 1000.0)
}

#[allow(clippy::too_many_arguments)]
fn wait_until_ready(
    client: &mut LspClient,
    caps: &ServerCaps,
    repo: &RepoSpec,
    repo_dir: &Path,
    file_cache: &BTreeMap<String, String>,
    spawn_instant: Instant,
    ready_timeout: Duration,
    request_timeout: Duration,
) -> Option<f64> {
    client.wait_for_progress_settle(ready_timeout);
    let deadline = spawn_instant + ready_timeout;

    let primary_id = repo.ready_probe.as_deref().unwrap_or("hover_workspace");
    let mut candidate_ids: Vec<&str> = vec![primary_id];
    for fallback in ["def_workspace", "hover_workspace", "hover_jdk"] {
        if !candidate_ids.contains(&fallback) {
            candidate_ids.push(fallback);
        }
    }

    let mut open_uris = BTreeMap::new();
    let mut candidates = Vec::new();
    for id in candidate_ids {
        if !op_supported_by_caps(id, caps) {
            continue;
        }
        let Some(spec) = repo.probes.get(id) else {
            continue;
        };
        let Some(text) = file_cache.get(&spec.file) else {
            continue;
        };
        let full = repo_dir.join(&spec.file);
        if let Ok(uri) = client.ensure_open(&full, text) {
            open_uris.insert(spec.file.clone(), uri);
            candidates.push(PlannedOp {
                id: id.to_string(),
                supported: true,
                kind: OpKind::Probe(spec.clone()),
            });
        }
    }

    if candidates.is_empty() {
        return Some(spawn_instant.elapsed().as_secs_f64() * 1000.0);
    }

    const SETTLED_MISS_GRACE: Duration = Duration::from_secs(10);
    let mut settled_since: Option<Instant> = None;

    loop {
        let now = Instant::now();
        if now >= deadline {
            return None;
        }

        let mut all_non_error = true;
        for planned in &candidates {
            let cur = Instant::now();
            if cur >= deadline {
                return None;
            }
            let remaining = deadline.saturating_duration_since(cur).min(request_timeout);
            let outcome =
                execute_op(client, caps, repo_dir, file_cache, &open_uris, planned, remaining);
            if !outcome.error && outcome.hit {
                return Some(spawn_instant.elapsed().as_secs_f64() * 1000.0);
            }
            if outcome.error {
                all_non_error = false;
            }
        }

        if all_non_error && !client.has_active_progress() {
            let started = *settled_since.get_or_insert_with(Instant::now);
            if started.elapsed() >= SETTLED_MISS_GRACE {
                return None;
            }
        } else {
            settled_since = None;
        }

        if Instant::now() + Duration::from_millis(100) >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn execute_op(
    client: &mut LspClient,
    caps: &ServerCaps,
    repo_dir: &Path,
    file_cache: &BTreeMap<String, String>,
    open_uris: &BTreeMap<String, String>,
    op: &PlannedOp,
    timeout: Duration,
) -> OpOutcome {
    let start = Instant::now();
    match &op.kind {
        OpKind::Probe(spec) => {
            execute_probe(client, repo_dir, file_cache, open_uris, &op.id, spec, timeout, start)
        }
        OpKind::Edit(spec) => {
            execute_edit(client, caps, repo_dir, file_cache, open_uris, &op.id, spec, timeout, start)
        }
        OpKind::Completion(spec) => execute_completion(
            client,
            repo_dir,
            file_cache,
            open_uris,
            op.id == "completion_resolve",
            spec,
            timeout,
            start,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_probe(
    client: &mut LspClient,
    repo_dir: &Path,
    file_cache: &BTreeMap<String, String>,
    open_uris: &BTreeMap<String, String>,
    op_id: &str,
    spec: &ProbeSpec,
    timeout: Duration,
    start: Instant,
) -> OpOutcome {
    let Some(text) = file_cache.get(&spec.file) else {
        return OpOutcome {
            elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
            error: true,
            hit: false,
        };
    };
    let uri = match resolve_uri(repo_dir, open_uris, &spec.file) {
        Ok(u) => u,
        Err(_) => {
            return OpOutcome {
                elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                error: true,
                hit: false,
            };
        }
    };

    let expect = spec.expect.as_deref();
    match op_id {
        "hover_workspace" | "hover_jdk" => {
            let Ok((line, ch)) = resolve_anchor_utf16(text, &spec.anchor, &spec.symbol) else {
                return fail_outcome(start);
            };
            let res = client.request(
                "textDocument/hover",
                td_pos_params(&uri, line, ch),
                timeout,
            );
            eval_response(start, res, |v| !v.is_null() && value_matches_expect(v, expect))
        }
        "def_workspace" | "def_jdk" => {
            let Ok((line, ch)) = resolve_anchor_utf16(text, &spec.anchor, &spec.symbol) else {
                return fail_outcome(start);
            };
            let res = client.request(
                "textDocument/definition",
                td_pos_params(&uri, line, ch),
                timeout,
            );
            eval_response(start, res, |v| !is_empty_result(v) && value_matches_expect(v, expect))
        }
        "type_def" => {
            let Ok((line, ch)) = resolve_anchor_utf16(text, &spec.anchor, &spec.symbol) else {
                return fail_outcome(start);
            };
            let res = client.request(
                "textDocument/typeDefinition",
                td_pos_params(&uri, line, ch),
                timeout,
            );
            eval_response(start, res, |v| !is_empty_result(v) && value_matches_expect(v, expect))
        }
        "doc_symbol_small" | "doc_symbol_large" => {
            let res = client.request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": uri } }),
                timeout,
            );
            eval_response(start, res, |v| !is_empty_result(v) && value_matches_expect(v, expect))
        }
        "workspace_symbol" => {
            let query = spec.query.as_deref().unwrap_or("");
            let res = client.request("workspace/symbol", json!({ "query": query }), timeout);
            eval_response(start, res, |v| !is_empty_result(v) && value_matches_expect(v, expect))
        }
        "impl_simple" | "impl_deep" | "impl_wide" => {
            let Ok((line, ch)) = resolve_anchor_utf16(text, &spec.anchor, &spec.symbol) else {
                return fail_outcome(start);
            };
            let res = client.request(
                "textDocument/implementation",
                td_pos_params(&uri, line, ch),
                timeout,
            );
            eval_response(start, res, |v| !is_empty_result(v) && value_matches_expect(v, expect))
        }
        "hierarchy_super" | "hierarchy_sub" => {
            let Ok((line, ch)) = resolve_anchor_utf16(text, &spec.anchor, &spec.symbol) else {
                return fail_outcome(start);
            };
            let prep = client.request(
                "textDocument/prepareTypeHierarchy",
                td_pos_params(&uri, line, ch),
                timeout,
            );
            let items = match prep {
                Ok(v) => v,
                Err(_) => return fail_outcome(start),
            };
            let Some(first_item) = items.as_array().and_then(|arr| arr.first()).cloned() else {
                return OpOutcome {
                    elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                    error: false,
                    hit: false,
                };
            };
            let method = if op_id == "hierarchy_super" {
                "typeHierarchy/supertypes"
            } else {
                "typeHierarchy/subtypes"
            };
            let res = client.request(method, json!({ "item": first_item }), timeout);
            eval_response(start, res, |v| !is_empty_result(v) && value_matches_expect(v, expect))
        }
        "refs_local" | "refs_cross_file" => {
            let Ok((line, ch)) = resolve_anchor_utf16(text, &spec.anchor, &spec.symbol) else {
                return fail_outcome(start);
            };
            let res = client.request(
                "textDocument/references",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": ch },
                    "context": { "includeDeclaration": true }
                }),
                timeout,
            );
            eval_response(start, res, |v| !is_empty_result(v) && value_matches_expect(v, expect))
        }
        "rename_local" | "rename_private" | "rename_public" => {
            let Ok((line, ch)) = resolve_anchor_utf16(text, &spec.anchor, &spec.symbol) else {
                return fail_outcome(start);
            };
            let new_name = spec.new_name.as_deref().unwrap_or("renamedSymbol");
            // Dry-run: `prepareRename` (optional) + `rename` returning WorkspaceEdit without touching disk.
            let _ = client.request(
                "textDocument/prepareRename",
                td_pos_params(&uri, line, ch),
                timeout,
            );
            let res = client.request(
                "textDocument/rename",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": ch },
                    "newName": new_name
                }),
                timeout,
            );
            eval_response(start, res, has_workspace_edits)
        }
        "code_action_refactor" => {
            let Ok((line, ch)) = resolve_anchor_utf16(text, &spec.anchor, &spec.symbol) else {
                return fail_outcome(start);
            };
            let end_col = ch + u32::try_from(spec.symbol.encode_utf16().count()).unwrap_or(1);
            let res = client.request(
                "textDocument/codeAction",
                json!({
                    "textDocument": { "uri": uri },
                    "range": {
                        "start": { "line": line, "character": ch },
                        "end": { "line": line, "character": end_col }
                    },
                    "context": { "diagnostics": [] }
                }),
                timeout,
            );
            eval_response(start, res, |v| !is_empty_result(v) && value_matches_expect(v, expect))
        }
        "signature_help" => {
            let Ok((line, ch)) = resolve_anchor_utf16(text, &spec.anchor, &spec.symbol) else {
                return fail_outcome(start);
            };
            let res = client.request(
                "textDocument/signatureHelp",
                td_pos_params(&uri, line, ch),
                timeout,
            );
            eval_response(start, res, |v| {
                v.get("signatures")
                    .and_then(Value::as_array)
                    .is_some_and(|arr| !arr.is_empty())
            })
        }
        "semantic_tokens" => {
            let res = client.request(
                "textDocument/semanticTokens/full",
                json!({ "textDocument": { "uri": uri } }),
                timeout,
            );
            eval_response(start, res, |v| {
                v.get("data")
                    .and_then(Value::as_array)
                    .is_some_and(|arr| !arr.is_empty())
            })
        }
        "inlay_hints" => {
            let Ok((s_line, s_ch)) = resolve_anchor_utf16(text, &spec.anchor, &spec.symbol) else {
                return fail_outcome(start);
            };
            let (e_line, e_ch) = match (spec.end_anchor.as_deref(), spec.end_symbol.as_deref()) {
                (Some(ea), es) => {
                    resolve_anchor_utf16(text, ea, es.unwrap_or("")).unwrap_or((s_line + 1, 0))
                }
                _ => (s_line + 1, 0),
            };
            let res = client.request(
                "textDocument/inlayHint",
                json!({
                    "textDocument": { "uri": uri },
                    "range": {
                        "start": { "line": s_line, "character": s_ch },
                        "end": { "line": e_line, "character": e_ch }
                    }
                }),
                timeout,
            );
            eval_response(start, res, |v| !is_empty_result(v))
        }
        "formatting" => {
            let res = client.request(
                "textDocument/formatting",
                json!({
                    "textDocument": { "uri": uri },
                    "options": { "tabSize": 4, "insertSpaces": true }
                }),
                timeout,
            );
            eval_response(start, res, |v| !v.is_null())
        }
        _ => fail_outcome(start),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_edit(
    client: &mut LspClient,
    caps: &ServerCaps,
    repo_dir: &Path,
    file_cache: &BTreeMap<String, String>,
    open_uris: &BTreeMap<String, String>,
    op_id: &str,
    spec: &EditProbeSpec,
    timeout: Duration,
    start: Instant,
) -> OpOutcome {
    let Some(orig_text) = file_cache.get(&spec.file) else {
        return fail_outcome(start);
    };
    let Ok(uri) = resolve_uri(repo_dir, open_uris, &spec.file) else {
        return fail_outcome(start);
    };
    let Some(edited_text) = replace_anchored_once(orig_text, &spec.anchor, &spec.replace, &spec.with)
    else {
        return fail_outcome(start);
    };

    let prev_seq = client.diagnostics_seq();
    if client.change_file(&uri, orig_text, &edited_text).is_err() {
        return fail_outcome(start);
    }

    let hover_anchor = spec.hover_anchor.as_deref().unwrap_or(&spec.with);
    let hover_sym = spec.hover_symbol.as_deref().unwrap_or(&spec.symbol);
    let outcome = if op_id == "code_action_fix" {
        let Ok((line, ch)) = resolve_anchor_utf16(&edited_text, hover_anchor, hover_sym) else {
            let _ = client.change_file(&uri, &edited_text, orig_text);
            return fail_outcome(start);
        };
        let _ = client.wait_diagnostics_after(&uri, prev_seq, Duration::from_millis(500));
        let end_col = ch + u32::try_from(hover_sym.encode_utf16().count()).unwrap_or(1);
        let res = client.request(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": uri },
                "range": {
                    "start": { "line": line, "character": ch },
                    "end": { "line": line, "character": end_col }
                },
                "context": { "diagnostics": [] }
            }),
            timeout,
        );
        eval_response(start, res, |v| {
            !is_empty_result(v) && value_matches_expect(v, spec.expect.as_deref())
        })
    } else if caps.hover {
        let Ok((line, ch)) = resolve_anchor_utf16(&edited_text, hover_anchor, hover_sym) else {
            let _ = client.change_file(&uri, &edited_text, orig_text);
            return fail_outcome(start);
        };
        let res = client.request(
            "textDocument/hover",
            td_pos_params(&uri, line, ch),
            timeout,
        );
        let _ = client.wait_diagnostics_after(&uri, prev_seq, Duration::from_millis(200));
        eval_response(start, res, |v| {
            !v.is_null() && value_matches_expect(v, spec.expect.as_deref())
        })
    } else {
        let res = client.request(
            "textDocument/documentSymbol",
            json!({ "textDocument": { "uri": uri } }),
            timeout,
        );
        let _ = client.wait_diagnostics_after(&uri, prev_seq, Duration::from_millis(200));
        eval_response(start, res, |v| !is_empty_result(v))
    };

    // Always restore original in-memory buffer.
    let _ = client.change_file(&uri, &edited_text, orig_text);
    outcome
}

#[allow(clippy::too_many_arguments)]
fn execute_completion(
    client: &mut LspClient,
    repo_dir: &Path,
    file_cache: &BTreeMap<String, String>,
    open_uris: &BTreeMap<String, String>,
    resolve_top: bool,
    spec: &CompletionProbeSpec,
    timeout: Duration,
    start: Instant,
) -> OpOutcome {
    let Some(orig_text) = file_cache.get(&spec.file) else {
        return fail_outcome(start);
    };
    let Ok(uri) = resolve_uri(repo_dir, open_uris, &spec.file) else {
        return fail_outcome(start);
    };
    let Some(edited_text) = replace_anchored_once(orig_text, &spec.anchor, &spec.replace, &spec.with)
    else {
        return fail_outcome(start);
    };

    let Ok((line, ch)) = resolve_cursor_after_utf16(&edited_text, &spec.with, &spec.cursor_after)
    else {
        return fail_outcome(start);
    };

    if client.change_file(&uri, orig_text, &edited_text).is_err() {
        return fail_outcome(start);
    }

    let context = match spec.trigger_character.as_deref() {
        Some(tc) => json!({ "triggerKind": 2, "triggerCharacter": tc }),
        None => json!({ "triggerKind": 1 }),
    };

    let comp_res = client.request(
        "textDocument/completion",
        json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": ch },
            "context": context
        }),
        timeout,
    );

    let outcome = if resolve_top {
        match comp_res {
            Ok(val) => {
                let first_item = extract_completion_items(&val).first().cloned().cloned();
                if let Some(item) = first_item {
                    let resolve_start = Instant::now();
                    let res = client.request("completionItem/resolve", item, timeout);
                    eval_response(resolve_start, res, |v| !v.is_null())
                } else {
                    OpOutcome {
                        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
                        error: false,
                        hit: false,
                    }
                }
            }
            Err(_) => fail_outcome(start),
        }
    } else {
        eval_response(start, comp_res, |v| {
            completion_matches_expect(v, spec.expect.as_deref())
        })
    };

    // Always restore original in-memory buffer.
    let _ = client.change_file(&uri, &edited_text, orig_text);
    outcome
}

fn replace_anchored_once(text: &str, anchor: &str, replace: &str, with: &str) -> Option<String> {
    if anchor.is_empty() {
        let idx = text.find(replace)?;
        let mut out = String::with_capacity(text.len() + with.len());
        out.push_str(text.get(..idx)?);
        out.push_str(with);
        out.push_str(text.get(idx + replace.len()..)?);
        return Some(out);
    }
    let anchor_pos = text.find(anchor)?;
    let search_slice = text.get(anchor_pos..)?;
    if let Some(rel) = search_slice.find(replace) {
        let idx = anchor_pos + rel;
        let mut out = String::with_capacity(text.len() + with.len());
        out.push_str(text.get(..idx)?);
        out.push_str(with);
        out.push_str(text.get(idx + replace.len()..)?);
        return Some(out);
    }
    let idx = text.find(replace)?;
    let mut out = String::with_capacity(text.len() + with.len());
    out.push_str(text.get(..idx)?);
    out.push_str(with);
    out.push_str(text.get(idx + replace.len()..)?);
    Some(out)
}

fn resolve_uri(
    repo_dir: &Path,
    open_uris: &BTreeMap<String, String>,
    rel: &str,
) -> Result<String, String> {
    if let Some(uri) = open_uris.get(rel) {
        return Ok(uri.clone());
    }
    path_to_file_uri(&repo_dir.join(rel))
}

fn td_pos_params(uri: &str, line: u32, ch: u32) -> Value {
    json!({
        "textDocument": { "uri": uri },
        "position": { "line": line, "character": ch }
    })
}

fn fail_outcome(start: Instant) -> OpOutcome {
    OpOutcome {
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        error: true,
        hit: false,
    }
}

fn eval_response<F>(
    start: Instant,
    res: Result<Value, client::RequestError>,
    check_hit: F,
) -> OpOutcome
where
    F: FnOnce(&Value) -> bool,
{
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    match res {
        Ok(val) => OpOutcome {
            elapsed_ms,
            error: false,
            hit: check_hit(&val),
        },
        Err(_) => OpOutcome {
            elapsed_ms,
            error: true,
            hit: false,
        },
    }
}

fn is_empty_result(val: &Value) -> bool {
    match val {
        Value::Null => true,
        Value::Array(arr) => arr.is_empty(),
        Value::Object(obj) => obj.is_empty(),
        _ => false,
    }
}

fn value_matches_expect(val: &Value, expect: Option<&str>) -> bool {
    let Some(exp) = expect else {
        return true;
    };
    if exp.is_empty() {
        return true;
    }
    val.to_string().contains(exp)
}

fn has_workspace_edits(val: &Value) -> bool {
    let has_changes = val
        .get("changes")
        .and_then(Value::as_object)
        .is_some_and(|m| !m.is_empty());
    let has_doc_changes = val
        .get("documentChanges")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty());
    has_changes || has_doc_changes
}

fn extract_completion_items(val: &Value) -> Vec<&Value> {
    if let Some(arr) = val.as_array() {
        return arr.iter().collect();
    }
    if let Some(arr) = val.get("items").and_then(Value::as_array) {
        return arr.iter().collect();
    }
    Vec::new()
}

fn completion_matches_expect(val: &Value, expect: Option<&str>) -> bool {
    let items = extract_completion_items(val);
    if items.is_empty() {
        return false;
    }
    let Some(exp) = expect else {
        return true;
    };
    if exp.is_empty() {
        return true;
    }
    items.iter().any(|item| {
        item.get("label")
            .and_then(Value::as_str)
            .is_some_and(|lbl| lbl.contains(exp))
    })
}

fn count_clean_file_errors(
    client: &LspClient,
    repo: &RepoSpec,
    open_uris: &BTreeMap<String, String>,
) -> usize {
    let mut total = 0usize;
    for rel in &repo.clean_files {
        if let Some(uri) = open_uris.get(rel)
            && let Some(count) = client.diagnostics_error_count(uri)
        {
            total = total.saturating_add(count);
        }
    }
    total
}

fn count_java_files(dir: &Path) -> usize {
    let mut count = 0usize;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(cur) = stack.pop() {
        let Ok(entries) = fs::read_dir(&cur) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name().and_then(|s| s.to_str())
                    && (name.starts_with('.') || name == "target" || name == "build")
                {
                    continue;
                }
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("java") {
                count += 1;
            }
        }
    }
    count
}

fn write_report_files(
    report: &BenchReport,
    json_out: &Path,
    md_out: &Path,
) -> Result<(), String> {
    if let Some(parent) = json_out.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let json_text = serde_json::to_string_pretty(report)
        .map_err(|e| format!("failed to serialize benchmark JSON: {e}"))?;
    fs::write(json_out, format!("{json_text}\n"))
        .map_err(|e| format!("failed to write {}: {e}", json_out.display()))?;

    if let Some(parent) = md_out.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
    }
    let md_text = format_markdown(report);
    fs::write(md_out, md_text)
        .map_err(|e| format!("failed to write {}: {e}", md_out.display()))?;
    Ok(())
}

fn collect_environment(
    servers: &[ServerSpec],
    revision_override: Option<&str>,
) -> EnvironmentInfo {
    let os = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let cpu_cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let cpu_model = detect_cpu_model();
    let ram_gb = detect_ram_gb();
    let java_version = detect_java_version();
    let revision = revision_override
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(detect_revision);
    let date = current_utc_date();

    let mut server_versions = BTreeMap::new();
    for s in servers {
        server_versions.insert(s.name.clone(), s.cmd.clone());
    }

    EnvironmentInfo {
        os,
        arch,
        cpu_model,
        cpu_cores,
        ram_gb,
        java_version,
        revision,
        date,
        server_versions,
    }
}

fn detect_cpu_model() -> String {
    #[cfg(target_os = "macos")]
    if let Ok(out) = Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    #[cfg(target_os = "linux")]
    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if let Some((_, val)) = line.split_once(':')
                && line.starts_with("model name")
            {
                let s = val.trim().to_string();
                if !s.is_empty() {
                    return s;
                }
            }
        }
    }
    "unknown".to_string()
}

fn detect_ram_gb() -> String {
    #[cfg(target_os = "macos")]
    if let Ok(out) = Command::new("sysctl").args(["-n", "hw.memsize"]).output()
        && out.status.success()
        && let Ok(bytes) = String::from_utf8_lossy(&out.stdout).trim().parse::<u64>()
    {
        let gb = bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        return format!("{gb:.0} GB");
    }
    #[cfg(target_os = "linux")]
    if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
        for line in meminfo.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let kb_str = rest.split_whitespace().next().unwrap_or("");
                if let Ok(kb) = kb_str.parse::<u64>() {
                    let gb = kb as f64 / (1024.0 * 1024.0);
                    return format!("{gb:.0} GB");
                }
            }
        }
    }
    "unknown".to_string()
}

fn detect_java_version() -> String {
    let Ok(out) = Command::new("java").arg("-version").output() else {
        return "not found".to_string();
    };
    let stderr_text = String::from_utf8_lossy(&out.stderr);
    let stdout_text = String::from_utf8_lossy(&out.stdout);
    let first = stderr_text
        .lines()
        .next()
        .or_else(|| stdout_text.lines().next())
        .unwrap_or("unknown")
        .trim();
    if first.is_empty() {
        "unknown".to_string()
    } else {
        first.to_string()
    }
}

fn detect_revision() -> String {
    if let Ok(out) = Command::new("jj")
        .args([
            "log",
            "-r",
            "@-",
            "--no-graph",
            "-T",
            "change_id.shortest(8) ++ \" (\" ++ commit_id.shortest(8) ++ \")\"",
        ])
        .output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    if let Ok(out) = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        && out.status.success()
    {
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !s.is_empty() {
            return s;
        }
    }
    "unknown".to_string()
}
