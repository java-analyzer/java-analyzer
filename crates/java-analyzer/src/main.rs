use clap::Parser;
use java_analyzer::cli::{Cli, Command};
use lsp_server::Connection;

fn main() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Command::Lsp) => {
            eprintln!("Starting java-analyzer LSP server...");
            let (connection, io_threads) = Connection::stdio();
            java_analyzer::server::run(connection)?;
            io_threads.join()?;
            eprintln!("java-analyzer LSP server shut down");
            Ok(())
        }
        Some(Command::Inspect(args)) => {
            java_analyzer::inspect::run(args)?;
            Ok(())
        }
        Some(Command::Configure(args)) => {
            java_analyzer::configure::run(args)?;
            Ok(())
        }
    }
}
