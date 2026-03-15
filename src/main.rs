mod config;
mod pool;
mod qemu;
mod ws;

use std::sync::Arc;
use axum::{
    Router,
    extract::{State, WebSocketUpgrade},
    response::Json,
    routing::get,
};
use tower_http::cors::{CorsLayer, Any};
use tracing::{info, Level};

use config::Config;
use pool::Pool;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let config = Config::from_env();
    let port = config.port;

    info!(?config, "Starting MerlionOS Playground Server");

    let pool = Pool::new(config);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/health", get(health))
        .route("/status", get(status_handler))
        .layer(cors)
        .with_state(pool);

    let addr = format!("0.0.0.0:{port}");
    info!(%addr, "Listening");

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(pool): State<Arc<Pool>>,
) -> axum::response::Response {
    ws.on_upgrade(move |socket| ws::handle_session(socket, pool))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"status": "ok"}))
}

async fn status_handler(
    State(pool): State<Arc<Pool>>,
) -> Json<pool::PoolStatus> {
    Json(pool.status(None).await)
}
