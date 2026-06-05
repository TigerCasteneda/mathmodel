use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct FileNode {
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub node_type: String,
    pub mime_type: Option<String>,
    pub size: i64,
    pub storage_path: Option<String>,
    pub zone: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct FileTree {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub zone: String,
    pub updated_at: i64,
    pub children: Option<Vec<FileTree>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFileRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub parent_id: Option<String>,
    pub zone: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    pub name: String,
}
