mod agent_bridge;
mod ai;
mod auth;
mod compute;
mod config;
mod db;
mod error;
mod file;
mod history;
mod project;
mod sync;

use axum::routing::get;
use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let cfg = config::Config::from_env();
    let pool = db::init_pool(&cfg.database_url).await;

    let app_state = AppState {
        pool,
        config: cfg.clone(),
        room_registry: Arc::new(sync::room::RoomRegistry::new()),
        agent_registry: Arc::new(agent_bridge::registry::AgentRegistry::new()),
    };

    let app = Router::new()
        .nest("/auth", auth::handlers::routes())
        .nest("/projects", project::handlers::routes())
        .merge(file::handlers::routes())
        .route("/sync", get(sync::handlers::ws_handler))
        .route("/agent", get(agent_bridge::handlers::agent_ws_handler))
        .merge(ai::handlers::routes())
        .merge(compute::handlers::routes())
        .merge(history::handlers::routes())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cfg.port)).await?;
    tracing::info!("Server running on port {}", cfg.port);
    axum::serve(listener, app).await?;

    Ok(())
}

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub config: config::Config,
    pub room_registry: Arc<sync::room::RoomRegistry>,
    pub agent_registry: Arc<agent_bridge::registry::AgentRegistry>,
}
