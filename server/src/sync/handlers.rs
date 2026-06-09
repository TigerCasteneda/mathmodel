use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::room::RoomRegistry;
use crate::auth::model::Claims;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct SyncQuery {
    pub file_id: String,
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SyncMessage {
    #[serde(rename = "sync_update")]
    SyncUpdate { update: Vec<u8> },
    #[serde(rename = "sync_full")]
    SyncFull { state: Vec<u8> },
    #[serde(rename = "awareness")]
    Awareness { state: Vec<u8> },
}

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(query): Query<SyncQuery>,
) -> Result<impl IntoResponse, AppError> {
    let file_id = query.file_id;
    let claims = decode::<Claims>(
        &query.token,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("invalid token".into()))?;

    if claims.claims.token_type != "access" {
        return Err(AppError::Unauthorized("invalid token".into()));
    }

    let can_write = ensure_sync_access(&state.pool, &file_id, &claims.claims.sub).await?;

    let pool = state.pool.clone();
    let registry = state.room_registry.clone();

    Ok(ws.on_upgrade(move |socket| handle_socket(socket, file_id, pool, registry, can_write)))
}

async fn ensure_sync_access(
    pool: &sqlx::SqlitePool,
    file_id: &str,
    user_id: &str,
) -> Result<bool, AppError> {
    let row: Option<(String, Option<String>)> = sqlx::query_as(
        "SELECT pm.role, pm.capabilities
            FROM files f
            JOIN project_members pm ON pm.project_id = f.project_id
            WHERE f.id = ?
              AND f.type = 'file'
              AND f.storage_path IS NULL
              AND pm.user_id = ?
            LIMIT 1",
    )
    .bind(file_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    let (role, raw_capabilities) = row
        .ok_or_else(|| AppError::NotFound("sync file not found".into()))?;
    let capabilities = match raw_capabilities {
        Some(raw) if !raw.trim().is_empty() => serde_json::from_str::<Vec<String>>(&raw)
            .map_err(|e| AppError::Internal(format!("capabilities decode: {e}")))?,
        _ if role == "owner" => vec![
            "files.read".into(),
            "files.write".into(),
        ],
        _ if role == "editor" => vec![
            "files.read".into(),
            "files.write".into(),
        ],
        _ => vec!["files.read".into()],
    };

    sqlx::query(
        "INSERT OR IGNORE INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)",
    )
    .bind(file_id)
    .bind(Vec::<u8>::new())
    .bind(chrono::Utc::now().timestamp())
    .execute(pool)
    .await?;

    Ok(capabilities.iter().any(|cap| cap == "files.write"))
}

async fn handle_socket(
    mut socket: WebSocket,
    file_id: String,
    pool: sqlx::SqlitePool,
    registry: Arc<RoomRegistry>,
    can_write: bool,
) {
    let room = registry.get_or_create(&file_id, &pool).await;

    // Send current full state to new client
    {
        let r = room.read().await;
        let state = r.encode_state();
        let msg = SyncMessage::SyncFull { state };
        if let Ok(json) = serde_json::to_string(&msg) {
            let _ = socket.send(Message::Text(json.into())).await;
        }
    }

    // Subscribe to updates from other clients
    let mut update_rx = room.read().await.update_tx.subscribe();
    // Subscribe to awareness updates from other clients
    let mut awareness_rx = room.read().await.awareness_tx.subscribe();

    loop {
        tokio::select! {
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<SyncMessage>(&text) {
                            Ok(SyncMessage::SyncUpdate { update }) => {
                                if !can_write {
                                    continue;
                                }
                                let mut r = room.write().await;
                                if r.apply_update(&update).is_ok() {
                                    let _ = r.update_tx.send(update);
                                }
                            }
                            // Fan-out incoming awareness to all other clients
                            Ok(SyncMessage::Awareness { state }) => {
                                let r = room.read().await;
                                let _ = r.awareness_tx.send(state);
                            }
                            _ => continue,
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => continue,
                }
            }
            Ok(update) = update_rx.recv() => {
                let msg = SyncMessage::SyncUpdate { update };
                if let Ok(json) = serde_json::to_string(&msg) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
            // Fan-out awareness to this client
            Ok(awareness_update) = awareness_rx.recv() => {
                let msg = SyncMessage::Awareness { state: awareness_update };
                if let Ok(json) = serde_json::to_string(&msg) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
        }
    }

    // Persist CRDT state on disconnect
    registry.release(&file_id, &pool).await;
}
