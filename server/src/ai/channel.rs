use crate::ai::model::*;
use crate::error::AppError;
use rand::Rng;

/// Select the best channel for a given model name.
/// Filters channels that support the model, then picks randomly from the highest-weight tier.
pub async fn get_best_channel(pool: &sqlx::SqlitePool, model: &str) -> Result<Channel, AppError> {
    let channels: Vec<Channel> =
        sqlx::query_as("SELECT * FROM channels WHERE status = 1 ORDER BY weight DESC")
            .fetch_all(pool)
            .await?;

    let matching: Vec<&Channel> = channels
        .iter()
        .filter(|c| c.supports_model(model))
        .collect();

    if matching.is_empty() {
        return Err(AppError::NotFound(format!(
            "no channel available for model: {}",
            model
        )));
    }

    let top_weight = matching[0].weight;
    let top_tier: Vec<&Channel> = matching
        .into_iter()
        .filter(|c| c.weight == top_weight)
        .collect();

    let mut rng = rand::thread_rng();
    let idx = rng.gen_range(0..top_tier.len());

    Ok(top_tier[idx].clone())
}

/// List all channels (admin)
pub async fn list_channels(pool: &sqlx::SqlitePool) -> Result<Vec<Channel>, AppError> {
    Ok(sqlx::query_as("SELECT * FROM channels ORDER BY name")
        .fetch_all(pool)
        .await?)
}

/// Create a new channel (admin)
pub async fn create_channel(
    pool: &sqlx::SqlitePool,
    req: CreateChannelRequest,
) -> Result<Channel, AppError> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO channels (id, name, channel_type, base_url, api_key, models, model_mapping, weight, status, config, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?)"
    )
    .bind(&id)
    .bind(&req.name)
    .bind(req.channel_type)
    .bind(&req.base_url)
    .bind(&req.api_key)
    .bind(&req.models)
    .bind(req.model_mapping.unwrap_or_default())
    .bind(req.weight.unwrap_or(1))
    .bind(req.config.unwrap_or_default())
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    let channel: Channel = sqlx::query_as("SELECT * FROM channels WHERE id = ?")
        .bind(&id)
        .fetch_one(pool)
        .await?;

    Ok(channel)
}

/// Delete a channel (admin)
pub async fn delete_channel(pool: &sqlx::SqlitePool, id: &str) -> Result<(), AppError> {
    let result = sqlx::query("DELETE FROM channels WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("channel not found".into()));
    }
    Ok(())
}

/// Update a channel (admin)
pub async fn update_channel(
    pool: &sqlx::SqlitePool,
    id: &str,
    req: UpdateChannelRequest,
) -> Result<Channel, AppError> {
    let now = chrono::Utc::now().timestamp();

    if let Some(name) = &req.name {
        sqlx::query("UPDATE channels SET name = ?, updated_at = ? WHERE id = ?")
            .bind(name)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if let Some(base_url) = &req.base_url {
        sqlx::query("UPDATE channels SET base_url = ?, updated_at = ? WHERE id = ?")
            .bind(base_url)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if let Some(api_key) = &req.api_key {
        sqlx::query("UPDATE channels SET api_key = ?, updated_at = ? WHERE id = ?")
            .bind(api_key)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if let Some(models) = &req.models {
        sqlx::query("UPDATE channels SET models = ?, updated_at = ? WHERE id = ?")
            .bind(models)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if let Some(model_mapping) = &req.model_mapping {
        sqlx::query("UPDATE channels SET model_mapping = ?, updated_at = ? WHERE id = ?")
            .bind(model_mapping)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if let Some(weight) = req.weight {
        sqlx::query("UPDATE channels SET weight = ?, updated_at = ? WHERE id = ?")
            .bind(weight)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if let Some(status) = req.status {
        sqlx::query("UPDATE channels SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await?;
    }
    if let Some(config) = &req.config {
        sqlx::query("UPDATE channels SET config = ?, updated_at = ? WHERE id = ?")
            .bind(config)
            .bind(now)
            .bind(id)
            .execute(pool)
            .await?;
    }

    let channel: Channel = sqlx::query_as("SELECT * FROM channels WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await?;

    Ok(channel)
}

/// Get distinct model names from all enabled channels
pub async fn list_available_models(pool: &sqlx::SqlitePool) -> Result<Vec<String>, AppError> {
    let channels: Vec<Channel> = sqlx::query_as("SELECT * FROM channels WHERE status = 1")
        .fetch_all(pool)
        .await?;

    let mut models = std::collections::HashSet::new();
    for c in &channels {
        for m in c.parsed_models() {
            models.insert(m);
        }
    }
    let mut result: Vec<String> = models.into_iter().collect();
    result.sort();
    Ok(result)
}
