use axum::{extract::State, http::StatusCode, routing::post, Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, net::SocketAddr};
use tokio::sync::mpsc;

use crate::ws_client::AgentMessage;

#[derive(Clone)]
pub struct TabbitState {
    pub outbound_tx: mpsc::UnboundedSender<AgentMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabbitContextPage {
    pub url: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,
    #[serde(default)]
    pub ordinal: Option<i64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabbitPayload {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_pages: Vec<TabbitContextPage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabbitRequest {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_pages: Vec<TabbitContextPage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl From<TabbitRequest> for TabbitPayload {
    fn from(request: TabbitRequest) -> Self {
        Self {
            url: request.url,
            title: request.title,
            summary: request.summary,
            notes: request.notes,
            context_pages: request.context_pages,
            raw: request.raw,
        }
    }
}

pub async fn run_server(
    port: u16,
    outbound_tx: mpsc::UnboundedSender<AgentMessage>,
) -> anyhow::Result<tokio::task::JoinHandle<()>> {
    let app = Router::new()
        .route("/tabbit", post(post_tabbit))
        .with_state(TabbitState { outbound_tx });

    let addr: SocketAddr = format!("127.0.0.1:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    Ok(tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!("tabbit server error: {err:#}");
        }
    }))
}

async fn post_tabbit(
    State(state): State<TabbitState>,
    Json(request): Json<TabbitRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if request.url.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "url is required" })),
        ));
    }

    let raw_json = serde_json::to_value(&request).unwrap_or(Value::Null);
    let payload = TabbitPayload::from(request);

    state
        .outbound_tx
        .send(AgentMessage::TabbitData { payload, raw_json })
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "agent websocket is not connected" })),
            )
        })?;

    Ok(Json(json!({ "status": "ok" })))
}
