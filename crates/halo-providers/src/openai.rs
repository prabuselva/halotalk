use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use futures::{StreamExt, stream::BoxStream};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

/// Streaming chunk forwarded to the server. Empty fields mean "nothing in this frame".
/// `tool_calls` holds raw per-frame delta fragments (OpenAI shape), to be merged by
/// the consumer keyed on `index`.
#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub content: String,
    pub reasoning: String,
    pub finish_reason: Option<String>,
    pub tool_calls: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutMessage {
    pub role: String,
    pub content: OutContent,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Value>>,
}

impl OutMessage {
    pub fn text(role: &str, content: OutContent) -> Self {
        Self {
            role: role.into(),
            content,
            tool_call_id: None,
            tool_calls: None,
        }
    }

    pub fn with_tool_call_id(mut self, id: &str) -> Self {
        self.tool_call_id = Some(id.to_string());
        self
    }

    pub fn with_tool_calls(mut self, calls: Vec<Value>) -> Self {
        self.tool_calls = Some(calls);
        self
    }
}

/// Text-only messages serialize to a plain JSON string (maximum compatibility);
/// anything with parts serializes to the multimodal array.
#[derive(Debug, Clone)]
pub enum OutContent {
    Text(String),
    Parts(Vec<OutPart>),
}

impl Serialize for OutContent {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text(t) => s.serialize_str(t),
            Self::Parts(parts) => parts.serialize(s),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChatParams {
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub max_tokens: Option<i64>,
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<OutMessage>,
    pub params: ChatParams,
    /// OpenAI `tools` array (function definitions), passed through verbatim.
    pub tools: Option<Vec<Value>>,
    /// OpenAI `tool_choice`, passed through verbatim when set.
    pub tool_choice: Option<Value>,
}

impl ChatCompletionRequest {
    fn to_body(&self) -> Result<Value> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": self.messages,
            "stream": true,
        });
        let p = &self.params;
        if let Some(v) = p.temperature {
            body["temperature"] = serde_json::json!(v);
        }
        if let Some(v) = p.top_p {
            body["top_p"] = serde_json::json!(v);
        }
        if let Some(v) = p.max_tokens {
            body["max_tokens"] = serde_json::json!(v);
        }
        if let Some(v) = p
            .reasoning_effort
            .as_deref()
            .filter(|s| !s.is_empty() && *s != "none")
        {
            body["reasoning_effort"] = serde_json::json!(v);
        }
        if let Some(tools) = &self.tools {
            body["tools"] = serde_json::json!(tools);
        }
        if let Some(tc) = &self.tool_choice {
            body["tool_choice"] = tc.clone();
        }
        Ok(body)
    }
}

#[derive(Debug, Deserialize)]
struct SseChoice {
    delta: Option<SseDelta>,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SseDelta {
    content: Option<String>,
    reasoning: Option<String>,
    #[serde(rename = "reasoning_content")]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct SseChunk {
    #[serde(default)]
    choices: Vec<SseChoice>,
    error: Option<SseError>,
}

#[derive(Debug, Deserialize)]
struct SseError {
    message: String,
}

/// Lenient parse of one `data:` payload into a [`Chunk`]. None = irrelevant frame.
fn parse_sse_data(payload: &str) -> Result<Option<Chunk>> {
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(None);
    }
    let chunk: SseChunk = match serde_json::from_str(payload) {
        Ok(c) => c,
        Err(_) => return Ok(None), // ignore keepalives / unknown formats
    };
    if let Some(err) = chunk.error {
        return Err(anyhow!("provider error: {}", err.message));
    }
    let Some(choice) = chunk.choices.first() else {
        return Ok(None);
    };
    let mut out = Chunk::default();
    if let Some(d) = &choice.delta {
        out.content = d.content.clone().unwrap_or_default();
        out.reasoning = d
            .reasoning
            .clone()
            .or_else(|| d.reasoning_content.clone())
            .unwrap_or_default();
        out.tool_calls = d.tool_calls.clone();
    }
    out.finish_reason = choice.finish_reason.clone();
    Ok(Some(out))
}

#[derive(Clone)]
pub struct OpenAiClient {
    http: reqwest::Client,
}

impl Default for OpenAiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl OpenAiClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
        }
    }

    /// GET `<base_url>/models` — returns raw JSON `data` array value.
    pub async fn list_models(&self, base_url: &str, api_key: &str) -> Result<Vec<Value>> {
        let url = format!("{base_url}/models");
        let mut req = self.http.get(&url);
        if !api_key.is_empty() {
            req = req.bearer_auth(api_key);
        }
        let resp = req.send().await.context("connecting to provider")?;
        let status = resp.status();
        if !status.is_success() {
            return Err(upstream_error(status, resp).await);
        }
        let body: Value = resp.json().await.context("decoding model list")?;
        let data = body
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .or_else(|| body.as_array().cloned())
            .unwrap_or_default();
        Ok(data)
    }

    /// Non-streaming completion; returns the assistant message's text content.
    /// Used for side tasks like chat-title generation.
    pub async fn complete(
        &self,
        base_url: &str,
        api_key: &str,
        req_body: &ChatCompletionRequest,
    ) -> Result<String> {
        let mut body = req_body.to_body()?;
        body["stream"] = Value::Bool(false);
        let url = format!("{base_url}/chat/completions");
        let mut req = self.http.post(&url).json(&body);
        if !api_key.is_empty() {
            req = req.bearer_auth(api_key);
        }
        let resp = req.send().await.context("connecting to provider")?;
        let status = resp.status();
        if !status.is_success() {
            return Err(upstream_error(status, resp).await);
        }
        let body: Value = resp.json().await.context("decoding completion")?;
        body.get("choices")
            .and_then(Value::as_array)
            .and_then(|c| c.first())
            .and_then(|c| c.get("message"))
            .and_then(|m| m.get("content"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .context("completion response had no message content")
    }

    /// POST `<base_url>/embeddings` (OpenAI-compatible; also served by Ollama's /v1).
    /// Returns one vector per input, in request order.
    pub async fn embed(
        &self,
        base_url: &str,
        api_key: &str,
        model: &str,
        inputs: &[String],
    ) -> Result<Vec<Vec<f32>>> {
        let url = format!("{base_url}/embeddings");
        let body = serde_json::json!({ "model": model, "input": inputs });
        let mut req = self.http.post(&url).json(&body);
        if !api_key.is_empty() {
            req = req.bearer_auth(api_key);
        }
        let resp = req
            .send()
            .await
            .context("connecting to embeddings endpoint")?;
        let status = resp.status();
        if !status.is_success() {
            return Err(upstream_error(status, resp).await);
        }
        let body: Value = resp.json().await.context("decoding embeddings")?;
        let items = body
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut by_index: Vec<(usize, Vec<f32>)> = items
            .iter()
            .filter_map(|d| {
                let idx = d.get("index").and_then(Value::as_u64)? as usize;
                let v = d
                    .get("embedding")?
                    .as_array()?
                    .iter()
                    .map(|x| x.as_f64().unwrap_or(0.0) as f32)
                    .collect();
                Some((idx, v))
            })
            .collect();
        by_index.sort_by_key(|(i, _)| *i);
        Ok(by_index.into_iter().map(|(_, v)| v).collect())
    }

    /// Stream a chat completion. The returned stream yields content/reasoning
    /// deltas and ends after a chunk with `finish_reason` or upstream close.
    pub async fn stream(
        &self,
        base_url: &str,
        api_key: &str,
        req_body: &ChatCompletionRequest,
    ) -> Result<BoxStream<'static, Result<Chunk>>> {
        let body = req_body.to_body()?;
        let url = format!("{base_url}/chat/completions");
        let mut req = self
            .http
            .post(&url)
            .header("accept", "text/event-stream")
            .json(&body);
        if !api_key.is_empty() {
            req = req.bearer_auth(api_key);
        }
        let resp = req.send().await.context("connecting to provider")?;
        let status = resp.status();
        if !status.is_success() {
            return Err(upstream_error(status, resp).await);
        }

        let (tx, rx) = mpsc::channel::<Result<Chunk>>(256);
        tokio::spawn(async move {
            let mut stream = resp.bytes_stream();
            let mut buf = String::new();
            let mut finished = false;
            while let Some(item) = stream.next().await {
                let bytes = match item {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(Err(anyhow!("stream error: {e}"))).await;
                        return;
                    }
                };
                buf.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim_end_matches('\r').to_string();
                    buf.drain(..=pos);
                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }
                    let Some(data) = line
                        .strip_prefix("data:")
                        .or_else(|| line.strip_prefix("data").filter(|s| s.starts_with('{')))
                    else {
                        continue;
                    };
                    if data.trim() == "[DONE]" {
                        finished = true;
                        break;
                    }
                    match parse_sse_data(data) {
                        Ok(Some(chunk)) => {
                            let done = chunk.finish_reason.is_some();
                            let content = chunk.content.clone();
                            let reasoning = chunk.reasoning.clone();
                            if content.is_empty()
                                && reasoning.is_empty()
                                && chunk.tool_calls.is_empty()
                                && !done
                            {
                                continue;
                            }
                            if tx.send(Ok(chunk)).await.is_err() {
                                return; // consumer gone
                            }
                            if done {
                                finished = true;
                                break;
                            }
                        }
                        Ok(None) => {}
                        Err(e) => {
                            let _ = tx.send(Err(e)).await;
                            return;
                        }
                    }
                }
                if finished {
                    break;
                }
            }
            // Drain a final unterminated line, tolerant of servers without trailing newline.
            if !finished {
                let line = buf.trim();
                if let Some(data) = line
                    .strip_prefix("data:")
                    .filter(|s| !s.trim().is_empty() && s.trim() != "[DONE]")
                    && let Ok(Some(chunk)) = parse_sse_data(data)
                {
                    let _ = tx.send(Ok(chunk)).await;
                }
            }
        });
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
            as BoxStream<'static, Result<Chunk>>)
    }
}

pub(crate) async fn upstream_error(status: StatusCode, resp: reqwest::Response) -> anyhow::Error {
    let text = resp.text().await.unwrap_or_default();
    let detail = extract_error_message(&text)
        .or((!text.is_empty()).then(|| text.chars().take(400).collect()))
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("error").to_string());
    anyhow!("upstream HTTP {}: {detail}", status.as_u16())
}

/// Pull `error.message` out of an OpenAI-style error body when present.
fn extract_error_message(text: &str) -> Option<String> {
    let v: Value = serde_json::from_str(text).ok()?;
    let m = v.get("error")?.get("message")?.as_str()?;
    Some(m.chars().take(400).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_content_reasoning_and_finish() {
        let c = parse_sse_data(r#"{"choices":[{"delta":{"content":"hi"}}]}"#)
            .unwrap()
            .unwrap();
        assert_eq!(c.content, "hi");
        assert!(c.reasoning.is_empty());
        assert!(c.tool_calls.is_empty());

        let c = parse_sse_data(
            r#"{"choices":[{"delta":{"reasoning_content":"think","content":"x"}}]}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.reasoning, "think");
        assert_eq!(c.content, "x");

        let c = parse_sse_data(r#"{"choices":[{"delta":{},"finish_reason":"stop"}]}"#)
            .unwrap()
            .unwrap();
        assert_eq!(c.finish_reason.as_deref(), Some("stop"));

        assert!(parse_sse_data("[DONE]").unwrap().is_none());
        assert!(parse_sse_data(": keepalive").unwrap().is_none());
        assert!(parse_sse_data("garbage").unwrap().is_none());
        assert!(parse_sse_data(r#"{"error":{"message":"boom"}}"#).is_err());
    }

    #[test]
    fn parses_tool_call_deltas() {
        let c = parse_sse_data(
            r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"web_search","arguments":"{\"q\":"}}]}}]}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.tool_calls.len(), 1);
        assert_eq!(c.tool_calls[0]["id"], "call_1");
        assert_eq!(c.tool_calls[0]["function"]["name"], "web_search");

        let c = parse_sse_data(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#)
            .unwrap()
            .unwrap();
        assert_eq!(c.finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn body_passthrough_and_serialization() {
        let req = ChatCompletionRequest {
            model: "m1".into(),
            messages: vec![
                OutMessage::text("system", OutContent::Text("be nice".into())),
                OutMessage::text(
                    "user",
                    OutContent::Parts(vec![
                        OutPart::Text {
                            text: "look".into(),
                        },
                        OutPart::ImageUrl {
                            image_url: ImageUrl {
                                url: "data:image/png;base64,AAA".into(),
                            },
                        },
                    ]),
                ),
                OutMessage::text("tool", OutContent::Text("\"3 results\"".into()))
                    .with_tool_call_id("call_1"),
            ],
            params: ChatParams {
                temperature: Some(0.7),
                top_p: None,
                max_tokens: Some(512),
                reasoning_effort: Some("low".into()),
            },
            tools: None,
            tool_choice: None,
        };
        let body = req.to_body().unwrap();
        assert_eq!(body["model"], "m1");
        assert_eq!(body["stream"], true);
        assert_eq!(body["temperature"], 0.7);
        assert_eq!(body["max_tokens"], 512);
        assert_eq!(body["reasoning_effort"], "low");
        assert!(body.get("top_p").is_none());
        assert_eq!(body["messages"][0]["content"], "be nice");
        assert_eq!(body["messages"][1]["content"][0]["type"], "text");
        assert_eq!(
            body["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,AAA"
        );
        // tool message carries tool_call_id, no tool_calls, and role "tool".
        assert_eq!(body["messages"][2]["role"], "tool");
        assert_eq!(body["messages"][2]["tool_call_id"], "call_1");
        assert!(body["messages"][2].get("tool_calls").is_none());
        // no tools configured -> "tools" key is absent.
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn tools_and_tool_choice_pass_through() {
        let req = ChatCompletionRequest {
            model: "m".into(),
            messages: vec![
                OutMessage::text("user", OutContent::Text("hi".into())).with_tool_calls(vec![
                    json!({
                        "id": "c1", "type": "function",
                        "function": {"name": "web_search", "arguments": "{}"}
                    }),
                ]),
            ],
            tools: Some(vec![json!({
                "type": "function",
                "function": {"name": "web_search", "parameters": {}}
            })]),
            tool_choice: Some(json!("auto")),
            ..Default::default()
        };
        let body = req.to_body().unwrap();
        assert_eq!(body["tools"][0]["function"]["name"], "web_search");
        assert_eq!(body["tool_choice"], "auto");
        assert_eq!(body["messages"][0]["tool_calls"][0]["id"], "c1");
    }
}
