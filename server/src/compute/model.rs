use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct RunCodeRequest {
    pub project_id: String,
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct RunCodeResponse {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct PackageList {
    pub packages: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequest {
    pub project_id: String,
    pub packages: Vec<String>,
}
