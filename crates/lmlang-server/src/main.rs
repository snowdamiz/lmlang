//! Binary entrypoint for the lmlang HTTP server.
//!
//! Reads configuration from environment variables:
//! - `LMLANG_DB_PATH`: SQLite database file path (default: "lmlang.db")
//! - `LMLANG_PORT`: Server listen port (default: "3000")

use lmlang_server::router::build_router;
use lmlang_server::state::AppState;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let db_path = std::env::var("LMLANG_DB_PATH")
        .unwrap_or_else(|_| "lmlang.db".to_string());
    let port = std::env::var("LMLANG_PORT")
        .unwrap_or_else(|_| "3000".to_string());

    let state = AppState::new(&db_path)
        .expect("Failed to initialize application state");

    let app = build_router(state);

    let addr = format!("0.0.0.0:{}", port);
    tracing::info!("lmlang server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
