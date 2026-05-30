use axum::{
    extract::{Path, State},
    routing::{delete, get, post},
    Json, Router,
};
use chrono::Utc;
use uuid::Uuid;

use super::model::*;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", post(create_project).get(list_projects))
        .route(
            "/{id}",
            get(get_project).put(update_project).delete(delete_project),
        )
        .route("/{id}/members", get(list_members))
        .route("/{id}/members/{user_id}", delete(remove_member))
        .route("/{id}/invite", post(create_invite))
        .route("/join", post(join_by_code))
}

async fn create_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateProjectRequest>,
) -> Result<Json<Project>, AppError> {
    if req.name.trim().is_empty() {
        return Err(AppError::BadRequest("project name required".into()));
    }

    let project_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    let mut tx = state.pool.begin().await?;

    sqlx::query(
        "INSERT INTO projects (id, name, owner_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&project_id)
    .bind(req.name.trim())
    .bind(&auth.user_id)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO project_members (project_id, user_id, role, joined_at) VALUES (?, ?, 'owner', ?)"
    )
    .bind(&project_id)
    .bind(&auth.user_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    for (name, zone) in [
        ("Code", "code"),
        ("Paper", "paper"),
        ("Research", "research"),
    ] {
        sqlx::query(
            "INSERT INTO files (id, project_id, parent_id, name, type, zone, created_at, updated_at) VALUES (?, ?, NULL, ?, 'folder', ?, ?, ?)"
        )
        .bind(Uuid::new_v4().to_string())
        .bind(&project_id)
        .bind(name)
        .bind(zone)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(Json(Project {
        id: project_id,
        name: req.name.trim().to_string(),
        owner_id: auth.user_id,
        created_at: now,
        updated_at: now,
    }))
}

async fn list_projects(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<ProjectWithRole>>, AppError> {
    let projects = sqlx::query_as(
        "SELECT p.id, p.name, p.owner_id, pm.role, p.created_at, p.updated_at
         FROM projects p
         JOIN project_members pm ON p.id = pm.project_id
         WHERE pm.user_id = ?
         ORDER BY p.updated_at DESC",
    )
    .bind(&auth.user_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(projects))
}

async fn get_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<ProjectWithRole>, AppError> {
    let project = sqlx::query_as(
        "SELECT p.id, p.name, p.owner_id, pm.role, p.created_at, p.updated_at
         FROM projects p
         JOIN project_members pm ON p.id = pm.project_id
         WHERE p.id = ? AND pm.user_id = ?",
    )
    .bind(&id)
    .bind(&auth.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("project not found".into()))?;

    Ok(Json(project))
}

async fn update_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateProjectRequest>,
) -> Result<Json<Project>, AppError> {
    let project: Project = sqlx::query_as("SELECT * FROM projects WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("project not found".into()))?;

    if project.owner_id != auth.user_id {
        return Err(AppError::Forbidden("only owner can update project".into()));
    }

    let Some(name) = req.name else {
        return Ok(Json(project));
    };
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("project name required".into()));
    }

    let now = Utc::now().timestamp();
    let updated: Project =
        sqlx::query_as("UPDATE projects SET name = ?, updated_at = ? WHERE id = ? RETURNING *")
            .bind(name)
            .bind(now)
            .bind(&id)
            .fetch_one(&state.pool)
            .await?;

    Ok(Json(updated))
}

async fn delete_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let owner_id: String = sqlx::query_scalar("SELECT owner_id FROM projects WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("project not found".into()))?;

    if owner_id != auth.user_id {
        return Err(AppError::Forbidden("only owner can delete project".into()));
    }

    sqlx::query(
        "DELETE FROM file_blobs WHERE file_id IN (SELECT id FROM files WHERE project_id = ?)",
    )
    .bind(&id)
    .execute(&state.pool)
    .await?;
    sqlx::query(
        "DELETE FROM crdt_docs WHERE file_id IN (SELECT id FROM files WHERE project_id = ?)",
    )
    .bind(&id)
    .execute(&state.pool)
    .await?;
    sqlx::query("DELETE FROM files WHERE project_id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    sqlx::query("DELETE FROM project_members WHERE project_id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    sqlx::query("DELETE FROM invite_codes WHERE project_id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;
    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(&id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn list_members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<ProjectMember>>, AppError> {
    let _: (String,) =
        sqlx::query_as("SELECT role FROM project_members WHERE project_id = ? AND user_id = ?")
            .bind(&id)
            .bind(&auth.user_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::NotFound("project not found".into()))?;

    let members = sqlx::query_as(
        "SELECT pm.user_id, u.email, u.display_name, pm.role, pm.joined_at
         FROM project_members pm
         JOIN users u ON pm.user_id = u.id
         WHERE pm.project_id = ?
         ORDER BY pm.joined_at",
    )
    .bind(&id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(members))
}

async fn remove_member(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, target_user_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (role,): (String,) =
        sqlx::query_as("SELECT role FROM project_members WHERE project_id = ? AND user_id = ?")
            .bind(&project_id)
            .bind(&auth.user_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| AppError::NotFound("project not found".into()))?;

    if role != "owner" {
        return Err(AppError::Forbidden("only owner can remove members".into()));
    }

    if target_user_id == auth.user_id {
        return Err(AppError::BadRequest("cannot remove yourself".into()));
    }

    sqlx::query("DELETE FROM project_members WHERE project_id = ? AND user_id = ?")
        .bind(&project_id)
        .bind(&target_user_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "removed": true })))
}

async fn create_invite(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
    Json(req): Json<CreateInviteRequest>,
) -> Result<Json<InviteCodeResponse>, AppError> {
    let (owner_id,): (String,) = sqlx::query_as("SELECT owner_id FROM projects WHERE id = ?")
        .bind(&project_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("project not found".into()))?;

    if owner_id != auth.user_id {
        return Err(AppError::Forbidden("only owner can create invites".into()));
    }

    let code = Uuid::new_v4().to_string().replace('-', "")[..8].to_string();
    let now = Utc::now().timestamp();
    let expires_at = req.expires_in_hours.map(|h| now + h * 3600);

    sqlx::query(
        "INSERT INTO invite_codes (id, project_id, code, max_uses, expires_at, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(Uuid::new_v4().to_string())
    .bind(&project_id)
    .bind(&code)
    .bind(req.max_uses.unwrap_or(10))
    .bind(expires_at)
    .bind(&auth.user_id)
    .bind(now)
    .execute(&state.pool)
    .await?;

    Ok(Json(InviteCodeResponse { code, expires_at }))
}

async fn join_by_code(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<JoinRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let invite: (String, String, i32, i32, Option<i64>) = sqlx::query_as(
        "SELECT id, project_id, max_uses, used_count, expires_at FROM invite_codes WHERE code = ?",
    )
    .bind(&req.code)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("invalid invite code".into()))?;

    let (invite_id, project_id, max_uses, used_count, expires_at) = invite;
    let now = Utc::now().timestamp();

    let already_member: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_members WHERE project_id = ? AND user_id = ?)",
    )
    .bind(&project_id)
    .bind(&auth.user_id)
    .fetch_one(&state.pool)
    .await?;
    if already_member != 0 {
        return Ok(Json(serde_json::json!({ "project_id": project_id })));
    }

    if used_count >= max_uses {
        return Err(AppError::BadRequest(
            "invite code expired (max uses)".into(),
        ));
    }
    if let Some(exp) = expires_at {
        if now > exp {
            return Err(AppError::BadRequest("invite code expired".into()));
        }
    }

    let mut tx = state.pool.begin().await?;

    let updated = sqlx::query(
        "UPDATE invite_codes SET used_count = used_count + 1 WHERE id = ? AND used_count < max_uses",
    )
    .bind(&invite_id)
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::BadRequest(
            "invite code expired (max uses)".into(),
        ));
    }

    sqlx::query(
        "INSERT OR IGNORE INTO project_members (project_id, user_id, role, joined_at) VALUES (?, ?, 'editor', ?)"
    )
    .bind(&project_id)
    .bind(&auth.user_id)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Json(serde_json::json!({ "project_id": project_id })))
}
