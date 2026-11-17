//! Command line of `cargo xtask`.

use std::{path::PathBuf, time::Duration};

use crate::conformance::{self, Config, Kind, Source};

pub const USAGE: &str = "\
Usage: cargo xtask conformance [OPTIONS]

Parses every .java file under the given directories and writes a report. Report only: the exit
code is 0 whatever the conformance results are, and non-zero only if the tool itself fails.

Options:
  --valid <DIR>      A directory of files that must parse without errors (repeatable)
  --invalid <DIR>    A directory of files that must produce at least one error (repeatable)
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
    pub timeout_secs: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Conformance(Args),
    Help,
}

pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter();
    match args.next().as_deref() {
        Some("conformance") => {}
        Some("-h" | "--help") | None => return Ok(Command::Help),
        Some(other) => return Err(format!("unknown command `{other}`")),
    }

    let mut sources = Vec::new();
    let mut out = PathBuf::from("target/conformance");
    let mut timeout_secs = 5;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--valid" => sources.push((Kind::Valid, PathBuf::from(value("--valid")?))),
            "--invalid" => sources.push((Kind::Invalid, PathBuf::from(value("--invalid")?))),
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
            timeout_secs,
        },
        out,
    }))
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
        timeout: Duration::from_secs(args.timeout_secs),
    }
}

/// Runs the command and returns the process exit code.
pub fn run(args: impl IntoIterator<Item = String>) -> i32 {
    let command = match parse_args(args) {
        Ok(command) => command,
        Err(message) => {
            eprintln!("error: {message}\n\n{USAGE}");
            return 2;
        }
    };
    let args = match command {
        Command::Help => {
            print!("{USAGE}");
            return 0;
        }
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

    for s in &report.sources {
        match &s.skipped {
            Some(reason) => println!("{}: skipped ({reason})", s.id),
            None => println!(
                "{}: {} file(s), {} conforming, {} critical",
                s.id,
                s.results.len(),
                s.conforming(),
                s.critical().count()
            ),
        }
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
