use std::{ffi::OsString, process::ExitCode};

use java_analyzer::cli::{Command, parse_outcome};

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

    if let Err(err) = usage::Run::run(cli.command.unwrap_or(Command::Lsp)) {
        eprintln!("Error: {err:?}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
