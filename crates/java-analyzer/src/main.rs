use std::env;

use java_analyzer::cli::{help_text, parse_cli_args, run_configure, CliCommand};
use lsp_server::Connection;

fn main() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let command = match parse_cli_args(env::args().skip(1)) {
        Ok(cmd) => cmd,
        Err(err) => {
            eprintln!("error: {err}");
            std::process::exit(2);
        }
    };

    match command {
        CliCommand::Help => {
            print!("{}", help_text());
            Ok(())
        }
        CliCommand::Configure(args) => {
            run_configure(args)?;
            Ok(())
        }
        CliCommand::Lsp => {
            eprintln!("Starting java-analyzer LSP server...");
            let (connection, io_threads) = Connection::stdio();
            java_analyzer::server::run(connection)?;
            io_threads.join()?;
            eprintln!("java-analyzer LSP server shut down");
            Ok(())
        }
    }
}
