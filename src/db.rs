use std::{collections::HashSet, path::Path};

use anyhow::{Context as _, Result};
use chrono::{Local, Utc};
use percent_encoding::utf8_percent_encode;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, ConnectionTrait, Database, DatabaseBackend,
    DatabaseConnection, DatabaseTransaction, EntityTrait, QueryFilter, QueryOrder, Set, Statement,
    TransactionTrait, prelude::DateTime,
};

use crate::entity::{hosted_file, token};

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

const CREATE_HOSTED_TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS hosted_files (
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

    /// Insert a token with a freshly generated random value; returns the new
    /// record.
    pub async fn add(&self, name: &str, days: Option<i64>, config: &str) -> Result<token::Model> {
        let config = normalize_config(config)?;
        let txn = self.conn.begin().await?;
        let token = unused_token(&txn).await?;

        let inserted = token::ActiveModel {
            token: Set(token),
            name: Set(name.to_owned()),
            config: Set(config),
            enabled: Set(true),
            expires_at: Set(days.map(|days| now() + chrono::Duration::days(days))),
            created_at: Set(now()),
            last_used_at: Set(None),
            ..Default::default()
        }
        .insert(&txn)
        .await
        .context("failed to insert the token")?;

        txn.commit().await?;
        Ok(inserted)
    }

    /// Fetch a token by id.
    pub async fn get(&self, id: i32) -> Result<Option<token::Model>> {
        token::Entity::find_by_id(id)
            .one(&self.conn)
            .await
            .context("failed to fetch the token")
    }

    /// Duplicate a token: a fresh random token value with the same name,
    /// expiry and config, plus copies of all its hosted files. Hosted links
    /// inside the config are re-pointed at the new token value, so the copy
    /// keeps working if the source is later deleted or disabled. The copy
    /// starts enabled and unused; returns the new record, or `None` when the
    /// source token does not exist.
    pub async fn duplicate(&self, id: i32) -> Result<Option<token::Model>> {
        let txn = self.conn.begin().await?;
        let Some(source) = token::Entity::find_by_id(id)
            .one(&txn)
            .await
            .context("failed to fetch the token")?
        else {
            return Ok(None);
        };
        let new_token = unused_token(&txn).await?;
        let new_config = swap_token_in_config(&source.config, &source.token, &new_token);

        let inserted = token::ActiveModel {
            token: Set(new_token),
            name: Set(source.name.clone()),
            config: Set(new_config),
            enabled: Set(true),
            expires_at: Set(source.expires_at),
            created_at: Set(now()),
            last_used_at: Set(None),
            ..Default::default()
        }
        .insert(&txn)
        .await
        .context("failed to insert the token")?;

        txn.execute_unprepared(&format!(
            "INSERT INTO hosted_files (token_id, name, source_url, content, updated_at)
             SELECT {}, name, source_url, content, updated_at
             FROM hosted_files WHERE token_id = {}",
            inserted.id, source.id
        ))
        .await
        .context("failed to copy the token's hosted files")?;

        txn.commit().await?;
        Ok(Some(inserted))
    }

    /// Delete a token by id along with its hosted files; returns the number
    /// of removed token rows.
    pub async fn remove(&self, id: i32) -> Result<u64> {
        let txn = self.conn.begin().await?;
        delete_hosted_files(&txn, id).await?;
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

    /// Sync the hosted files of a token after an mrs conversion run:
    /// re-converted files are replaced with fresh content, files whose
    /// provider is still in the config (`keep_names`) but was skipped or
    /// failed this run are preserved, and files whose provider is gone from
    /// the config are dropped.
    pub async fn set_hosted_files(
        &self,
        token_id: i32,
        files: Vec<NewHostedFile>,
        keep_names: &HashSet<String>,
    ) -> Result<()> {
        let txn = self.conn.begin().await?;
        if keep_names.is_empty() {
            delete_hosted_files(&txn, token_id).await?;
        } else {
            hosted_file::Entity::delete_many()
                .filter(hosted_file::Column::TokenId.eq(token_id))
                .filter(
                    hosted_file::Column::Name
                        .is_not_in(keep_names.iter().cloned().collect::<Vec<_>>()),
                )
                .exec(&txn)
                .await
                .map(|_| ())
                .context("failed to delete the token's stale hosted files")?;
        }
        upsert_hosted_files(&txn, token_id, files).await?;
        txn.commit().await?;
        Ok(())
    }

    /// Add or replace the given hosted files without touching anything else;
    /// used by the geo conversion, whose file set is fixed.
    pub async fn upsert_hosted_files(
        &self,
        token_id: i32,
        files: Vec<NewHostedFile>,
    ) -> Result<()> {
        let txn = self.conn.begin().await?;
        upsert_hosted_files(&txn, token_id, files).await?;
        txn.commit().await?;
        Ok(())
    }

    /// Fetch one of a token's hosted files by name.
    pub async fn get_hosted_file(
        &self,
        token_id: i32,
        name: &str,
    ) -> Result<Option<hosted_file::Model>> {
        hosted_file::Entity::find()
            .filter(hosted_file::Column::TokenId.eq(token_id))
            .filter(hosted_file::Column::Name.eq(name))
            .one(&self.conn)
            .await
            .context("failed to fetch the hosted file")
    }
}

/// A new hosted file for a token.
pub struct NewHostedFile {
    pub name: String,
    pub source_url: String,
    pub content: Vec<u8>,
}

async fn upsert_hosted_files(
    txn: &DatabaseTransaction,
    token_id: i32,
    files: Vec<NewHostedFile>,
) -> Result<()> {
    for file in files {
        hosted_file::Entity::delete_many()
            .filter(hosted_file::Column::TokenId.eq(token_id))
            .filter(hosted_file::Column::Name.eq(&file.name))
            .exec(txn)
            .await
            .map(|_| ())
            .context("failed to replace the hosted file")?;
        hosted_file::ActiveModel {
            token_id: Set(token_id),
            name: Set(file.name),
            source_url: Set(file.source_url),
            content: Set(file.content),
            updated_at: Set(now()),
            ..Default::default()
        }
        .insert(txn)
        .await
        .context("failed to store the hosted file")?;
    }
    Ok(())
}

async fn delete_hosted_files(txn: &DatabaseTransaction, token_id: i32) -> Result<()> {
    hosted_file::Entity::delete_many()
        .filter(hosted_file::Column::TokenId.eq(token_id))
        .exec(txn)
        .await
        .map(|_| ())
        .context("failed to delete the token's hosted files")
}

/// Re-point hosted download links at a new token value: rewrite every
/// `token=<old>` occurrence in the config, in raw or percent-encoded form
/// (the form `download_url` writes).
fn swap_token_in_config(config: &str, old: &str, new: &str) -> String {
    let encode = |value: &str| utf8_percent_encode(value, crate::mrs::URL_SAFE).to_string();
    let mut swapped = config.replace(&format!("token={old}"), &format!("token={new}"));
    let (encoded_old, encoded_new) = (encode(old), encode(new));
    if encoded_old != old {
        swapped = swapped.replace(
            &format!("token={encoded_old}"),
            &format!("token={encoded_new}"),
        );
    }
    swapped
}

/// Alphabet for generated tokens: URL-safe, 6 bits of entropy per character.
const TOKEN_ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
const TOKEN_LENGTH: usize = 16;

/// A random URL-safe token value, 96 bits of entropy.
fn generate_token() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..TOKEN_LENGTH)
        .map(|_| TOKEN_ALPHABET[rng.random_range(0..TOKEN_ALPHABET.len())] as char)
        .collect()
}

/// A generated token value not yet present in the database.
async fn unused_token(txn: &DatabaseTransaction) -> Result<String> {
    loop {
        let candidate = generate_token();
        let taken = token::Entity::find()
            .filter(token::Column::Token.eq(&candidate))
            .one(txn)
            .await
            .context("failed to check the token value")?
            .is_some();
        if !taken {
            return Ok(candidate);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_token_rewrites_raw_and_encoded_links() {
        let config = "\
rule-providers:
  a:
    url: http://h/files/a.mrs?token=tok+1
geox-url:
  geoip: http://h/files/geoip?token=tok%2B1
unrelated:
  secret: token=other-value
";
        let swapped = swap_token_in_config(config, "tok+1", "new");
        assert!(swapped.contains("a.mrs?token=new"));
        assert!(swapped.contains("geoip?token=new"));
        // Different token values stay untouched.
        assert!(swapped.contains("token=other-value"));
        assert!(!swapped.contains("tok%2B1"));
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
        return create_hosted_table(conn).await;
    }

    let has_id = conn
        .query_one(query(
            "SELECT 1 FROM pragma_table_info('tokens') WHERE name = 'id'",
        ))
        .await
        .context("failed to inspect the `tokens` table")?
        .is_some();

    if has_id {
        // v0.2: tokens gained a bound config.
        add_config_column_if_missing(conn).await?;
        return create_hosted_table(conn).await;
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

    create_hosted_table(conn).await
}

/// Create the `hosted_files` table; databases from before the rename carry
/// the same table as `mrs_files`, which is renamed in place.
async fn create_hosted_table(conn: &DatabaseConnection) -> Result<()> {
    let table_exists = |name: &str| {
        Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '{name}'"),
        )
    };
    let hosted_exists = conn
        .query_one(table_exists("hosted_files"))
        .await
        .context("failed to inspect the database")?
        .is_some();
    if hosted_exists {
        return Ok(());
    }

    let mrs_exists = conn
        .query_one(table_exists("mrs_files"))
        .await
        .context("failed to inspect the database")?
        .is_some();
    if mrs_exists {
        tracing::info!("Renaming `mrs_files` to `hosted_files`");
        conn.execute_unprepared("ALTER TABLE mrs_files RENAME TO hosted_files")
            .await
            .context("failed to rename the `mrs_files` table")?;
        return Ok(());
    }

    conn.execute_unprepared(CREATE_HOSTED_TABLE_SQL)
        .await
        .context("failed to create the `hosted_files` table")?;
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
