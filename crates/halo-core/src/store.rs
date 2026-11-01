use anyhow::{Result, anyhow};
use chrono::{TimeDelta, Utc};
use serde_json::{Value, json};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::crypto::Cipher;
use crate::models::{Chat, KbDoc, Message, Preset, Prompt, Provider, User};

pub type Json = Value;

pub type ChatWithMessages = (Chat, Vec<Message>);

/// Thin persistence layer over SQLite. CRUD helpers used by halo-server.
#[derive(Clone)]
pub struct Store {
    pub pool: SqlitePool,
    cipher: Option<Cipher>,
}

impl Store {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool, cipher: None }
    }

    pub fn with_cipher(pool: SqlitePool, cipher: Cipher) -> Self {
        Self {
            pool,
            cipher: Some(cipher),
        }
    }

    fn encrypt(&self, key: &str) -> String {
        match &self.cipher {
            Some(c) => c.encrypt(key),
            None => key.to_string(),
        }
    }

    fn decrypt(&self, stored: &str) -> String {
        match &self.cipher {
            Some(c) => c.decrypt(stored),
            None => stored.to_string(),
        }
    }

    /// Encrypt any still-plaintext provider keys once (post-upgrade migration).
    pub async fn migrate_plaintext_keys(&self) -> Result<()> {
        if self.cipher.is_none() {
            return Ok(());
        }
        let rows = sqlx::query("SELECT id, api_key FROM providers")
            .fetch_all(&self.pool)
            .await?;
        for r in rows {
            let key: String = r.try_get("api_key")?;
            if !key.is_empty() && !crate::crypto::is_cipher(&key) {
                let enc = self.encrypt(&key);
                sqlx::query("UPDATE providers SET api_key = ? WHERE id = ?")
                    .bind(enc)
                    .bind(r.try_get::<String, _>("id")?)
                    .execute(&self.pool)
                    .await?;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------------ providers

    pub async fn list_providers(&self, user_id: &str) -> Result<Vec<Provider>> {
        let mut rows = sqlx::query_as::<_, Provider>(
            "SELECT id, name, base_url, api_key, is_default, kind, user_id, created_at
             FROM providers WHERE user_id = ? ORDER BY is_default DESC, created_at",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        for p in &mut rows {
            p.api_key = self.decrypt(&p.api_key);
        }
        Ok(rows)
    }

    /// Owner-scoped provider fetch; other users' providers read as missing.
    pub async fn get_provider(&self, user_id: &str, id: &str) -> Result<Option<Provider>> {
        let mut row = sqlx::query_as::<_, Provider>(
            "SELECT id, name, base_url, api_key, is_default, kind, user_id, created_at
             FROM providers WHERE id = ? AND user_id = ?",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(p) = &mut row {
            p.api_key = self.decrypt(&p.api_key);
        }
        Ok(row)
    }

    pub async fn create_provider(&self, user_id: &str, v: &Json) -> Result<Provider> {
        let id = Uuid::now_v7().to_string();
        let is_first = self.list_providers(user_id).await?.is_empty();
        let is_default = is_first || v.get("is_default").and_then(Json::as_bool).unwrap_or(false);
        if is_default {
            sqlx::query("UPDATE providers SET is_default = 0 WHERE user_id = ?")
                .bind(user_id)
                .execute(&self.pool)
                .await?;
        }
        sqlx::query("INSERT INTO providers (id, name, base_url, api_key, is_default, kind, user_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(&id)
            .bind(required_str(v, "name")?)
            .bind(provider_base_url(v)?)
            .bind(self.encrypt(&optional_str(v, "api_key").unwrap_or_default()))
            .bind(is_default)
            .bind(optional_str(v, "kind").unwrap_or_else(|| "openai".to_string()))
            .bind(user_id)
            .bind(Utc::now())
            .execute(&self.pool)
            .await?;
        self.get_provider(user_id, &id)
            .await?
            .ok_or_else(|| anyhow!("inserted provider vanished"))
    }

    /// Full-row update after applying a partial JSON patch. An empty `api_key`
    /// patch keeps the existing key.
    pub async fn update_provider(
        &self,
        user_id: &str,
        id: &str,
        patch: &Json,
    ) -> Result<Option<Provider>> {
        let Some(mut p) = self.get_provider(user_id, id).await? else {
            return Ok(None);
        };
        if let Some(name) = optional_str(patch, "name").filter(|s| !s.trim().is_empty()) {
            p.name = name;
        }
        if let Some(value) = patch.get("base_url").filter(|v| !v.is_null()) {
            p.base_url = provider_base_url(&Json::Object(serde_json::Map::from_iter([(
                "base_url".to_string(),
                value.clone(),
            )])))?;
        }
        if let Some(key) = optional_str(patch, "api_key").filter(|s| !s.is_empty()) {
            p.api_key = key;
        }
        if let Some(kind) = optional_str(patch, "kind") {
            p.kind = kind;
        }
        if patch
            .get("is_default")
            .and_then(Json::as_bool)
            .unwrap_or(false)
        {
            sqlx::query("UPDATE providers SET is_default = 0 WHERE user_id = ?")
                .bind(user_id)
                .execute(&self.pool)
                .await?;
            p.is_default = true;
        }
        sqlx::query("UPDATE providers SET name = ?, base_url = ?, api_key = ?, is_default = ?, kind = ? WHERE id = ? AND user_id = ?")
            .bind(&p.name)
            .bind(&p.base_url)
            .bind(self.encrypt(&p.api_key))
            .bind(p.is_default)
            .bind(&p.kind)
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(Some(p))
    }

    pub async fn delete_provider(&self, user_id: &str, id: &str) -> Result<bool> {
        Ok(
            sqlx::query("DELETE FROM providers WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }

    // ----------------------------------------------------------------------- chats

    pub async fn list_chats(&self, user_id: &str, include_archived: bool) -> Result<Vec<Chat>> {
        let sql = if include_archived {
            "SELECT * FROM chats WHERE user_id = ? ORDER BY pinned DESC, updated_at DESC LIMIT 500"
        } else {
            "SELECT * FROM chats WHERE user_id = ? AND archived = 0 ORDER BY pinned DESC, updated_at DESC LIMIT 500"
        };
        Ok(sqlx::query_as::<_, Chat>(sql)
            .bind(user_id)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Owner-scoped chat fetch; other users' chats read as missing.
    pub async fn get_chat_for_user(&self, user_id: &str, id: &str) -> Result<Option<Chat>> {
        Ok(
            sqlx::query_as::<_, Chat>("SELECT * FROM chats WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    async fn get_chat(&self, id: &str) -> Result<Option<Chat>> {
        Ok(
            sqlx::query_as::<_, Chat>("SELECT * FROM chats WHERE id = ?")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn get_messages(&self, chat_id: &str) -> Result<Vec<Message>> {
        Ok(sqlx::query_as::<_, Message>(
            "SELECT * FROM messages WHERE chat_id = ? ORDER BY created_at, rowid",
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn get_chat_with_messages_for_user(
        &self,
        user_id: &str,
        id: &str,
    ) -> Result<Option<ChatWithMessages>> {
        let Some(chat) = self.get_chat_for_user(user_id, id).await? else {
            return Ok(None);
        };
        let messages = self.get_messages(id).await?;
        Ok(Some((chat, messages)))
    }

    /// Create a chat owned by `user_id`, defaulting provider to the user's
    /// default one (or their first known).
    pub async fn create_chat(&self, user_id: &str, v: &Json) -> Result<Chat> {
        let id = Uuid::now_v7().to_string();
        let mut provider_id = optional_str(v, "provider_id");
        if provider_id.is_none() {
            let providers = self.list_providers(user_id).await?;
            provider_id = providers
                .iter()
                .find(|p| p.is_default)
                .or_else(|| providers.first())
                .map(|p| p.id.clone());
        }
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO chats (id, title, provider_id, model, temperature, top_p, max_tokens,
                                system_prompt, reasoning_effort, user_id, tags, tools_enabled,
                                kb_enabled, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(
            optional_str(v, "title")
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "New chat".to_string()),
        )
        .bind(provider_id)
        .bind(optional_str(v, "model"))
        .bind(v.get("temperature").and_then(Json::as_f64))
        .bind(v.get("top_p").and_then(Json::as_f64))
        .bind(v.get("max_tokens").and_then(Json::as_i64))
        .bind(optional_str(v, "system_prompt"))
        .bind(optional_str(v, "reasoning_effort").filter(|s| s != "none"))
        .bind(user_id)
        .bind(normalize_tags(v.get("tags")))
        .bind(
            v.get("tools_enabled")
                .and_then(Json::as_bool)
                .unwrap_or(false),
        )
        .bind(v.get("kb_enabled").and_then(Json::as_bool).unwrap_or(false))
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        self.get_chat(&id)
            .await?
            .ok_or_else(|| anyhow!("inserted chat vanished"))
    }

    /// Partial update of chat settings (ADR-005): present JSON keys are applied,
    /// explicit nulls clear the column.
    pub async fn update_chat(&self, user_id: &str, id: &str, patch: &Json) -> Result<Option<Chat>> {
        let Some(mut c) = self.get_chat_for_user(user_id, id).await? else {
            return Ok(None);
        };
        if let Some(v) = patch.get("title")
            && let Some(s) = v.as_str().map(str::trim).filter(|s| !s.is_empty())
        {
            c.title = s.to_string();
        }
        if let Some(v) = patch.get("provider_id") {
            c.provider_id = v.as_str().map(str::to_string);
        }
        if let Some(v) = patch.get("model") {
            c.model = v.as_str().map(str::to_string);
        }
        if let Some(v) = patch.get("temperature") {
            c.temperature = opt_f64(v);
        }
        if let Some(v) = patch.get("top_p") {
            c.top_p = opt_f64(v);
        }
        if let Some(v) = patch.get("max_tokens") {
            c.max_tokens = v.as_i64();
        }
        if let Some(v) = patch.get("system_prompt") {
            c.system_prompt = v.as_str().filter(|s| !s.is_empty()).map(str::to_string);
        }
        if let Some(v) = patch.get("reasoning_effort") {
            c.reasoning_effort = v
                .as_str()
                .filter(|s| !s.is_empty() && *s != "none")
                .map(str::to_string);
        }
        if let Some(v) = patch.get("pinned") {
            c.pinned = v.as_bool().unwrap_or(false);
        }
        if let Some(v) = patch.get("archived") {
            c.archived = v.as_bool().unwrap_or(false);
        }
        if let Some(v) = patch.get("tools_enabled") {
            c.tools_enabled = v.as_bool().unwrap_or(false);
        }
        if let Some(v) = patch.get("kb_enabled") {
            c.kb_enabled = v.as_bool().unwrap_or(false);
        }
        if patch.get("tags").is_some() {
            c.tags = normalize_tags(patch.get("tags"));
        }
        sqlx::query(
            "UPDATE chats SET title = ?, provider_id = ?, model = ?, temperature = ?, top_p = ?,
             max_tokens = ?, system_prompt = ?, reasoning_effort = ?, pinned = ?, archived = ?,
             tools_enabled = ?, kb_enabled = ?, tags = ?, updated_at = ? WHERE id = ? AND user_id = ?",
        )
        .bind(&c.title)
        .bind(&c.provider_id)
        .bind(&c.model)
        .bind(c.temperature)
        .bind(c.top_p)
        .bind(c.max_tokens)
        .bind(&c.system_prompt)
        .bind(&c.reasoning_effort)
        .bind(c.pinned)
        .bind(c.archived)
        .bind(c.tools_enabled)
        .bind(c.kb_enabled)
        .bind(&c.tags)
        .bind(Utc::now())
        .bind(id)
        .bind(user_id)
        .execute(&self.pool)
        .await?;
        Ok(Some(c))
    }

    pub async fn touch_chat(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE chats SET updated_at = ? WHERE id = ?")
            .bind(Utc::now())
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_chat(&self, user_id: &str, id: &str) -> Result<bool> {
        if self.get_chat_for_user(user_id, id).await?.is_none() {
            return Ok(false);
        }
        sqlx::query("DELETE FROM messages WHERE chat_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM messages_fts WHERE chat_id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(
            sqlx::query("DELETE FROM chats WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }

    // ------------------------------------------------------------------- messages

    pub async fn insert_message(
        &self,
        chat_id: &str,
        role: &str,
        content: &str,
        images: Option<&str>,
    ) -> Result<Message> {
        let id = Uuid::now_v7().to_string();
        sqlx::query("INSERT INTO messages (id, chat_id, role, content, images, created_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(&id)
            .bind(chat_id)
            .bind(role)
            .bind(content)
            .bind(images)
            .bind(Utc::now())
            .execute(&self.pool)
            .await?;
        if !content.is_empty() {
            self.fts_upsert(&id, chat_id, content).await?;
        }
        sqlx::query_as::<_, Message>("SELECT * FROM messages WHERE id = ?")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    /// Persist a `tool`-role row: `tool_calls` holds the full assistant tool_calls
    /// array for that round (so history can be replayed); `tool_call_id` is the id
    /// this row answers. Not indexed in FTS (machine output, not user content).
    pub async fn insert_tool_message(
        &self,
        chat_id: &str,
        tool_call_id: &str,
        tool_calls: &str,
        content: &str,
    ) -> Result<Message> {
        let id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO messages (id, chat_id, role, content, tool_calls, tool_call_id, created_at)
             VALUES (?, ?, 'tool', ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(chat_id)
        .bind(content)
        .bind(tool_calls)
        .bind(tool_call_id)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        sqlx::query_as::<_, Message>("SELECT * FROM messages WHERE id = ?")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    /// Persist KB chunks for one uploaded source; `vectors` (if present) must
    /// parallel `chunks`. Returns the stored rows in order.
    pub async fn insert_kb_docs(
        &self,
        user_id: &str,
        source: &str,
        embed_model: &str,
        chunks: &[String],
        vectors: Option<&[Vec<f32>]>,
    ) -> Result<Vec<KbDoc>> {
        let mut out = Vec::with_capacity(chunks.len());
        for (i, chunk) in chunks.iter().enumerate() {
            let id = Uuid::now_v7().to_string();
            let title = if chunks.len() == 1 {
                source.to_string()
            } else {
                format!("{source} · {}", i + 1)
            };
            let vector = vectors.map(|vs| serde_json::to_string(&vs[i]).unwrap_or_default());
            sqlx::query(
                "INSERT INTO kb_documents (id, user_id, source, title, text, vector, embed_model, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&id)
            .bind(user_id)
            .bind(source)
            .bind(&title)
            .bind(chunk)
            .bind(&vector)
            .bind(embed_model)
            .bind(Utc::now())
            .execute(&self.pool)
            .await?;
            out.push(KbDoc {
                id,
                user_id: user_id.to_string(),
                source: source.to_string(),
                title,
                text: chunk.clone(),
                vector,
                embed_model: embed_model.to_string(),
                created_at: Utc::now(),
            });
        }
        Ok(out)
    }

    pub async fn list_kb(&self, user_id: &str) -> Result<Vec<KbDoc>> {
        Ok(sqlx::query_as::<_, KbDoc>(
            "SELECT * FROM kb_documents WHERE user_id = ? ORDER BY source, created_at, rowid",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    /// All chunks that have vectors, decoded, for brute-force cosine search.
    pub async fn kb_search_pool(&self, user_id: &str) -> Result<Vec<(KbDoc, Vec<f32>)>> {
        let rows = sqlx::query_as::<_, KbDoc>(
            "SELECT * FROM kb_documents WHERE user_id = ? AND vector IS NOT NULL",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .filter_map(|d| {
                let v = serde_json::from_str::<Vec<f32>>(d.vector.as_deref()?).ok()?;
                Some((d, v))
            })
            .collect())
    }

    pub async fn delete_kb_doc(&self, user_id: &str, id: &str) -> Result<bool> {
        let n = sqlx::query("DELETE FROM kb_documents WHERE id = ? AND user_id = ?")
            .bind(id)
            .bind(user_id)
            .execute(&self.pool)
            .await?
            .rows_affected();
        Ok(n > 0)
    }

    pub async fn delete_kb_source(&self, user_id: &str, source: &str) -> Result<u64> {
        Ok(
            sqlx::query("DELETE FROM kb_documents WHERE user_id = ? AND source = ?")
                .bind(user_id)
                .bind(source)
                .execute(&self.pool)
                .await?
                .rows_affected(),
        )
    }

    pub async fn set_message_sources(&self, message_id: &str, sources_json: &str) -> Result<()> {
        sqlx::query("UPDATE messages SET sources = ? WHERE id = ?")
            .bind(sources_json)
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Insert a new assistant reply in `reply_to`'s branch group, deactivating
    /// previous siblings (regenerate-as-branch, 2.2d).
    pub async fn insert_assistant_reply(&self, chat_id: &str, reply_to: &str) -> Result<Message> {
        sqlx::query(
            "UPDATE messages SET active = 0 WHERE chat_id = ? AND reply_to = ? AND active = 1",
        )
        .bind(chat_id)
        .bind(reply_to)
        .execute(&self.pool)
        .await?;
        let id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO messages (id, chat_id, role, content, images, reply_to, active, created_at)
             VALUES (?, ?, 'assistant', '', NULL, ?, 1, ?)",
        )
        .bind(&id)
        .bind(chat_id)
        .bind(reply_to)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        sqlx::query_as::<_, Message>("SELECT * FROM messages WHERE id = ?")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(Into::into)
    }

    /// Switch the active branch reply for a user message; returns the new active id.
    pub async fn set_active_message(&self, chat_id: &str, msg_id: &str) -> Result<bool> {
        let Some(row) = sqlx::query_as::<_, (String, String, bool)>(
            "SELECT role, reply_to, active FROM messages WHERE id = ? AND chat_id = ?",
        )
        .bind(msg_id)
        .bind(chat_id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(false);
        };
        if row.0 != "assistant" {
            return Ok(false);
        }
        sqlx::query(
            "UPDATE messages SET active = 0 WHERE chat_id = ? AND reply_to = ? AND reply_to != ''",
        )
        .bind(chat_id)
        .bind(&row.1)
        .execute(&self.pool)
        .await?;
        sqlx::query("UPDATE messages SET active = 1 WHERE id = ?")
            .bind(msg_id)
            .execute(&self.pool)
            .await?;
        Ok(!row.2)
    }

    /// Count + position of `msg_id` among its branch siblings (for the ‹i/N› pager).
    pub async fn branch_info(&self, chat_id: &str, msg_id: &str) -> Result<(i64, i64)> {
        let reply_to: Option<String> =
            sqlx::query_as("SELECT reply_to FROM messages WHERE id = ? AND chat_id = ?")
                .bind(msg_id)
                .bind(chat_id)
                .fetch_optional(&self.pool)
                .await?
                .map(|(r,)| r);
        let Some(reply_to) = reply_to.filter(|r| !r.is_empty()) else {
            return Ok((0, 0));
        };
        let siblings = sqlx::query_as::<_, (String,)>(
            "SELECT id FROM messages WHERE chat_id = ? AND reply_to = ? ORDER BY created_at, rowid",
        )
        .bind(chat_id)
        .bind(reply_to)
        .fetch_all(&self.pool)
        .await?;
        let index = siblings
            .iter()
            .position(|s| s.0 == msg_id)
            .map_or(0, |p| p as i64);
        Ok((index, siblings.len() as i64))
    }

    /// Idempotent finalize of a streamed assistant message.
    pub async fn finalize_message(
        &self,
        id: &str,
        content: &str,
        reasoning: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query("UPDATE messages SET content = ?, reasoning = ?, error = ? WHERE id = ?")
            .bind(content)
            .bind(reasoning)
            .bind(error)
            .bind(id)
            .execute(&self.pool)
            .await?;
        self.fts_sync(id).await?;
        Ok(())
    }

    pub async fn update_message_content(&self, id: &str, content: &str) -> Result<bool> {
        let changed = sqlx::query("UPDATE messages SET content = ? WHERE id = ?")
            .bind(content)
            .bind(id)
            .execute(&self.pool)
            .await?
            .rows_affected()
            > 0;
        if changed {
            self.fts_sync(id).await?;
        }
        Ok(changed)
    }

    /// Delete messages of a chat starting at `from_id`; `inclusive` decides
    /// whether the anchor message itself is deleted. Returns deleted ids.
    pub async fn truncate_messages(
        &self,
        chat_id: &str,
        from_id: &str,
        inclusive: bool,
    ) -> Result<Vec<String>> {
        let all = self.get_messages(chat_id).await?;
        let Some(pos) = all.iter().position(|m| m.id == from_id) else {
            return Ok(Vec::new());
        };
        let start = if inclusive { pos } else { pos + 1 };
        let doomed: Vec<String> = all[start..].iter().map(|m| m.id.clone()).collect();
        for id in &doomed {
            sqlx::query("DELETE FROM messages WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
            sqlx::query("DELETE FROM messages_fts WHERE message_id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
        }
        Ok(doomed)
    }

    /// Drop everything after the user message `user_msg_id` except its
    /// assistant replies, which stay as branch siblings managed by
    /// [`Store::insert_assistant_reply`] / [`Store::set_active_message`].
    pub async fn truncate_after_keeping_branch(
        &self,
        chat_id: &str,
        user_msg_id: &str,
    ) -> Result<Vec<String>> {
        let all = self.get_messages(chat_id).await?;
        let Some(pos) = all.iter().position(|m| m.id == user_msg_id) else {
            return Ok(Vec::new());
        };
        let doomed: Vec<String> = all[pos + 1..]
            .iter()
            .filter(|m| !(m.role == "assistant" && m.reply_to == user_msg_id))
            .map(|m| m.id.clone())
            .collect();
        for id in &doomed {
            sqlx::query("DELETE FROM messages WHERE id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
            sqlx::query("DELETE FROM messages_fts WHERE message_id = ?")
                .bind(id)
                .execute(&self.pool)
                .await?;
        }
        Ok(doomed)
    }

    // ------------------------------------------------------------------- search

    /// Prefix search over the user's non-archived chats. Each whitespace-
    /// separated token becomes an AND-ed FTS prefix term; FTS operators are
    /// stripped so any user input is safe. Tokens with a `#` prefix match chat
    /// tags instead of message text (tag-only queries scan chat tags directly).
    pub async fn search(&self, user_id: &str, query: &str, limit: i64) -> Result<Vec<Json>> {
        let (terms, tags) = split_search_tokens(query);
        if terms.is_empty() && tags.is_empty() {
            return Ok(Vec::new());
        }

        fn parse_tags(raw: &str) -> Vec<String> {
            serde_json::from_str(raw).unwrap_or_default()
        }
        let has_tags = |raw: &str, want: &[String]| -> bool {
            let own = parse_tags(raw);
            want.iter().all(|t| own.iter().any(|o| o == t))
        };

        let chat_tags_map = if tags.is_empty() {
            None
        } else {
            let rows = sqlx::query("SELECT id, tags FROM chats WHERE user_id = ? AND archived = 0")
                .bind(user_id)
                .fetch_all(&self.pool)
                .await?;
            let mut m = serde_json::Map::new();
            for r in rows {
                let id: String = r.try_get("id")?;
                let raw: String = r.try_get("tags")?;
                m.insert(id, Json::String(raw));
            }
            Some(m)
        };
        let tag_hit = |id: &str| -> bool {
            match &chat_tags_map {
                Some(m) => m
                    .get(id)
                    .and_then(|v| v.as_str())
                    .is_some_and(|raw| has_tags(raw, &tags)),
                None => true,
            }
        };

        if terms.is_empty() {
            let rows = sqlx::query(
                "SELECT id, title, tags FROM chats
                 WHERE user_id = ? AND archived = 0
                 ORDER BY updated_at DESC LIMIT ?",
            )
            .bind(user_id)
            .bind(limit * 8)
            .fetch_all(&self.pool)
            .await?;
            return rows
                .iter()
                .filter(|r| tag_hit(&r.try_get::<String, _>("id").unwrap_or_default()))
                .take(limit as usize)
                .map(|r| {
                    Ok(json!({
                        "chat_id": r.try_get::<String, _>("id")?,
                        "message_id": format!("{}:tag", r.try_get::<String, _>("id")?),
                        "chat_title": r.try_get::<String, _>("title")?,
                        "role": "chat",
                        "snippet": parse_tags(&r.try_get::<String, _>("tags")?)
                            .iter()
                            .map(|t| format!("#{t}"))
                            .collect::<Vec<_>>()
                            .join(" "),
                    }))
                })
                .collect();
        }

        let match_expr = terms
            .iter()
            .map(|t| format!("\"{t}\"*"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let rows = sqlx::query(
            "SELECT f.message_id, f.chat_id, c.title, m.role,
                    snippet(messages_fts, 0, '[', ']', ' … ', 16) AS snip
             FROM messages_fts f
             JOIN chats c ON c.id = f.chat_id
             JOIN messages m ON m.id = f.message_id
             WHERE messages_fts MATCH ? AND c.user_id = ? AND c.archived = 0
             ORDER BY rank
             LIMIT ?",
        )
        .bind(&match_expr)
        .bind(user_id)
        .bind(limit * 8)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .filter(|r| tag_hit(&r.try_get::<String, _>("chat_id").unwrap_or_default()))
            .take(limit as usize)
            .map(|r| {
                Ok(json!({
                    "chat_id": r.try_get::<String, _>("chat_id")?,
                    "message_id": r.try_get::<String, _>("message_id")?,
                    "chat_title": r.try_get::<String, _>("title")?,
                    "role": r.try_get::<String, _>("role")?,
                    "snippet": r.try_get::<String, _>("snip")?,
                }))
            })
            .collect()
    }

    // ------------------------------------------------------------------ users

    pub async fn count_users(&self) -> Result<i64> {
        Ok(sqlx::query_as::<_, (i64,)>("SELECT COUNT(*) FROM users")
            .fetch_one(&self.pool)
            .await?
            .0)
    }

    pub async fn create_user(
        &self,
        username: &str,
        pass_hash: &str,
        is_admin: bool,
    ) -> Result<User> {
        let id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO users (id, username, pass_hash, is_admin, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(username)
        .bind(pass_hash)
        .bind(is_admin)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        self.get_user_by_name(username)
            .await?
            .ok_or_else(|| anyhow!("inserted user vanished"))
    }

    pub async fn get_user_by_name(&self, username: &str) -> Result<Option<User>> {
        Ok(sqlx::query_as::<_, User>(
            "SELECT id, username, pass_hash, is_admin, created_at FROM users WHERE username = ?",
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn get_user(&self, id: &str) -> Result<Option<User>> {
        Ok(sqlx::query_as::<_, User>(
            "SELECT id, username, pass_hash, is_admin, created_at FROM users WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Adopt pre-auth orphan rows (empty `user_id`) for the bootstrap admin.
    pub async fn assign_legacy_rows(&self, user_id: &str) -> Result<()> {
        sqlx::query("UPDATE chats SET user_id = ? WHERE user_id = ''")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("UPDATE providers SET user_id = ? WHERE user_id = ''")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------- sessions

    pub async fn create_session(&self, user_id: &str, token_hash: &str, days: i64) -> Result<()> {
        sqlx::query("INSERT INTO sessions (token_hash, user_id, expires_at) VALUES (?, ?, ?)")
            .bind(token_hash)
            .bind(user_id)
            .bind(Utc::now() + TimeDelta::days(days))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Resolve a valid session, sliding its expiry forward.
    pub async fn validate_session(&self, token_hash: &str) -> Result<Option<String>> {
        let Some((user_id,)) = sqlx::query_as::<_, (String,)>(
            "SELECT user_id FROM sessions WHERE token_hash = ? AND expires_at > ?",
        )
        .bind(token_hash)
        .bind(Utc::now())
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        sqlx::query("UPDATE sessions SET expires_at = ? WHERE token_hash = ?")
            .bind(Utc::now() + TimeDelta::days(30))
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(Some(user_id))
    }

    pub async fn delete_session(&self, token_hash: &str) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
            .bind(token_hash)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn prune_sessions(&self) -> Result<()> {
        sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
            .bind(Utc::now())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // ---------------------------------------------------------------- settings

    pub async fn get_user_setting(&self, user_id: &str, key: &str) -> Result<Option<String>> {
        Ok(sqlx::query_as::<_, (String,)>(
            "SELECT value FROM user_settings WHERE user_id = ? AND key = ?",
        )
        .bind(user_id)
        .bind(key)
        .fetch_optional(&self.pool)
        .await?
        .map(|(v,)| v))
    }

    pub async fn list_user_settings(&self, user_id: &str) -> Result<Vec<(String, String)>> {
        Ok(
            sqlx::query_as("SELECT key, value FROM user_settings WHERE user_id = ? ORDER BY key")
                .bind(user_id)
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn set_user_setting(&self, user_id: &str, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO user_settings (user_id, key, value) VALUES (?, ?, ?)
             ON CONFLICT(user_id, key) DO UPDATE SET value = excluded.value",
        )
        .bind(user_id)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn get_app_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(
            sqlx::query_as::<_, (String,)>("SELECT value FROM app_settings WHERE key = ?")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?
                .map(|(v,)| v),
        )
    }

    pub async fn set_app_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES (?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ----------------------------------------------------------------- prompts

    pub async fn list_prompts(&self, user_id: &str) -> Result<Vec<Prompt>> {
        Ok(sqlx::query_as::<_, Prompt>(
            "SELECT id, user_id, title, body, created_at FROM prompts WHERE user_id = ? ORDER BY title",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn create_prompt(&self, user_id: &str, title: &str, body: &str) -> Result<Prompt> {
        let id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO prompts (id, user_id, title, body, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(user_id)
        .bind(title)
        .bind(body)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        self.prompt(user_id, &id)
            .await?
            .ok_or_else(|| anyhow!("inserted prompt vanished"))
    }

    pub async fn update_prompt(
        &self,
        user_id: &str,
        id: &str,
        title: &str,
        body: &str,
    ) -> Result<bool> {
        Ok(
            sqlx::query("UPDATE prompts SET title = ?, body = ? WHERE id = ? AND user_id = ?")
                .bind(title)
                .bind(body)
                .bind(id)
                .bind(user_id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }

    pub async fn delete_prompt(&self, user_id: &str, id: &str) -> Result<bool> {
        Ok(
            sqlx::query("DELETE FROM prompts WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }

    async fn prompt(&self, user_id: &str, id: &str) -> Result<Option<Prompt>> {
        Ok(sqlx::query_as::<_, Prompt>(
            "SELECT id, user_id, title, body, created_at FROM prompts WHERE id = ? AND user_id = ?",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    // ----------------------------------------------------------------- presets

    pub async fn list_presets(&self, user_id: &str) -> Result<Vec<Preset>> {
        Ok(sqlx::query_as::<_, Preset>(
            "SELECT id, user_id, name, params, created_at FROM presets WHERE user_id = ? ORDER BY name",
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn create_preset(&self, user_id: &str, name: &str, params: &str) -> Result<Preset> {
        let id = Uuid::now_v7().to_string();
        sqlx::query(
            "INSERT INTO presets (id, user_id, name, params, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(user_id)
        .bind(name)
        .bind(params)
        .bind(Utc::now())
        .execute(&self.pool)
        .await?;
        self.preset(user_id, &id)
            .await?
            .ok_or_else(|| anyhow!("inserted preset vanished"))
    }

    pub async fn delete_preset(&self, user_id: &str, id: &str) -> Result<bool> {
        Ok(
            sqlx::query("DELETE FROM presets WHERE id = ? AND user_id = ?")
                .bind(id)
                .bind(user_id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                > 0,
        )
    }

    async fn preset(&self, user_id: &str, id: &str) -> Result<Option<Preset>> {
        Ok(sqlx::query_as::<_, Preset>(
            "SELECT id, user_id, name, params, created_at FROM presets WHERE id = ? AND user_id = ?",
        )
        .bind(id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    // ---------------------------------------------------------- model registry

    pub async fn list_registry(
        &self,
        provider_id: &str,
    ) -> Result<Vec<(String, bool, bool, Option<i64>)>> {
        Ok(sqlx::query_as(
            "SELECT model_id, vision, reasoning, ctx_len FROM model_registry WHERE provider_id = ? ORDER BY model_id",
        )
        .bind(provider_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn registry_has(&self, provider_id: &str, model_id: &str) -> Result<bool> {
        Ok(sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM model_registry WHERE provider_id = ? AND model_id = ?",
        )
        .bind(provider_id)
        .bind(model_id)
        .fetch_one(&self.pool)
        .await?
        .0 > 0)
    }

    /// Insert-or-update capability metadata for one provider model.
    pub async fn upsert_registry(
        &self,
        provider_id: &str,
        model_id: &str,
        vision: bool,
        reasoning: bool,
        ctx_len: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO model_registry (provider_id, model_id, vision, reasoning, ctx_len)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(provider_id, model_id) DO UPDATE SET
               vision = excluded.vision, reasoning = excluded.reasoning,
               ctx_len = COALESCE(excluded.ctx_len, model_registry.ctx_len)",
        )
        .bind(provider_id)
        .bind(model_id)
        .bind(vision)
        .bind(reasoning)
        .bind(ctx_len)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    // ---------------------------------------------------------------------- fts

    async fn fts_upsert(&self, message_id: &str, chat_id: &str, content: &str) -> Result<()> {
        sqlx::query("DELETE FROM messages_fts WHERE message_id = ?")
            .bind(message_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("INSERT INTO messages_fts (content, message_id, chat_id) VALUES (?, ?, ?)")
            .bind(content)
            .bind(message_id)
            .bind(chat_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Re-sync the FTS row for a message from its current `messages` content.
    async fn fts_sync(&self, message_id: &str) -> Result<()> {
        let row: Option<(String, String)> =
            sqlx::query_as("SELECT chat_id, content FROM messages WHERE id = ?")
                .bind(message_id)
                .fetch_optional(&self.pool)
                .await?;
        match row {
            Some((chat_id, content)) if !content.is_empty() => {
                self.fts_upsert(message_id, &chat_id, &content).await
            }
            Some(_) => {
                sqlx::query("DELETE FROM messages_fts WHERE message_id = ?")
                    .bind(message_id)
                    .execute(&self.pool)
                    .await?;
                Ok(())
            }
            None => Ok(()),
        }
    }
}

fn opt_f64(v: &Json) -> Option<f64> {
    v.as_f64()
}

/// Normalize a tag list JSON value into a canonical JSON array string:
/// lowercase, trimmed, deduplicated, sorted. Anything non-array becomes `[]`.
fn normalize_tags(v: Option<&Json>) -> String {
    let mut tags: Vec<String> = v
        .and_then(Json::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Json::as_str)
                .map(|t| t.trim().to_lowercase())
                .filter(|t| !t.is_empty())
                .collect()
        })
        .unwrap_or_default();
    tags.sort();
    tags.dedup();
    serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string())
}

/// Split a search query into FTS terms and lowercased `#tag` filters,
/// stripping everything that is not alphanumeric, `_`, `-` or `.`.
fn split_search_tokens(query: &str) -> (Vec<String>, Vec<String>) {
    let mut terms = Vec::new();
    let mut tags = Vec::new();
    for tok in query.split_whitespace() {
        let (is_tag, rest) = match tok.strip_prefix('#') {
            Some(r) => (true, r),
            None => (false, tok),
        };
        let cleaned: String = rest
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '.')
            .collect();
        if cleaned.is_empty() {
            continue;
        }
        if is_tag {
            tags.push(cleaned.to_lowercase());
        } else {
            terms.push(cleaned);
        }
    }
    (terms, tags)
}

fn required_str(v: &Json, key: &str) -> Result<String> {
    v.get(key)
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("field `{key}` is required"))
}

fn optional_str(v: &Json, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Json::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Normalize a base URL: require or assume a scheme, strip trailing `/`.
fn provider_base_url(v: &Json) -> Result<String> {
    let raw = required_str(v, "base_url")?;
    let with_scheme = if raw.starts_with("http://") || raw.starts_with("https://") {
        raw
    } else {
        format!("https://{raw}")
    };
    Ok(with_scheme.trim_end_matches('/').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_token_splitting() {
        let (terms, tags) = split_search_tokens("hello #Work wor-ld #a.b #");
        assert_eq!(terms, ["hello", "wor-ld"]);
        assert_eq!(tags, ["work", "a.b"]);
        let (terms, tags) = split_search_tokens("  ");
        assert!(terms.is_empty() && tags.is_empty());
    }

    #[test]
    fn base_url_normalization() {
        let v = serde_json::json!({ "base_url": "http://localhost:11434/v1/" });
        assert_eq!(provider_base_url(&v).unwrap(), "http://localhost:11434/v1");
        let v = serde_json::json!({ "base_url": "api.openai.com/v1" });
        assert_eq!(provider_base_url(&v).unwrap(), "https://api.openai.com/v1");
        let v = serde_json::json!({ "base_url": "  " });
        assert!(provider_base_url(&v).is_err());
    }

    #[test]
    fn patch_helpers() {
        let v = serde_json::json!({ "name": " x ", "api_key": "", "n": 1.5 });
        assert_eq!(optional_str(&v, "name").as_deref(), Some("x"));
        assert_eq!(optional_str(&v, "api_key"), None);
        assert_eq!(opt_f64(&v["n"]), Some(1.5));
        assert_eq!(opt_f64(&serde_json::Value::Null), None);
    }

    #[test]
    fn tag_normalization() {
        let v = serde_json::json!(["Work", " rfc ", "work", "", 3]);
        assert_eq!(normalize_tags(Some(&v)), r#"["rfc","work"]"#);
        assert_eq!(normalize_tags(Some(&serde_json::json!("x"))), "[]");
        assert_eq!(normalize_tags(None), "[]");
    }
}
