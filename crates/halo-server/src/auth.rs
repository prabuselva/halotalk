//! Cookie-session auth: argon2 password hashing, hashed session tokens, and
//! the `require_auth` middleware that populates `Extension<AuthUser>`.

use argon2::{
    Argon2, PasswordHash, PasswordHasher as _, PasswordVerifier as _, password_hash::SaltString,
};
use axum::{
    Json,
    body::Body,
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use halo_core::models::User;
use serde_json::json;
use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::AppState;

pub const SESSION_COOKIE: &str = "halo_session";
pub const SESSION_DAYS: i64 = 30;

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    // Salt drawn from two uuids (hex ⊂ base64 alphabet); avoids pulling in an
    // OsRng-compatible rand_core version.
    let salt_str = format!("{}{}", Uuid::now_v7().simple(), Uuid::new_v4().simple());
    let salt = SaltString::from_b64(&salt_str)
        .map_err(|_| anyhow::anyhow!("could not build password salt"))?;
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("password hashing failed: {e:?}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

/// Random opaque session token (sent to the browser; only its hash is stored).
pub fn new_session_token() -> String {
    format!("{}{}", Uuid::now_v7().simple(), Uuid::new_v4().simple())
}

pub fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
    }
}

pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .map(str::trim)
        .find_map(|pair| {
            let (k, val) = pair.split_once('=')?;
            (k.trim() == name && !val.is_empty()).then(|| val.to_string())
        })
}

/// `Set-Cookie` value for a fresh session (lax, http-only, 30 days).
pub fn session_cookie(token: &str) -> String {
    format!("{SESSION_COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000")
}

pub fn cleared_session_cookie() -> String {
    format!("{SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

#[derive(Clone)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub is_admin: bool,
}

impl From<User> for AuthUser {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            is_admin: u.is_admin,
        }
    }
}

/// Handler extractor for the session user injected by `require_auth`.
pub struct Authed(pub AuthUser);

impl FromRequestParts<AppState> for Authed {
    type Rejection = Response<Body>;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthUser>()
            .cloned()
            .map(Authed)
            .ok_or_else(unauthorized)
    }
}

fn unauthorized() -> Response<Body> {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "session missing or expired" })),
    )
        .into_response()
}

/// Gate for all `/api/*` routes except auth itself and the health probe.
/// Sessions are resolved (and injected) for every path so that auth handlers
/// like `/api/auth/me` can see the current user.
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response<Body> {
    let path = req.uri().path();
    let protected =
        path.starts_with("/api/") && !path.starts_with("/api/auth/") && path != "/api/health";
    let mut resolved: Option<User> = None;
    if let Some(token) = cookie_value(req.headers(), SESSION_COOKIE)
        && let Ok(Some(user_id)) = state.store.validate_session(&token_hash(&token)).await
    {
        resolved = state.store.get_user(&user_id).await.ok().flatten();
    }
    match resolved {
        Some(user) => {
            req.extensions_mut().insert(AuthUser::from(user));
        }
        None if protected => return unauthorized(),
        None => {}
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_roundtrip() {
        let hash = hash_password("hunter2!").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password(&hash, "hunter2!"));
        assert!(!verify_password(&hash, "wrong"));
        assert!(!verify_password("not-a-hash", "hunter2!"));
    }

    #[test]
    fn cookie_parsing() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "theme=dark; halo_session=abc.def; x=1".parse().unwrap(),
        );
        assert_eq!(
            cookie_value(&headers, SESSION_COOKIE).as_deref(),
            Some("abc.def")
        );
        assert_eq!(cookie_value(&headers, "theme").as_deref(), Some("dark"));
        assert_eq!(cookie_value(&headers, "missing"), None);
    }

    #[test]
    fn token_hash_is_stable_hex() {
        assert_eq!(token_hash("a").len(), 64);
        assert_eq!(token_hash("a"), token_hash("a"));
        assert_ne!(token_hash("a"), token_hash("b"));
    }
}
