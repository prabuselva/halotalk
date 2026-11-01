use serde_json::{Value, json};

/// Tool definitions advertised to the model when a chat has `tools_enabled`.
pub fn tool_definitions() -> Vec<Value> {
    vec![json!({
        "type": "function",
        "function": {
            "name": "web_search",
            "description": "Search the web for current information. Returns a JSON list of results.",
            "parameters": {
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    }
                },
                "required": ["query"]
            }
        }
    })]
}

/// Cap a tool result so a verbose backend cannot blow up the next request.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Execute one model-requested tool call. Returns `(ok, result_text)`; on failure
/// `result_text` is a JSON `{"error": …}` object so the model can recover gracefully.
pub async fn run_tool(
    http: &reqwest::Client,
    name: &str,
    args_json: &str,
    search_url: Option<&str>,
) -> (bool, String) {
    match name {
        "web_search" => search(http, args_json, search_url).await,
        other => (
            false,
            json!({ "error": format!("unknown tool: {other}") }).to_string(),
        ),
    }
}

async fn search(
    http: &reqwest::Client,
    args_json: &str,
    search_url: Option<&str>,
) -> (bool, String) {
    let Some(url) = search_url else {
        return (
            false,
            json!({ "error": "web search is not configured (set HALO_WEB_SEARCH_URL)" })
                .to_string(),
        );
    };
    let args: Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => {
            return (
                false,
                json!({ "error": format!("invalid tool arguments: {e}") }).to_string(),
            );
        }
    };
    let Some(query) = args.get("query").and_then(Value::as_str) else {
        return (
            false,
            json!({ "error": "missing required argument: query" }).to_string(),
        );
    };
    let resp = match http
        .post(url)
        .json(&json!({ "query": query }))
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return (
                false,
                json!({ "error": format!("search backend unreachable: {e}") }).to_string(),
            );
        }
    };
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return (
            false,
            json!({ "error": format!("search backend HTTP {}: {}", status.as_u16(), truncate(&body, 200)) })
                .to_string(),
        );
    }
    match resp.json::<Value>().await {
        Ok(v) => (true, truncate(&v.to_string(), 6000)),
        Err(e) => (
            false,
            json!({ "error": format!("search backend returned invalid JSON: {e}") }).to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defines_web_search() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["function"]["name"], "web_search");
        assert!(defs[0]["function"]["parameters"]["required"].is_array());
    }

    #[test]
    fn truncation_counts_chars_not_bytes() {
        assert_eq!(truncate("héllo", 3), "hél…");
        assert_eq!(truncate("ab", 5), "ab");
    }

    #[tokio::test]
    async fn unknown_tool_is_error_result() {
        let (ok, res) = run_tool(&reqwest::Client::new(), "frobnicate", "{}", None).await;
        assert!(!ok);
        assert!(res.contains("unknown tool"));
    }

    #[tokio::test]
    async fn web_search_without_url_or_args_is_error_result() {
        let http = reqwest::Client::new();
        let (ok, res) = run_tool(&http, "web_search", "{}", None).await;
        assert!(!ok);
        assert!(res.contains("not configured"));
        let (ok, res) = run_tool(
            &http,
            "web_search",
            "not json",
            Some("http://127.0.0.1:9/x"),
        )
        .await;
        assert!(!ok);
        assert!(res.contains("invalid tool arguments"));
    }
}
