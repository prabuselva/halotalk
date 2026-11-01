//! Native Google Gemini `generateContent` adapter; translates the internal
//! OpenAI-shaped [`ChatCompletionRequest`] to/from Gemini's format (ADR-002 revisit).
//! `base_url` points at the version root, e.g. `https://generativelanguage.googleapis.com/v1beta`.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use futures::{StreamExt, stream::BoxStream};
use serde_json::{Value, json};
use tokio::sync::mpsc;

use crate::openai::{ChatCompletionRequest, Chunk, OutContent, OutPart, upstream_error};

fn text_of(content: &OutContent) -> String {
    match content {
        OutContent::Text(t) => t.clone(),
        OutContent::Parts(parts) => parts
            .iter()
            .filter_map(|p| match p {
                OutPart::Text { text } => Some(text.as_str()),
                OutPart::ImageUrl { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn parts_of(content: &OutContent) -> Vec<Value> {
    let mut parts = Vec::new();
    match content {
        OutContent::Text(_) => {
            let t = text_of(content);
            if !t.is_empty() {
                parts.push(json!({"text": t}));
            }
        }
        OutContent::Parts(items) => {
            for item in items {
                match item {
                    OutPart::Text { text } => {
                        if !text.is_empty() {
                            parts.push(json!({"text": text}));
                        }
                    }
                    OutPart::ImageUrl { image_url } => {
                        if let Some(rest) = image_url.url.strip_prefix("data:")
                            && let Some((meta, data)) = rest.split_once(',')
                        {
                            let mime = meta.split(';').next().unwrap_or("image/png");
                            parts.push(json!({"inline_data": {"mime_type": mime, "data": data}}));
                        }
                    }
                }
            }
        }
    }
    parts
}

pub(crate) fn to_body(req: &ChatCompletionRequest) -> Result<Value> {
    // Map tool_call ids to their function names for functionResponse parts.
    let mut names: HashMap<&str, &str> = HashMap::new();
    for m in &req.messages {
        if let Some(calls) = &m.tool_calls {
            for c in calls {
                if let (Some(id), Some(name)) = (c["id"].as_str(), c["function"]["name"].as_str()) {
                    names.insert(id, name);
                }
            }
        }
    }

    let system: Vec<String> = req
        .messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| text_of(&m.content))
        .filter(|t| !t.is_empty())
        .collect();

    let mut contents: Vec<Value> = Vec::new();
    for m in req.messages.iter().filter(|m| m.role != "system") {
        let mut parts = if m.role == "tool" {
            let response: Value = serde_json::from_str(&text_of(&m.content))
                .unwrap_or_else(|_| json!({"result": text_of(&m.content)}));
            vec![json!({
                "functionResponse": {
                    "name": m.tool_call_id.as_deref().and_then(|id| names.get(id)).copied().unwrap_or("tool"),
                    "response": response,
                }
            })]
        } else {
            parts_of(&m.content)
        };
        if let Some(calls) = &m.tool_calls {
            for c in calls {
                let args: Value = c["function"]["arguments"]
                    .as_str()
                    .and_then(|s| serde_json::from_str(s).ok())
                    .unwrap_or_else(|| json!({}));
                parts.push(json!({
                    "functionCall": {
                        "name": c["function"]["name"].as_str().unwrap_or_default(),
                        "args": args,
                    }
                }));
            }
        }
        if parts.is_empty() {
            parts.push(json!({"text": ""}));
        }
        let role = if m.role == "assistant" {
            "model"
        } else {
            "user"
        };
        match contents.last_mut() {
            Some(prev) if prev["role"] == role => {
                if let Some(arr) = prev["parts"].as_array_mut() {
                    arr.append(&mut parts);
                }
            }
            _ => contents.push(json!({"role": role, "parts": parts})),
        }
    }

    let mut body = json!({"contents": contents});
    if !system.is_empty() {
        body["systemInstruction"] = json!({"parts": [{"text": system.join("\n\n")}]});
    }
    let mut gcfg = serde_json::Map::new();
    if let Some(v) = req.params.temperature {
        gcfg.insert("temperature".into(), json!(v));
    }
    if let Some(v) = req.params.top_p {
        gcfg.insert("topP".into(), json!(v));
    }
    if let Some(v) = req.params.max_tokens {
        gcfg.insert("maxOutputTokens".into(), json!(v));
    }
    if !gcfg.is_empty() {
        body["generationConfig"] = Value::Object(gcfg);
    }
    if let Some(tools) = &req.tools {
        let decls: Vec<Value> = tools
            .iter()
            .map(|t| {
                json!({
                    "name": t["function"]["name"].as_str().unwrap_or_default(),
                    "description": t["function"]["description"].as_str().unwrap_or_default(),
                    "parameters": t["function"]["parameters"].clone(),
                })
            })
            .collect();
        body["tools"] = json!([{"functionDeclarations": decls}]);
        if req.tool_choice.as_ref() == Some(&json!("required")) {
            body["toolConfig"] = json!({"functionCallingConfig": {"mode": "ANY"}});
        }
    }
    Ok(body)
}

/// Parse one Gemini SSE `data:` frame into chunks. `seq` numbers synthesized
/// tool-call ids across the whole stream (Gemini omits ids on function calls).
fn parse_frame(payload: &str, seq: &mut u64) -> Result<Vec<Chunk>> {
    let payload = payload.trim();
    if payload.is_empty() || payload == "[DONE]" {
        return Ok(vec![]);
    }
    let Ok(v) = serde_json::from_str::<Value>(payload) else {
        return Ok(vec![]);
    };
    if let Some(err) = v.get("error") {
        return Err(anyhow!(
            "provider error: {}",
            err["message"].as_str().unwrap_or("unknown")
        ));
    }
    let mut out = Vec::new();
    let Some(candidate) = v["candidates"].as_array().and_then(|c| c.first()) else {
        return Ok(out);
    };
    for part in candidate["content"]["parts"].as_array().unwrap_or(&vec![]) {
        if part["thought"].as_bool().unwrap_or(false) {
            if let Some(t) = part["text"].as_str() {
                out.push(Chunk {
                    reasoning: t.to_string(),
                    ..Default::default()
                });
            }
        } else if let Some(text) = part["text"].as_str() {
            if !text.is_empty() {
                out.push(Chunk {
                    content: text.to_string(),
                    ..Default::default()
                });
            }
        } else if let Some(fc) = part.get("functionCall") {
            let id = fc["id"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| format!("call_{seq}"));
            let frag = json!({
                "index": *seq,
                "id": id,
                "type": "function",
                "function": {
                    "name": fc["name"].as_str().unwrap_or_default(),
                    "arguments": fc["args"].to_string(),
                },
            });
            out.push(Chunk {
                tool_calls: vec![frag],
                ..Default::default()
            });
            *seq += 1;
        }
    }
    if let Some(fr) = candidate["finishReason"].as_str() {
        out.push(Chunk {
            finish_reason: Some(
                match fr {
                    "MAX_TOKENS" => "length",
                    _ => "stop",
                }
                .to_string(),
            ),
            ..Default::default()
        });
    }
    Ok(out)
}

#[derive(Clone)]
pub struct GeminiClient {
    http: reqwest::Client,
}

impl Default for GeminiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl GeminiClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
        }
    }

    fn with_auth(&self, req: reqwest::RequestBuilder, api_key: &str) -> reqwest::RequestBuilder {
        if api_key.is_empty() {
            req
        } else {
            req.header("x-goog-api-key", api_key)
        }
    }

    pub async fn list_models(&self, base_url: &str, api_key: &str) -> Result<Vec<Value>> {
        let url = format!("{base_url}/models");
        let req = self.with_auth(self.http.get(&url), api_key);
        let resp = req.send().await.context("connecting to provider")?;
        let status = resp.status();
        if !status.is_success() {
            return Err(upstream_error(status, resp).await);
        }
        let body: Value = resp.json().await.context("decoding model list")?;
        let items = body
            .get("models")
            .and_then(Value::as_array)
            .or_else(|| body.get("data").and_then(Value::as_array))
            .or_else(|| body.as_array())
            .cloned()
            .unwrap_or_default();
        // Gemini lists models as `{"name": "models/<id>"}`; expose an `id` field.
        Ok(items
            .into_iter()
            .map(|mut m| {
                if m.get("id").is_none()
                    && let Some(name) = m["name"].as_str()
                {
                    m["id"] = json!(name.strip_prefix("models/").unwrap_or(name));
                }
                m
            })
            .collect())
    }

    /// Non-streaming completion via `:generateContent`; joined text parts.
    pub async fn complete(
        &self,
        base_url: &str,
        api_key: &str,
        req_body: &ChatCompletionRequest,
    ) -> Result<String> {
        let body = to_body(req_body)?;
        let url = format!("{base_url}/models/{}:generateContent", req_body.model);
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
        let text = body["candidates"]
            .as_array()
            .and_then(|c| c.first())
            .map(|c| {
                c["content"]["parts"]
                    .as_array()
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|p| p["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if text.is_empty() {
            return Err(anyhow!("completion response had no text content"));
        }
        Ok(text)
    }

    /// Stream via `:streamGenerateContent?alt=sse`, emitting OpenAI-shaped chunks.
    pub async fn stream(
        &self,
        base_url: &str,
        api_key: &str,
        req_body: &ChatCompletionRequest,
    ) -> Result<BoxStream<'static, Result<Chunk>>> {
        let body = to_body(req_body)?;
        let url = format!(
            "{base_url}/models/{}:streamGenerateContent?alt=sse",
            req_body.model
        );
        let resp = self
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
        let status = resp.status();
        if !status.is_success() {
            return Err(upstream_error(status, resp).await);
        }

        let (tx, rx) = mpsc::channel::<Result<Chunk>>(256);
        tokio::spawn(async move {
            let mut stream = resp.bytes_stream();
            let mut buf = String::new();
            let mut seq: u64 = 0;
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
                    match parse_frame(data, &mut seq) {
                        Ok(chunks) => {
                            for chunk in chunks {
                                let done = chunk.finish_reason.is_some();
                                if chunk.content.is_empty()
                                    && chunk.reasoning.is_empty()
                                    && chunk.tool_calls.is_empty()
                                    && !done
                                {
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
                        }
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
    use crate::ImageUrl;
    use crate::openai::{ChatParams, OutMessage, OutPart};

    fn req() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "gemini-x".into(),
            messages: vec![
                OutMessage::text("system", OutContent::Text("be nice".into())),
                OutMessage::text(
                    "user",
                    OutContent::Parts(vec![
                        OutPart::Text { text: "look".into() },
                        OutPart::ImageUrl {
                            image_url: ImageUrl {
                                url: "data:image/png;base64,QUJD".into(),
                            },
                        },
                    ]),
                ),
                OutMessage::text("assistant", OutContent::Text("".into())).with_tool_calls(vec![
                    json!({"id": "c1", "type": "function", "function": {"name": "web_search", "arguments": "{\"query\":\"q\"}"}}),
                ]),
                OutMessage::text("tool", OutContent::Text("{\"ok\":true}".into()))
                    .with_tool_call_id("c1"),
            ],
            params: ChatParams {
                temperature: Some(0.5),
                max_tokens: Some(400),
                ..Default::default()
            },
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
        let body = to_body(&req()).unwrap();
        assert_eq!(body["systemInstruction"]["parts"][0]["text"], "be nice");
        assert_eq!(body["generationConfig"]["temperature"], 0.5);
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 400);
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["name"],
            "web_search"
        );
        assert_eq!(body["toolConfig"]["functionCallingConfig"]["mode"], "ANY");
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "look");
        assert_eq!(contents[0]["parts"][1]["inline_data"]["data"], "QUJD");
        assert_eq!(contents[1]["role"], "model");
        assert_eq!(
            contents[1]["parts"][0]["functionCall"]["name"],
            "web_search"
        );
        assert_eq!(
            contents[1]["parts"][0]["functionCall"]["args"]["query"],
            "q"
        );
        assert_eq!(contents[2]["role"], "user");
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["name"],
            "web_search"
        );
        assert_eq!(
            contents[2]["parts"][0]["functionResponse"]["response"]["ok"],
            true
        );
    }

    #[test]
    fn parses_stream_frames() {
        let mut seq = 0;
        let c = parse_frame(
            r#"{"candidates":[{"content":{"parts":[{"text":"hi"},{"thought":true,"text":"mm"}]}}]}"#,
            &mut seq,
        )
        .unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].content, "hi");
        assert_eq!(c[1].reasoning, "mm");

        let c = parse_frame(
            r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"web_search","args":{"query":"q"}}},{"functionCall":{"name":"other","args":{}}}]}}]}"#,
            &mut seq,
        )
        .unwrap();
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].tool_calls[0]["id"], "call_0");
        assert_eq!(
            c[0].tool_calls[0]["function"]["arguments"],
            "{\"query\":\"q\"}"
        );
        assert_eq!(c[1].tool_calls[0]["id"], "call_1");
        assert_eq!(c[1].tool_calls[0]["index"], 1);
        assert_eq!(seq, 2);

        let c = parse_frame(
            r#"{"candidates":[{"content":{"parts":[{"text":"x"}]},"finishReason":"STOP"}]}"#,
            &mut seq,
        )
        .unwrap();
        assert_eq!(c.last().unwrap().finish_reason.as_deref(), Some("stop"));

        assert!(parse_frame("{}", &mut seq).unwrap().is_empty());
        assert!(parse_frame(r#"{"error":{"message":"boom"}}"#, &mut seq).is_err());
    }
}
