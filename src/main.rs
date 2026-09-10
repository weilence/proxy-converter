mod admin;
mod db;
mod entity;
mod js;
mod server;

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context as _, Result};
use clap::{Parser, Subcommand};

use crate::entity::Token;

/// Fetch remote proxy YAML configs, optionally transform them with a
/// JavaScript script, and download the result as `config.yaml`.
#[derive(Parser, Debug)]
#[command(name = "proxy-converter", version, about, long_about = None, arg_required_else_help = true)]
struct Cli {
    /// SQLite database storing the access tokens.
    #[arg(long, value_name = "FILE", global = true, default_value = "proxy-converter.db")]
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

        /// JavaScript file defining a `main(data)` function to transform configs.
        #[arg(short, long, value_name = "FILE")]
        script: Option<PathBuf>,
    },
    /// Manage the access tokens.
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },
}

#[derive(Subcommand, Debug)]
enum TokenAction {
    /// Add a new token.
    Add {
        /// Token value used by clients.
        token: String,

        /// Label describing the token.
        #[arg(short, long)]
        name: Option<String>,

        /// Days until the token expires (omit for no expiry).
        #[arg(short, long)]
        days: Option<i64>,
    },
    /// List all tokens.
    List,
    /// Remove a token.
    Remove {
        /// Token value to remove.
        token: String,
    },
    /// Enable a disabled token.
    Enable {
        /// Token value to enable.
        token: String,
    },
    /// Disable a token without deleting it.
    Disable {
        /// Token value to disable.
        token: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,sqlx=warn")),
        )
        .init();

    match cli.command {
        Command::Run { addr, script } => server::run(addr, script, cli.database).await,
        Command::Token { action } => run_token_action(&cli.database, action).await,
    }
}

async fn run_token_action(database: &Path, action: TokenAction) -> Result<()> {
    if let TokenAction::Add { days: Some(days), .. } = &action {
        if *days < 0 {
            bail!("--days must not be negative");
        }
    }

    let db = db::Db::open(database).await?;

    match action {
        TokenAction::Add { token, name, days } => {
            if !db
                .add(&token, name.as_deref().unwrap_or_default(), days)
                .await
                .with_context(|| format!("failed to add the token `{token}`"))?
            {
                bail!("Token `{token}` already exists");
            }
            println!("Added token `{token}`");
        }
        TokenAction::List => {
            print_token_table(&db.list().await.context("failed to list the tokens")?);
        }
        TokenAction::Remove { token } => {
            if db
                .remove(&token)
                .await
                .with_context(|| format!("failed to remove the token `{token}`"))?
                == 0
            {
                bail!("Token `{token}` not found");
            }
            println!("Removed token `{token}`");
        }
        TokenAction::Enable { token } => update_enabled(&db, &token, true).await?,
        TokenAction::Disable { token } => update_enabled(&db, &token, false).await?,
    }

    Ok(())
}
async fn update_enabled(db: &db::Db, token: &str, enabled: bool) -> Result<()> {
    let (verb, past) = if enabled {
        ("enable", "Enabled")
    } else {
        ("disable", "Disabled")
    };
    if db
        .set_enabled(token, enabled)
        .await
        .with_context(|| format!("failed to {verb} the token `{token}`"))?
        == 0
    {
        bail!("Token `{token}` not found");
    }
    println!("{past} token `{token}`");
    Ok(())
}

fn print_token_table(records: &[Token]) {
    if records.is_empty() {
        println!("No tokens yet; add one with `proxy-converter token add <TOKEN>`.");
        return;
    }

    let now = db::now();
    let header = [
        "ID",
        "TOKEN",
        "NAME",
        "STATUS",
        "EXPIRES_AT",
        "LAST_USED_AT",
        "CREATED_AT",
    ];
    let rows: Vec<[String; 7]> = records
        .iter()
        .map(|record| {
            [
                record.id.to_string(),
                record.token.clone(),
                record.name.clone(),
                db::token_status(record, now).to_owned(),
                db::fmt_datetime(record.expires_at, "never"),
                db::fmt_datetime(record.last_used_at, "-"),
                db::fmt_datetime(Some(record.created_at), "-"),
            ]
        })
        .collect();

    let mut widths = header.iter().map(|cell| cell.len()).collect::<Vec<_>>();
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let print_row = |cells: [&str; 7]| {
        let line = cells
            .iter()
            .enumerate()
            .map(|(i, cell)| format!("{:<width$}", cell, width = widths[i]))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{line}");
    };

    print_row(header);
    for row in &rows {
        print_row([
            row[0].as_str(),
            row[1].as_str(),
            row[2].as_str(),
            row[3].as_str(),
            row[4].as_str(),
            row[5].as_str(),
            row[6].as_str(),
        ]);
    }
}
