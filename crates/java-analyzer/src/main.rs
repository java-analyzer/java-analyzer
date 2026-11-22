use clap::Parser;
use java_analyzer::cli::{Cli, Command};
use lsp_server::Connection;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

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
    }
}
