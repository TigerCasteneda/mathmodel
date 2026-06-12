use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, Query, State,
    },
    response::IntoResponse,
    routing::get,
    Json,
};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
    time::{Duration, Instant},
};
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;

use crate::auth::model::Claims;
use crate::error::AppError;
use crate::AppState;

// ── Types ──

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub id: String,
    pub project_id: String,
    pub user_id: String,
    pub display_name: String,
    pub content: String,
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_mime: Option<String>,
    pub content_attributes: serde_json::Value,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub echo_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replied_to: Option<RepliedTo>,
    pub created_at: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RepliedTo {
    pub user_id: String,
    pub display_name: String,
    pub content_preview: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChatWsMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub content: Option<String>,
    pub content_type: Option<String>,
    pub reply_to_id: Option<String>,
    pub file_id: Option<String>,
    pub content_attributes: Option<serde_json::Value>,
    pub echo_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ChatWsEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_users: Option<Vec<OnlineUser>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub echo_id: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct OnlineUser {
    pub user_id: String,
    pub display_name: String,
}

#[derive(Debug, Serialize)]
pub struct ChatHistoryPage {
    pub messages: Vec<ChatMessage>,
    pub has_more: bool,
    pub next_cursor: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ChatHistoryQuery {
    pub before: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ChatWsQuery {
    pub token: String,
}

// ── Global room + presence state ──

type ChatRooms = Arc<Mutex<HashMap<String, broadcast::Sender<ChatWsEvent>>>>;
type PresenceMap = Arc<Mutex<HashMap<String, HashMap<String, Instant>>>>;

fn chat_rooms() -> &'static ChatRooms {
    static ROOMS: OnceLock<ChatRooms> = OnceLock::new();
    ROOMS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn presence() -> &'static PresenceMap {
    static P: OnceLock<PresenceMap> = OnceLock::new();
    P.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

// ── Route registration ──

pub fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/projects/{project_id}/arena/chat/ws",
            get(chat_ws_handler),
        )
        .route(
            "/projects/{project_id}/arena/chat/messages",
            get(get_chat_history),
        )
}

// ── WebSocket handler ──

async fn chat_ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Query(query): Query<ChatWsQuery>,
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
    // Verify membership
    let _row: (String,) =
        sqlx::query_as("SELECT role FROM project_members WHERE project_id = ? AND user_id = ? LIMIT 1")
            .bind(&project_id)
            .bind(&user_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::Forbidden("not a member of this project".into()))?;

    Ok(ws.on_upgrade(move |socket| {
        handle_chat_socket(socket, project_id, user_id, state)
    }))
}

// ── Helpers ──

async fn room_sender(project_id: &str) -> broadcast::Sender<ChatWsEvent> {
    let mut guard = chat_rooms().lock().await;
    guard
        .entry(project_id.to_string())
        .or_insert_with(|| broadcast::channel(256).0)
        .clone()
}

async fn load_user_name(pool: &sqlx::SqlitePool, user_id: &str) -> String {
    sqlx::query_scalar::<_, String>("SELECT display_name FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .unwrap_or_default()
        .unwrap_or_else(|| user_id.to_string())
}

async fn load_file_info(
    pool: &sqlx::SqlitePool,
    file_id: &str,
    project_id: &str,
) -> Option<(String, Option<String>)> {
    sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT name, mime_type FROM files WHERE id = ? AND project_id = ? LIMIT 1",
    )
    .bind(file_id)
    .bind(project_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
}

async fn resolve_replied_to(
    pool: &sqlx::SqlitePool,
    reply_to_id: &str,
) -> Option<RepliedTo> {
    let row: (String, String, String) = sqlx::query_as(
        "SELECT m.user_id, u.display_name, m.content
         FROM arena_chat_messages m
         JOIN users u ON u.id = m.user_id
         WHERE m.id = ?",
    )
    .bind(reply_to_id)
    .fetch_optional(pool)
    .await
    .unwrap_or(None)?;

    let preview = if row.2.len() > 200 {
        format!("{}...", &row.2[..200])
    } else {
        row.2
    };

    Some(RepliedTo {
        user_id: row.0,
        display_name: row.1,
        content_preview: preview,
    })
}

async fn build_online_list(project_id: &str, pool: &sqlx::SqlitePool) -> Vec<OnlineUser> {
    let guard = presence().lock().await;
    let users = guard.get(project_id).cloned().unwrap_or_default();
    let now = Instant::now();
    let mut list: Vec<OnlineUser> = Vec::new();
    for (user_id, last_seen) in &users {
        if now.duration_since(*last_seen) < Duration::from_secs(35) {
            let display_name = load_user_name(pool, user_id).await;
            list.push(OnlineUser {
                user_id: user_id.clone(),
                display_name,
            });
        }
    }
    list
}

async fn broadcast_presence(project_id: &str, pool: &sqlx::SqlitePool) {
    let online = build_online_list(project_id, pool).await;
    let tx = room_sender(project_id).await;
    let _ = tx.send(ChatWsEvent {
        event_type: "presence".into(),
        message: None,
        online_users: Some(online),
        echo_id: None,
    });
}

// ── Socket handler ──

async fn handle_chat_socket(
    mut socket: WebSocket,
    project_id: String,
    user_id: String,
    state: AppState,
) {
    let display_name = load_user_name(&state.pool, &user_id).await;
    let tx = room_sender(&project_id).await;
    let mut rx = tx.subscribe();

    // Register presence
    {
        let mut guard = presence().lock().await;
        guard
            .entry(project_id.clone())
            .or_default()
            .insert(user_id.clone(), Instant::now());
    }
    broadcast_presence(&project_id, &state.pool).await;

    // Send join message
    let join_id = Uuid::new_v4().to_string();
    let join_msg = ChatWsEvent {
        event_type: "message".into(),
        message: Some(ChatMessage {
            id: join_id,
            project_id: project_id.clone(),
            user_id: user_id.clone(),
            display_name: String::new(),
            content: format!("{} joined the chat", display_name),
            content_type: "system".into(),
            reply_to_id: None,
            file_id: None,
            file_name: None,
            file_mime: None,
            content_attributes: serde_json::json!({}),
            status: "sent".into(),
            echo_id: None,
            replied_to: None,
            created_at: chrono::Utc::now().timestamp_millis(),
        }),
        online_users: None,
        echo_id: None,
    };
    let _ = tx.send(join_msg);

    let mut heartbeat = tokio::time::interval(Duration::from_secs(60));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                // Prune stale presence entries
                let mut guard = presence().lock().await;
                if let Some(users) = guard.get_mut(&project_id) {
                    let now = Instant::now();
                    users.retain(|_, last| now.duration_since(*last) < Duration::from_secs(35));
                    if !users.contains_key(&user_id) {
                        users.insert(user_id.clone(), Instant::now());
                    }
                }
                drop(guard);
                broadcast_presence(&project_id, &state.pool).await;
            }

            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(Message::Text(text))) => {
                        let msg: ChatWsMessage = match serde_json::from_str(&text) {
                            Ok(m) => m,
                            Err(_) => continue,
                        };

                        match msg.msg_type.as_str() {
                            "ping" => {
                                let mut guard = presence().lock().await;
                                guard
                                    .entry(project_id.clone())
                                    .or_default()
                                    .insert(user_id.clone(), Instant::now());
                                drop(guard);

                                let event = ChatWsEvent {
                                    event_type: "heartbeat_ack".into(),
                                    message: None,
                                    online_users: None,
                                    echo_id: None,
                                };
                                let json = serde_json::to_string(&event).unwrap_or_default();
                                let _ = socket.send(Message::Text(json.into())).await;
                            }

                            "message" => {
                                let content = msg.content.as_deref().unwrap_or("").trim().to_string();
                                if content.is_empty() && msg.file_id.is_none() {
                                    continue;
                                }

                                let content_type = msg.content_type.as_deref().unwrap_or("text").to_string();
                                let now = chrono::Utc::now().timestamp_millis();
                                let message_id = Uuid::new_v4().to_string();

                                // Resolve file info
                                let (file_name, file_mime) = if let Some(ref fid) = msg.file_id {
                                    load_file_info(&state.pool, fid, &project_id).await
                                        .map(|(n, m)| (Some(n), m))
                                        .unwrap_or((None, None))
                                } else {
                                    (None, None)
                                };

                                // Resolve reply-to
                                let replied_to = if let Some(ref rid) = msg.reply_to_id {
                                    resolve_replied_to(&state.pool, rid).await
                                } else {
                                    None
                                };

                                let content_attributes = msg.content_attributes.unwrap_or(serde_json::json!({}));

                                // Persist to database
                                let db_result = sqlx::query(
                                    "INSERT INTO arena_chat_messages (id, project_id, user_id, content, content_type, reply_to_id, file_id, content_attributes, status, created_at)
                                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'sent', ?)",
                                )
                                .bind(&message_id)
                                .bind(&project_id)
                                .bind(&user_id)
                                .bind(&content)
                                .bind(&content_type)
                                .bind(&msg.reply_to_id)
                                .bind(&msg.file_id)
                                .bind(&serde_json::to_string(&content_attributes).unwrap_or_default())
                                .bind(now)
                                .execute(&state.pool)
                                .await;

                                let status = if db_result.is_ok() { "sent" } else { "failed" };

                                let chat_msg = ChatMessage {
                                    id: message_id,
                                    project_id: project_id.clone(),
                                    user_id: user_id.clone(),
                                    display_name: display_name.clone(),
                                    content,
                                    content_type,
                                    reply_to_id: msg.reply_to_id,
                                    file_id: msg.file_id,
                                    file_name,
                                    file_mime,
                                    content_attributes,
                                    status: status.into(),
                                    echo_id: msg.echo_id.clone(),
                                    replied_to,
                                    created_at: now,
                                };

                                let event = ChatWsEvent {
                                    event_type: "message".into(),
                                    message: Some(chat_msg),
                                    online_users: None,
                                    echo_id: msg.echo_id,
                                };
                                let _ = tx.send(event);
                            }

                            _ => {}
                        }
                    }

                    Some(Ok(Message::Ping(data))) => {
                        let _ = socket.send(Message::Pong(data)).await;
                    }

                    Some(Ok(Message::Close(_))) | None => break,

                    _ => continue,
                }
            }

            broadcast_msg = rx.recv() => {
                match broadcast_msg {
                    Ok(event) => {
                        let json = serde_json::to_string(&event).unwrap_or_default();
                        let _ = socket.send(Message::Text(json.into())).await;
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Chat WS lagged by {n} messages for user {user_id}");
                        // Lagged: client will catch up via re-fetch on reconnect
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    // Disconnect: remove presence, broadcast
    {
        let mut guard = presence().lock().await;
        if let Some(users) = guard.get_mut(&project_id) {
            users.remove(&user_id);
        }
    }
    broadcast_presence(&project_id, &state.pool).await;

    let leave_id = Uuid::new_v4().to_string();
    let leave_msg = ChatWsEvent {
        event_type: "message".into(),
        message: Some(ChatMessage {
            id: leave_id,
            project_id: project_id.clone(),
            user_id: user_id.clone(),
            display_name: String::new(),
            content: format!("{} left the chat", display_name),
            content_type: "system".into(),
            reply_to_id: None,
            file_id: None,
            file_name: None,
            file_mime: None,
            content_attributes: serde_json::json!({}),
            status: "sent".into(),
            echo_id: None,
            replied_to: None,
            created_at: chrono::Utc::now().timestamp_millis(),
        }),
        online_users: None,
        echo_id: None,
    };
    let _ = tx.send(leave_msg);
}

// ── REST: message history ──

async fn get_chat_history(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Query(query): Query<ChatHistoryQuery>,
) -> Result<Json<ChatHistoryPage>, AppError> {
    let limit = query.limit.unwrap_or(50).clamp(1, 100);

    #[derive(sqlx::FromRow)]
    struct DbMessage {
        id: String,
        project_id: String,
        user_id: String,
        display_name: String,
        content: String,
        content_type: String,
        reply_to_id: Option<String>,
        file_id: Option<String>,
        content_attributes: String,
        status: String,
        created_at: i64,
    }

    let rows: Vec<DbMessage> = if let Some(before) = query.before {
        sqlx::query_as(
            "SELECT m.id, m.project_id, m.user_id, u.display_name, m.content,
                    m.content_type, m.reply_to_id, m.file_id, m.content_attributes,
                    m.status, m.created_at
             FROM arena_chat_messages m
             JOIN users u ON u.id = m.user_id
             WHERE m.project_id = ? AND m.created_at < ?
             ORDER BY m.created_at DESC
             LIMIT ?",
        )
        .bind(&project_id)
        .bind(before)
        .bind(limit + 1) // fetch one extra to determine has_more
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT m.id, m.project_id, m.user_id, u.display_name, m.content,
                    m.content_type, m.reply_to_id, m.file_id, m.content_attributes,
                    m.status, m.created_at
             FROM arena_chat_messages m
             JOIN users u ON u.id = m.user_id
             WHERE m.project_id = ?
             ORDER BY m.created_at DESC
             LIMIT ?",
        )
        .bind(&project_id)
        .bind(limit + 1)
        .fetch_all(&state.pool)
        .await?
    };

    let has_more = rows.len() > limit as usize;
    let rows = if has_more {
        &rows[..limit as usize]
    } else {
        &rows[..]
    };

    let next_cursor = rows.last().map(|r| r.created_at);

    let mut messages: Vec<ChatMessage> = Vec::with_capacity(rows.len());
    for row in rows.iter().rev() {
        // Only resolve reply-to for the history endpoint
        let replied_to = if let Some(ref rid) = row.reply_to_id {
            resolve_replied_to(&state.pool, rid).await
        } else {
            None
        };

        let content_attributes: serde_json::Value =
            serde_json::from_str(&row.content_attributes).unwrap_or(serde_json::json!({}));

        messages.push(ChatMessage {
            id: row.id.clone(),
            project_id: row.project_id.clone(),
            user_id: row.user_id.clone(),
            display_name: row.display_name.clone(),
            content: row.content.clone(),
            content_type: row.content_type.clone(),
            reply_to_id: row.reply_to_id.clone(),
            file_id: row.file_id.clone(),
            file_name: None,
            file_mime: None,
            content_attributes,
            status: row.status.clone(),
            echo_id: None,
            replied_to,
            created_at: row.created_at,
        });
    }

    Ok(Json(ChatHistoryPage {
        messages,
        has_more,
        next_cursor,
    }))
}
