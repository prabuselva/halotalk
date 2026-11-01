mod auth;
mod hub;
mod routes;
mod tools;

use std::path::PathBuf;

use anyhow::Context;
use halo_core::{crypto::Cipher, db::init_pool, store::Store};
use halo_providers::AnyClient;
use hub::Hub;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub store: Store,
    pub provider: AnyClient,
    pub uploads: PathBuf,
    pub web_dist: PathBuf,
    pub limiter: routes::RateLimiter,
    pub hub: Arc<Hub>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "halotalk=info,tower_http=debug".into()),
        )
        .init();

    let db_path =
        PathBuf::from(std::env::var("HALO_DB_PATH").unwrap_or_else(|_| "data/halotalk.db".into()));
    let uploads =
        PathBuf::from(std::env::var("HALO_UPLOAD_DIR").unwrap_or_else(|_| "data/uploads".into()));
    std::fs::create_dir_all(&uploads).context("create upload dir")?;

    let cipher = Cipher::load_or_create(db_path.parent().unwrap_or(std::path::Path::new(".")))
        .context("load encryption key")?;
    let pool = init_pool(&db_path).await.context("open sqlite")?;
    let store = Store::with_cipher(pool, cipher);
    store
        .migrate_plaintext_keys()
        .await
        .context("migrate keys")?;
    store.prune_sessions().await.context("prune sessions")?;
    let state = AppState {
        store,
        provider: AnyClient::new(),
        uploads,
        web_dist: PathBuf::from(
            std::env::var("HALO_WEB_DIST").unwrap_or_else(|_| "web/dist".into()),
        ),
        limiter: routes::RateLimiter::default(),
        hub: Arc::new(Hub::new()),
    };

    let host = std::env::var("HALO_HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("HALO_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8420);

    let listener = tokio::net::TcpListener::bind((host.as_str(), port))
        .await
        .context("bind listener")?;
    tracing::info!("halotalk listening on http://{host}:{port}");
    axum::serve(
        listener,
        routes::router(state).into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
