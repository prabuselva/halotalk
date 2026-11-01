//! Chat-completion providers (ADR-002 + 3.2b revisit).
//!
//! `OpenAiClient` speaks `<base_url>/chat/completions` with `stream: true`,
//! covering OpenAI, OpenRouter, Ollama, LM Studio, vLLM, and compatible proxies.
//! `AnthropicClient` / `GeminiClient` translate the same request shape to the
//! native Anthropic Messages API and Gemini generateContent API; `AnyClient`
//! dispatches by the `providers.kind` column (unknown kinds fall back to OpenAI).

mod anthropic;
mod gemini;
mod openai;

pub use anthropic::AnthropicClient;
pub use gemini::GeminiClient;
pub use openai::{
    ChatCompletionRequest, ChatParams, Chunk, ImageUrl, OpenAiClient, OutContent, OutMessage,
    OutPart,
};

use anyhow::Result;
use futures::stream::BoxStream;

/// Kind-dispatched facade over the available provider adapters.
#[derive(Clone, Default)]
pub struct AnyClient {
    openai: OpenAiClient,
    anthropic: AnthropicClient,
    gemini: GeminiClient,
}

impl AnyClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn list_models(
        &self,
        kind: &str,
        base_url: &str,
        api_key: &str,
    ) -> Result<Vec<serde_json::Value>> {
        match kind {
            "anthropic" => self.anthropic.list_models(base_url, api_key).await,
            "gemini" => self.gemini.list_models(base_url, api_key).await,
            _ => self.openai.list_models(base_url, api_key).await,
        }
    }

    pub async fn complete(
        &self,
        kind: &str,
        base_url: &str,
        api_key: &str,
        req_body: &ChatCompletionRequest,
    ) -> Result<String> {
        match kind {
            "anthropic" => self.anthropic.complete(base_url, api_key, req_body).await,
            "gemini" => self.gemini.complete(base_url, api_key, req_body).await,
            _ => self.openai.complete(base_url, api_key, req_body).await,
        }
    }

    /// Embeddings use the OpenAI-compatible `/embeddings` path for every kind
    /// (Ollama, OpenAI, vLLM, …); Anthropic/Gemini have no compatible endpoint.
    pub async fn embed(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        inputs: &[String],
    ) -> Result<Vec<Vec<f32>>> {
        self.openai.embed(base_url, api_key, model, inputs).await
    }

    pub async fn stream(
        &self,
        kind: &str,
        base_url: &str,
        api_key: &str,
        req_body: &ChatCompletionRequest,
    ) -> Result<BoxStream<'static, Result<Chunk>>> {
        match kind {
            "anthropic" => self.anthropic.stream(base_url, api_key, req_body).await,
            "gemini" => self.gemini.stream(base_url, api_key, req_body).await,
            _ => self.openai.stream(base_url, api_key, req_body).await,
        }
    }
}
