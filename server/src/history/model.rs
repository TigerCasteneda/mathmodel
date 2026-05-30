use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct Snapshot {
    pub id: String,
    pub file_id: String,
    pub project_id: String,
    pub label: Option<String>,
    pub created_by: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct TimelineResponse {
    pub file_id: String,
    pub snapshots: Vec<Snapshot>,
}

#[derive(Debug, Deserialize)]
pub struct CreateCheckpointRequest {
    pub label: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiffResponse {
    pub from_id: String,
    pub to_id: String,
    pub from_time: i64,
    pub to_time: i64,
    pub diff: String,
}
