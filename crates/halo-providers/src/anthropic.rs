//! Native Anthropic Messages-API adapter; translates the internal
//! OpenAI-shaped [`ChatCompletionRequest`] to/from Anthropic's format (ADR-002 revisit).

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use futures::{StreamExt, stream::BoxStream};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::openai::{ChatCompletionRequest, Chunk, OutContent, OutMessage, upstream_error};

const VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: i64 = 8192;

#[derive(Clone)]
pub struct AnthropicClient {
    http: reqwest::Client,
}

impl Default for AnthropicClient {
    fn default() -> Self {
        Self::new()
    }
}

fn text_of(content: &OutContent) -> String {
    match content {
        OutContent::Text(t) => t.clone(),
        OutContent::Parts(parts) => parts
            .iter()
            .filter_map(|p| match p {
                crate::openai::OutPart::Text { text } => Some(text.as_str()),
                crate::openai::OutPart::ImageUrl { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn image_block(url: &str) -> Value {
    if let Some(rest) = url.strip_prefix("data:")
        && let Some((meta, data)) = rest.split_once(',')
    {
        let mime = meta.split(';').next().unwrap_or("image/png");
        return json!({"type": "image", "source": {"type": "base64", "media_type": mime, "data": data}});
    }
    json!({"type": "image", "source": {"type": "url", "url": url}})
}

fn blocks_of(m: &OutMessage) -> Vec<Value> {
    let mut blocks = Vec::new();
    match &m.content {
        OutContent::Text(_) => {
            let t = text_of(&m.content);
            if !t.is_empty() {
                blocks.push(json!({"type": "text", "text": t}));
            }
        }
        OutContent::Parts(parts) => {
            for p in parts {
                match p {
                    crate::openai::OutPart::Text { text } => {
                        if !text.is_empty() {
                            blocks.push(json!({"type": "text", "text": text}));
                        }
                    }
                    crate::openai::OutPart::ImageUrl { image_url } => {
                        blocks.push(image_block(&image_url.url));
                    }
                }
            }
        }
    }
    if let Some(calls) = &m.tool_calls {
        for c in calls {
            let args: Value = c["function"]["arguments"]
                .as_str()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_else(|| json!({}));
            blocks.push(json!({
                "type": "tool_use",
                "id": c["id"].as_str().unwrap_or_default(),
                "name": c["function"]["name"].as_str().unwrap_or_default(),
                "input": args,
            }));
        }
    }
    blocks
}

pub(crate) fn to_body(req: &ChatCompletionRequest, stream: bool) -> Result<Value> {
    let system: Vec<String> = req
        .messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| text_of(&m.content))
        .filter(|t| !t.is_empty())
        .collect();

    let mut messages: Vec<Value> = Vec::new();
    for m in req.messages.iter().filter(|m| m.role != "system") {
        let (role, mut blocks) = if m.role == "tool" {
            (
                "user",
                vec![json!({
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                    "content": text_of(&m.content),
                })],
            )
        } else {
            let role = if m.role == "assistant" {
                "assistant"
            } else {
                "user"
            };
            (role, blocks_of(m))
        };
        if blocks.is_empty() {
            blocks.push(json!({"type": "text", "text": ""}));
        }
        // Anthropic requires alternating roles: fold into the previous same-role turn.
        match messages.last_mut() {
            Some(prev) if prev["role"] == role => {
                if let Some(arr) = prev["content"].as_array_mut() {
                    arr.append(&mut blocks);
                }
            }
            _ => messages.push(json!({"role": role, "content": blocks})),
        }
    }

    let mut body = json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.params.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
        "stream": stream,
    });
    if !system.is_empty() {
        body["system"] = json!(system.join("\n\n"));
    }
    if let Some(v) = req.params.temperature {
        body["temperature"] = json!(v);
    }
    if let Some(v) = req.params.top_p {
        body["top_p"] = json!(v);
    }
    if let Some(tools) = &req.tools {
        let defs: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t["function"]["name"].as_str().unwrap_or_default(),
                    "description": t["function"]["description"].as_str().unwrap_or_default(),
                    "input_schema": t["function"]["parameters"].clone(),
                })
            })
            .collect();
        body["tools"] = json!(defs);
    }
    if let Some(tc) = &req.tool_choice {
        if tc == &json!("required") {
            body["tool_choice"] = json!({"type": "any"});
        } else if tc == &json!("none") {
            body["tool_choice"] = json!({"type": "none"});
        } else if tc["function"]["name"].is_string() {
            body["tool_choice"] = json!({"type": "tool", "name": tc["function"]["name"]});
        }
    }
    Ok(body)
}

#[derive(Debug, Deserialize)]
struct SseEvent {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    index: u64,
    #[serde(default)]
    delta: Option<Value>,
    #[serde(default)]
    content_block: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
}

/// Map one `data:` payload of Anthropic SSE into zero/one internal chunks.
fn parse_sse_data(payload: &str) -> Result<Option<Chunk>> {
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(None);
    }
    let Ok(ev) = serde_json::from_str::<SseEvent>(payload) else {
        return Ok(None);
    };
    match ev.kind.as_str() {
        "content_block_start" => match &ev.content_block {
            Some(b) if b["type"] == "tool_use" => Ok(Some(Chunk {
                tool_calls: vec![json!({
                    "index": ev.index,
                    "id": b["id"].as_str().unwrap_or_default(),
                    "type": "function",
                    "function": {
                        "name": b["name"].as_str().unwrap_or_default(),
                        "arguments": "",
                    },
                })],
                ..Default::default()
            })),
            _ => Ok(None),
        },
        "content_block_delta" => {
            let Some(d) = ev.delta.as_ref() else {
                return Ok(None);
            };
            match d["type"].as_str().unwrap_or_default() {
                "text_delta" => Ok(Some(Chunk {
                    content: d["text"].as_str().unwrap_or_default().to_string(),
                    ..Default::default()
                })),
                "thinking_delta" => Ok(Some(Chunk {
                    reasoning: d["thinking"].as_str().unwrap_or_default().to_string(),
                    ..Default::default()
                })),
                "input_json_delta" => Ok(Some(Chunk {
                    tool_calls: vec![json!({
                        "index": ev.index,
                        "function": {"arguments": d["partial_json"].as_str().unwrap_or_default()},
                    })],
                    ..Default::default()
                })),
                _ => Ok(None),
            }
        }
        "message_delta" => {
            let stop = ev
                .delta
                .as_ref()
                .and_then(|d| d["stop_reason"].as_str())
                .unwrap_or("end_turn");
            Ok(Some(Chunk {
                finish_reason: Some(
                    match stop {
                        "tool_use" => "tool_calls",
                        "max_tokens" => "length",
                        _ => "stop",
                    }
                    .to_string(),
                ),
                ..Default::default()
            }))
        }
        "message_stop" => Ok(Some(Chunk {
            finish_reason: Some("stop".into()),
            ..Default::default()
        })),
        "error" => Err(anyhow!(
            "provider error: {}",
            ev.error
                .as_ref()
                .and_then(|e| e["message"].as_str())
                .unwrap_or("unknown")
        )),
        _ => Ok(None),
    }
}

impl AnthropicClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
        }
    }

    fn with_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        let req = req.header("anthropic-version", VERSION);
        if api_key.is_empty() {
            req
        } else {
            req.header("x-api-key", api_key)
        }
    }

    /// GET `<base>/models` with Anthropic auth; returns raw model list array.
    pub async fn list_models(&self, base_url: &str, api_key: &str) -> Result<Vec<Value>> {
        let url = format!("{base_url}/models");
        let req = self.with_auth(self.http.get(&url), api_key);
        let resp = req.send().await.context("connecting to provider")?;
        let status = resp.status();
        if !status.is_success() {
            return Err(upstream_error(status, resp).await);
        }
        let body: Value = resp.json().await.context("decoding model list")?;
        Ok(body
            .get("data")
            .and_then(Value::as_array)
            .or_else(|| body.as_array())
            .cloned()
            .unwrap_or_default())
    }

    /// Non-streaming completion; concatenated text blocks of the reply.
    pub async fn complete(
        &self,
        base_url: &str,
        api_key: &str,
        req_body: &ChatCompletionRequest,
    ) -> Result<String> {
        let body = to_body(req_body, false)?;
        let url = format!("{base_url}/messages");
        let resp = self
            .with_auth(self.http.post(&url).json(&body), api_key)
            .send()
            .await
            .context("connecting to provider")?;
        let status = resp.status();
        if !status.is_success() {
            return Err(upstream_error(status, resp).await);
        }
        let body: Value = resp.json().await.context("decoding completion")?;
        let text = body["content"]
            .as_array()
            .map(|blocks| {
                blocks
                    .iter()
                    .filter(|b| b["type"] == "text")
                    .filter_map(|b| b["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        if text.is_empty() {
            return Err(anyhow!("completion response had no text content"));
        }
        Ok(text)
    }

    /// Stream a completion, emitting OpenAI-shaped [`Chunk`]s.
    pub async fn stream(
        &self,
        base_url: &str,
        api_key: &str,
        req_body: &ChatCompletionRequest,
    ) -> Result<BoxStream<'static, Result<Chunk>>> {
        let body = to_body(req_body, true)?;
        let url = format!("{base_url}/messages");
        let req = self
            .with_auth(
                self.http
                    .post(&url)
                    .header("accept", "text/event-stream")
                    .json(&body),
                api_key,
            )
            .send()
            .await
            .context("connecting to provider")?;
        let status = req.status();
        if !status.is_success() {
            return Err(upstream_error(status, req).await);
        }

        let (tx, rx) = mpsc::channel::<Result<Chunk>>(256);
        tokio::spawn(async move {
            let mut stream = req.bytes_stream();
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
                    if line.is_empty() || line.starts_with(':') || line.starts_with("event:") {
                        continue;
                    }
                    let Some(data) = line
                        .strip_prefix("data:")
                        .or_else(|| line.strip_prefix("data").filter(|s| s.starts_with('{')))
                    else {
                        continue;
                    };
                    match parse_sse_data(data) {
                        Ok(Some(chunk)) => {
                            let done = chunk.finish_reason.is_some();
                            let empty = chunk.content.is_empty()
                                && chunk.reasoning.is_empty()
                                && chunk.tool_calls.is_empty();
                            if empty && !done {
                                continue;
                            }
                            if tx.send(Ok(chunk)).await.is_err() {
                                return;
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
        });
        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx))
            as BoxStream<'static, Result<Chunk>>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::{ChatParams, OutMessage};

    fn req() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "claude-x".into(),
            messages: vec![
                OutMessage::text("system", OutContent::Text("be nice".into())),
                OutMessage::text("user", OutContent::Text("hi".into())),
                OutMessage::text("assistant", OutContent::Text("".into())).with_tool_calls(vec![
                    json!({"id": "c1", "type": "function", "function": {"name": "web_search", "arguments": "{\"query\":\"q\"}"}}),
                ]),
                OutMessage::text("tool", OutContent::Text("result".into()))
                    .with_tool_call_id("c1"),
                OutMessage::text("user", OutContent::Text("and more".into())),
            ],
            params: ChatParams::default(),
            tools: Some(vec![json!({
                "type": "function",
                "function": {
                    "name": "web_search",
                    "description": "s",
                    "parameters": {"type": "object", "properties": {}}
                }
            })]),
            tool_choice: Some(json!("required")),
        }
    }

    #[test]
    fn translates_request() {
        let body = to_body(&req(), true).unwrap();
        assert_eq!(body["system"], "be nice");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], DEFAULT_MAX_TOKENS);
        assert_eq!(body["tool_choice"], json!({"type": "any"}));
        assert_eq!(body["tools"][0]["name"], "web_search");
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        let msgs = body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3); // system extracted; roles alternate
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"][0]["text"], "hi");
        // assistant turn carries a tool_use block with parsed input
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"][0]["type"], "tool_use");
        assert_eq!(msgs[1]["content"][0]["input"]["query"], "q");
        // tool result + follow-up user turn folded into one user message
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
        assert_eq!(msgs[2]["content"][0]["tool_use_id"], "c1");
        assert_eq!(msgs[2]["content"][1]["text"], "and more");
    }

    #[test]
    fn parses_stream_events() {
        let c = parse_sse_data(r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"c1","name":"web_search"}}"#)
            .unwrap().unwrap();
        assert_eq!(c.tool_calls[0]["id"], "c1");
        assert_eq!(c.tool_calls[0]["function"]["arguments"], "");

        let c = parse_sse_data(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.content, "hi");

        let c = parse_sse_data(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"mm"}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.reasoning, "mm");

        let c = parse_sse_data(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"q\"}}"}}"#,
        )
        .unwrap()
        .unwrap();
        assert_eq!(c.tool_calls[0]["function"]["arguments"], "{\"q\"}}");

        let c = parse_sse_data(r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"}}"#)
            .unwrap()
            .unwrap();
        assert_eq!(c.finish_reason.as_deref(), Some("tool_calls"));

        assert!(parse_sse_data(r#"{"type":"ping"}"#).unwrap().is_none());
        assert!(
            parse_sse_data(r#"{"type":"content_block_stop","index":0}"#)
                .unwrap()
                .is_none()
        );
        assert!(
            parse_sse_data(r#"{"type":"error","error":{"type":"overloaded","message":"busy"}}"#)
                .is_err()
        );
    }
}
