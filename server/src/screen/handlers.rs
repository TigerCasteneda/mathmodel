use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};
use tokio::sync::{broadcast, Mutex};

use crate::auth::model::Claims;
use crate::error::AppError;
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ScreenQuery {
    pub token: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScreenSignal {
    #[serde(rename = "type")]
    pub signal_type: String,
    #[serde(default)]
    pub sender_id: Option<String>,
    #[serde(default)]
    pub target_user_id: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
}

type ScreenRooms = Arc<Mutex<HashMap<String, broadcast::Sender<ScreenSignal>>>>;

fn rooms() -> &'static ScreenRooms {
    static ROOMS: OnceLock<ScreenRooms> = OnceLock::new();
    ROOMS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/projects/{project_id}/screen/ws",
        get(screen_ws_handler),
    )
}

async fn member_capabilities(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    user_id: &str,
) -> Result<Vec<String>, AppError> {
    let row: (String, Option<String>) = sqlx::query_as(
        "SELECT role, capabilities FROM project_members WHERE project_id = ? AND user_id = ? LIMIT 1",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Forbidden("not a member of this project".into()))?;

    let (role, capabilities) = row;
    match capabilities {
        Some(raw) if !raw.trim().is_empty() => serde_json::from_str::<Vec<String>>(&raw)
            .map_err(|e| AppError::Internal(format!("capabilities decode: {e}"))),
        _ if role == "owner" => Ok(vec![
            "screen.share".into(),
            "screen.view".into(),
        ]),
        _ if role == "editor" => Ok(vec!["screen.share".into()]),
        _ => Ok(Vec::new()),
    }
}

async fn screen_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Query(query): Query<ScreenQuery>,
) -> Result<impl IntoResponse, AppError> {
    let claims = decode::<Claims>(
        &query.token,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|_| AppError::Unauthorized("invalid token".into()))?;

    if claims.claims.token_type != "access" {
        return Err(AppError::Unauthorized("invalid token".into()));
    }

    let user_id = claims.claims.sub;
    let capabilities = member_capabilities(&state.pool, &project_id, &user_id).await?;
    if !capabilities.iter().any(|cap| cap == "screen.share" || cap == "screen.view") {
        return Err(AppError::Forbidden("screen permission required".into()));
    }

    Ok(ws.on_upgrade(move |socket| {
        handle_socket(socket, project_id, user_id, capabilities)
    }))
}

fn can_send(signal_type: &str, capabilities: &[String]) -> bool {
    match signal_type {
        "share_offer" | "share_started" | "share_stopped" => {
            capabilities.iter().any(|cap| cap == "screen.share")
        }
        "share_answer" => capabilities.iter().any(|cap| cap == "screen.view"),
        "ice_candidate" => capabilities
            .iter()
            .any(|cap| cap == "screen.share" || cap == "screen.view"),
        _ => false,
    }
}

async fn room_sender(project_id: &str) -> broadcast::Sender<ScreenSignal> {
    let mut guard = rooms().lock().await;
    guard
        .entry(project_id.to_string())
        .or_insert_with(|| broadcast::channel(256).0)
        .clone()
}

async fn handle_socket(
    mut socket: WebSocket,
    project_id: String,
    user_id: String,
    capabilities: Vec<String>,
) {
    let tx = room_sender(&project_id).await;
    let mut rx = tx.subscribe();

    loop {
        tokio::select! {
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(Message::Text(text))) => {
                        let Ok(mut signal) = serde_json::from_str::<ScreenSignal>(&text) else {
                            continue;
                        };
                        if !can_send(&signal.signal_type, &capabilities) {
                            continue;
                        }
                        signal.sender_id = Some(user_id.clone());
                        let _ = tx.send(signal);
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => continue,
                }
            }
            Ok(signal) = rx.recv() => {
                if signal.sender_id.as_deref() == Some(&user_id) {
                    continue;
                }
                if signal
                    .target_user_id
                    .as_deref()
                    .is_some_and(|target| target != user_id)
                {
                    continue;
                }
                if let Ok(json) = serde_json::to_string(&signal) {
                    let _ = socket.send(Message::Text(json.into())).await;
                }
            }
        }
    }
}

#[allow(dead_code)]
async fn _health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}
