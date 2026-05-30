use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use chrono::Utc;
use uuid::Uuid;

use crate::ai::adaptor::anthropic::AnthropicAdaptor;
use crate::ai::adaptor::openai::OpenAIAdaptor;
use crate::ai::adaptor::tavily::TavilyAdaptor;
use crate::ai::adaptor::Adaptor;
use crate::ai::model::channel_type;
use crate::ai::model::*;
use crate::ai::{channel, quota};
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/ai/v1/chat/completions", post(chat_completions))
        .route("/ai/v1/models", get(list_models))
        .route("/ai/search", post(search))
        .route(
            "/ai/admin/channels",
            get(list_channels).post(create_channel),
        )
        .route(
            "/ai/admin/channels/{id}",
            put(update_channel).delete(delete_channel),
        )
}

fn get_adaptor(ctype: i32, channel_name: &str) -> Box<dyn Adaptor> {
    match ctype {
        channel_type::ANTHROPIC => Box::new(AnthropicAdaptor),
        channel_type::TAVILY => Box::new(TavilyAdaptor),
        _ => Box::new(OpenAIAdaptor {
            custom_provider: Some(channel_name.to_string()),
        }),
    }
}

async fn chat_completions(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Json<ChatCompletionResponse>, AppError> {
    let start = std::time::Instant::now();

    // 1. Reject streaming for now
    if req.stream.unwrap_or(false) {
        return Err(AppError::BadRequest(
            "streaming is not supported yet".into(),
        ));
    }

    // 2. Check project quota
    if !quota::check_project_quota(&state.pool, &req.project_id).await? {
        return Err(AppError::Forbidden("project quota exceeded".into()));
    }

    let model = req.model.clone();

    // 3. Select best channel
    let ch = channel::get_best_channel(&state.pool, &model).await?;

    // 4. Get adaptor
    let adaptor = get_adaptor(ch.channel_type, &ch.name);

    // 5. Build and send HTTP request
    let url = adaptor.build_url(&ch.base_url, &model);
    let headers = adaptor.build_headers(&ch.api_key);
    let body = adaptor.convert_request(&req, &model)?;

    let client = reqwest::Client::new();
    let mut http_req = client.post(&url);
    for (k, v) in &headers {
        http_req = http_req.header(k.as_str(), v.as_str());
    }

    let resp = http_req
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Internal(format!("upstream error: {}", e)))?;

    let resp_status = resp.status();
    let resp_body = resp
        .text()
        .await
        .map_err(|e| AppError::Internal(format!("read error: {}", e)))?;

    let duration_ms = start.elapsed().as_millis() as i32;

    if !resp_status.is_success() {
        log_usage(
            &state.pool,
            &auth.user_id,
            &req.project_id,
            Some(&ch.id),
            &model,
            0,
            0,
            0,
            "error",
            Some(&resp_body),
            duration_ms,
        )
        .await;
        return Err(AppError::Internal(format!(
            "upstream {} returned {}: {}",
            ch.name,
            resp_status.as_u16(),
            truncate(&resp_body, 200)
        )));
    }

    // 6. Parse response
    let parse_result = adaptor.parse_response(&resp_body).await;
    let (response_json, usage) = match parse_result {
        Ok(r) => r,
        Err(e) => {
            log_usage(
                &state.pool,
                &auth.user_id,
                &req.project_id,
                Some(&ch.id),
                &model,
                0,
                0,
                0,
                "parse_error",
                Some(&resp_body),
                duration_ms,
            )
            .await;
            return Err(e);
        }
    };

    let total_tokens = usage.prompt_tokens + usage.completion_tokens;

    // 7. Log usage
    log_usage(
        &state.pool,
        &auth.user_id,
        &req.project_id,
        Some(&ch.id),
        &model,
        usage.prompt_tokens,
        usage.completion_tokens,
        total_tokens,
        "success",
        None,
        duration_ms,
    )
    .await;

    // 8. Deduct quota
    quota::deduct_project_quota(&state.pool, &req.project_id, total_tokens).await?;

    // 9. Return OpenAI-compatible response
    let result: ChatCompletionResponse = serde_json::from_value(response_json)
        .map_err(|e| AppError::Internal(format!("response mapping error: {}", e)))?;

    Ok(Json(result))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

async fn log_usage(
    pool: &sqlx::SqlitePool,
    user_id: &str,
    project_id: &str,
    channel_id: Option<&str>,
    model: &str,
    prompt_tokens: i32,
    completion_tokens: i32,
    total_tokens: i32,
    status: &str,
    error_message: Option<&str>,
    duration_ms: i32,
) {
    sqlx::query(
        "INSERT INTO ai_usage_logs (id, user_id, project_id, channel_id, model, prompt_tokens, completion_tokens, total_tokens, status, error_message, duration_ms, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(Uuid::new_v4().to_string())
    .bind(user_id)
    .bind(project_id)
    .bind(channel_id)
    .bind(model)
    .bind(prompt_tokens)
    .bind(completion_tokens)
    .bind(total_tokens)
    .bind(status)
    .bind(error_message)
    .bind(duration_ms)
    .bind(Utc::now().timestamp())
    .execute(pool)
    .await
    .ok();
}

async fn list_models(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<ModelListResponse>, AppError> {
    let models = channel::list_available_models(&state.pool).await?;
    Ok(Json(ModelListResponse {
        object: "list".into(),
        data: models
            .into_iter()
            .map(|id| ModelInfo {
                id,
                object: "model".into(),
                created: 0,
                owned_by: "modeler-ai".into(),
            })
            .collect(),
    }))
}

async fn search(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SearchRequest>,
) -> Result<Json<ChatCompletionResponse>, AppError> {
    let chat_req = ChatCompletionRequest {
        project_id: req.project_id.clone(),
        model: "tavily-search".into(),
        messages: vec![ChatMessage {
            role: "user".into(),
            content: req.query.clone(),
        }],
        temperature: None,
        max_tokens: Some(1024),
        stream: Some(false),
        top_p: None,
        n: None,
        stop: None,
    };

    chat_completions(State(state), auth, Json(chat_req)).await
}

// Admin channel management

async fn list_channels(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<Vec<Channel>>, AppError> {
    Ok(Json(channel::list_channels(&state.pool).await?))
}

async fn create_channel(
    State(state): State<AppState>,
    _auth: AuthUser,
    Json(req): Json<CreateChannelRequest>,
) -> Result<Json<Channel>, AppError> {
    Ok(Json(channel::create_channel(&state.pool, req).await?))
}

async fn update_channel(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateChannelRequest>,
) -> Result<Json<Channel>, AppError> {
    Ok(Json(channel::update_channel(&state.pool, &id, req).await?))
}

async fn delete_channel(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    channel::delete_channel(&state.pool, &id).await?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}
