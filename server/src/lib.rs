pub mod agent_bridge;
pub mod ai;
pub mod auth;
pub mod compute;
pub mod config;
pub mod db;
pub mod error;
pub mod file;
pub mod history;
pub mod morphic;
pub mod project;
pub mod research;
pub mod sync;

use axum::routing::get;
use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

pub use config::Config;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub config: Config,
    pub room_registry: Arc<sync::room::RoomRegistry>,
    pub agent_registry: Arc<agent_bridge::registry::AgentRegistry>,
}

/// Build the full Axum router for the server.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .nest("/auth", auth::handlers::routes())
        .nest("/projects", project::handlers::routes())
        .merge(file::handlers::routes())
        .route("/sync", get(sync::handlers::ws_handler))
        .route("/agent", get(agent_bridge::handlers::agent_ws_handler))
        .merge(ai::handlers::routes())
        .merge(compute::handlers::routes())
        .merge(history::handlers::routes())
        .merge(research::handlers::routes())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Bind and serve on a random port. Returns the port number.
/// Spawns the server in a background tokio task.
pub async fn serve(state: AppState) -> anyhow::Result<u16> {
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("Embedded server crashed: {e}");
        }
    });
    Ok(port)
}
