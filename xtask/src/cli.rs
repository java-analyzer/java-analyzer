//! Command line of `cargo xtask`.

use std::{
    ffi::{OsStr, OsString},
    num::{NonZeroU64, NonZeroUsize},
    path::PathBuf,
    time::Duration,
};

use crate::{
    bench::{
        self, BenchArgs, GenPlainArgs, ServerSpec,
        probes::{OpCategory, Shape, Tier},
    },
    conformance::{self, Config, Kind, Source},
    report::{self, ReportArgs},
};

#[derive(Debug, usage::Cli)]
#[usage(crate = usage, name = "xtask", bin = "cargo xtask", unknown_flags = "error")]
pub struct XtaskCli {
    #[usage(subcommand)]
    pub command: Option<XtaskSubcommand>,
}

#[derive(Debug, usage::Subcommands)]
#[usage(crate = usage)]
pub enum XtaskSubcommand {
    /// Parse Java corpora and write conformance reports
    Conformance(ConformanceCliArgs),
    /// Assemble unified CONFORMANCE.md from verification outputs
    Report(ReportArgs),
    /// Run the reproducible Java LSP benchmark suite
    Bench(Box<BenchCliArgs>),
    /// Generate the synthetic plain-small Java corpus
    BenchGenPlain(GenPlainArgs),
    /// Print the canonical KDL usage spec for xtask to stdout
    UsageSpec,
}

fn default_release() -> syntax::Release {
    syntax::Release::LATEST
}

/// Parses every .java file under the given directories and writes a report.
///
/// Report only: the exit code is 0 whatever the conformance results are, and non-zero only if the
/// tool itself fails.
#[derive(Debug, usage::Args)]
#[usage(crate = usage)]
pub struct ConformanceCliArgs {
    /// A directory of files that must parse without errors (repeatable)
    #[usage(long, var, value_name = "DIR")]
    pub valid: Vec<PathBuf>,
    /// A directory of files that must produce at least one error (repeatable)
    #[usage(long, var, value_name = "DIR")]
    pub invalid: Vec<PathBuf>,
    /// Declared Java release version for parsing
    #[usage(long, value_name = "N", default_fn = default_release, default_note = "latest")]
    pub release: syntax::Release,
    /// Enable preview features at the declared release
    #[usage(long)]
    pub preview: bool,
    /// Where to write report.json, report.md and failures/
    #[usage(long, value_name = "DIR", default = "target/conformance")]
    pub out: PathBuf,
    /// Per-file parse limit in seconds; a file over it is a critical finding
    #[usage(long, value_name = "SECS", default = "5")]
    pub timeout: NonZeroU64,
}

impl From<ConformanceCliArgs> for Args {
    fn from(cli: ConformanceCliArgs) -> Self {
        let mut sources = Vec::with_capacity(cli.valid.len() + cli.invalid.len());
        sources.extend(cli.valid.into_iter().map(|p| (Kind::Valid, p)));
        sources.extend(cli.invalid.into_iter().map(|p| (Kind::Invalid, p)));
        Self {
            config: ConfigArgs {
                sources,
                level: syntax::LanguageLevel::new(cli.release, cli.preview),
                timeout_secs: cli.timeout.get(),
            },
            out: cli.out,
        }
    }
}

/// Repository argument parsed from `--repo <ID=PATH>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoArg(pub String, pub PathBuf);

impl std::str::FromStr for RepoArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (id, path) = bench::parse_repo_arg(s)?;
        Ok(Self(id, path))
    }
}

/// Run the reproducible Java LSP benchmark suite across servers and repositories.
#[derive(Debug, usage::Args)]
#[usage(crate = usage)]
pub struct BenchCliArgs {
    /// Server spec to benchmark (repeatable)
    #[usage(long = "server", var, value_name = "NAME=CMD")]
    pub servers: Vec<ServerSpec>,
    /// Repository path to benchmark (repeatable)
    #[usage(long = "repo", var, value_name = "ID=PATH")]
    pub repos: Vec<RepoArg>,
    /// Filter matrix by size tier (repeatable)
    #[usage(long = "tier", var, value_enum)]
    pub tiers: Vec<Tier>,
    /// Filter matrix by shape (repeatable)
    #[usage(long = "shape", var, value_enum)]
    pub shapes: Vec<Shape>,
    /// Comma-separated op categories [all]
    #[usage(long = "ops", var, delimiter = ',', value_enum, value_name = "CATEGORIES")]
    pub ops: Vec<OpCategory>,
    /// TOML probe manifest [embedded xtask/bench/probes.toml]
    #[usage(long, value_name = "PATH")]
    pub probes: Option<PathBuf>,
    /// Warmup iterations per probe
    #[usage(long, value_name = "N", default = "5")]
    pub warmup: usize,
    /// Measured iterations per probe
    #[usage(long, value_name = "N", default = "15")]
    pub iterations: NonZeroUsize,
    /// Max seconds to wait for server readiness
    #[usage(long = "ready-timeout", value_name = "SECS", default = "180")]
    pub ready_timeout_secs: NonZeroU64,
    /// Max seconds per individual LSP request
    #[usage(long = "request-timeout", value_name = "SECS", default = "30")]
    pub request_timeout_secs: NonZeroU64,
    /// Skip Phase 4 warm-restart measurement
    #[usage(long)]
    pub skip_warm_restart: bool,
    /// Revision string for the report header [auto-detected]
    #[usage(long, value_name = "TEXT")]
    pub revision: Option<String>,
    /// Output JSON path
    #[usage(long, value_name = "PATH", default = "target/bench/results.json")]
    pub json_out: PathBuf,
    /// Output Markdown path
    #[usage(long, value_name = "PATH", default = "target/bench/BENCHMARKS.md")]
    pub md_out: PathBuf,
}

impl From<BenchCliArgs> for BenchArgs {
    fn from(cli: BenchCliArgs) -> Self {
        Self {
            servers: cli.servers,
            repos: cli
                .repos
                .into_iter()
                .map(|RepoArg(id, path)| (id, path))
                .collect(),
            tiers: cli.tiers,
            shapes: cli.shapes,
            ops: cli.ops,
            probes: cli.probes,
            warmup: cli.warmup,
            iterations: cli.iterations.get(),
            ready_timeout_secs: cli.ready_timeout_secs.get(),
            request_timeout_secs: cli.request_timeout_secs.get(),
            skip_warm_restart: cli.skip_warm_restart,
            revision: cli.revision,
            json_out: cli.json_out,
            md_out: cli.md_out,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub config: ConfigArgs,
    pub out: PathBuf,
}

/// The parts of [`Config`] that come from the command line.
#[derive(Debug, PartialEq, Eq)]
pub struct ConfigArgs {
    pub sources: Vec<(Kind, PathBuf)>,
    pub level: syntax::LanguageLevel,
    pub timeout_secs: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Conformance(Args),
    Report(ReportArgs),
    Bench(Box<BenchArgs>),
    BenchGenPlain(GenPlainArgs),
    UsageSpec,
    Help,
    ReportHelp,
    BenchHelp,
    BenchGenPlainHelp,
}

pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let os_args: Vec<OsString> = args.into_iter().map(OsString::from).collect();
    let refs: Vec<&OsStr> = os_args.iter().map(OsString::as_os_str).collect();
    match XtaskCli::parse_from(&refs) {
        Ok(cli) => Ok(match cli.command {
            Some(XtaskSubcommand::Conformance(c)) => Command::Conformance(c.into()),
            Some(XtaskSubcommand::Report(r)) => Command::Report(r),
            Some(XtaskSubcommand::Bench(b)) => Command::Bench(Box::new((*b).into())),
            Some(XtaskSubcommand::BenchGenPlain(g)) => Command::BenchGenPlain(g),
            Some(XtaskSubcommand::UsageSpec) => Command::UsageSpec,
            None => Command::Help,
        }),
        Err(usage::Error::Help { cmd, .. }) => Ok(match cmd.name {
            "report" => Command::ReportHelp,
            "bench" => Command::BenchHelp,
            "bench-gen-plain" => Command::BenchGenPlainHelp,
            _ => Command::Help,
        }),
        Err(err) => Err(usage::render_failure_plain(XtaskCli::spec(), &refs, &err)),
    }
}

/// Source ids are the directory names, made unique by a numeric suffix if needed.
fn source_ids(sources: &[(Kind, PathBuf)]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for (_, dir) in sources {
        let base = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "source".to_string());
        let mut id = base.clone();
        let mut n = 2;
        while ids.contains(&id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        ids.push(id);
    }
    ids
}

pub fn to_config(args: &ConfigArgs) -> Config {
    let ids = source_ids(&args.sources);
    let known_divergences =
        conformance::parse_known_divergences(include_str!("../conformance/known-divergences.toml"))
            .unwrap_or_else(|err| {
                eprintln!("warning: invalid embedded known-divergences.toml: {err}");
                Vec::new()
            });
    Config {
        sources: args
            .sources
            .iter()
            .zip(ids)
            .map(|((kind, dir), id)| Source {
                id,
                dir: dir.clone(),
                kind: *kind,
            })
            .collect(),
        level: args.level,
        timeout: Duration::from_secs(args.timeout_secs),
        known_divergences,
    }
}

/// Runs the command and returns the process exit code.
pub fn run(args: impl IntoIterator<Item = String>) -> i32 {
    let os_args: Vec<OsString> = args.into_iter().map(OsString::from).collect();
    let cli = match XtaskCli::embedded_outcome(&os_args) {
        usage::embedded::Outcome::Parsed(cli) => cli,
        usage::embedded::Outcome::Exit(exit) => {
            if exit.stderr {
                eprint!("{}", exit.text);
            } else {
                print!("{}", exit.text);
            }
            return exit.code;
        }
    };

    let Some(subcmd) = cli.command else {
        if let Some(help) = XtaskCli::render_help(XtaskCli::command(), false) {
            print!("{help}");
        }
        return 0;
    };

    match subcmd {
        XtaskSubcommand::Report(report_args) => report::run(&report_args),
        XtaskSubcommand::Bench(bench_args) => bench::run(&(*bench_args).into()),
        XtaskSubcommand::BenchGenPlain(gen_args) => bench::run_gen_plain(&gen_args),
        XtaskSubcommand::UsageSpec => {
            print!("{}", XtaskCli::to_kdl());
            0
        }
        XtaskSubcommand::Conformance(conformance_args) => {
            run_conformance(Args::from(conformance_args))
        }
    }
}

fn run_conformance(args: Args) -> i32 {
    let config = to_config(&args.config);
    if config.sources.is_empty() {
        eprintln!("note: no sources given (use --valid / --invalid); writing an empty report");
    }
    crate::isolate::silence_panics();
    let (report, took) = conformance::timed(|| conformance::run(&config));
    if let Err(e) = conformance::write_report(&report, &args.out) {
        eprintln!(
            "error: cannot write the report to {}: {e}",
            args.out.display()
        );
        return 1;
    }

    let table = conformance::format_summary_table(&report);
    if !table.is_empty() {
        print!("\n{table}");
    }
    println!("took {took:.1?}; report written to {}", args.out.display());
    if report.critical_count() > 0 {
        println!(
            "{} critical finding(s) (parser bugs): see {}/report.md",
            report.critical_count(),
            args.out.display()
        );
    }
    0
}
