use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};
use chrono::Utc;
use uuid::Uuid;
use yrs::updates::decoder::Decode;
use yrs::GetString;
use yrs::Transact;

use super::model::*;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

struct FileAccess {
    project_id: String,
    role: String,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{project_id}/history/{file_id}",
            get(get_timeline),
        )
        .route(
            "/projects/{project_id}/history/{file_id}/checkpoint",
            post(create_checkpoint),
        )
        .route(
            "/projects/{project_id}/history/{file_id}/restore/{snapshot_id}",
            post(restore_snapshot),
        )
        .route(
            "/projects/{project_id}/history/{file_id}/diff/{from_id}/{to_id}",
            get(get_diff),
        )
}

async fn verify_file_access(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    file_id: &str,
    user_id: &str,
) -> Result<FileAccess, AppError> {
    let row: (String, String) = sqlx::query_as(
        "SELECT f.project_id, pm.role FROM files f
         JOIN project_members pm ON pm.project_id = f.project_id
         WHERE f.id = ?
           AND f.project_id = ?
           AND f.type = 'file'
           AND f.storage_path IS NULL
           AND pm.user_id = ?",
    )
    .bind(file_id)
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("file not found".into()))?;

    Ok(FileAccess {
        project_id: row.0,
        role: row.1,
    })
}

fn ensure_can_edit(access: &FileAccess) -> Result<(), AppError> {
    match access.role.as_str() {
        "owner" | "editor" => Ok(()),
        _ => Err(AppError::Forbidden("write access required".into())),
    }
}

/// Decode CRDT binary state back to plain text using yrs
fn decode_crdt_to_text(data: &[u8]) -> Result<String, AppError> {
    if data.is_empty() {
        return Ok(String::new());
    }
    let doc = yrs::Doc::new();
    let mut txn = doc.transact_mut();
    let update = yrs::Update::decode_v1(data)
        .map_err(|e| AppError::Internal(format!("crdt decode: {}", e)))?;
    txn.apply_update(update);
    drop(txn);

    let text = doc.get_or_insert_text("content");
    let txn = doc.transact();
    Ok(text.get_string(&txn))
}

/// GET timeline for a file
async fn get_timeline(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, file_id)): Path<(String, String)>,
) -> Result<Json<TimelineResponse>, AppError> {
    verify_file_access(&state.pool, &project_id, &file_id, &auth.user_id).await?;

    let snapshots: Vec<Snapshot> = sqlx::query_as(
        "SELECT id, file_id, project_id, label, created_by, created_at
         FROM snapshots WHERE file_id = ? AND project_id = ? ORDER BY created_at DESC LIMIT 100",
    )
    .bind(&file_id)
    .bind(&project_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(TimelineResponse { file_id, snapshots }))
}

/// Create a manual checkpoint
async fn create_checkpoint(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, file_id)): Path<(String, String)>,
    Json(req): Json<CreateCheckpointRequest>,
) -> Result<Json<Snapshot>, AppError> {
    let access = verify_file_access(&state.pool, &project_id, &file_id, &auth.user_id).await?;
    ensure_can_edit(&access)?;

    let crdt_row: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT ydoc_state FROM crdt_docs WHERE file_id = ?")
            .bind(&file_id)
            .fetch_optional(&state.pool)
            .await?;

    let ydoc_state = crdt_row.map(|(s,)| s).unwrap_or_default();
    let snapshot_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO snapshots (id, file_id, project_id, label, ydoc_state, created_by, source, created_at) VALUES (?, ?, ?, ?, ?, ?, 'manual', ?)",
    )
    .bind(&snapshot_id)
    .bind(&file_id)
    .bind(&access.project_id)
    .bind(&req.label)
    .bind(&ydoc_state)
    .bind(&auth.user_id)
    .bind(now)
    .execute(&state.pool)
    .await?;

    prune_snapshots(&state.pool, &file_id, 100).await;

    Ok(Json(Snapshot {
        id: snapshot_id,
        file_id,
        project_id: access.project_id,
        label: req.label,
        created_by: auth.user_id,
        created_at: now,
    }))
}

/// Restore file content to a snapshot
async fn restore_snapshot(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, file_id, snapshot_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let access = verify_file_access(&state.pool, &project_id, &file_id, &auth.user_id).await?;
    ensure_can_edit(&access)?;

    if state.room_registry.has_active_clients(&file_id).await {
        return Err(AppError::Conflict(
            "cannot restore while file is open by active collaborators".into(),
        ));
    }

    let snap_state: (Vec<u8>,) = sqlx::query_as(
        "SELECT ydoc_state FROM snapshots WHERE id = ? AND file_id = ? AND project_id = ?",
    )
    .bind(&snapshot_id)
    .bind(&file_id)
    .bind(&access.project_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("snapshot not found".into()))?;

    let now = Utc::now().timestamp();

    // Safety: auto-snapshot current state before restoring
    let current: Option<(Vec<u8>,)> =
        sqlx::query_as("SELECT ydoc_state FROM crdt_docs WHERE file_id = ?")
            .bind(&file_id)
            .fetch_optional(&state.pool)
            .await?;

    if let Some((cs,)) = current {
        if !cs.is_empty() {
            sqlx::query(
                "INSERT INTO snapshots (id, file_id, project_id, label, ydoc_state, created_by, source, created_at) VALUES (?, ?, ?, 'auto-before-restore', ?, ?, 'restore_backup', ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(&file_id)
            .bind(&access.project_id)
            .bind(&cs)
            .bind(&auth.user_id)
            .bind(now)
            .execute(&state.pool)
            .await?;
        }
    }

    // Replace current CRDT state
    sqlx::query(
        "INSERT OR REPLACE INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)",
    )
    .bind(&file_id)
    .bind(&snap_state.0)
    .bind(now)
    .execute(&state.pool)
    .await?;

    Ok(Json(
        serde_json::json!({ "restored": true, "snapshot_id": snapshot_id }),
    ))
}

/// Get diff between two snapshots
async fn get_diff(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, file_id, from_id, to_id)): Path<(String, String, String, String)>,
) -> Result<Json<DiffResponse>, AppError> {
    verify_file_access(&state.pool, &project_id, &file_id, &auth.user_id).await?;

    let from_row: (String, i64) = sqlx::query_as(
        "SELECT id, created_at FROM snapshots WHERE id = ? AND file_id = ? AND project_id = ?",
    )
    .bind(&from_id)
    .bind(&file_id)
    .bind(&project_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("from snapshot not found".into()))?;

    let to_row: (String, i64) = sqlx::query_as(
        "SELECT id, created_at FROM snapshots WHERE id = ? AND file_id = ? AND project_id = ?",
    )
    .bind(&to_id)
    .bind(&file_id)
    .bind(&project_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("to snapshot not found".into()))?;

    let from_text = get_snapshot_text(&state.pool, &from_id).await?;
    let to_text = get_snapshot_text(&state.pool, &to_id).await?;
    let diff = compute_line_diff(&from_text, &to_text);

    Ok(Json(DiffResponse {
        from_id: from_row.0,
        to_id: to_row.0,
        from_time: from_row.1,
        to_time: to_row.1,
        diff,
    }))
}

async fn get_snapshot_text(pool: &sqlx::SqlitePool, snapshot_id: &str) -> Result<String, AppError> {
    let state: (Vec<u8>,) = sqlx::query_as("SELECT ydoc_state FROM snapshots WHERE id = ?")
        .bind(snapshot_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("snapshot not found".into()))?;
    decode_crdt_to_text(&state.0)
}

fn compute_line_diff(old: &str, new: &str) -> String {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut result = String::new();
    let max = old_lines.len().max(new_lines.len());

    for i in 0..max {
        match (old_lines.get(i), new_lines.get(i)) {
            (Some(o), Some(n)) if o != n => {
                result.push_str(&format!("{} | - {}\n{} | + {}\n", i + 1, o, i + 1, n));
            }
            (Some(o), None) => {
                result.push_str(&format!("{} | - {}\n", i + 1, o));
            }
            (None, Some(n)) => {
                result.push_str(&format!("{} | + {}\n", i + 1, n));
            }
            _ => {}
        }
    }

    if result.is_empty() {
        "No differences".into()
    } else {
        result
    }
}

async fn prune_snapshots(pool: &sqlx::SqlitePool, file_id: &str, limit: i64) {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM snapshots WHERE file_id = ?")
        .bind(file_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    if count > limit {
        sqlx::query(
            "DELETE FROM snapshots WHERE id IN (
                SELECT id FROM snapshots WHERE file_id = ? ORDER BY created_at ASC LIMIT ?
            )",
        )
        .bind(file_id)
        .bind(count - limit)
        .execute(pool)
        .await
        .ok();
    }
}
