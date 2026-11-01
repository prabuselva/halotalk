use std::{collections::HashMap, convert::Infallible, path::PathBuf};

use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Multipart, Path, Query, Request, State, connect_info::ConnectInfo,
    },
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{
        IntoResponse, Response,
        sse::{Event, KeepAlive, Sse},
    },
    routing::{delete, get, patch, post},
};
use futures::StreamExt;
use halo_core::models::{Chat, Message, Provider};
use halo_providers::{
    AnyClient, ChatCompletionRequest, ChatParams, ImageUrl, OutContent, OutMessage, OutPart,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::services::{ServeDir, ServeFile};

use crate::AppState;
use crate::auth::{self, AuthUser, Authed};
use crate::tools;

pub fn router(state: AppState) -> Router {
    let cors = tower_http::cors::CorsLayer::permissive();

    let mut app = Router::new()
        .route("/api/health", get(health))
        .route("/api/auth/status", get(auth_status))
        .route("/api/auth/setup", post(auth_setup))
        .route("/api/auth/signup", post(auth_signup))
        .route("/api/auth/login", post(auth_login))
        .route("/api/auth/logout", post(auth_logout))
        .route("/api/auth/me", get(auth_me))
        .route("/api/auth/open-signup", patch(set_open_signup))
        .route(
            "/api/settings",
            get(get_user_settings).put(put_user_settings),
        )
        .route("/api/providers", get(list_providers).post(create_provider))
        .route(
            "/api/providers/{id}",
            patch(update_provider).delete(delete_provider),
        )
        .route("/api/providers/{id}/models", get(list_models))
        .route("/api/providers/{id}/test", post(test_provider))
        .route("/api/providers/{id}/metrics", get(provider_metrics))
        .route("/api/chats", get(list_chats).post(create_chat))
        .route(
            "/api/chats/{id}",
            get(get_chat).patch(update_chat).delete(delete_chat),
        )
        .route("/api/chats/{id}/messages", post(send_message))
        .route("/api/chats/{id}/events", get(chat_events))
        .route("/api/chats/{id}/stop", post(chat_stop))
        .route("/api/chats/{id}/regenerate", post(regenerate))
        .route("/api/chats/{id}/messages/{mid}/edit", post(edit_message))
        .route(
            "/api/chats/{id}/messages/{mid}/activate",
            post(activate_message),
        )
        .route("/api/search", get(search))
        .route("/api/kb", get(list_kb).post(add_kb).delete(delete_kb_file))
        .route("/api/kb/{id}", delete(delete_kb_item))
        .route("/api/audio/speech", post(audio_speech))
        .route(
            "/api/audio/transcribe",
            post(audio_transcribe).layer(DefaultBodyLimit::max(25 * 1024 * 1024)),
        )
        .route("/api/prompts", get(list_prompts).post(create_prompt))
        .route(
            "/api/prompts/{id}",
            patch(update_prompt).delete(delete_prompt),
        )
        .route("/api/presets", get(list_presets).post(create_preset))
        .route("/api/presets/{id}", delete(delete_preset))
        .route("/api/registry", get(registry_list).put(registry_set))
        .route("/api/config/export", get(config_export))
        .route("/api/config/import", post(config_import))
        .route(
            "/api/uploads",
            post(upload).layer(DefaultBodyLimit::max(20 * 1024 * 1024)),
        )
        .nest_service("/uploads", ServeDir::new(state.uploads.clone()))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_auth,
        ))
        .layer(cors)
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(middleware::from_fn_with_state(
            state.limiter.clone(),
            enforce_rate_limit,
        ))
        .with_state(state.clone());

    if state.web_dist.join("index.html").exists() {
        // Live disk copy wins so `npm run build` refreshes without a restart.
        app = app.fallback_service(
            ServeDir::new(state.web_dist.clone())
                .fallback(ServeFile::new(state.web_dist.join("index.html"))),
        );
    } else {
        app = app.fallback(embedded_web);
    }
    app
}

/// Fixed-window per-IP request budget (600/min). In-memory; resets on restart.
/// Behind a reverse proxy all clients share one bucket — prefer proxy-level
/// rate limiting for that topology.
#[derive(Clone, Default)]
pub struct RateLimiter {
    hits: std::sync::Arc<std::sync::Mutex<HashMap<std::net::IpAddr, (u32, std::time::Instant)>>>,
}

const RATE_MAX: u32 = 600;
const RATE_WINDOW: std::time::Duration = std::time::Duration::from_secs(60);

impl RateLimiter {
    fn allow(&self, ip: std::net::IpAddr) -> bool {
        let mut g = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        if g.len() > 4096 {
            g.retain(|_, (_, started)| started.elapsed() < RATE_WINDOW);
        }
        let e = g.entry(ip).or_insert((0, std::time::Instant::now()));
        if e.1.elapsed() >= RATE_WINDOW {
            *e = (0, std::time::Instant::now());
        }
        if e.0 >= RATE_MAX {
            return false;
        }
        e.0 += 1;
        true
    }
}

async fn enforce_rate_limit(
    State(limiter): State<RateLimiter>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    req: Request,
    next: Next,
) -> Response {
    if !limiter.allow(addr.ip()) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "rate limit exceeded, retry shortly" })),
        )
            .into_response();
    }
    next.run(req).await
}

const SPA_HINT: &str = concat!(
    "HaloTalk API is running, but no web UI build was found in ./web/dist. ",
    "For development run `npm run dev` in web/ (proxies to this port), ",
    "or `npm run build` to serve the UI from here."
);

#[derive(rust_embed::Embed)]
#[folder = "../../web/dist"]
struct EmbeddedWeb;

const PLACEHOLDER: &[u8] = b"<!--HALO-PLACEHOLDER-->";

async fn embedded_web(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let Some(f) = EmbeddedWeb::get(path).or_else(|| EmbeddedWeb::get("index.html")) else {
        return spa_hint().await.into_response();
    };
    if f.data.starts_with(PLACEHOLDER) {
        return spa_hint().await.into_response();
    }
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        mime_guess::from_path(path)
            .first_or_octet_stream()
            .as_ref()
            .parse()
            .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
    );
    h.insert(
        header::CACHE_CONTROL,
        if path == "index.html" {
            "no-store"
        } else {
            "public, max-age=31536000, immutable"
        }
        .parse()
        .unwrap(),
    );
    (h, f.data).into_response()
}

async fn spa_hint() -> &'static str {
    SPA_HINT
}

// ---------------------------------------------------------------------- errors

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
    fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, msg)
    }
    fn conflict(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, msg)
    }

    fn not_found(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, msg)
    }
    fn unauthorized(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, msg)
    }
    fn forbidden(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, msg)
    }
    fn internal(msg: impl std::fmt::Display) -> Self {
        tracing::error!("internal error: {msg}");
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error")
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self::internal(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

type ApiResult<T> = Result<T, ApiError>;

// ----------------------------------------------------------------------- misc

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true, "app": "halotalk", "version": env!("CARGO_PKG_VERSION") }))
}

// ----------------------------------------------------------------------- auth

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
    #[serde(default)]
    recovery_question: Option<String>,
    #[serde(default)]
    recovery_answer: Option<String>,
}

/// Lowercase `[a-z0-9._-]` usernames, 3–32 chars. None = invalid.
fn normalize_username(raw: &str) -> Option<String> {
    let u = raw.trim().to_lowercase();
    let ok = (3..=32).contains(&u.len())
        && u.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    ok.then_some(u)
}

async fn auth_status(State(state): State<AppState>) -> ApiResult<Json<Value>> {
    let count = state.store.count_users().await?;
    let open = state
        .store
        .get_app_setting("open_signup")
        .await?
        .is_some_and(|v| v == "1");
    Ok(Json(json!({
        "needs_setup": count == 0,
        "open_signup": count > 0 && open,
    })))
}

async fn create_session_response(
    state: &AppState,
    user: &AuthUser,
) -> ApiResult<(StatusCode, HeaderMap, Json<Value>)> {
    let token = auth::new_session_token();
    state
        .store
        .create_session(&user.id, &auth::token_hash(&token), auth::SESSION_DAYS)
        .await?;
    let mut headers = HeaderMap::new();
    headers.append(
        header::SET_COOKIE,
        auth::session_cookie(&token).parse().unwrap(),
    );
    Ok((
        StatusCode::OK,
        headers,
        Json(json!({ "id": user.id, "username": user.username, "is_admin": user.is_admin })),
    ))
}

async fn register_user(
    state: &AppState,
    creds: Credentials,
    as_admin: bool,
) -> ApiResult<AuthUser> {
    let username = normalize_username(&creds.username).ok_or_else(|| {
        ApiError::bad_request("username must be 3-32 chars: a-z, 0-9, '.', '_', '-'")
    })?;
    if creds.password.chars().count() < 8 {
        return Err(ApiError::bad_request(
            "password must be at least 8 characters",
        ));
    }
    let question = creds
        .recovery_question
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    if question.is_empty() {
        return Err(ApiError::bad_request("recovery question is required"));
    }
    if question.chars().count() > 200 {
        return Err(ApiError::bad_request(
            "recovery question must be at most 200 characters",
        ));
    }
    let answer = creds
        .recovery_answer
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if answer.chars().count() < 3 {
        return Err(ApiError::bad_request(
            "recovery answer must be at least 3 characters",
        ));
    }
    if state.store.get_user_by_name(&username).await?.is_some() {
        return Err(ApiError::bad_request("username already taken"));
    }
    let hash = auth::hash_password(&creds.password)
        .map_err(|e| ApiError::internal(format!("hash: {e}")))?;
    let answer_hash =
        auth::hash_password(&answer).map_err(|e| ApiError::internal(format!("hash: {e}")))?;
    let user = state
        .store
        .create_user(&username, &hash, as_admin, &question, &answer_hash)
        .await?;
    if as_admin {
        state.store.assign_legacy_rows(&user.id).await?;
    }
    Ok(AuthUser::from(user))
}

async fn auth_setup(
    State(state): State<AppState>,
    Json(creds): Json<Credentials>,
) -> ApiResult<(StatusCode, HeaderMap, Json<Value>)> {
    if state.store.count_users().await? > 0 {
        return Err(ApiError::forbidden("setup already completed"));
    }
    let user = register_user(&state, creds, true).await?;
    create_session_response(&state, &user).await
}

async fn auth_signup(
    State(state): State<AppState>,
    Json(creds): Json<Credentials>,
) -> ApiResult<(StatusCode, HeaderMap, Json<Value>)> {
    if state.store.count_users().await? == 0 {
        return Err(ApiError::forbidden("run setup first"));
    }
    let open = state
        .store
        .get_app_setting("open_signup")
        .await?
        .is_some_and(|v| v == "1");
    if !open {
        return Err(ApiError::forbidden("sign-up is disabled"));
    }
    let user = register_user(&state, creds, false).await?;
    create_session_response(&state, &user).await
}

async fn auth_login(
    State(state): State<AppState>,
    Json(creds): Json<Credentials>,
) -> ApiResult<(StatusCode, HeaderMap, Json<Value>)> {
    let username = normalize_username(&creds.username)
        .ok_or_else(|| ApiError::bad_request("invalid username"))?;
    let user = state.store.get_user_by_name(&username).await?;
    let valid = match &user {
        Some(u) => auth::verify_password(&u.pass_hash, &creds.password),
        None => false,
    };
    let Some(user) = user.filter(|_| valid) else {
        return Err(ApiError::unauthorized("wrong username or password"));
    };
    create_session_response(&state, &AuthUser::from(user)).await
}

async fn auth_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<impl IntoResponse> {
    if let Some(token) = auth::cookie_value(&headers, auth::SESSION_COOKIE) {
        state
            .store
            .delete_session(&auth::token_hash(&token))
            .await?;
    }
    let mut out = HeaderMap::new();
    out.append(
        header::SET_COOKIE,
        auth::cleared_session_cookie().parse().unwrap(),
    );
    Ok((StatusCode::NO_CONTENT, out))
}

async fn auth_me(Authed(user): Authed) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "id": user.id, "username": user.username, "is_admin": user.is_admin }),
    ))
}

#[derive(Deserialize)]
struct OpenSignup {
    open_signup: bool,
}

async fn set_open_signup(
    State(state): State<AppState>,
    Authed(user): Authed,
    Json(body): Json<OpenSignup>,
) -> ApiResult<StatusCode> {
    if !user.is_admin {
        return Err(ApiError::forbidden("admin only"));
    }
    state
        .store
        .set_app_setting("open_signup", if body.open_signup { "1" } else { "0" })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn get_user_settings(
    State(state): State<AppState>,
    Authed(user): Authed,
) -> ApiResult<Json<Value>> {
    let map: Value = state
        .store
        .list_user_settings(&user.id)
        .await?
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect::<serde_json::Map<_, _>>()
        .into();
    Ok(Json(json!({ "settings": map })))
}

async fn put_user_settings(
    State(state): State<AppState>,
    Authed(user): Authed,
    Json(body): Json<Value>,
) -> ApiResult<StatusCode> {
    let obj = body
        .get("settings")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::bad_request("expected { settings: {…} }"))?;
    if obj.len() > 50 {
        return Err(ApiError::bad_request("too many settings keys"));
    }
    for (k, v) in obj {
        if k.is_empty() || k.len() > 64 {
            return Err(ApiError::bad_request("invalid settings key"));
        }
        let sv = match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        state.store.set_user_setting(&user.id, k, &sv).await?;
    }
    Ok(StatusCode::NO_CONTENT)
}

// -------------------------------------------------------------------- prompts

async fn list_prompts(
    State(state): State<AppState>,
    Authed(user): Authed,
) -> ApiResult<Json<Value>> {
    Ok(Json(
        json!({ "prompts": state.store.list_prompts(&user.id).await? }),
    ))
}

async fn create_prompt(
    State(state): State<AppState>,
    Authed(user): Authed,
    Json(body): Json<Value>,
) -> ApiResult<(StatusCode, Json<halo_core::models::Prompt>)> {
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 120)
        .ok_or_else(|| ApiError::bad_request("title is required (max 120 chars)"))?;
    let text = body
        .get("body")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("body is required"))?;
    let prompt = state.store.create_prompt(&user.id, title, text).await?;
    Ok((StatusCode::CREATED, Json(prompt)))
}

async fn update_prompt(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult<StatusCode> {
    let title = body
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 120)
        .ok_or_else(|| ApiError::bad_request("title is required (max 120 chars)"))?;
    let text = body
        .get("body")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("body is required"))?;
    if !state
        .store
        .update_prompt(&user.id, &id, title, text)
        .await?
    {
        return Err(ApiError::not_found("prompt not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_prompt(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_prompt(&user.id, &id).await? {
        return Err(ApiError::not_found("prompt not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------ knowledge base

const KB_CHUNK_CHARS: usize = 1200;
const KB_MAX_CHARS: usize = 300_000;

/// Embeddings settings (user-level): provider + model. `None` when unset or
/// the referenced provider no longer exists.
async fn embed_provider_for(
    state: &AppState,
    user_id: &str,
) -> Option<(halo_core::models::Provider, String)> {
    let pid = state
        .store
        .get_user_setting(user_id, "embed_provider_id")
        .await
        .ok()
        .flatten()?;
    let model = state
        .store
        .get_user_setting(user_id, "embed_model")
        .await
        .ok()
        .flatten()?;
    if pid.is_empty() || model.is_empty() {
        return None;
    }
    let provider = state
        .store
        .get_provider(user_id, &pid)
        .await
        .ok()
        .flatten()?;
    Some((provider, model))
}

async fn list_kb(State(state): State<AppState>, Authed(user): Authed) -> ApiResult<Json<Value>> {
    let docs = state.store.list_kb(&user.id).await?;
    Ok(Json(json!({
        "documents": docs.iter().map(|d| d.public()).collect::<Vec<_>>()
    })))
}

async fn add_kb(
    State(state): State<AppState>,
    Authed(user): Authed,
    Json(body): Json<Value>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let source = body
        .get("source")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.chars().count() <= 200)
        .ok_or_else(|| ApiError::bad_request("source (file name) is required, max 200 chars"))?;
    let text = body
        .get("text")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("text is required and must not be empty"))?;
    if text.chars().count() > KB_MAX_CHARS {
        return Err(ApiError::bad_request(format!(
            "text too large (max {KB_MAX_CHARS} chars)"
        )));
    }
    let chunks = halo_core::rag::chunk_text(text, KB_CHUNK_CHARS);
    let cfg = embed_provider_for(&state, &user.id).await;
    let vectors: Option<Vec<Vec<f32>>> = match &cfg {
        Some((p, model)) => Some(
            state
                .provider
                .embed(&p.base_url, &p.api_key, model, &chunks)
                .await
                .map_err(|e| {
                    ApiError::new(
                        StatusCode::BAD_GATEWAY,
                        format!("embeddings request failed: {e}"),
                    )
                })?,
        ),
        None => None,
    };
    if let Some(vs) = &vectors
        && vs.len() != chunks.len()
    {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "embeddings response had missing vectors",
        ));
    }
    let embed_model = cfg.as_ref().map(|(_, m)| m.clone()).unwrap_or_default();
    let docs = state
        .store
        .insert_kb_docs(&user.id, source, &embed_model, &chunks, vectors.as_deref())
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "ids": docs.iter().map(|d| d.id.clone()).collect::<Vec<_>>(),
            "chunks": docs.len(),
            "embedded": vectors.is_some(),
        })),
    ))
}

async fn delete_kb_item(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_kb_doc(&user.id, &id).await? {
        return Err(ApiError::not_found("document not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_kb_file(
    State(state): State<AppState>,
    Authed(user): Authed,
    Query(q): Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let source = q
        .get("source")
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("source query param is required"))?;
    let n = state.store.delete_kb_source(&user.id, source).await?;
    Ok(Json(json!({ "deleted": n })))
}

// ---------------------------------------------------------------------- audio

async fn audio_provider_for(
    state: &AppState,
    user_id: &str,
    provider_key: &str,
    model_key: &str,
    what: &str,
) -> ApiResult<(Provider, String)> {
    let not_set = || {
        ApiError::bad_request(format!(
            "{what} is not configured — set provider and model in Settings"
        ))
    };
    let pid = state
        .store
        .get_user_setting(user_id, provider_key)
        .await?
        .filter(|s| !s.is_empty())
        .ok_or_else(not_set)?;
    let model = state
        .store
        .get_user_setting(user_id, model_key)
        .await?
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(not_set)?;
    let provider = state
        .store
        .get_provider(user_id, &pid)
        .await?
        .ok_or_else(|| ApiError::bad_request(format!("{what} provider no longer exists")))?;
    Ok((provider, model))
}

/// TTS passthrough: forwards `{text}` to the configured provider's
/// OpenAI-compatible `/audio/speech` and streams the audio bytes back.
async fn audio_speech(
    State(state): State<AppState>,
    Authed(user): Authed,
    Json(body): Json<Value>,
) -> ApiResult<Response> {
    let text = body
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| ApiError::bad_request("text is required"))?;
    if text.chars().count() > 4000 {
        return Err(ApiError::bad_request(
            "text too long for TTS (4000 char limit)",
        ));
    }
    let (provider, model) =
        audio_provider_for(&state, &user.id, "tts_provider_id", "tts_model", "TTS").await?;
    let url = format!("{}/audio/speech", provider.base_url.trim_end_matches('/'));
    let mut rb = reqwest::Client::new().post(&url).json(&json!({
        "model": model,
        "input": text,
        "response_format": "mp3",
    }));
    if !provider.api_key.is_empty() {
        rb = rb.bearer_auth(&provider.api_key);
    }
    let upstream = rb
        .send()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("tts request failed: {e}")))?;
    let status = upstream.status();
    let ct = upstream
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("audio/mpeg")
        .to_string();
    let bytes = upstream
        .bytes()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("tts body failed: {e}")))?;
    if !status.is_success() {
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            format!(
                "tts upstream {status}: {}",
                String::from_utf8_lossy(&bytes)
                    .chars()
                    .take(300)
                    .collect::<String>()
            ),
        ));
    }
    Ok((
        [
            (header::CONTENT_TYPE, ct),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        bytes,
    )
        .into_response())
}

/// STT passthrough: forwards the `file` part to the configured provider's
/// OpenAI-compatible `/audio/transcriptions` (whisper-style) and returns `{text}`.
async fn audio_transcribe(
    State(state): State<AppState>,
    Authed(user): Authed,
    mut multipart: Multipart,
) -> ApiResult<Json<Value>> {
    let mut audio: Option<(String, String, Vec<u8>)> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("multipart error: {e}")))?
    {
        if field.name() == Some("file") {
            let name = field.file_name().unwrap_or("speech.webm").to_string();
            let ct = field.content_type().unwrap_or("audio/webm").to_string();
            let bytes = field
                .bytes()
                .await
                .map_err(|e| ApiError::bad_request(e.to_string()))?;
            audio = Some((name, ct, bytes.to_vec()));
            break;
        }
    }
    let (name, ct, bytes) = audio
        .filter(|(_, _, b)| !b.is_empty())
        .ok_or_else(|| ApiError::bad_request("file part is required"))?;
    let (provider, model) =
        audio_provider_for(&state, &user.id, "stt_provider_id", "stt_model", "STT").await?;
    let url = format!(
        "{}/audio/transcriptions",
        provider.base_url.trim_end_matches('/')
    );
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(name)
        .mime_str(&ct)
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", model);
    let mut rb = reqwest::Client::new().post(&url);
    if !provider.api_key.is_empty() {
        rb = rb.bearer_auth(&provider.api_key);
    }
    let parsed: Value = rb
        .multipart(form)
        .send()
        .await
        .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, format!("stt request failed: {e}")))?
        .json()
        .await
        .map_err(|e| {
            ApiError::new(
                StatusCode::BAD_GATEWAY,
                format!("stt upstream returned non-JSON: {e}"),
            )
        })?;
    let text = parsed
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    Ok(Json(json!({ "text": text })))
}

// -------------------------------------------------------------------- presets

async fn list_presets(
    State(state): State<AppState>,
    Authed(user): Authed,
) -> ApiResult<Json<Value>> {
    let presets = state
        .store
        .list_presets(&user.id)
        .await?
        .into_iter()
        .map(|p| {
            json!({
                "id": p.id, "name": p.name, "created_at": p.created_at,
                "params": serde_json::from_str::<Value>(&p.params).unwrap_or_else(|_| json!({})),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "presets": presets })))
}

async fn create_preset(
    State(state): State<AppState>,
    Authed(user): Authed,
    Json(body): Json<Value>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let name = body
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.len() <= 80)
        .ok_or_else(|| ApiError::bad_request("name is required (max 80 chars)"))?;
    let params = body
        .get("params")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::bad_request("params must be an object"))?;
    for k in params.keys() {
        if !PRESET_KEYS.contains(&k.as_str()) {
            return Err(ApiError::bad_request(format!("unknown preset param `{k}`")));
        }
    }
    let preset = state
        .store
        .create_preset(&user.id, name, &json!(params).to_string())
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": preset.id, "name": preset.name, "created_at": preset.created_at,
            "params": params.clone(),
        })),
    ))
}

async fn delete_preset(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_preset(&user.id, &id).await? {
        return Err(ApiError::not_found("preset not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ------------------------------------------------------------ model registry

async fn registry_list(
    State(state): State<AppState>,
    Authed(user): Authed,
    Query(params): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let provider = params
        .get("provider_id")
        .cloned()
        .ok_or_else(|| ApiError::bad_request("provider_id is required"))?;
    if state
        .store
        .get_provider(&user.id, &provider)
        .await?
        .is_none()
    {
        return Err(ApiError::not_found("provider not found"));
    }
    let rows = state
        .store
        .list_registry(&provider)
        .await?
        .into_iter()
        .map(|(model_id, vision, reasoning, ctx_len)| {
            json!({ "model_id": model_id, "vision": vision, "reasoning": reasoning, "ctx_len": ctx_len })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({ "registry": rows })))
}

async fn registry_set(
    State(state): State<AppState>,
    Authed(user): Authed,
    Json(body): Json<Value>,
) -> ApiResult<StatusCode> {
    let provider = body
        .get("provider_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("provider_id is required"))?;
    if state
        .store
        .get_provider(&user.id, provider)
        .await?
        .is_none()
    {
        return Err(ApiError::not_found("provider not found"));
    }
    let model_id = body
        .get("model_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| ApiError::bad_request("model_id is required"))?;
    state
        .store
        .upsert_registry(
            provider,
            model_id,
            body.get("vision").and_then(Value::as_bool).unwrap_or(false),
            body.get("reasoning")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            body.get("ctx_len").and_then(Value::as_i64),
        )
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// --------------------------------------------------------------- config I/O

const PRESET_KEYS: [&str; 6] = [
    "model",
    "temperature",
    "top_p",
    "max_tokens",
    "system_prompt",
    "reasoning_effort",
];

/// Portability bundle for one user: providers (keys only on demand), prompts,
/// presets, registry rows (keyed by provider NAME so re-import can remap),
/// and user settings. Chats/messages are intentionally excluded.
async fn config_export(
    State(state): State<AppState>,
    Authed(user): Authed,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let secrets = q
        .get("include_secrets")
        .is_some_and(|v| v == "1" || v == "true");
    let mut providers = Vec::new();
    let mut registry = Vec::new();
    for p in state.store.list_providers(&user.id).await? {
        let mut obj = json!({
            "name": p.name, "base_url": p.base_url,
            "kind": p.kind, "is_default": p.is_default,
        });
        if secrets {
            obj["api_key"] = json!(p.api_key);
        }
        for (model_id, vision, reasoning, ctx_len) in state.store.list_registry(&p.id).await? {
            registry.push(json!({
                "provider": p.name, "model_id": model_id,
                "vision": vision, "reasoning": reasoning, "ctx_len": ctx_len,
            }));
        }
        providers.push(obj);
    }
    let prompts: Vec<Value> = state
        .store
        .list_prompts(&user.id)
        .await?
        .into_iter()
        .map(|p| json!({ "title": p.title, "body": p.body }))
        .collect();
    let presets: Vec<Value> = state
        .store
        .list_presets(&user.id)
        .await?
        .into_iter()
        .map(|p| {
            let params: Value = serde_json::from_str(&p.params).unwrap_or_else(|_| json!({}));
            json!({ "name": p.name, "params": params })
        })
        .collect();
    let settings: Value = state
        .store
        .list_user_settings(&user.id)
        .await?
        .into_iter()
        .map(|(k, v)| (k, Value::String(v)))
        .collect::<serde_json::Map<_, _>>()
        .into();
    Ok(Json(json!({
        "app": "halotalk",
        "config_version": 1,
        "providers": providers,
        "prompts": prompts,
        "presets": presets,
        "registry": registry,
        "settings": settings,
    })))
}

async fn config_import(
    State(state): State<AppState>,
    Authed(user): Authed,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    if body.get("app").and_then(Value::as_str) != Some("halotalk") {
        return Err(ApiError::bad_request("not a halotalk config file"));
    }
    let mut existing = state.store.list_providers(&user.id).await?;
    let mut name_to_new: HashMap<String, String> = HashMap::new();
    let mut imported = json!({
        "providers": 0, "prompts": 0, "presets": 0, "registry": 0, "settings": 0,
    });
    if let Some(arr) = body.get("providers").and_then(Value::as_array) {
        for p in arr {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("").trim();
            let base_url = p
                .get("base_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            if name.is_empty() || base_url.is_empty() {
                continue;
            }
            if existing
                .iter()
                .any(|e| e.name == name && e.base_url == base_url)
            {
                continue;
            }
            let mut spec = json!({ "name": name, "base_url": base_url });
            if let Some(k) = p.get("api_key").and_then(Value::as_str) {
                spec["api_key"] = json!(k);
            }
            let created = state
                .store
                .create_provider(&user.id, &spec)
                .await
                .map_err(|e| ApiError::bad_request(format!("provider `{name}`: {e}")))?;
            name_to_new.insert(name.to_string(), created.id.clone());
            existing.push(created);
            imported["providers"] = json!(imported["providers"].as_i64().unwrap_or(0) + 1);
        }
    }
    if let Some(arr) = body.get("prompts").and_then(Value::as_array) {
        for p in arr {
            let title = p.get("title").and_then(Value::as_str).unwrap_or("").trim();
            let pb = p.get("body").and_then(Value::as_str).unwrap_or("");
            if title.is_empty() || title.len() > 120 || pb.trim().is_empty() {
                continue;
            }
            if let Ok(ps) = state.store.list_prompts(&user.id).await
                && ps.iter().any(|e| e.title == title && e.body == pb)
            {
                continue;
            }
            state.store.create_prompt(&user.id, title, pb).await?;
            imported["prompts"] = json!(imported["prompts"].as_i64().unwrap_or(0) + 1);
        }
    }
    if let Some(arr) = body.get("presets").and_then(Value::as_array) {
        for p in arr {
            let name = p.get("name").and_then(Value::as_str).unwrap_or("").trim();
            let Some(params) = p.get("params").and_then(Value::as_object) else {
                continue;
            };
            if name.is_empty()
                || name.len() > 80
                || !params.keys().all(|k| PRESET_KEYS.contains(&k.as_str()))
            {
                continue;
            }
            let params_str = json!(params).to_string();
            if let Ok(ps) = state.store.list_presets(&user.id).await
                && ps.iter().any(|e| e.name == name && e.params == params_str)
            {
                continue;
            }
            state
                .store
                .create_preset(&user.id, name, &params_str)
                .await?;
            imported["presets"] = json!(imported["presets"].as_i64().unwrap_or(0) + 1);
        }
    }
    if let Some(arr) = body.get("registry").and_then(Value::as_array) {
        for r in arr {
            let pname = r.get("provider").and_then(Value::as_str).unwrap_or("");
            let model_id = r.get("model_id").and_then(Value::as_str).unwrap_or("");
            let provider_id = name_to_new.get(pname).cloned().or_else(|| {
                existing
                    .iter()
                    .find(|e| e.name == pname)
                    .map(|e| e.id.clone())
            });
            let (Some(pid), false) = (provider_id, model_id.is_empty()) else {
                continue;
            };
            state
                .store
                .upsert_registry(
                    &pid,
                    model_id,
                    r.get("vision").and_then(Value::as_bool).unwrap_or(false),
                    r.get("reasoning").and_then(Value::as_bool).unwrap_or(false),
                    r.get("ctx_len").and_then(Value::as_i64),
                )
                .await?;
            imported["registry"] = json!(imported["registry"].as_i64().unwrap_or(0) + 1);
        }
    }
    if let Some(obj) = body.get("settings").and_then(Value::as_object) {
        for (k, v) in obj {
            if k.is_empty() || k.len() > 64 {
                continue;
            }
            let sv = match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            state.store.set_user_setting(&user.id, k, &sv).await?;
            imported["settings"] = json!(imported["settings"].as_i64().unwrap_or(0) + 1);
        }
    }
    Ok(Json(json!({ "imported": imported })))
}

// ------------------------------------------------------------------ providers

async fn list_providers(
    State(state): State<AppState>,
    Authed(user): Authed,
) -> ApiResult<Json<Value>> {
    let providers = state.store.list_providers(&user.id).await?;
    Ok(Json(json!({
        "providers": providers
            .into_iter()
            .map(|p| serde_json::to_value(p.redacted()).unwrap_or(Value::Null))
            .collect::<Vec<_>>()
    })))
}

async fn create_provider(
    State(state): State<AppState>,
    Authed(user): Authed,
    Json(body): Json<Value>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let provider = state
        .store
        .create_provider(&user.id, &body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::to_value(provider.redacted()).unwrap()),
    ))
}

async fn update_provider(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Value>> {
    let provider = state
        .store
        .update_provider(&user.id, &id, &body)
        .await?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    Ok(Json(serde_json::to_value(provider.redacted()).unwrap()))
}

async fn delete_provider(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_provider(&user.id, &id).await? {
        return Err(ApiError::not_found("provider not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_models(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let provider = state
        .store
        .get_provider(&user.id, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    let models = list_models_for(
        &state.provider,
        &provider.kind,
        &provider.base_url,
        &provider.api_key,
    )
    .await
    .map_err(|e| ApiError::new(StatusCode::BAD_GATEWAY, e))?;
    for (info, caps) in &models {
        let Some((vision, reasoning, ctx)) = caps else {
            continue;
        };
        let Some(model_id) = info["id"].as_str() else {
            continue;
        };
        // Manual registry edits always win; detect only seeds new rows.
        if !state
            .store
            .registry_has(&provider.id, model_id)
            .await
            .map_err(ApiError::internal)?
        {
            state
                .store
                .upsert_registry(&provider.id, model_id, *vision, *reasoning, *ctx)
                .await
                .map_err(ApiError::internal)?;
        }
    }
    let ids: Vec<&Value> = models.iter().map(|(info, _)| info).collect();
    Ok(Json(json!({ "models": ids })))
}

/// Connectivity check: hit `<base_url>/models` and report reachability.
/// Always 200 — the result is data, not an HTTP failure.
async fn test_provider(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let provider = state
        .store
        .get_provider(&user.id, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    let started = std::time::Instant::now();
    let outcome = list_models_for(
        &state.provider,
        &provider.kind,
        &provider.base_url,
        &provider.api_key,
    )
    .await;
    let ms = started.elapsed().as_millis();
    Ok(Json(match outcome {
        Ok(models) => json!({ "ok": true, "models": models.len(), "ms": ms }),
        Err(error) => json!({ "ok": false, "error": error, "ms": ms }),
    }))
}

/// llama.cpp-style Prometheus counters live at `<host>/metrics`, not under the
/// API prefix; strip a trailing `/v1` from the configured base URL.
fn metrics_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let base = base
        .strip_suffix("/v1")
        .unwrap_or(base)
        .trim_end_matches('/');
    format!("{base}/metrics")
}

/// llama.cpp `/metrics` passthrough (prefill/decode + spec-decode counters).
/// Unreachable or non-200 upstream is data, not an error: `{"metrics": null}`.
async fn provider_metrics(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let provider = state
        .store
        .get_provider(&user.id, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("provider not found"))?;
    let mut rb = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(ApiError::internal)?
        .get(metrics_url(&provider.base_url));
    if !provider.api_key.is_empty() {
        rb = rb.bearer_auth(&provider.api_key);
    }
    let metrics = match rb.send().await {
        Ok(res) if res.status().is_success() => res.text().await.ok(),
        _ => None,
    };
    Ok(Json(json!({ "metrics": metrics })))
}

type ModelCaps = Option<(bool, bool, Option<i64>)>;

type DetectedModel = (Value, ModelCaps);

async fn list_models_for(
    client: &AnyClient,
    kind: &str,
    base_url: &str,
    api_key: &str,
) -> Result<Vec<DetectedModel>, String> {
    let items = client
        .list_models(kind, base_url, api_key)
        .await
        .map_err(|e| format!("provider unavailable: {e}"))?;
    let mut models: Vec<DetectedModel> = items
        .iter()
        .filter_map(|m| {
            let id = m.get("id").and_then(Value::as_str).or(m.as_str())?;
            let name = m.get("name").and_then(Value::as_str).unwrap_or(id);
            Some((json!({ "id": id, "name": name }), detect_caps(m)))
        })
        .collect();
    models.sort_by(|a, b| {
        a.0["id"]
            .as_str()
            .unwrap_or("")
            .cmp(b.0["id"].as_str().unwrap_or(""))
    });
    Ok(models)
}

/// Heuristic caps from fields providers may advertise: `vision`,
/// `architecture.input_modalities`, `reasoning`, `supported_parameters`,
/// `context_length`. None when the model advertises nothing usable.
fn detect_caps(m: &Value) -> ModelCaps {
    let vision = m.get("vision").and_then(Value::as_bool).or_else(|| {
        m.get("architecture")
            .and_then(|a| a.get("input_modalities"))
            .and_then(Value::as_array)
            .map(|a| a.iter().any(|x| x.as_str() == Some("image")))
    });
    let reasoning = m.get("reasoning").and_then(Value::as_bool).or_else(|| {
        m.get("supported_parameters")
            .and_then(Value::as_array)
            .map(|a| a.iter().any(|x| x.as_str() == Some("reasoning_effort")))
    });
    let ctx = m
        .get("context_length")
        .and_then(Value::as_i64)
        .or_else(|| m.get("max_context_length").and_then(Value::as_i64));
    if vision.is_some() || reasoning.is_some() {
        Some((vision.unwrap_or(false), reasoning.unwrap_or(false), ctx))
    } else {
        None
    }
}

// ---------------------------------------------------------------------- chats

async fn list_chats(
    State(state): State<AppState>,
    Authed(user): Authed,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let include_archived = params.get("archived").map(String::as_str) == Some("1");
    let chats = state.store.list_chats(&user.id, include_archived).await?;
    Ok(Json(json!({ "chats": chats })))
}

async fn create_chat(
    State(state): State<AppState>,
    Authed(user): Authed,
    Json(mut body): Json<Value>,
) -> ApiResult<(StatusCode, Json<Chat>)> {
    let has_sp = body
        .get("system_prompt")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.trim().is_empty());
    if !has_sp {
        let sp = state
            .store
            .get_user_setting(&user.id, "default_system_prompt")
            .await?;
        if let Some(sp) = sp.filter(|s| !s.trim().is_empty()) {
            body["system_prompt"] = json!(sp);
        }
    }
    // Onboarding fallback: chats created without an explicit pick inherit the
    // default provider and, if known, one of its registered models.
    if body.get("provider_id").and_then(Value::as_str).is_none()
        && let Some(def) = state
            .store
            .list_providers(&user.id)
            .await?
            .into_iter()
            .find(|p| p.is_default)
    {
        body["provider_id"] = json!(def.id);
        if body.get("model").and_then(Value::as_str).is_none()
            && let Some((mid, _, _, _)) =
                state.store.list_registry(&def.id).await?.into_iter().next()
        {
            body["model"] = json!(mid);
        }
    }
    let chat = state
        .store
        .create_chat(&user.id, &body)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok((StatusCode::CREATED, Json(chat)))
}

async fn get_chat(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
) -> ApiResult<Json<Value>> {
    let (chat, messages) = state
        .store
        .get_chat_with_messages_for_user(&user.id, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("chat not found"))?;
    Ok(Json(json!({
        "chat": chat,
        "messages": messages.into_iter().map(message_out).collect::<Vec<_>>(),
    })))
}

async fn update_chat(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
    Json(body): Json<Value>,
) -> ApiResult<Json<Chat>> {
    state
        .store
        .update_chat(&user.id, &id, &body)
        .await?
        .map(Json)
        .ok_or_else(|| ApiError::not_found("chat not found"))
}

async fn delete_chat(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    if !state.store.delete_chat(&user.id, &id).await? {
        return Err(ApiError::not_found("chat not found"));
    }
    Ok(StatusCode::NO_CONTENT)
}

fn message_out(m: Message) -> Value {
    let images: Vec<String> = m
        .images
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let sources: Vec<Value> = m
        .sources
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    json!({
        "id": m.id,
        "chat_id": m.chat_id,
        "role": m.role,
        "content": m.content,
        "reasoning": m.reasoning,
        "images": images,
        "error": m.error,
        "reply_to": m.reply_to,
        "active": m.active,
        "sources": sources,
        "created_at": m.created_at,
    })
}

// ------------------------------------------------------------------ chat send

#[derive(Deserialize)]
struct SendRequest {
    content: String,
    #[serde(default)]
    images: Vec<String>,
}

const MAX_IMAGES: usize = 4;
const MAX_IMAGE_BYTES: usize = 15 * 1024 * 1024;

async fn send_message(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(chat_id): Path<String>,
    Json(req): Json<SendRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    send_and_stream(state, user.id, chat_id, req).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "ok": true }))))
}

#[derive(Deserialize)]
struct RegenerateRequest {
    /// Assistant message to drop and redo; defaults to the last assistant reply.
    #[serde(default)]
    message_id: Option<String>,
}

async fn regenerate(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(chat_id): Path<String>,
    Json(req): Json<RegenerateRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    regenerate_and_stream(state, user.id, chat_id, req).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "ok": true }))))
}

#[derive(Deserialize)]
struct EditRequest {
    content: String,
}

async fn edit_message(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path((chat_id, mid)): Path<(String, String)>,
    Json(req): Json<EditRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    edit_and_stream(state, user.id, chat_id, mid, req).await?;
    Ok((StatusCode::ACCEPTED, Json(json!({ "ok": true }))))
}

/// Live event stream for a chat: initial `state` snapshot frame (or
/// `{"streaming": false}`) followed by all live frames until the channel
/// closes; EventSource reconnects and re-snapshots.
async fn chat_events(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(chat_id): Path<String>,
) -> ApiResult<Response> {
    if state
        .store
        .get_chat_for_user(&user.id, &chat_id)
        .await?
        .is_none()
    {
        return Err(ApiError::not_found("chat not found"));
    }
    let turn = state
        .hub
        .current(&chat_id)
        .unwrap_or_else(|| state.hub.join_idle(&chat_id));
    let initial = if turn.live() {
        turn.snapshot_frame()
    } else {
        json!({ "streaming": false })
    };
    let rx = turn.subscribe();
    let stream = futures::stream::once(async move {
        Ok::<_, Infallible>(Event::default().event("state").data(initial.to_string()))
    })
    .chain(futures::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok((event, data)) => Some((
                Ok::<_, Infallible>(Event::default().event(&event).data(data)),
                rx,
            )),
            Err(_) => None,
        }
    }))
    .boxed();
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

async fn chat_stop(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path(chat_id): Path<String>,
) -> ApiResult<Json<Value>> {
    if state
        .store
        .get_chat_for_user(&user.id, &chat_id)
        .await?
        .is_none()
    {
        return Err(ApiError::not_found("chat not found"));
    }
    let stopped = state.hub.stop(&chat_id);
    Ok(Json(json!({ "ok": true, "stopped": stopped })))
}

fn conflict_running(turn: &crate::hub::Turn) -> ApiError {
    ApiError::conflict(format!(
        "generation already running (message {})",
        turn.id.as_deref().unwrap_or("?")
    ))
}

/// Switch which sibling reply is active for a branched assistant message.
async fn activate_message(
    State(state): State<AppState>,
    Authed(user): Authed,
    Path((chat_id, mid)): Path<(String, String)>,
) -> ApiResult<Json<Value>> {
    if state
        .store
        .get_chat_for_user(&user.id, &chat_id)
        .await?
        .is_none()
    {
        return Err(ApiError::not_found("chat not found"));
    }
    let changed = state.store.set_active_message(&chat_id, &mid).await?;
    let (index, total) = state.store.branch_info(&chat_id, &mid).await?;
    Ok(Json(
        json!({ "ok": true, "changed": changed, "index": index, "total": total }),
    ))
}

async fn search(
    State(state): State<AppState>,
    Authed(user): Authed,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> ApiResult<Json<Value>> {
    let q = params.get("q").map(String::as_str).unwrap_or("").trim();
    if q.is_empty() {
        return Ok(Json(json!({ "hits": [] })));
    }
    let hits = state.store.search(&user.id, q, 24).await?;
    Ok(Json(json!({ "hits": hits })))
}

/// Persist the user turn (auto-titling the chat) and stream the reply.
async fn send_and_stream(
    state: AppState,
    user_id: String,
    chat_id: String,
    req: SendRequest,
) -> ApiResult<()> {
    let chat = state
        .store
        .get_chat_for_user(&user_id, &chat_id)
        .await?
        .ok_or_else(|| ApiError::not_found("chat not found"))?;
    let content = req.content.trim().to_string();
    if content.is_empty() && req.images.is_empty() {
        return Err(ApiError::bad_request("message is empty"));
    }
    if req.images.len() > MAX_IMAGES {
        return Err(ApiError::bad_request(format!(
            "at most {MAX_IMAGES} images per message"
        )));
    }
    // Validate provider/model before persisting the user turn, so a rejected
    // send does not leave an orphan message that replays on the next attempt.
    resolve_provider(&state, &chat).await?;
    require_model(&chat)?;
    if let Some(cur) = state.hub.live_turn(&chat_id) {
        return Err(conflict_running(&cur));
    }
    let images_json = (!req.images.is_empty()).then(|| serde_json::to_string(&req.images).unwrap());
    let auto_title = chat.title == "New chat" && !content.is_empty();
    let title = if auto_title {
        title_from(&content)
    } else {
        chat.title.clone()
    };
    let user_msg = state
        .store
        .insert_message(&chat_id, "user", &content, images_json.as_deref())
        .await?;
    if auto_title {
        state
            .store
            .update_chat(&user_id, &chat_id, &json!({ "title": &title }))
            .await?;
    }
    let history = state.store.get_messages(&chat_id).await?;
    completion_stream(
        state,
        &chat,
        history,
        title,
        auto_title,
        vec![("user", message_out(user_msg))],
    )
    .await
}

/// Regenerate an assistant reply. Modern replies become a new sibling in the
/// branch of their user message (older replies stay switchable); later turns
/// after the branch point are dropped. Legacy replies without a parent are
/// dropped and redone in place.
async fn regenerate_and_stream(
    state: AppState,
    user_id: String,
    chat_id: String,
    req: RegenerateRequest,
) -> ApiResult<()> {
    let chat = state
        .store
        .get_chat_for_user(&user_id, &chat_id)
        .await?
        .ok_or_else(|| ApiError::not_found("chat not found"))?;
    if let Some(cur) = state.hub.live_turn(&chat_id) {
        return Err(conflict_running(&cur));
    }
    let history = state.store.get_messages(&chat_id).await?;
    let anchor = match req.message_id {
        Some(id) => Some(id),
        None => history
            .iter()
            .rev()
            .find(|m| m.role == "assistant")
            .map(|m| m.id.clone()),
    }
    .ok_or_else(|| ApiError::bad_request("nothing to regenerate"))?;
    if !history
        .iter()
        .any(|m| m.id == anchor && m.role == "assistant")
    {
        return Err(ApiError::bad_request(
            "regenerate target is not an assistant message",
        ));
    }
    let parent = history
        .iter()
        .find(|m| m.id == anchor)
        .map(|m| m.reply_to.clone())
        .filter(|r| !r.is_empty());
    let history = match parent {
        // Branch: keep previous replies to that user message as switchable
        // siblings; the fresh one becomes active (insert_assistant_reply).
        Some(parent) => {
            state
                .store
                .truncate_after_keeping_branch(&chat_id, &parent)
                .await?;
            let mut history = history;
            if let Some(a) = history.iter_mut().find(|m| m.id == anchor) {
                a.active = false;
            }
            history
        }
        // Legacy replies without a branch parent: drop and redo in place.
        None => {
            state
                .store
                .truncate_messages(&chat_id, &anchor, true)
                .await?;
            let history = state.store.get_messages(&chat_id).await?;
            if history.last().map(|m| m.role.as_str()) != Some("user") {
                return Err(ApiError::bad_request(
                    "conversation must end with a user message",
                ));
            }
            history
        }
    };
    completion_stream(
        state,
        &chat,
        history,
        chat.title.clone(),
        false,
        vec![("resync", json!({ "reason": "regenerate" }))],
    )
    .await
}

/// Replace a user message's text, drop everything after it, and stream a fresh
/// reply from the edited point.
async fn edit_and_stream(
    state: AppState,
    user_id: String,
    chat_id: String,
    mid: String,
    req: EditRequest,
) -> ApiResult<()> {
    let chat = state
        .store
        .get_chat_for_user(&user_id, &chat_id)
        .await?
        .ok_or_else(|| ApiError::not_found("chat not found"))?;
    if let Some(cur) = state.hub.live_turn(&chat_id) {
        return Err(conflict_running(&cur));
    }
    let history = state.store.get_messages(&chat_id).await?;
    let Some(target) = history.iter().find(|m| m.id == mid) else {
        return Err(ApiError::not_found("message not found"));
    };
    if target.role != "user" {
        return Err(ApiError::bad_request("only user messages can be edited"));
    }
    let content = req.content.trim();
    if content.is_empty() {
        return Err(ApiError::bad_request("message is empty"));
    }
    state.store.update_message_content(&mid, content).await?;
    state.store.truncate_messages(&chat_id, &mid, false).await?;
    let history = state.store.get_messages(&chat_id).await?;
    completion_stream(
        state,
        &chat,
        history,
        chat.title.clone(),
        false,
        vec![("resync", json!({ "reason": "edit" }))],
    )
    .await
}

/// Brute-force cosine top-k over the user's knowledge base (3.2c). `None` when
/// embeddings are not configured, the call fails, or nothing clears the bar.
async fn kb_retrieve(state: &AppState, user_id: &str, query: &str) -> Option<(String, Vec<Value>)> {
    let (provider, model) = embed_provider_for(state, user_id).await?;
    let mut qvs = state
        .provider
        .embed(
            &provider.base_url,
            &provider.api_key,
            &model,
            &[query.to_string()],
        )
        .await
        .ok()?;
    let qv = qvs.pop()?;
    let pool = state.store.kb_search_pool(user_id).await.ok()?;
    let mut scored: Vec<(f32, halo_core::models::KbDoc)> = pool
        .into_iter()
        .map(|(d, v)| (halo_core::rag::cosine(&qv, &v), d))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    let top: Vec<&(f32, halo_core::models::KbDoc)> =
        scored.iter().filter(|(s, _)| *s > 0.15).take(4).collect();
    if top.is_empty() {
        return None;
    }
    let ctx = top
        .iter()
        .map(|(_, d)| format!("[{}] {}", d.title, d.text))
        .collect::<Vec<_>>()
        .join("\n\n");
    let cites = top
        .iter()
        .map(|(s, d)| json!({ "title": d.title, "score": (s * 1000.0).round() / 1000.0 }))
        .collect();
    Some((ctx, cites))
}

/// Shared pipeline: map history to OpenAI messages, insert the assistant row,
/// stream deltas as SSE while persisting. Client disconnect (Stop button or
/// closed tab) cancels the upstream request and finalizes the partial reply.
async fn resolve_provider(state: &AppState, chat: &Chat) -> ApiResult<Provider> {
    state
        .store
        .get_provider(&chat.user_id, chat.provider_id.as_deref().unwrap_or(""))
        .await?
        .ok_or_else(|| ApiError::bad_request("this chat has no provider — pick one in Settings"))
}

fn require_model(chat: &Chat) -> ApiResult<String> {
    chat.model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("no model selected — pick one in Settings"))
}

async fn completion_stream(
    state: AppState,
    chat: &Chat,
    history: Vec<Message>,
    title: String,
    suggest_title: bool,
    pre_events: Vec<(&'static str, Value)>,
) -> ApiResult<()> {
    let provider = resolve_provider(&state, chat).await?;
    let model = require_model(chat)?;

    let mut messages: Vec<OutMessage> = Vec::new();
    if let Some(sp) = chat
        .system_prompt
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        messages.push(OutMessage::text("system", OutContent::Text(sp.to_string())));
    }
    // RAG v1 (3.2c): when enabled, embed the last user message and inject the
    // best-matching knowledge-base chunks as a second system message; the same
    // chunks travel back as `sources` citations on the reply.
    let mut kb_citations: Vec<Value> = Vec::new();
    if chat.kb_enabled {
        let query = history
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_default();
        if !query.trim().is_empty()
            && let Some((ctx, cites)) = kb_retrieve(&state, &chat.user_id, &query).await
        {
            kb_citations = cites;
            messages.push(OutMessage::text(
                "system",
                OutContent::Text(format!(
                    "Knowledge base excerpts — ground your answer in these and cite by title:\n\n{ctx}"
                )),
            ));
        }
    }
    // Rebuild the conversation OpenAI-style: `tool` rows become
    // assistant{tool_calls} + tool{tool_call_id} pairs; the group JSON stored on
    // each tool row is emitted as the assistant message only once per round.
    let mut last_group: Option<String> = None;
    for m in &history {
        if m.role == "tool" {
            let group = m.tool_calls.clone().unwrap_or_default();
            if Some(group.as_str()) != last_group.as_deref()
                && let Ok(calls) = serde_json::from_str::<Vec<Value>>(&group)
            {
                messages.push(
                    OutMessage::text("assistant", OutContent::Text(String::new()))
                        .with_tool_calls(calls),
                );
            }
            last_group = Some(group);
            messages.push(
                OutMessage::text("tool", OutContent::Text(m.content.clone()))
                    .with_tool_call_id(m.tool_call_id.as_deref().unwrap_or("")),
            );
            continue;
        }
        let images: Vec<String> = m
            .images
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();
        if m.role == "assistant"
            && (!m.active || (m.content.trim().is_empty() && m.error.is_none()))
        {
            continue;
        }
        let body = if m.role == "user" && !images.is_empty() {
            let mut parts = Vec::new();
            if !m.content.is_empty() {
                parts.push(OutPart::Text {
                    text: m.content.clone(),
                });
            }
            parts.extend(images.into_iter().map(|url| OutPart::ImageUrl {
                image_url: ImageUrl { url },
            }));
            OutContent::Parts(parts)
        } else {
            OutContent::Text(m.content.clone())
        };
        messages.push(OutMessage::text(&m.role, body));
    }

    let completion = ChatCompletionRequest {
        model,
        messages,
        params: ChatParams {
            temperature: chat.temperature,
            top_p: chat.top_p,
            max_tokens: chat.max_tokens,
            reasoning_effort: chat.reasoning_effort.clone(),
        },
        tools: chat.tools_enabled.then(tools::tool_definitions),
        tool_choice: None,
    };

    let reply_to = history
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.id.clone())
        .unwrap_or_default();
    let assistant = if reply_to.is_empty() {
        state
            .store
            .insert_message(&chat.id, "assistant", "", None)
            .await?
    } else {
        state
            .store
            .insert_assistant_reply(&chat.id, &reply_to)
            .await?
    };
    let meta = json!({
        "chat_id": &chat.id,
        "message_id": assistant.id,
        "title": title,
    });

    let turn = state
        .hub
        .start(&chat.id, &assistant.id)
        .map_err(|cur| conflict_running(&cur))?;

    // First exchange: ask the model for a short title in parallel with the
    // answer. Emits an SSE `title` event when ready; silent on any failure
    // (the content-derived fallback title stays in place).
    if suggest_title {
        let title_turn = turn.clone();
        let client = state.provider.clone();
        let store = state.store.clone();
        let base = provider.base_url.clone();
        let key = provider.api_key.clone();
        let kind = provider.kind.clone();
        let model = completion.model.clone();
        let chat_id = chat.id.clone();
        let chat_user_id = chat.user_id.clone();
        let seed = history
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_default();
        tokio::spawn(async move {
            let req = ChatCompletionRequest {
                model,
                messages: vec![
                    OutMessage::text("system", OutContent::Text(TITLE_PROMPT.to_string())),
                    OutMessage::text("user", OutContent::Text(seed.chars().take(600).collect())),
                ],
                params: ChatParams {
                    max_tokens: Some(24),
                    ..Default::default()
                },
                tools: None,
                tool_choice: None,
            };
            let outcome = tokio::time::timeout(
                std::time::Duration::from_secs(20),
                client.complete(&kind, &base, &key, &req),
            );
            if let Ok(Ok(raw)) = outcome.await
                && let Some(t) = clean_title(&raw)
            {
                let _ = store
                    .update_chat(&chat_user_id, &chat_id, &json!({ "title": &t }))
                    .await;
                title_turn.emit("title", json!({ "title": t }));
            }
        });
    }

    let store = state.store.clone();
    let msg_id = assistant.id;
    let chat_id = chat.id.clone();
    let client = state.provider.clone();
    let base = provider.base_url.clone();
    let key = provider.api_key.clone();
    let kind = provider.kind.clone();
    let search_url = std::env::var("HALO_WEB_SEARCH_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let mut completion = completion;
    let kb_sources_json = json!(kb_citations).to_string();
    let hub = state.hub.clone();
    tokio::spawn(async move {
        for (event, data) in pre_events {
            turn.emit(event, data);
        }
        turn.emit("meta", meta);
        if kb_sources_json != "[]" {
            let _ = store.set_message_sources(&msg_id, &kb_sources_json).await;
            turn.emit(
                "sources",
                serde_json::from_str::<Value>(&kb_sources_json).unwrap_or(Value::Null),
            );
        }
        let (mut content, mut reasoning): (String, String) = (String::new(), String::new());
        let mut error: Option<String> = None;
        let http = reqwest::Client::new();
        // Rounds: when a round ends in tool calls, results are executed, persisted
        // and appended, then the request is re-streamed; bounded to keep loops finite.
        for _round in 0..4u32 {
            if turn.stopping() || error.is_some() {
                break;
            }
            let mut upstream = match client.stream(&kind, &base, &key, &completion).await {
                Err(e) => {
                    error = Some(e.to_string());
                    break;
                }
                Ok(s) => s,
            };
            let mut round_text = String::new();
            // Merged tool-call fragments keyed by OpenAI `index`: (id, name, arguments).
            let mut calls: std::collections::BTreeMap<u64, (String, String, String)> =
                Default::default();
            loop {
                let item = tokio::select! {
                    _ = turn.stop_signal() => None,
                    item = upstream.next() => item,
                };
                let Some(item) = item else { break };
                match item {
                    Ok(chunk) => {
                        if !chunk.reasoning.is_empty() {
                            reasoning.push_str(&chunk.reasoning);
                            turn.emit_delta("reasoning", &chunk.reasoning, &content, &reasoning);
                        }
                        if !chunk.content.is_empty() {
                            content.push_str(&chunk.content);
                            round_text.push_str(&chunk.content);
                            turn.emit_delta("token", &chunk.content, &content, &reasoning);
                        }
                        for frag in &chunk.tool_calls {
                            let idx = frag.get("index").and_then(Value::as_u64).unwrap_or(0);
                            let entry = calls.entry(idx).or_default();
                            if let Some(id) = frag.get("id").and_then(Value::as_str) {
                                entry.0 = id.to_string();
                            }
                            if let Some(name) = frag["function"]["name"].as_str() {
                                entry.1 = name.to_string();
                            }
                            if let Some(args) = frag["function"]["arguments"].as_str() {
                                entry.2.push_str(args);
                            }
                        }
                    }
                    Err(e) => {
                        error = Some(e.to_string());
                        break;
                    }
                }
            }
            if error.is_some() || turn.stopping() || calls.is_empty() {
                break;
            }
            let group: Vec<Value> = calls
                .values()
                .map(|(id, name, args)| {
                    json!({ "id": id, "type": "function",
                            "function": { "name": name, "arguments": args } })
                })
                .collect();
            let group_json = serde_json::to_string(&group).unwrap_or_else(|_| "[]".into());
            completion.messages.push(
                OutMessage::text("assistant", OutContent::Text(round_text)).with_tool_calls(group),
            );
            for (id, name, args) in calls.values() {
                if turn.stopping() {
                    break;
                }
                turn.emit(
                    "tool",
                    json!({ "phase": "call", "id": id, "name": name, "arguments": args }),
                );
                let (ok, result) = tools::run_tool(&http, name, args, search_url.as_deref()).await;
                if let Err(e) = store
                    .insert_tool_message(&chat_id, id, &group_json, &result)
                    .await
                {
                    tracing::error!("failed to persist tool result {id}: {e}");
                }
                let preview: String = result.chars().take(200).collect();
                turn.emit(
                    "tool",
                    json!({ "phase": "result", "id": id, "ok": ok, "preview": preview }),
                );
                completion
                    .messages
                    .push(OutMessage::text("tool", OutContent::Text(result)).with_tool_call_id(id));
            }
        }
        if turn.stopping() {
            tracing::info!("stop requested — finalizing partial generation for {msg_id}");
        }
        if let Err(e) = store
            .finalize_message(
                &msg_id,
                &content,
                (!reasoning.is_empty()).then_some(reasoning.as_str()),
                error.as_deref(),
            )
            .await
        {
            tracing::error!("failed to finalize message {msg_id}: {e}");
        }
        let _ = store.touch_chat(&chat_id).await;
        if let Some(err) = &error {
            turn.emit("error", json!({ "message": err }));
        }
        turn.emit("done", json!({}));
        hub.finish(&chat_id, &turn);
    });

    Ok(())
}

fn title_from(content: &str) -> String {
    let line = content
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    let short: String = line.chars().take(48).collect();
    if line.chars().count() > 48 {
        format!("{short}…")
    } else {
        short
    }
}

const TITLE_PROMPT: &str = "Write a short title (at most 6 words) that names the topic of this \
                             conversation. Reply with only the title, no quotes or trailing \
                             punctuation.";

/// Normalize a model-proposed title: first non-empty line, stripped of quotes
/// and trailing punctuation, capped at 48 chars. None = unusable.
fn clean_title(raw: &str) -> Option<String> {
    let line = raw
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())?
        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
        .trim_end_matches(['.', ':', ';', '!', '?'])
        .trim();
    if line.is_empty() {
        return None;
    }
    let short: String = line.chars().take(48).collect();
    Some(short)
}

// -------------------------------------------------------------------- uploads

async fn upload(State(state): State<AppState>, mut multipart: Multipart) -> ApiResult<Json<Value>> {
    let mut saved: Vec<String> = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::bad_request(format!("multipart error: {e}")))?
    {
        let ct = field
            .content_type()
            .unwrap_or("application/octet-stream")
            .to_string();
        if !ct.starts_with("image/") {
            return Err(ApiError::bad_request("only image files are allowed"));
        }
        let bytes = field
            .bytes()
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        if bytes.is_empty() {
            return Err(ApiError::bad_request("empty file"));
        }
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(ApiError::bad_request("image exceeds 15 MB"));
        }
        let ext = match ct.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => "bin",
        };
        let name = format!("{}.{}", uuid::Uuid::now_v7(), ext);
        tokio::fs::write(upload_path(&state.uploads, &name), &bytes)
            .await
            .map_err(|e| ApiError::internal(format!("write upload: {e}")))?;
        saved.push(format!("/uploads/{name}"));
    }
    if saved.is_empty() {
        return Err(ApiError::bad_request("no file part in request"));
    }
    Ok(Json(json!({ "urls": saved })))
}

/// Join with traversal protection (defense in depth below ServeDir's own guard).
fn upload_path(dir: &std::path::Path, name: &str) -> PathBuf {
    let safe = name.rsplit('/').next().unwrap_or(name);
    let safe = safe.rsplit('\\').next().unwrap_or(safe);
    dir.join(safe)
}

#[cfg(test)]
mod cap_tests {
    use super::detect_caps;
    use serde_json::json;

    #[test]
    fn detect_nothing() {
        assert_eq!(detect_caps(&json!({ "id": "m" })), None);
    }

    #[test]
    fn detect_vision_modality_and_ctx() {
        let m = json!({
            "id": "m",
            "architecture": { "input_modalities": ["text", "image"] },
            "context_length": 128000
        });
        assert_eq!(detect_caps(&m), Some((true, false, Some(128000))));
    }

    #[test]
    fn detect_supported_params_reasoning() {
        let m = json!({
            "id": "m",
            "supported_parameters": ["temperature", "reasoning_effort"]
        });
        assert_eq!(detect_caps(&m), Some((false, true, None)));
    }

    #[test]
    fn detect_explicit_flags_win() {
        let m = json!({
            "id": "m",
            "vision": false,
            "architecture": { "input_modalities": ["text", "image"] },
            "reasoning": true
        });
        assert_eq!(detect_caps(&m), Some((false, true, None)));
    }
}

#[cfg(test)]
mod recovery_tests {
    use super::auth;
    use halo_core::db::init_pool;
    use halo_core::store::Store;

    #[tokio::test]
    async fn recovery_answer_roundtrip_and_normalization() {
        let dir = std::env::temp_dir().join(format!("halotalk-rec-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(init_pool(&dir.join("t.db")).await.unwrap());
        let hash = auth::hash_password("password123").unwrap();
        let answer = "  SnowBall ".trim().to_lowercase();
        let ahash = auth::hash_password(&answer).unwrap();
        let user = store
            .create_user("rec_user", &hash, false, "First pet?", &ahash)
            .await
            .unwrap();
        assert_eq!(user.recovery_question, "First pet?");
        assert_eq!(user.public()["has_recovery"], true);
        let (q, h) = store.get_recovery("rec_user").await.unwrap().unwrap();
        assert_eq!(q, "First pet?");
        assert!(auth::verify_password(
            &h,
            &" SnowBall ".trim().to_lowercase()
        ));
        assert!(!auth::verify_password(&h, "SnowBall"));
        assert!(!auth::verify_password(&h, "snowbo"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod rate_tests {
    use super::{RATE_MAX, RateLimiter};
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn window_allows_up_to_max_then_blocks() {
        let lim = RateLimiter::default();
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        for _ in 0..RATE_MAX {
            assert!(lim.allow(ip));
        }
        assert!(!lim.allow(ip));
        // other IPs are unaffected
        assert!(lim.allow(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    }
}

#[cfg(test)]
mod metrics_url_tests {
    use super::metrics_url;

    #[test]
    fn strips_api_prefix_to_reach_root_metrics() {
        assert_eq!(metrics_url("http://h:8080/v1/"), "http://h:8080/metrics");
        assert_eq!(metrics_url("http://h:8080/v1"), "http://h:8080/metrics");
        assert_eq!(metrics_url("http://h:8080"), "http://h:8080/metrics");
        assert_eq!(
            metrics_url("https://host/path/"),
            "https://host/path/metrics"
        );
        // "/v10" is not an API prefix and stays put
        assert_eq!(metrics_url("http://h/v10"), "http://h/v10/metrics");
    }
}
