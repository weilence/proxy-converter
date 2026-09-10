use std::path::Path;

use anyhow::{Context as _, Result};
use chrono::{Local, Utc};
use sea_orm::{
    prelude::DateTime, ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, Database,
    DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, Set, Statement,
};

use crate::entity::token;

const CREATE_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS tokens (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    token        TEXT NOT NULL UNIQUE,
    name         TEXT NOT NULL DEFAULT '',
    enabled      INTEGER NOT NULL DEFAULT 1,
    expires_at   TEXT,
    created_at   TEXT NOT NULL,
    last_used_at TEXT
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

    /// Check whether a token is currently valid and record its use.
    pub async fn verify(&self, token: &str) -> bool {
        let found = token::Entity::find()
            .filter(token::Column::Token.eq(token))
            .filter(token::Column::Enabled.eq(true))
            .filter(
                Condition::any()
                    .add(token::Column::ExpiresAt.is_null())
                    .add(token::Column::ExpiresAt.gt(now())),
            )
            .one(&self.conn)
            .await;

        let valid = matches!(found, Ok(Some(_)));

        if valid {
            let _ = token::Entity::update_many()
                .filter(token::Column::Token.eq(token))
                .set(token::ActiveModel {
                    last_used_at: Set(Some(now())),
                    ..Default::default()
                })
                .exec(&self.conn)
                .await;
        }

        valid
    }

    /// Insert a token; returns `false` when it already exists.
    pub async fn add(&self, token: &str, name: &str, days: Option<i64>) -> Result<bool> {
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

    /// Delete a token; returns the number of removed rows.
    pub async fn remove(&self, token: &str) -> Result<u64> {
        token::Entity::delete_many()
            .filter(token::Column::Token.eq(token))
            .exec(&self.conn)
            .await
            .map(|result| result.rows_affected)
            .context("failed to delete the token")
    }

    /// Delete a token by id; returns the number of removed rows.
    pub async fn remove_by_id(&self, id: i32) -> Result<u64> {
        token::Entity::delete_by_id(id)
            .exec(&self.conn)
            .await
            .map(|result| result.rows_affected)
            .context("failed to delete the token")
    }

    /// Enable or disable a token; returns the number of updated rows.
    pub async fn set_enabled(&self, token: &str, enabled: bool) -> Result<u64> {
        token::Entity::update_many()
            .filter(token::Column::Token.eq(token))
            .set(token::ActiveModel {
                enabled: Set(enabled),
                ..Default::default()
            })
            .exec(&self.conn)
            .await
            .map(|result| result.rows_affected)
            .context("failed to update the token")
    }

    /// Enable or disable a token by id; returns the number of updated rows.
    pub async fn set_enabled_by_id(&self, id: i32, enabled: bool) -> Result<u64> {
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

    /// List all tokens, oldest first.
    pub async fn list(&self) -> Result<Vec<token::Model>> {
        token::Entity::find()
            .order_by_asc(token::Column::Id)
            .all(&self.conn)
            .await
            .context("failed to list the tokens")
    }
}

/// Current UTC time; stored as `YYYY-MM-DD HH:MM:SS`.
pub fn now() -> DateTime {
    Utc::now().naive_utc()
}

/// Human-readable status of a token record.
pub fn token_status(record: &token::Model, now: DateTime) -> &'static str {
    if !record.enabled {
        "disabled"
    } else if record.expires_at.is_some_and(|expires_at| expires_at <= now) {
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
        return Ok(());
    }

    let has_id = conn
        .query_one(query(
            "SELECT 1 FROM pragma_table_info('tokens') WHERE name = 'id'",
        ))
        .await
        .context("failed to inspect the `tokens` table")?
        .is_some();

    if has_id {
        return Ok(());
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

    Ok(())
}
