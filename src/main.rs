mod admin;
#[cfg(not(debug_assertions))]
mod assets;
mod db;
mod entity;
mod server;

use std::{net::SocketAddr, path::PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Serve admin-bound YAML proxy configs as `config.yaml` downloads.
#[derive(Parser, Debug)]
#[command(name = "proxy-converter", version, about, long_about = None, arg_required_else_help = true)]
struct Cli {
    /// SQLite database storing the access tokens.
    #[arg(
        long,
        value_name = "FILE",
        global = true,
        default_value = "proxy-converter.db"
    )]
    database: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Start the HTTP server.
    Run {
        /// Address the HTTP server listens on.
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        addr: SocketAddr,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    load_dotenv();

    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,sqlx=warn")),
        )
        .init();

    let Command::Run { addr } = cli.command;
    server::run(addr, cli.database).await
}

/// Load `.env` from the working directory (or any parent) before anything
/// reads the environment. Existing variables keep precedence over file values.
fn load_dotenv() {
    if let Err(err) = dotenvy::dotenv() {
        // A missing file is the normal case; anything else is worth reporting,
        // since a broken file would silently drop all of its variables.
        if !matches!(
            err,
            dotenvy::Error::Io(ref e) if e.kind() == std::io::ErrorKind::NotFound
        ) {
            eprintln!("warning: failed to load .env: {err}");
        }
    }
}
