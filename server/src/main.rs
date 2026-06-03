use modeler_server::{self, AppState};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let cfg = modeler_server::Config::from_env();
    let pool = modeler_server::db::init_pool(&cfg.database_url).await;

    let state = AppState {
        pool,
        config: cfg.clone(),
        room_registry: Arc::new(modeler_server::sync::room::RoomRegistry::new()),
        agent_registry: Arc::new(modeler_server::agent_bridge::registry::AgentRegistry::new()),
    };

    let app = modeler_server::build_router(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cfg.port)).await?;
    tracing::info!("Server running on port {}", cfg.port);
    axum::serve(listener, app).await?;

    Ok(())
}
