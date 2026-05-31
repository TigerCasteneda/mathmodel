use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, patch, post},
    Json, Router,
};
use chrono::Utc;
use uuid::Uuid;

use super::model::*;
use super::references;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::morphic::client::MorphicClient;
use crate::morphic::model::AdvancedSearchResponse;
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/research/search", post(search))
        .route("/research/items", get(list_items).post(save_items))
        .route(
            "/research/items/{item_id}",
            get(get_item).patch(update_item).delete(delete_item),
        )
}

// ── Helpers ──

async fn verify_membership(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    user_id: &str,
) -> Result<(), AppError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_members WHERE project_id = ? AND user_id = ?)",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    if exists == 0 {
        Err(AppError::Forbidden("not a member of this project".into()))
    } else {
        Ok(())
    }
}

fn validate_category(cat: &str) -> Result<&str, AppError> {
    match cat {
        "literature" | "dataset" | "code" | "formula" | "competition" => Ok(cat),
        _ => Err(AppError::BadRequest(format!("invalid category: {cat}"))),
    }
}

// ── POST /research/search ──

async fn search(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, AppError> {
    verify_membership(&state.pool, &req.project_id, &auth.user_id).await?;
    validate_category(&req.category)?;

    let client = MorphicClient::from_env();

    let morphic_resp = client
        .advanced_search(&req.query, req.max_results)
        .await
        .unwrap_or_else(|_| AdvancedSearchResponse {
            query: req.query.clone(),
            results: vec![],
            number_of_results: 0,
        });

    let results: Vec<SearchResultItem> = morphic_resp
        .results
        .into_iter()
        .map(|r| SearchResultItem {
            title: r.title,
            url: r.url,
            content: r.content,
            authors: None,
            publish_year: None,
            keywords: None,
            relevance_score: 0.0,
        })
        .collect();

    Ok(Json(SearchResponse {
        query: morphic_resp.query,
        results,
    }))
}

// ── POST /research/items — save search results ──

async fn save_items(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SaveItemsRequest>,
) -> Result<Json<SaveItemsResponse>, AppError> {
    verify_membership(&state.pool, &req.project_id, &auth.user_id).await?;

    let mut saved = Vec::new();
    let mut files_created = 0i32;
    let now = Utc::now().timestamp();

    for input in &req.items {
        let cat = validate_category(&input.category)?;
        let item_id = Uuid::new_v4().to_string();
        let summary = input.summary.clone().unwrap_or_default();
        let authors = input.authors.clone().unwrap_or_default();
        let keywords = input.keywords.clone().unwrap_or_default();
        let relevance = input.relevance_score.unwrap_or(0.0);
        let raw_json = input
            .raw_json
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "{}".to_string());

        sqlx::query(
            "INSERT INTO research_items
             (id, project_id, created_by, source, category, url, title, summary,
              authors, publish_year, keywords, notes, relevance_score, raw_json,
              created_at, updated_at)
             VALUES (?, ?, ?, 'morphic', ?, ?, ?, ?, ?, ?, ?, '', ?, ?, ?, ?)",
        )
        .bind(&item_id)
        .bind(&req.project_id)
        .bind(&auth.user_id)
        .bind(cat)
        .bind(&input.url)
        .bind(&input.title)
        .bind(&summary)
        .bind(&authors)
        .bind(input.publish_year)
        .bind(&keywords)
        .bind(relevance)
        .bind(&raw_json)
        .bind(now)
        .bind(now)
        .execute(&state.pool)
        .await?;

        // Generate cloud .md file via existing CRDT path
        let slug = references::title_to_slug(&input.title);
        let filename = format!("references/{slug}.md");
        let md_content = references::render_md(input);

        match references::create_cloud_md_file(
            &state.pool,
            &req.project_id,
            &filename,
            &md_content,
        )
        .await
        {
            Ok(_file_id) => {
                // Best-effort: also send to Agent for local copy
                files_created += references::notify_agent_create_file(
                    &state.agent_registry,
                    &req.project_id,
                    &filename,
                    &md_content,
                )
                .await;
            }
            Err(err) => {
                tracing::warn!("Failed to create cloud md file: {err:?}");
            }
        }

        let item: ResearchItem = sqlx::query_as("SELECT * FROM research_items WHERE id = ?")
            .bind(&item_id)
            .fetch_one(&state.pool)
            .await?;

        saved.push(item);
    }

    Ok(Json(SaveItemsResponse {
        saved: saved.len() as i32,
        items: saved,
        files_created,
    }))
}

// ── GET /research/items — list ──

async fn list_items(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ListItemsQuery>,
) -> Result<Json<Vec<ResearchItem>>, AppError> {
    verify_membership(&state.pool, &q.project_id, &auth.user_id).await?;

    let sort_col = match q.sort.as_str() {
        "created_at" | "updated_at" | "title" | "category" | "relevance_score" => q.sort.as_str(),
        _ => "created_at",
    };
    let order = if q.order == "asc" { "ASC" } else { "DESC" };

    let items: Vec<ResearchItem> = if let Some(ref cat) = q.category {
        sqlx::query_as(&format!(
            "SELECT * FROM research_items WHERE project_id = ? AND category = ? ORDER BY {sort_col} {order} LIMIT ? OFFSET ?"
        ))
        .bind(&q.project_id)
        .bind(cat)
        .bind(q.limit)
        .bind(q.offset)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(&format!(
            "SELECT * FROM research_items WHERE project_id = ? ORDER BY {sort_col} {order} LIMIT ? OFFSET ?"
        ))
        .bind(&q.project_id)
        .bind(q.limit)
        .bind(q.offset)
        .fetch_all(&state.pool)
        .await?
    };

    Ok(Json(items))
}

// ── GET /research/items/{item_id} — detail ──

async fn get_item(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(params): Path<ItemPathParam>,
) -> Result<Json<ResearchItem>, AppError> {
    let item: ResearchItem = sqlx::query_as("SELECT * FROM research_items WHERE id = ?")
        .bind(&params.item_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("research item not found".into()))?;

    Ok(Json(item))
}

// ── PATCH /research/items/{item_id} — update notes/category ──

async fn update_item(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(params): Path<ItemPathParam>,
    Json(req): Json<UpdateItemRequest>,
) -> Result<Json<ResearchItem>, AppError> {
    let now = Utc::now().timestamp();

    if let Some(ref cat) = req.category {
        validate_category(cat)?;
        sqlx::query("UPDATE research_items SET category = ?, updated_at = ? WHERE id = ?")
            .bind(cat)
            .bind(now)
            .bind(&params.item_id)
            .execute(&state.pool)
            .await?;
    }

    if let Some(ref notes) = req.notes {
        sqlx::query("UPDATE research_items SET notes = ?, updated_at = ? WHERE id = ?")
            .bind(notes)
            .bind(now)
            .bind(&params.item_id)
            .execute(&state.pool)
            .await?;
    }

    let item: ResearchItem = sqlx::query_as("SELECT * FROM research_items WHERE id = ?")
        .bind(&params.item_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("research item not found".into()))?;

    Ok(Json(item))
}

// ── DELETE /research/items/{item_id} ──

async fn delete_item(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(params): Path<ItemPathParam>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Delete context pages first (FK)
    sqlx::query("DELETE FROM research_context_pages WHERE item_id = ?")
        .bind(&params.item_id)
        .execute(&state.pool)
        .await?;

    let result = sqlx::query("DELETE FROM research_items WHERE id = ?")
        .bind(&params.item_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("research item not found".into()));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}
