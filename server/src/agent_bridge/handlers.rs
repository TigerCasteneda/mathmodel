use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct AgentQuery {
    pub token: String,
    pub project_id: String,
    pub role: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TabbitPayload {
    pub url: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub notes: Option<String>,
    #[serde(default)]
    pub context_pages: Vec<TabbitContextPage>,
}

#[derive(Debug, Deserialize)]
struct TabbitContextPage {
    pub url: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,
    #[serde(default)]
    pub ordinal: Option<i64>,
}

pub async fn agent_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<AgentQuery>,
) -> Result<impl IntoResponse, AppError> {
    use jsonwebtoken::{decode, DecodingKey, Validation};

    let claims = decode::<crate::auth::model::Claims>(
        &query.token,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("invalid agent token".into()))?;

    if claims.claims.token_type != "access" {
        return Err(AppError::Unauthorized("invalid agent token".into()));
    }

    let user_id = claims.claims.sub;
    let project_id = query.project_id;

    // Verify project membership
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_members WHERE project_id = ? AND user_id = ?)",
    )
    .bind(&project_id)
    .bind(&user_id)
    .fetch_one(&state.pool)
    .await?;

    if exists == 0 {
        return Err(AppError::Forbidden("not a project member".into()));
    }

    let pool = state.pool.clone();
    let bridge = state.agent_registry.get_or_create(&project_id).await;

    if query.role.as_deref() == Some("frontend") {
        Ok(ws.on_upgrade(move |socket| handle_frontend(socket, project_id, user_id, bridge)))
    } else {
        Ok(ws.on_upgrade(move |socket| handle_agent(socket, project_id, user_id, bridge, pool)))
    }
}

async fn handle_agent(
    socket: WebSocket,
    project_id: String,
    user_id: String,
    bridge: std::sync::Arc<crate::agent_bridge::registry::ProjectAgentBridge>,
    pool: sqlx::SqlitePool,
) {
    tracing::info!("Agent connected: user={} project={}", user_id, project_id);

    let connection_id = Uuid::new_v4().to_string();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<Value>();
    bridge.set_agent(connection_id.clone(), outbound_tx).await;

    let (mut ws_tx, mut ws_rx) = socket.split();

    loop {
        tokio::select! {
            Some(server_msg) = outbound_rx.recv() => {
                match serde_json::to_string(&server_msg) {
                    Ok(text) => {
                        if ws_tx.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => tracing::warn!("failed to encode agent outbound json: {}", err),
                }
            }
            agent_msg = ws_rx.next() => {
                match agent_msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<Value>(&text) {
                            Ok(value) => {
                                let msg_type = value["type"].as_str().unwrap_or("unknown");
                                match msg_type {
                                    "ready" => {
                                        tracing::info!("Agent ready: user={}", user_id);
                                        bridge.broadcast_to_frontends(serde_json::json!({
                                            "type": "agent_status",
                                            "status": "ready"
                                        }));
                                    }
                                    "terminal_output"
                                    | "file_tree"
                                    | "file_content"
                                    | "file_change"
                                    | "work_dir" => {
                                        bridge.broadcast_to_frontends(value);
                                    }
                                    "tabbit_data" => {
                                        if let Err(err) =
                                            handle_tabbit_data(&pool, &project_id, &user_id, &value).await
                                        {
                                            tracing::warn!("failed to store tabbit data: {:?}", err);
                                        }
                                    }
                                    "error" => {
                                        tracing::error!("agent error: {}", value["message"]);
                                        bridge.broadcast_to_frontends(value);
                                    }
                                    _ => {
                                        tracing::debug!("agent unknown msg: {}", msg_type);
                                    }
                                }
                            }
                            Err(err) => {
                                tracing::warn!("agent sent invalid json: {}", err);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
        }
    }

    bridge.clear_agent(&connection_id).await;
    tracing::info!("Agent disconnected: user={}", user_id);
}

async fn handle_frontend(
    socket: WebSocket,
    project_id: String,
    user_id: String,
    bridge: std::sync::Arc<crate::agent_bridge::registry::ProjectAgentBridge>,
) {
    tracing::info!(
        "Agent frontend connected: user={} project={}",
        user_id,
        project_id
    );

    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut frontend_rx = bridge.subscribe_frontend();

    let status = if bridge.has_agent().await {
        "ready"
    } else {
        "disconnected"
    };
    let _ = ws_tx
        .send(Message::Text(
            serde_json::json!({
                "type": "agent_status",
                "status": status
            })
            .to_string()
            .into(),
        ))
        .await;

    loop {
        tokio::select! {
            frontend_msg = ws_rx.next() => {
                match frontend_msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<Value>(&text) {
                            Ok(value) => {
                                let msg_type = value["type"].as_str().unwrap_or("unknown");
                                match msg_type {
                                    "terminal_input"
                                    | "terminal_resize"
                                    | "claude_command"
                                    | "open_file"
                                    | "list_files"
                                    | "change_work_dir"
                                    | "new_file"
                                    | "new_folder" => {
                                        if bridge.send_to_agent(value).await.is_err() {
                                            let _ = ws_tx.send(Message::Text(
                                                serde_json::json!({
                                                    "type": "error",
                                                    "message": "local agent is not connected"
                                                }).to_string().into()
                                            )).await;
                                        }
                                    }
                                    _ => tracing::debug!("frontend unknown msg: {}", msg_type),
                                }
                            }
                            Err(err) => tracing::warn!("frontend sent invalid json: {}", err),
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            Ok(agent_event) = frontend_rx.recv() => {
                match serde_json::to_string(&agent_event) {
                    Ok(text) => {
                        if ws_tx.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => tracing::warn!("failed to encode frontend json: {}", err),
                }
            }
        }
    }

    tracing::info!("Agent frontend disconnected: user={}", user_id);
}

async fn handle_tabbit_data(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    user_id: &str,
    value: &Value,
) -> Result<(), AppError> {
    let Some(payload_value) = value.get("payload") else {
        tracing::warn!("tabbit_data missing payload");
        return Ok(());
    };

    let payload = match serde_json::from_value::<TabbitPayload>(payload_value.clone()) {
        Ok(payload) => payload,
        Err(err) => {
            tracing::warn!("invalid tabbit_data payload: {}", err);
            return Ok(());
        }
    };

    if payload.url.trim().is_empty() {
        tracing::warn!("invalid tabbit_data payload: url is required");
        return Ok(());
    }

    let item_id = insert_research_item(pool, project_id, user_id, payload, value).await?;
    tracing::info!(
        "stored tabbit research item: project={} user={} item={}",
        project_id,
        user_id,
        item_id
    );

    Ok(())
}

async fn insert_research_item(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    user_id: &str,
    payload: TabbitPayload,
    raw_json: &Value,
) -> Result<String, AppError> {
    let now = chrono::Utc::now().timestamp();
    let item_id = Uuid::new_v4().to_string();
    let raw_json = serde_json::to_string(raw_json).unwrap_or_else(|_| "{}".to_string());
    let mut tx = pool.begin().await?;

    sqlx::query(
        "INSERT INTO research_items
         (id, project_id, created_by, source, url, title, summary, notes, raw_json, created_at)
         VALUES (?, ?, ?, 'tabbit', ?, ?, ?, ?, ?, ?)",
    )
    .bind(&item_id)
    .bind(project_id)
    .bind(user_id)
    .bind(payload.url.trim())
    .bind(payload.title)
    .bind(payload.summary)
    .bind(payload.notes)
    .bind(raw_json)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    for (index, page) in payload.context_pages.into_iter().enumerate() {
        sqlx::query(
            "INSERT INTO research_context_pages
             (id, item_id, url, title, content, ordinal)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&item_id)
        .bind(page.url)
        .bind(page.title)
        .bind(page.content)
        .bind(page.ordinal.unwrap_or(index as i64))
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(item_id)
}
