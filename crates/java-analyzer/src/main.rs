use std::{ffi::OsString, process::ExitCode};

use java_analyzer::cli::{Cli, Command, parse_outcome};
use lsp_server::Connection;

fn main() -> ExitCode {
    let os_args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let cli = match parse_outcome(&os_args) {
        usage::embedded::Outcome::Parsed(cli) => cli,
        usage::embedded::Outcome::Exit(exit) => {
            if exit.stderr {
                eprint!("{}", exit.text);
            } else {
                print!("{}", exit.text);
            }
            return ExitCode::from(u8::try_from(exit.code).unwrap_or(1));
        }
    };

    if let Err(err) = run(cli) {
        eprintln!("Error: {err:?}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        None | Some(Command::Lsp) => {
            let _ = tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
                .with_ansi(false)
                .try_init();
            tracing::info!("Starting java-analyzer LSP server...");
            let (connection, io_threads) = Connection::stdio();
            java_analyzer::server::run(connection)?;
            io_threads.join()?;
            tracing::info!("java-analyzer LSP server shut down");
            Ok(())
        }
        Some(Command::Inspect(args)) => {
            java_analyzer::inspect::run(args)?;
            Ok(())
        }
        Some(Command::Configure(args)) => {
            java_analyzer::configure::run(*args)?;
            Ok(())
        }
        Some(Command::UsageSpec) => {
            print!("{}", Cli::to_kdl());
            Ok(())
        }
    }
}
