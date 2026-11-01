use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};

/// Idempotent schema bootstrap (statement-per-`;` split; keep comments out).
const DDL: &str = r"
CREATE TABLE IF NOT EXISTS providers (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    api_key TEXT NOT NULL DEFAULT '',
    is_default INTEGER NOT NULL DEFAULT 0,
    kind TEXT NOT NULL DEFAULT 'openai',
    user_id TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS chats (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT 'New chat',
    provider_id TEXT,
    model TEXT,
    temperature REAL,
    top_p REAL,
    max_tokens INTEGER,
    system_prompt TEXT,
    reasoning_effort TEXT,
    user_id TEXT NOT NULL DEFAULT '',
    pinned INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0,
    tags TEXT NOT NULL DEFAULT '[]',
    tools_enabled INTEGER NOT NULL DEFAULT 0,
    kb_enabled INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    chat_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    reasoning TEXT,
    images TEXT,
    error TEXT,
    reply_to TEXT NOT NULL DEFAULT '',
    active INTEGER NOT NULL DEFAULT 1,
    tool_calls TEXT,
    tool_call_id TEXT,
    sources TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS kb_documents (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    source TEXT NOT NULL,
    title TEXT NOT NULL,
    text TEXT NOT NULL,
    vector TEXT,
    embed_model TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL UNIQUE,
    pass_hash TEXT NOT NULL,
    is_admin INTEGER NOT NULL DEFAULT 0,
    recovery_question TEXT NOT NULL DEFAULT '',
    recovery_answer_hash TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS user_settings (
    user_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (user_id, key)
);
CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS prompts (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS presets (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    name TEXT NOT NULL,
    params TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE IF NOT EXISTS model_registry (
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    vision INTEGER NOT NULL DEFAULT 0,
    reasoning INTEGER NOT NULL DEFAULT 0,
    ctx_len INTEGER,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    PRIMARY KEY (provider_id, model_id)
);
CREATE INDEX IF NOT EXISTS idx_messages_chat ON messages (chat_id, created_at);
CREATE INDEX IF NOT EXISTS idx_chats_updated ON chats (updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_sessions_expiry ON sessions (expires_at);
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    message_id UNINDEXED,
    chat_id UNINDEXED,
    tokenize = 'unicode61 remove_diacritics 2'
)
";

/// Additive ALTERs for pre-existing databases (duplicate column errors are fine).
const MIGRATIONS: &[&str] = &[
    "ALTER TABLE providers ADD COLUMN kind TEXT NOT NULL DEFAULT 'openai'",
    "ALTER TABLE providers ADD COLUMN user_id TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE chats ADD COLUMN user_id TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE chats ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE chats ADD COLUMN archived INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE chats ADD COLUMN tags TEXT NOT NULL DEFAULT '[]'",
    "ALTER TABLE messages ADD COLUMN reply_to TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE messages ADD COLUMN active INTEGER NOT NULL DEFAULT 1",
    "ALTER TABLE chats ADD COLUMN tools_enabled INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE messages ADD COLUMN tool_calls TEXT",
    "ALTER TABLE messages ADD COLUMN tool_call_id TEXT",
    "ALTER TABLE chats ADD COLUMN kb_enabled INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE messages ADD COLUMN sources TEXT",
    "ALTER TABLE users ADD COLUMN recovery_question TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE users ADD COLUMN recovery_answer_hash TEXT NOT NULL DEFAULT ''",
];

pub async fn init_pool(path: &Path) -> Result<SqlitePool> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", path.display()))?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;
    for stmt in DDL.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        sqlx::query(stmt).execute(&pool).await?;
    }
    for stmt in MIGRATIONS.iter().copied() {
        let res = sqlx::query(stmt).execute(&pool).await;
        if let Err(e) = res
            && !e.to_string().contains("duplicate column")
        {
            return Err(e.into());
        }
    }
    // Assign pre-auth rows to the first (bootstrap/admin) user once one exists.
    sqlx::query(
        "UPDATE chats SET user_id = (SELECT id FROM users ORDER BY created_at LIMIT 1)
         WHERE user_id = '' AND EXISTS (SELECT 1 FROM users)",
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "UPDATE providers SET user_id = (SELECT id FROM users ORDER BY created_at LIMIT 1)
         WHERE user_id = '' AND EXISTS (SELECT 1 FROM users)",
    )
    .execute(&pool)
    .await?;
    // One-time backfill of the search index for pre-FTS databases.
    let indexed: i64 = sqlx::query_scalar("SELECT count(*) FROM messages_fts")
        .fetch_one(&pool)
        .await?;
    if indexed == 0 {
        sqlx::query(
            "INSERT INTO messages_fts (content, message_id, chat_id)
             SELECT content, id, chat_id FROM messages WHERE content <> ''",
        )
        .execute(&pool)
        .await?;
    }
    Ok(pool)
}
