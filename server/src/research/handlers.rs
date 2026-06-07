use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use chrono::Utc;
use uuid::Uuid;

use super::model::*;
use super::references;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/research/items", get(list_items).post(save_items))
        .route(
            "/research/items/{item_id}",
            get(get_item).patch(update_item).delete(delete_item),
        )
}

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

async fn load_item_for_user(
    pool: &sqlx::SqlitePool,
    item_id: &str,
    user_id: &str,
) -> Result<ResearchItem, AppError> {
    let item: ResearchItem = sqlx::query_as("SELECT * FROM research_items WHERE id = ?")
        .bind(item_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("research item not found".into()))?;

    verify_membership(pool, &item.project_id, user_id).await?;
    Ok(item)
}

async fn save_items(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SaveItemsRequest>,
) -> Result<Json<SaveItemsResponse>, AppError> {
    verify_membership(&state.pool, &req.project_id, &auth.user_id).await?;

    let mut saved = Vec::new();
    let mut files_created = 0i32;
    let mut warnings = Vec::new();
    let now = Utc::now().timestamp();

    for input in &req.items {
        let cat = validate_category(&input.category)?;
        let item_id = Uuid::new_v4().to_string();
        let cloud_file_id = Uuid::new_v4().to_string();
        let bib_file_id = Uuid::new_v4().to_string();
        let summary = input.summary.clone().unwrap_or_default();
        let authors = input.authors.clone().unwrap_or_default();
        let keywords = input.keywords.clone().unwrap_or_default();
        let methodology = input.methodology.clone().unwrap_or_default();
        let key_parameters = input.key_parameters.clone().unwrap_or_default();
        let ai_relevance = input.ai_relevance.clone().unwrap_or_default();
        let relevance = input.relevance_score.unwrap_or(0.0);
        let mut raw_value = input
            .raw_json
            .clone()
            .unwrap_or_else(|| serde_json::json!({}));
        raw_value["bib_file_id"] = serde_json::json!(bib_file_id);
        if let Some(bibtex) = &input.bibtex {
            raw_value["bibtex"] = serde_json::json!(bibtex);
        }
        let pdf_attachment = references::pdf_attachment_from_input(input);
        let raw_json = raw_value.to_string();

        sqlx::query(
            "INSERT INTO research_items
             (id, project_id, created_by, source, category, url, title, summary,
              authors, publish_year, keywords, notes, relevance_score, cloud_file_id,
              methodology, key_parameters, ai_relevance, raw_json, created_at, updated_at)
             VALUES (?, ?, ?, 'ai', ?, ?, ?, ?, ?, ?, ?, '', ?, ?, ?, ?, ?, ?, ?, ?)",
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
        .bind(&cloud_file_id)
        .bind(&methodology)
        .bind(&key_parameters)
        .bind(&ai_relevance)
        .bind(&raw_json)
        .bind(now)
        .bind(now)
        .execute(&state.pool)
        .await?;

        let md_content = references::render_md(input);
        let bib_content = input.bibtex.clone().unwrap_or_default();
        match references::create_cloud_text_file(
            &state.pool,
            &req.project_id,
            &cloud_file_id,
            &input.title,
            "md",
            &md_content,
        )
        .await
        {
            Ok(()) => {
                files_created += 1;
                if !bib_content.trim().is_empty() {
                    match references::create_cloud_text_file(
                        &state.pool,
                        &req.project_id,
                        &bib_file_id,
                        &input.title,
                        "bib",
                        &bib_content,
                    )
                    .await
                    {
                        Ok(()) => files_created += 1,
                        Err(err) => tracing::warn!("Failed to create cloud bib file: {err:?}"),
                    }
                }
                if let Some(attachment) = pdf_attachment {
                    let pdf_file_id = Uuid::new_v4().to_string();
                    match download_pdf_attachment(&attachment.url).await {
                        Ok(bytes) => match references::create_cloud_binary_file(
                            &state.pool,
                            &req.project_id,
                            &pdf_file_id,
                            &attachment.filename,
                            "application/pdf",
                            &bytes,
                        )
                        .await
                        {
                            Ok(()) => {
                                files_created += 1;
                                raw_value["pdf_file_id"] = serde_json::json!(pdf_file_id);
                                if let Err(err) = sqlx::query(
                                    "UPDATE research_items SET raw_json = ? WHERE id = ?",
                                )
                                .bind(raw_value.to_string())
                                .bind(&item_id)
                                .execute(&state.pool)
                                .await
                                {
                                    tracing::warn!(
                                        "Failed to link research PDF attachment in raw_json: {err:?}"
                                    );
                                }
                            }
                            Err(err) => {
                                tracing::warn!("Failed to create cloud PDF file: {err:?}");
                                warnings.push(format!(
                                    "PDF attachment could not be saved for {}.",
                                    input.title
                                ));
                            }
                        },
                        Err(err) => {
                            tracing::warn!("Failed to download PDF attachment: {err:?}");
                            warnings.push(format!(
                                "PDF attachment could not be downloaded for {}.",
                                input.title
                            ));
                        }
                    }
                }
            }
            Err(err) => {
                tracing::warn!("Failed to create cloud md file: {err:?}");
                sqlx::query("UPDATE research_items SET cloud_file_id = NULL WHERE id = ?")
                    .bind(&item_id)
                    .execute(&state.pool)
                    .await?;
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
        warnings,
    }))
}

async fn download_pdf_attachment(url: &str) -> Result<Vec<u8>, AppError> {
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|error| AppError::Internal(error.to_string()))?
        .get(url)
        .send()
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Internal(format!(
            "PDF download failed ({status})"
        )));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| AppError::Internal(error.to_string()))?;
    Ok(bytes.to_vec())
}

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

async fn get_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(params): Path<ItemPathParam>,
) -> Result<Json<ResearchItem>, AppError> {
    let item = load_item_for_user(&state.pool, &params.item_id, &auth.user_id).await?;
    Ok(Json(item))
}

async fn update_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(params): Path<ItemPathParam>,
    Json(req): Json<UpdateItemRequest>,
) -> Result<Json<ResearchItem>, AppError> {
    let _item = load_item_for_user(&state.pool, &params.item_id, &auth.user_id).await?;
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

async fn delete_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(params): Path<ItemPathParam>,
) -> Result<Json<serde_json::Value>, AppError> {
    let item = load_item_for_user(&state.pool, &params.item_id, &auth.user_id).await?;
    let mut tx = state.pool.begin().await?;

    sqlx::query("DELETE FROM research_context_pages WHERE item_id = ?")
        .bind(&params.item_id)
        .execute(&mut *tx)
        .await?;

    let result = sqlx::query("DELETE FROM research_items WHERE id = ?")
        .bind(&params.item_id)
        .execute(&mut *tx)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("research item not found".into()));
    }

    let mut linked_file_ids = Vec::new();
    if let Some(cloud_file_id) = item.cloud_file_id {
        linked_file_ids.push(cloud_file_id);
    }
    if let Ok(raw_json) = serde_json::from_str::<serde_json::Value>(&item.raw_json) {
        if let Some(bib_file_id) = raw_json["bib_file_id"].as_str() {
            linked_file_ids.push(bib_file_id.to_string());
        }
        if let Some(pdf_file_id) = raw_json["pdf_file_id"].as_str() {
            linked_file_ids.push(pdf_file_id.to_string());
        }
    }

    for file_id in linked_file_ids {
        sqlx::query("DELETE FROM crdt_docs WHERE file_id = ?")
            .bind(&file_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM file_blobs WHERE file_id = ?")
            .bind(&file_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM files WHERE id = ? AND project_id = ?")
            .bind(&file_id)
            .bind(&item.project_id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}
