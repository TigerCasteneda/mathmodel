use crate::auth::middleware::AuthUser;
use crate::compute::executor::ComputeExecutor;
use crate::compute::model::*;
use crate::error::AppError;
use crate::AppState;
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/compute/run", post(run_code))
        .route("/compute/packages/{project_id}", get(list_packages))
        .route("/compute/reset/{project_id}", post(reset_environment))
        .route("/compute/install", post(install_packages))
}

fn get_executor() -> Result<ComputeExecutor, AppError> {
    ComputeExecutor::new()
}

async fn verify_member(
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
        Err(AppError::Forbidden("not a project member".into()))
    } else {
        Ok(())
    }
}

async fn run_code(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<RunCodeRequest>,
) -> Result<Json<RunCodeResponse>, AppError> {
    verify_member(&state.pool, &req.project_id, &auth.user_id).await?;

    let executor = get_executor()?;
    let (stdout, stderr, exit_code, duration_ms) =
        executor.execute_python(&req.project_id, &req.code).await?;

    Ok(Json(RunCodeResponse {
        stdout,
        stderr,
        exit_code,
        duration_ms,
    }))
}

async fn list_packages(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
) -> Result<Json<PackageList>, AppError> {
    verify_member(&state.pool, &project_id, &auth.user_id).await?;
    let executor = get_executor()?;
    let packages = executor.list_packages(&project_id).await?;
    Ok(Json(PackageList { packages }))
}

async fn reset_environment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_member(&state.pool, &project_id, &auth.user_id).await?;
    let executor = get_executor()?;
    executor.reset_environment(&project_id).await?;
    Ok(Json(serde_json::json!({ "reset": true })))
}

async fn install_packages(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<InstallRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_member(&state.pool, &req.project_id, &auth.user_id).await?;
    let executor = get_executor()?;
    let output: String = executor
        .install_packages(&req.project_id, &req.packages)
        .await?;
    Ok(Json(serde_json::json!({ "output": output })))
}
