use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer};
use sqlx::FromRow;

/// An OpenAI-compatible upstream endpoint the user has registered.
/// `api_key` is skipped by default serialization; handlers send `Provider::redacted()`.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    #[serde(skip_serializing)]
    pub api_key: String,
    pub is_default: bool,
    pub kind: String,
    pub user_id: String,
    pub created_at: DateTime<Utc>,
}

impl Provider {
    pub fn redacted(mut self) -> Self {
        self.api_key = String::new();
        self
    }
}

/// A conversation plus the per-chat model settings (ADR-005).
/// `tags` is a JSON array string, e.g. `["work","rfc"]`.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Chat {
    pub id: String,
    pub title: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<i64>,
    pub system_prompt: Option<String>,
    pub reasoning_effort: Option<String>,
    pub user_id: String,
    pub pinned: bool,
    pub archived: bool,
    #[serde(serialize_with = "serialize_tags")]
    pub tags: String,
    pub tools_enabled: bool,
    pub kb_enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One turn in a conversation. Assistant replies to the same user message form a
/// branch group (`reply_to` = that user message id); only one sibling is `active`
/// at a time, and only the active reply is replayed to the model.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Message {
    pub id: String,
    pub chat_id: String,
    pub role: String,
    pub content: String,
    pub reasoning: Option<String>,
    /// JSON-encoded array of image references (data URLs or `/uploads/…` paths).
    pub images: Option<String>,
    pub error: Option<String>,
    pub reply_to: String,
    pub active: bool,
    /// JSON array of the tool calls this assistant message requested (OpenAI shape).
    pub tool_calls: Option<String>,
    /// For `tool` role: the id of the call this row answers.
    pub tool_call_id: Option<String>,
    /// JSON array of knowledge-base citations for this reply: `[{title, score}]`.
    pub sources: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// One knowledge-base chunk (RAG v1). `vector` is a JSON array of f32, NULL
/// when embeddings were unavailable at upload time.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct KbDoc {
    pub id: String,
    pub user_id: String,
    pub source: String,
    pub title: String,
    pub text: String,
    pub vector: Option<String>,
    pub embed_model: String,
    pub created_at: DateTime<Utc>,
}

impl KbDoc {
    pub fn public(&self) -> Value {
        serde_json::json!({
            "id": self.id,
            "source": self.source,
            "title": self.title,
            "chars": self.text.chars().count(),
            "has_vector": self.vector.is_some(),
            "embed_model": self.embed_model,
            "created_at": self.created_at,
        })
    }
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct User {
    pub id: String,
    pub username: String,
    #[serde(skip_serializing)]
    pub pass_hash: String,
    pub is_admin: bool,
    pub recovery_question: String,
    pub created_at: DateTime<Utc>,
}

impl User {
    pub fn public(self) -> Value {
        serde_json::json!({
            "id": self.id,
            "username": self.username,
            "is_admin": self.is_admin,
            "has_recovery": !self.recovery_question.is_empty(),
        })
    }
}

use serde_json::Value;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Prompt {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Preset {
    pub id: String,
    pub user_id: String,
    pub name: String,
    /// JSON object of chat params (temperature, top_p, max_tokens, reasoning_effort, …).
    pub params: String,
    pub created_at: DateTime<Utc>,
}

/// `Chat.tags` is stored as a JSON array string; the API contract is a real array.
// serde's serialize_with contract passes `&String` (the field type), not `&str`.
#[allow(clippy::ptr_arg)]
fn serialize_tags<S: Serializer>(raw: &String, s: S) -> Result<S::Ok, S::Error> {
    let v: Vec<String> = serde_json::from_str(raw).unwrap_or_default();
    v.serialize(s)
}
