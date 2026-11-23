//! Command line of `cargo xtask`.

use std::{path::PathBuf, time::Duration};

use crate::{
    bench::{self, BenchArgs, GenPlainArgs, ServerSpec, probes::{OpCategory, Shape, Tier}},
    conformance::{self, Config, Kind, Source},
    report::{self, ClippyStatus, ReportArgs},
};

pub const USAGE: &str = "\
Usage: cargo xtask <COMMAND> [OPTIONS]

Commands:
  conformance      Parse Java corpora and write conformance reports
  report           Assemble unified CONFORMANCE.md from verification outputs
  bench            Run the reproducible Java LSP benchmark suite
  bench-gen-plain  Generate the synthetic plain-small Java corpus

Run `cargo xtask <COMMAND> --help` for command-specific options.

---

Usage: cargo xtask conformance [OPTIONS]

Parses every .java file under the given directories and writes a report. Report only: the exit
code is 0 whatever the conformance results are, and non-zero only if the tool itself fails.

Options:
  --valid <DIR>      A directory of files that must parse without errors (repeatable)
  --invalid <DIR>    A directory of files that must produce at least one error (repeatable)
  --release <N>      Declared Java release version for parsing [latest]
  --preview          Enable preview features at the declared release
  --out <DIR>        Where to write report.json, report.md and failures/ [target/conformance]
  --timeout <SECS>   Per-file parse limit; a file over it is a critical finding [5]
  -h, --help         Show this help
";

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
    Help,
    ReportHelp,
    BenchHelp,
    BenchGenPlainHelp,
}

pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("conformance") => parse_conformance_args(args),
        Some("report") => parse_report_args(args),
        Some("bench") => parse_bench_args(args),
        Some("bench-gen-plain") => parse_gen_plain_args(args),
        Some("-h" | "--help") | None => Ok(Command::Help),
        Some(other) => Err(format!("unknown command `{other}`")),
    }
}

fn parse_conformance_args(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut sources = Vec::new();
    let mut release = syntax::Release::LATEST;
    let mut preview = false;
    let mut out = PathBuf::from("target/conformance");
    let mut timeout_secs = 5;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--valid" => sources.push((Kind::Valid, PathBuf::from(value("--valid")?))),
            "--invalid" => sources.push((Kind::Invalid, PathBuf::from(value("--invalid")?))),
            "--release" => {
                let text = value("--release")?;
                release = text
                    .parse::<syntax::Release>()
                    .map_err(|e| format!("--release: {e}"))?;
            }
            "--preview" => preview = true,
            "--out" => out = PathBuf::from(value("--out")?),
            "--timeout" => {
                let text = value("--timeout")?;
                timeout_secs =
                    text.parse::<u64>().ok().filter(|&n| n > 0).ok_or_else(|| {
                        format!("--timeout needs a positive number, got `{text}`")
                    })?;
            }
            "-h" | "--help" => return Ok(Command::Help),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(Command::Conformance(Args {
        config: ConfigArgs {
            sources,
            level: syntax::LanguageLevel::new(release, preview),
            timeout_secs,
        },
        out,
    }))
}

fn parse_report_args(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut parsed = ReportArgs::default();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--conformance" => parsed.conformance = PathBuf::from(value("--conformance")?),
            "--workspace-corpus" => {
                parsed.workspace_corpus = PathBuf::from(value("--workspace-corpus")?)
            }
            "--test-output" => parsed.test_output = Some(PathBuf::from(value("--test-output")?)),
            "--clippy-status" => {
                let text = value("--clippy-status")?;
                parsed.clippy_status = ClippyStatus::parse(&text)?;
            }
            "--known-findings" => {
                parsed.known_findings = Some(PathBuf::from(value("--known-findings")?))
            }
            "--revision" => parsed.revision = Some(value("--revision")?),
            "--date" => parsed.date = Some(value("--date")?),
            "--out" => parsed.out = PathBuf::from(value("--out")?),
            "--strict" => parsed.strict = true,
            "-h" | "--help" => return Ok(Command::ReportHelp),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(Command::Report(parsed))
}

fn parse_bench_args(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut parsed = BenchArgs::default();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--server" => parsed.servers.push(ServerSpec::parse(&value("--server")?)?),
            "--repo" => parsed.repos.push(bench::parse_repo_arg(&value("--repo")?)?),
            "--tier" => parsed.tiers.push(Tier::parse(&value("--tier")?)?),
            "--shape" => parsed.shapes.push(Shape::parse(&value("--shape")?)?),
            "--ops" => {
                let raw = value("--ops")?;
                for part in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    parsed.ops.push(OpCategory::parse(part)?);
                }
            }
            "--probes" => parsed.probes = Some(PathBuf::from(value("--probes")?)),
            "--warmup" => {
                let text = value("--warmup")?;
                parsed.warmup = text
                    .parse::<usize>()
                    .map_err(|_| format!("--warmup needs a non-negative integer, got `{text}`"))?;
            }
            "--iterations" => {
                let text = value("--iterations")?;
                parsed.iterations = text
                    .parse::<usize>()
                    .ok()
                    .filter(|&n| n > 0)
                    .ok_or_else(|| format!("--iterations needs a positive integer, got `{text}`"))?;
            }
            "--ready-timeout" => {
                let text = value("--ready-timeout")?;
                parsed.ready_timeout_secs = text
                    .parse::<u64>()
                    .ok()
                    .filter(|&n| n > 0)
                    .ok_or_else(|| {
                        format!("--ready-timeout needs a positive number, got `{text}`")
                    })?;
            }
            "--request-timeout" => {
                let text = value("--request-timeout")?;
                parsed.request_timeout_secs = text
                    .parse::<u64>()
                    .ok()
                    .filter(|&n| n > 0)
                    .ok_or_else(|| {
                        format!("--request-timeout needs a positive number, got `{text}`")
                    })?;
            }
            "--skip-warm-restart" => parsed.skip_warm_restart = true,
            "--json-out" => parsed.json_out = PathBuf::from(value("--json-out")?),
            "--md-out" => parsed.md_out = PathBuf::from(value("--md-out")?),
            "-h" | "--help" => return Ok(Command::BenchHelp),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(Command::Bench(Box::new(parsed)))
}

fn parse_gen_plain_args(mut args: impl Iterator<Item = String>) -> Result<Command, String> {
    let mut parsed = GenPlainArgs::default();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--out" => parsed.out = PathBuf::from(value("--out")?),
            "-h" | "--help" => return Ok(Command::BenchGenPlainHelp),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(Command::BenchGenPlain(parsed))
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
            .expect("embedded known-divergences.toml is valid");
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
    let args: Vec<String> = args.into_iter().collect();
    let subcmd = args.first().cloned();
    let command = match parse_args(args) {
        Ok(command) => command,
        Err(message) => {
            let usage = match subcmd.as_deref() {
                Some("report") => report::USAGE,
                Some("bench") => bench::USAGE,
                Some("bench-gen-plain") => bench::GEN_PLAIN_USAGE,
                _ => USAGE,
            };
            eprintln!("error: {message}\n\n{usage}");
            return 2;
        }
    };
    let args = match command {
        Command::Help => {
            print!("{USAGE}");
            return 0;
        }
        Command::ReportHelp => {
            print!("{}", report::USAGE);
            return 0;
        }
        Command::BenchHelp => {
            print!("{}", bench::USAGE);
            return 0;
        }
        Command::BenchGenPlainHelp => {
            print!("{}", bench::GEN_PLAIN_USAGE);
            return 0;
        }
        Command::Report(report_args) => return report::run(&report_args),
        Command::Bench(bench_args) => return bench::run(&bench_args),
        Command::BenchGenPlain(gen_args) => return bench::run_gen_plain(&gen_args),
        Command::Conformance(args) => args,
    };

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
