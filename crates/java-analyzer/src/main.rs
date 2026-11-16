use lsp_server::Connection;

fn main() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    eprintln!("Starting java-analyzer LSP server...");
    let (connection, io_threads) = Connection::stdio();
    java_analyzer::server::run(connection)?;
    io_threads.join()?;
    eprintln!("java-analyzer LSP server shut down");
    Ok(())
}
