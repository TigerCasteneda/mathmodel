use axum::{
    body::Bytes,
    extract::{Multipart, Path, Query, State},
    routing::{get, post, put},
    Json, Router,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use super::model::*;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

#[derive(sqlx::FromRow)]
struct DeleteTarget {
    id: String,
    storage_path: Option<String>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{project_id}/files",
            get(list_files).post(create_file),
        )
        .route("/projects/{project_id}/files/upload", post(upload_file))
        .route(
            "/projects/{project_id}/files/{file_id}",
            get(get_file).delete(delete_file),
        )
        .route(
            "/projects/{project_id}/files/{file_id}/rename",
            put(rename_file),
        )
        .route(
            "/projects/{project_id}/files/{file_id}/download",
            get(download_file),
        )
        .route("/projects/{project_id}/tree", get(get_file_tree))
}

#[derive(Deserialize)]
struct ListQuery {
    parent_id: Option<String>,
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

fn validate_file_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains('\0')
        || name.contains('\r')
        || name.contains('\n')
    {
        return Err(AppError::BadRequest("invalid file name".into()));
    }
    Ok(name.to_string())
}

fn validate_node_type(node_type: &str) -> Result<&str, AppError> {
    match node_type {
        "file" | "folder" => Ok(node_type),
        _ => Err(AppError::BadRequest("invalid file type".into())),
    }
}

fn validate_zone(zone: &str) -> Result<&str, AppError> {
    match zone {
        "code" | "paper" | "research" => Ok(zone),
        _ => Err(AppError::BadRequest("invalid file zone".into())),
    }
}

async fn resolve_zone(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    parent_id: Option<&String>,
    requested_zone: Option<&str>,
) -> Result<String, AppError> {
    if let Some(parent_id) = parent_id {
        let parent: (String, String) =
            sqlx::query_as("SELECT type, zone FROM files WHERE id = ? AND project_id = ?")
                .bind(parent_id)
                .bind(project_id)
                .fetch_optional(pool)
                .await?
                .ok_or_else(|| AppError::BadRequest("parent folder not found".into()))?;

        if parent.0 != "folder" {
            return Err(AppError::BadRequest("parent must be a folder".into()));
        }

        return match requested_zone {
            Some(zone) => Ok(validate_zone(zone)?.to_string()),
            None => Ok(parent.1),
        };
    }

    Ok(validate_zone(requested_zone.unwrap_or("code"))?.to_string())
}

async fn ensure_unique_name(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    parent_id: Option<&String>,
    name: &str,
    exclude_id: Option<&str>,
) -> Result<(), AppError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM files
            WHERE project_id = ?
              AND name = ?
              AND ((parent_id IS NULL AND ? IS NULL) OR parent_id = ?)
              AND (? IS NULL OR id != ?)
        )",
    )
    .bind(project_id)
    .bind(name)
    .bind(parent_id)
    .bind(parent_id)
    .bind(exclude_id)
    .bind(exclude_id)
    .fetch_one(pool)
    .await?;

    if exists != 0 {
        Err(AppError::Conflict("file name already exists".into()))
    } else {
        Ok(())
    }
}

async fn list_files(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<FileNode>>, AppError> {
    verify_membership(&state.pool, &project_id, &auth.user_id).await?;

    let files = if let Some(pid) = &query.parent_id {
        sqlx::query_as(
            "SELECT * FROM files WHERE project_id = ? AND parent_id = ? ORDER BY type DESC, name",
        )
        .bind(&project_id)
        .bind(pid)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(
            "SELECT * FROM files WHERE project_id = ? AND parent_id IS NULL ORDER BY type DESC, name"
        )
        .bind(&project_id)
        .fetch_all(&state.pool)
        .await?
    };

    Ok(Json(files))
}

async fn create_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
    Json(req): Json<CreateFileRequest>,
) -> Result<Json<FileNode>, AppError> {
    verify_membership(&state.pool, &project_id, &auth.user_id).await?;

    let name = validate_file_name(&req.name)?;
    let node_type = validate_node_type(&req.node_type)?;
    let zone = resolve_zone(
        &state.pool,
        &project_id,
        req.parent_id.as_ref(),
        req.zone.as_deref(),
    )
    .await?;
    ensure_unique_name(
        &state.pool,
        &project_id,
        req.parent_id.as_ref(),
        &name,
        None,
    )
    .await?;

    let file_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO files (id, project_id, parent_id, name, type, zone, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&file_id)
    .bind(&project_id)
    .bind(&req.parent_id)
    .bind(&name)
    .bind(node_type)
    .bind(&zone)
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await?;

    // For text files, create an empty CRDT doc placeholder
    if node_type == "file" {
        sqlx::query("INSERT INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)")
            .bind(&file_id)
            .bind(Vec::<u8>::new())
            .bind(now)
            .execute(&state.pool)
            .await?;
    }

    Ok(Json(FileNode {
        id: file_id,
        project_id,
        parent_id: req.parent_id,
        name,
        node_type: node_type.to_string(),
        mime_type: None,
        size: 0,
        storage_path: None,
        zone,
        created_at: now,
        updated_at: now,
    }))
}

async fn upload_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
    mut multipart: Multipart,
) -> Result<Json<FileNode>, AppError> {
    verify_membership(&state.pool, &project_id, &auth.user_id).await?;

    let mut file_name = String::new();
    let mut data = Vec::new();

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if let Some(name) = field.file_name() {
            file_name = name.to_string();
        }
        data = field.bytes().await.unwrap_or_default().to_vec();
    }

    let file_name = validate_file_name(&file_name)
        .map_err(|_| AppError::BadRequest("no file provided".into()))?;
    ensure_unique_name(&state.pool, &project_id, None, &file_name, None).await?;

    let file_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();
    let mime = mime_guess::from_path(&file_name)
        .first_or_octet_stream()
        .to_string();
    let storage_path = format!("{}/{}", project_id, &file_id);

    let full_path = std::path::Path::new(&state.config.data_dir).join(&storage_path);
    std::fs::create_dir_all(full_path.parent().unwrap()).ok();
    std::fs::write(&full_path, &data).map_err(|e| {
        tracing::error!("Failed to write file: {:?}", e);
        AppError::Internal("failed to store file".into())
    })?;

    sqlx::query(
        "INSERT INTO files (id, project_id, parent_id, name, type, mime_type, size, storage_path, zone, created_at, updated_at) VALUES (?, ?, NULL, ?, 'file', ?, ?, ?, 'research', ?, ?)"
    )
    .bind(&file_id)
    .bind(&project_id)
    .bind(&file_name)
    .bind(&mime)
    .bind(data.len() as i64)
    .bind(&storage_path)
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await?;

    Ok(Json(FileNode {
        id: file_id,
        project_id,
        parent_id: None,
        name: file_name,
        node_type: "file".into(),
        mime_type: Some(mime),
        size: data.len() as i64,
        storage_path: Some(storage_path),
        zone: "research".into(),
        created_at: now,
        updated_at: now,
    }))
}

async fn get_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, file_id)): Path<(String, String)>,
) -> Result<Json<FileNode>, AppError> {
    verify_membership(&state.pool, &project_id, &auth.user_id).await?;

    let file: FileNode = sqlx::query_as("SELECT * FROM files WHERE id = ? AND project_id = ?")
        .bind(&file_id)
        .bind(&project_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("file not found".into()))?;

    Ok(Json(file))
}

async fn delete_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, file_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_membership(&state.pool, &project_id, &auth.user_id).await?;

    let targets: Vec<DeleteTarget> = sqlx::query_as(
        "WITH RECURSIVE descendants(id, depth) AS (
            SELECT id, 0 FROM files WHERE id = ? AND project_id = ?
            UNION ALL
            SELECT f.id, descendants.depth + 1
            FROM files f
            JOIN descendants ON f.parent_id = descendants.id
            WHERE f.project_id = ?
        )
        SELECT f.id, f.storage_path
        FROM files f
        JOIN descendants ON f.id = descendants.id
        ORDER BY descendants.depth DESC",
    )
    .bind(&file_id)
    .bind(&project_id)
    .bind(&project_id)
    .fetch_all(&state.pool)
    .await?;

    if targets.is_empty() {
        return Err(AppError::NotFound("file not found".into()));
    }

    let mut tx = state.pool.begin().await?;
    for target in &targets {
        sqlx::query("DELETE FROM crdt_docs WHERE file_id = ?")
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM file_blobs WHERE file_id = ?")
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM files WHERE id = ?")
            .bind(&target.id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;

    for target in &targets {
        if let Some(path) = &target.storage_path {
            let full_path = std::path::Path::new(&state.config.data_dir).join(path);
            std::fs::remove_file(full_path).ok();
        }
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn rename_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, file_id)): Path<(String, String)>,
    Json(req): Json<RenameRequest>,
) -> Result<Json<FileNode>, AppError> {
    verify_membership(&state.pool, &project_id, &auth.user_id).await?;

    let existing: FileNode = sqlx::query_as("SELECT * FROM files WHERE id = ? AND project_id = ?")
        .bind(&file_id)
        .bind(&project_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("file not found".into()))?;

    let name = validate_file_name(&req.name)?;
    ensure_unique_name(
        &state.pool,
        &project_id,
        existing.parent_id.as_ref(),
        &name,
        Some(&file_id),
    )
    .await?;

    let now = Utc::now().timestamp();
    sqlx::query("UPDATE files SET name = ?, updated_at = ? WHERE id = ? AND project_id = ?")
        .bind(&name)
        .bind(now)
        .bind(&file_id)
        .bind(&project_id)
        .execute(&state.pool)
        .await?;

    let file: FileNode = sqlx::query_as("SELECT * FROM files WHERE id = ? AND project_id = ?")
        .bind(&file_id)
        .bind(&project_id)
        .fetch_one(&state.pool)
        .await?;

    Ok(Json(file))
}

async fn download_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, file_id)): Path<(String, String)>,
) -> Result<(axum::http::HeaderMap, Bytes), AppError> {
    verify_membership(&state.pool, &project_id, &auth.user_id).await?;

    let file: FileNode = sqlx::query_as("SELECT * FROM files WHERE id = ? AND project_id = ?")
        .bind(&file_id)
        .bind(&project_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("file not found".into()))?;

    let data = match &file.storage_path {
        Some(path) => {
            let full_path = std::path::Path::new(&state.config.data_dir).join(path);
            std::fs::read(full_path)
                .map_err(|_| AppError::NotFound("file data not found".into()))?
        }
        None => return Err(AppError::NotFound("no binary content for this file".into())),
    };

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        file.mime_type
            .unwrap_or_else(|| "application/octet-stream".into())
            .parse()
            .unwrap(),
    );
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", file.name)
            .parse()
            .unwrap(),
    );

    Ok((headers, Bytes::from(data)))
}

async fn get_file_tree(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<FileTree>>, AppError> {
    verify_membership(&state.pool, &project_id, &auth.user_id).await?;

    let files: Vec<FileNode> =
        sqlx::query_as("SELECT * FROM files WHERE project_id = ? ORDER BY type DESC, name")
            .bind(&project_id)
            .fetch_all(&state.pool)
            .await?;

    fn build_tree(nodes: &[FileNode], parent_id: Option<&String>) -> Vec<FileTree> {
        nodes
            .iter()
            .filter(|n| n.parent_id.as_ref() == parent_id)
            .map(|n| FileTree {
                id: n.id.clone(),
                name: n.name.clone(),
                node_type: n.node_type.clone(),
                zone: n.zone.clone(),
                children: if n.node_type == "folder" {
                    Some(build_tree(nodes, Some(&n.id)))
                } else {
                    None
                },
            })
            .collect()
    }

    let tree = build_tree(&files, None);
    Ok(Json(tree))
}
