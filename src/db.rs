use std::path::Path;

use anyhow::{Context as _, Result};
use chrono::{Local, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, Database, DatabaseBackend,
    DatabaseConnection, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder, Set, Statement,
    TransactionTrait, prelude::DateTime,
};

use crate::entity::{mrs_file, token};

const CREATE_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    token        TEXT NOT NULL UNIQUE,
    name         TEXT NOT NULL DEFAULT '',
    config       TEXT NOT NULL DEFAULT '',
    enabled      INTEGER NOT NULL DEFAULT 1,
    expires_at   TEXT,
    created_at   TEXT NOT NULL,
    last_used_at TEXT
)";

const CREATE_MRS_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS mrs_files (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    token_id   INTEGER NOT NULL,
    name       TEXT NOT NULL,
    source_url TEXT NOT NULL,
    content    BLOB NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (token_id, name)
)";

/// SQLite-backed token store built on SeaORM. Tokens are valid when they
/// exist, are enabled, and have not expired.
pub struct Db {
    conn: DatabaseConnection,
}

impl Db {
    pub async fn open(path: &Path) -> Result<Self> {
        let conn = Database::connect(format!("sqlite://{}?mode=rwc", path.display()))
            .await
            .with_context(|| format!("failed to open database `{}`", path.display()))?;

        migrate(&conn).await?;
        // Best effort: WAL improves concurrent readers while serving.
        let _ = conn.execute_unprepared("PRAGMA journal_mode = WAL;").await;

        Ok(Self { conn })
    }

    /// Check whether a token is currently valid; on success return its
    /// record and mark its use.
    pub async fn verify(&self, token: &str) -> Option<token::Model> {
        let found = token::Entity::find()
            .filter(token::Column::Token.eq(token))
            .filter(token::Column::Enabled.eq(true))
            .filter(
                Condition::any()
                    .add(token::Column::ExpiresAt.is_null())
                    .add(token::Column::ExpiresAt.gt(now())),
            )
            .one(&self.conn)
            .await
            .ok()
            .flatten();

        if found.is_some() {
            let _ = token::Entity::update_many()
                .filter(token::Column::Token.eq(token))
                .set(token::ActiveModel {
                    last_used_at: Set(Some(now())),
                    ..Default::default()
                })
                .exec(&self.conn)
                .await;
        }

        found
    }

    /// Insert a token; returns `false` when it already exists.
    pub async fn add(
        &self,
        token: &str,
        name: &str,
        days: Option<i64>,
        config: &str,
    ) -> Result<bool> {
        let config = normalize_config(config)?;
        if token::Entity::find()
            .filter(token::Column::Token.eq(token))
            .one(&self.conn)
            .await
            .is_ok_and(|record| record.is_some())
        {
            return Ok(false);
        }

        token::ActiveModel {
            token: Set(token.to_owned()),
            name: Set(name.to_owned()),
            config: Set(config),
            enabled: Set(true),
            expires_at: Set(days.map(|days| now() + chrono::Duration::days(days))),
            created_at: Set(now()),
            last_used_at: Set(None),
            ..Default::default()
        }
        .insert(&self.conn)
        .await
        .map_err(|err| anyhow::anyhow!("failed to insert the token: {err}"))?;

        Ok(true)
    }

    /// Fetch a token by id.
    pub async fn get(&self, id: i32) -> Result<Option<token::Model>> {
        token::Entity::find_by_id(id)
            .one(&self.conn)
            .await
            .context("failed to fetch the token")
    }

    /// Delete a token by id along with its hosted mrs files; returns the
    /// number of removed token rows.
    pub async fn remove(&self, id: i32) -> Result<u64> {
        let txn = self.conn.begin().await?;
        delete_mrs_files(&txn, id).await?;
        let removed = token::Entity::delete_by_id(id)
            .exec(&txn)
            .await
            .map(|result| result.rows_affected)
            .context("failed to delete the token")?;
        txn.commit().await?;
        Ok(removed)
    }

    /// Enable or disable a token by id; returns the number of updated rows.
    pub async fn set_enabled(&self, id: i32, enabled: bool) -> Result<u64> {
        token::Entity::update_many()
            .filter(token::Column::Id.eq(id))
            .set(token::ActiveModel {
                enabled: Set(enabled),
                ..Default::default()
            })
            .exec(&self.conn)
            .await
            .map(|result| result.rows_affected)
            .context("failed to update the token")
    }

    /// Bind config content to a token by id; returns the number of updated rows.
    pub async fn set_config(&self, id: i32, config: &str) -> Result<u64> {
        let config = normalize_config(config)?;
        token::Entity::update_many()
            .filter(token::Column::Id.eq(id))
            .set(token::ActiveModel {
                config: Set(config),
                ..Default::default()
            })
            .exec(&self.conn)
            .await
            .map(|result| result.rows_affected)
            .context("failed to update the token")
    }

    /// Rename a token by id; returns the number of updated rows.
    pub async fn set_name(&self, id: i32, name: &str) -> Result<u64> {
        token::Entity::update_many()
            .filter(token::Column::Id.eq(id))
            .set(token::ActiveModel {
                name: Set(name.to_owned()),
                ..Default::default()
            })
            .exec(&self.conn)
            .await
            .map(|result| result.rows_affected)
            .context("failed to update the token")
    }

    /// List all tokens, oldest first.
    pub async fn list(&self) -> Result<Vec<token::Model>> {
        token::Entity::find()
            .order_by_asc(token::Column::Id)
            .all(&self.conn)
            .await
            .context("failed to list the tokens")
    }

    /// Replace the mrs files hosted for a token with a fresh conversion run.
    pub async fn set_mrs_files(&self, token_id: i32, files: Vec<NewMrsFile>) -> Result<()> {
        let txn = self.conn.begin().await?;
        delete_mrs_files(&txn, token_id).await?;
        for file in files {
            mrs_file::ActiveModel {
                token_id: Set(token_id),
                name: Set(file.name),
                source_url: Set(file.source_url),
                content: Set(file.content),
                updated_at: Set(now()),
                ..Default::default()
            }
            .insert(&txn)
            .await
            .context("failed to store the mrs file")?;
        }
        txn.commit().await?;
        Ok(())
    }

    /// Fetch one of a token's mrs files by provider name.
    pub async fn get_mrs_file(&self, token_id: i32, name: &str) -> Result<Option<mrs_file::Model>> {
        mrs_file::Entity::find()
            .filter(mrs_file::Column::TokenId.eq(token_id))
            .filter(mrs_file::Column::Name.eq(name))
            .one(&self.conn)
            .await
            .context("failed to fetch the mrs file")
    }
}

/// A new mrs file to host for a token.
pub struct NewMrsFile {
    pub name: String,
    pub source_url: String,
    pub content: Vec<u8>,
}

async fn delete_mrs_files(txn: &DatabaseTransaction, token_id: i32) -> Result<()> {
    mrs_file::Entity::delete_many()
        .filter(mrs_file::Column::TokenId.eq(token_id))
        .exec(txn)
        .await
        .map(|_| ())
        .context("failed to delete the token's mrs files")
}

/// Current UTC time; stored as `YYYY-MM-DD HH:MM:SS`.
pub fn now() -> DateTime {
    Utc::now().naive_utc()
}

/// Human-readable status of a token record.
pub fn token_status(record: &token::Model, now: DateTime) -> &'static str {
    if !record.enabled {
        "disabled"
    } else if record
        .expires_at
        .is_some_and(|expires_at| expires_at <= now)
    {
        "expired"
    } else {
        "valid"
    }
}

/// Format a stored UTC datetime for display in the local timezone.
pub fn fmt_datetime(datetime: Option<DateTime>, none: &str) -> String {
    match datetime {
        None => none.to_owned(),
        Some(datetime) => datetime
            .and_utc()
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
            .to_string(),
    }
}

/// Check that config content parses as YAML; an empty string is allowed and
/// means "no config bound".
pub fn validate_config(config: &str) -> Result<()> {
    serde_yaml::from_str::<serde_yaml::Value>(config)
        .map(|_| ())
        .map_err(|err| anyhow::anyhow!("the config is not valid YAML: {err}"))
}

fn normalize_config(config: &str) -> Result<String> {
    let config = config.trim();
    validate_config(config)?;
    Ok(config.to_owned())
}

/// Create the table for fresh databases, or upgrade the legacy schema
/// (token as primary key, unix-epoch integers) to the current one.
async fn migrate(conn: &DatabaseConnection) -> Result<()> {
    let query = |sql: &str| Statement::from_string(DatabaseBackend::Sqlite, sql.to_owned());

    let table_exists = conn
        .query_one(query(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'tokens'",
        ))
        .await
        .context("failed to inspect the database")?
        .is_some();

    if !table_exists {
        conn.execute_unprepared(CREATE_TABLE_SQL)
            .await
            .context("failed to create the `tokens` table")?;
        return create_mrs_table(conn).await;
    }

    let has_id = conn
        .query_one(query(
            "SELECT 1 FROM pragma_table_info('tokens') WHERE name = 'id'",
        ))
        .await
        .context("failed to inspect the `tokens` table")?
        .is_some();

    if has_id {
        // v0.2: tokens gained a bound config served by `/convert`.
        add_config_column_if_missing(conn).await?;
        return create_mrs_table(conn).await;
    }

    tracing::info!("Migrating the `tokens` table to the new schema");
    for statement in [
        "ALTER TABLE tokens RENAME TO tokens_legacy".to_owned(),
        CREATE_TABLE_SQL.to_owned(),
        "INSERT INTO tokens (token, name, enabled, expires_at, created_at, last_used_at)
         SELECT token, name, enabled,
                datetime(expires_at, 'unixepoch'),
                datetime(created_at, 'unixepoch'),
                datetime(last_used_at, 'unixepoch')
         FROM tokens_legacy"
            .to_owned(),
        "DROP TABLE tokens_legacy".to_owned(),
    ] {
        conn.execute_unprepared(&statement)
            .await
            .context("failed to migrate the `tokens` table")?;
    }

    create_mrs_table(conn).await
}

/// Create the `mrs_files` table on databases that predate it.
async fn create_mrs_table(conn: &DatabaseConnection) -> Result<()> {
    conn.execute_unprepared(CREATE_MRS_TABLE_SQL)
        .await
        .context("failed to create the `mrs_files` table")?;
    Ok(())
}

async fn add_config_column_if_missing(conn: &DatabaseConnection) -> Result<()> {
    let has_config = conn
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT 1 FROM pragma_table_info('tokens') WHERE name = 'config'".to_owned(),
        ))
        .await
        .context("failed to inspect the `tokens` table")?
        .is_some();

    if !has_config {
        conn.execute_unprepared("ALTER TABLE tokens ADD COLUMN config TEXT NOT NULL DEFAULT ''")
            .await
            .context("failed to add the `config` column")?;
    }

    Ok(())
}
