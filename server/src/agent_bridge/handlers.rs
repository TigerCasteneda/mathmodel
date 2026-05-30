use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct AgentQuery {
    pub token: String,
    pub project_id: String,
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
    let registry = state.room_registry.clone();

    Ok(ws.on_upgrade(move |socket| handle_agent(socket, project_id, user_id, registry, pool)))
}

async fn handle_agent(
    mut socket: WebSocket,
    project_id: String,
    user_id: String,
    _registry: Arc<crate::sync::room::RoomRegistry>,
    pool: sqlx::SqlitePool,
) {
    tracing::info!("Agent connected: user={} project={}", user_id, project_id);

    // Message loop
    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            Message::Text(text) => {
                match serde_json::from_str::<Value>(&text) {
                    Ok(value) => {
                        let msg_type = value["type"].as_str().unwrap_or("unknown");
                        match msg_type {
                            "ready" => {
                                tracing::info!("Agent ready: user={}", user_id);
                            }
                            "terminal_output" => {
                                let data = value["data"].as_str().unwrap_or("");
                                tracing::debug!(
                                    "agent terminal: {}",
                                    data.chars().take(50).collect::<String>()
                                );
                                // Phase 6b: broadcast to frontend
                            }
                            "file_change" => {
                                let path = value["path"].as_str().unwrap_or("");
                                let content = value["content"].as_str().unwrap_or("");
                                tracing::info!(
                                    "agent file change: {} ({} bytes)",
                                    path,
                                    content.len()
                                );
                                // Phase 6b: apply to CRDT sync room
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
            Message::Close(_) => break,
            _ => {}
        }
    }

    tracing::info!("Agent disconnected: user={}", user_id);
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
