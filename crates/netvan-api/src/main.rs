mod http_server;
mod service;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "netvan-api", about = "Netvan local API Windows service")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run in foreground (dev / console)
    Run,
    /// Install Windows service
    Install,
    /// Uninstall Windows service
    Uninstall,
    /// Start Windows service
    Start,
    /// Stop Windows service
    Stop,
    /// Print service install/running status
    Status,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let cli = Cli::parse();
    match cli.command.unwrap_or(Commands::Run) {
        Commands::Run => http_server::run().await,
        Commands::Install => service::install(),
        Commands::Uninstall => service::uninstall(),
        Commands::Start => service::start(),
        Commands::Stop => service::stop(),
        Commands::Status => service::status(),
    }
}
