use crate::error::AppError;

/// Check if a project has remaining token quota
pub async fn check_project_quota(
    pool: &sqlx::SqlitePool,
    project_id: &str,
) -> Result<bool, AppError> {
    let row: Option<(i32, i32)> = sqlx::query_as(
        "SELECT total_tokens_used, token_limit FROM project_quotas WHERE project_id = ?",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;

    match row {
        Some((used, limit)) => Ok(used < limit),
        None => {
            // Auto-create quota row for this project
            let now = chrono::Utc::now().timestamp();
            sqlx::query(
                "INSERT OR IGNORE INTO project_quotas (project_id, total_tokens_used, token_limit, updated_at) VALUES (?, 0, 100000000, ?)"
            )
            .bind(project_id)
            .bind(now)
            .execute(pool)
            .await?;
            Ok(true)
        }
    }
}

/// Deduct tokens from a project's quota
pub async fn deduct_project_quota(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    tokens: i32,
) -> Result<(), AppError> {
    let now = chrono::Utc::now().timestamp();
    sqlx::query(
        "UPDATE project_quotas SET total_tokens_used = total_tokens_used + ?, updated_at = ? WHERE project_id = ?"
    )
    .bind(tokens)
    .bind(now)
    .bind(project_id)
    .execute(pool)
    .await?;
    Ok(())
}
