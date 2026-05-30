# Phase 1: Auth + Project + File 基础后端 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 搭建 Rust Axum 后端核心链路 — 用户注册登录、项目 CRUD、文件管理 + 前端对接

**Architecture:** Rust Axum monolith 通过 REST API 对外，SQLite 持久化，JWT Bearer 认证，Next.js 前端通过 API client 调用

**Tech Stack:** Rust (Axum, SQLx, jsonwebtoken, argon2, uuid), TypeScript (Next.js 16, React 19)

---

## 前置准备

### Prerequisite: 安装 Rust 工具链

- [ ] **Step 1: 安装 Rust**

- 打开 https://rustup.rs 下载 rustup-init.exe 并运行
- 或命令行: `winget install Rustlang.Rustup`
- 安装完成后重启终端

- [ ] **Step 2: 验证安装**

```bash
rustc --version  # >= 1.78
cargo --version  # >= 1.78
```

- [ ] **Step 3: 创建后端项目**

```bash
cd D:/5_user/mathmodel
cargo init server --name modeler-server
```

---

## Task 1: 项目骨架 — 依赖配置与入口

**Files:**
- Create: `server/Cargo.toml`
- Create: `server/src/main.rs`
- Create: `server/src/config.rs`
- Create: `server/src/error.rs`
- Create: `server/src/db.rs`
- Create: `server/src/auth/mod.rs`
- Create: `server/src/auth/model.rs`
- Create: `server/src/auth/handlers.rs`
- Create: `server/src/auth/middleware.rs`
- Create: `server/src/project/mod.rs`
- Create: `server/src/project/model.rs`
- Create: `server/src/project/handlers.rs`
- Create: `server/src/file/mod.rs`
- Create: `server/src/file/model.rs`
- Create: `server/src/file/handlers.rs`
- Create: `server/migrations/001_initial.sql`

- [ ] **Step 1: Write Cargo.toml**

```toml
[package]
name = "modeler-server"
version = "0.1.0"
edition = "2021"

[dependencies]
axum = { version = "0.8", features = ["macros", "ws"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "chrono", "uuid"] }
uuid = { version = "1", features = ["v4", "serde"] }
jsonwebtoken = "9"
argon2 = "0.5"
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace"] }
tracing = "0.1"
tracing-subscriber = "0.3"
anyhow = "1"
dotenvy = "0.15"
chrono = { version = "0.4", features = ["serde"] }
axum-extra = { version = "0.10", features = ["typed-header"] }
reqwest = { version = "0.12", features = ["json", "stream"] }
futures = "0.3"

[dev-dependencies]
axum-test = "0.18"
tower = { version = "0.5", features = ["util"] }
```

- [ ] **Step 2: Write config.rs**

```rust
use std::env;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub data_dir: String,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            database_url: env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:data/modeler.db?mode=rwc".to_string()),
            jwt_secret: env::var("JWT_SECRET")
                .unwrap_or_else(|_| "dev-secret-change-in-production".to_string()),
            data_dir: env::var("DATA_DIR")
                .unwrap_or_else(|_| "data".to_string()),
            port: env::var("PORT")
                .unwrap_or_else(|_| "3001".to_string())
                .parse()
                .unwrap_or(3001),
        }
    }
}
```

- [ ] **Step 3: Write error.rs**

```rust
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl AppError {
    fn status_and_message(&self) -> (StatusCode, &str) {
        match self {
            AppError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            AppError::Unauthorized(m) => (StatusCode::UNAUTHORIZED, m),
            AppError::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, m),
            AppError::Conflict(m) => (StatusCode::CONFLICT, m),
            AppError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = self.status_and_message();
        let body = Json(json!({ "error": message }));
        (status, body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        match e {
            sqlx::Error::RowNotFound => AppError::NotFound("resource not found".into()),
            sqlx::Error::Database(db_err) if db_err.is_unique_violation() => {
                AppError::Conflict("resource already exists".into())
            }
            _ => {
                tracing::error!("Database error: {:?}", e);
                AppError::Internal("internal server error".into())
            }
        }
    }
}

impl From<jsonwebtoken::errors::Error> for AppError {
    fn from(_: jsonwebtoken::errors::Error) -> Self {
        AppError::Unauthorized("invalid token".into())
    }
}

impl From<argon2::password_hash::Error> for AppError {
    fn from(e: argon2::password_hash::Error) -> Self {
        tracing::error!("Password hashing error: {:?}", e);
        AppError::Internal("internal server error".into())
    }
}
```

- [ ] **Step 4: Write db.rs**

```rust
use sqlx::sqlite::SqlitePool;
use std::path::Path;

pub async fn init_pool(database_url: &str) -> SqlitePool {
    // Ensure parent directory exists for file-based SQLite
    if database_url.starts_with("sqlite:") {
        let path = database_url.strip_prefix("sqlite:").unwrap();
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
    }

    let pool = SqlitePool::connect(database_url)
        .await
        .expect("Failed to connect to database");

    run_migrations(&pool).await;

    pool
}

async fn run_migrations(pool: &SqlitePool) {
    let sql = include_str!("../migrations/001_initial.sql");

    // Split and execute each statement
    for statement in sql.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sqlx::query(statement)
            .execute(pool)
            .await
            .expect("Failed to run migration");
    }
}
```

- [ ] **Step 5: Write auth/mod.rs**

```rust
pub mod model;
pub mod handlers;
pub mod middleware;
```

- [ ] **Step 6: Write project/mod.rs**

```rust
pub mod model;
pub mod handlers;
```

- [ ] **Step 7: Write file/mod.rs**

```rust
pub mod model;
pub mod handlers;
```

- [ ] **Step 8: Write main.rs (skeleton)**

```rust
mod config;
mod db;
mod error;
mod auth;
mod project;
mod file;

use axum::Router;
use tower_http::cors::{CorsLayer, Any};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::init();
    dotenvy::dotenv().ok();

    let cfg = config::Config::from_env();
    let pool = db::init_pool(&cfg.database_url).await;

    let app_state = AppState {
        pool,
        config: cfg.clone(),
    };

    let app = Router::new()
        .nest("/auth", auth::handlers::routes())
        .nest("/projects", project::handlers::routes())
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cfg.port)).await?;
    tracing::info!("Server running on port {}", cfg.port);
    axum::serve(listener, app).await?;

    Ok(())
}

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub config: config::Config,
}
```

- [ ] **Step 9: Write migration 001_initial.sql**

```sql
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    owner_id TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS project_members (
    project_id TEXT NOT NULL REFERENCES projects(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    role TEXT NOT NULL DEFAULT 'editor',
    joined_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, user_id)
);

CREATE TABLE IF NOT EXISTS invite_codes (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    code TEXT UNIQUE NOT NULL,
    max_uses INTEGER DEFAULT 10,
    used_count INTEGER DEFAULT 0,
    expires_at INTEGER,
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id),
    parent_id TEXT REFERENCES files(id),
    name TEXT NOT NULL,
    type TEXT NOT NULL,
    mime_type TEXT,
    size INTEGER DEFAULT 0,
    storage_path TEXT,
    zone TEXT NOT NULL DEFAULT 'code',
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    UNIQUE(project_id, parent_id, name)
);

CREATE TABLE IF NOT EXISTS file_blobs (
    file_id TEXT PRIMARY KEY REFERENCES files(id),
    content BLOB NOT NULL
);

CREATE TABLE IF NOT EXISTS crdt_docs (
    file_id TEXT PRIMARY KEY REFERENCES files(id),
    ydoc_state BLOB NOT NULL,
    updated_at INTEGER NOT NULL
);
```

- [ ] **Step 10: Verify compilation**

```bash
cd D:/5_user/mathmodel/server && cargo check
```

Expected: no errors (there will be unused import warnings, that's fine)

---

## Task 2: Auth — 用户注册与登录

- [ ] **Step 1: Write auth model types**

Write `server/src/auth/model.rs`:

```rust
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: String,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub refresh_token: String,
    pub user: UserProfile,
}

#[derive(Debug, Serialize)]
pub struct UserProfile {
    pub id: String,
    pub email: String,
    pub display_name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    pub iat: usize,
}
```

- [ ] **Step 2: Write auth handlers**

Write `server/src/auth/handlers.rs`:

```rust
use axum::{Router, routing::post, extract::State, Json};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::Utc;
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use uuid::Uuid;

use super::model::*;
use crate::{AppState, AppError, config::Config};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
}

fn generate_tokens(user_id: &str, config: &Config) -> Result<(String, String), AppError> {
    let now = Utc::now().timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        exp: now + 86400, // 24 hours
        iat: now,
    };
    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )?;

    let mut refresh_claims = claims;
    refresh_claims.exp = now + 604800; // 7 days
    let refresh_token = encode(
        &Header::default(),
        &refresh_claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )?;

    Ok((token, refresh_token))
}

fn hash_password(password: &str) -> Result<String, AppError> {
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)?
        .to_string();
    Ok(hash)
}

fn verify_password(password: &str, hash: &str) -> Result<bool, AppError> {
    let parsed_hash = PasswordHash::new(hash)?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    if req.email.is_empty() || req.password.len() < 6 || req.display_name.is_empty() {
        return Err(AppError::BadRequest("invalid input".into()));
    }

    let user_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();
    let password_hash = hash_password(&req.password)?;

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, display_name, created_at) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(&user_id)
    .bind(&req.email)
    .bind(&password_hash)
    .bind(&req.display_name)
    .bind(now)
    .execute(&state.pool)
    .await?;

    let (token, refresh_token) = generate_tokens(&user_id, &state.config)?;

    Ok(Json(AuthResponse {
        token,
        refresh_token,
        user: UserProfile {
            id: user_id,
            email: req.email,
            display_name: req.display_name,
        },
    }))
}

async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<Json<AuthResponse>, AppError> {
    let user: super::model::User = sqlx::query_as("SELECT * FROM users WHERE email = ?")
        .bind(&req.email)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::Unauthorized("invalid credentials".into()))?;

    if !verify_password(&req.password, &user.password_hash)? {
        return Err(AppError::Unauthorized("invalid credentials".into()));
    }

    let (token, refresh_token) = generate_tokens(&user.id, &state.config)?;

    Ok(Json(AuthResponse {
        token,
        refresh_token,
        user: UserProfile {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
        },
    }))
}

async fn refresh(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<AuthResponse>, AppError> {
    let refresh_str = body["refresh_token"]
        .as_str()
        .ok_or_else(|| AppError::BadRequest("refresh_token required".into()))?;

    let claims = decode::<Claims>(
        refresh_str,
        &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
        &Validation::default(),
    )?;

    let user: super::model::User = sqlx::query_as("SELECT * FROM users WHERE id = ?")
        .bind(&claims.claims.sub)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::Unauthorized("user not found".into()))?;

    let (token, refresh_token) = generate_tokens(&user.id, &state.config)?;

    Ok(Json(AuthResponse {
        token,
        refresh_token,
        user: UserProfile {
            id: user.id,
            email: user.email,
            display_name: user.display_name,
        },
    }))
}

```

- [ ] **Step 3: Write auth middleware**

Write `server/src/auth/middleware.rs`:

```rust
use axum::{
    extract::{FromRequestParts, State},
    http::request::Parts,
    async_trait,
};
use jsonwebtoken::{decode, DecodingKey, Validation};

use crate::{AppState, AppError};
use super::model::Claims;

/// Extract authenticated user from JWT Bearer token
pub struct AuthUser {
    pub user_id: String,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| AppError::Unauthorized("missing authorization header".into()))?;

        let claims = decode::<Claims>(
            header,
            &DecodingKey::from_secret(state.config.jwt_secret.as_bytes()),
            &Validation::default(),
        )
        .map_err(|_| AppError::Unauthorized("invalid token".into()))?;

        Ok(AuthUser {
            user_id: claims.claims.sub,
        })
    }
}
```

- [ ] **Step 4: Re-check compilation**

```bash
cd D:/5_user/mathmodel/server && cargo check
```

Expected: compiles, potentially with dead_code warnings

---

## Task 3: Project — CRUD + 成员管理

- [ ] **Step 1: Write project model types**

Write `server/src/project/model.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub owner_id: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ProjectWithRole {
    pub id: String,
    pub name: String,
    pub owner_id: String,
    pub role: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateProjectRequest {
    pub name: Option<String>,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct ProjectMember {
    pub user_id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub joined_at: i64,
}
```

- [ ] **Step 2: Write project handlers**

Write `server/src/project/handlers.rs`:

```rust
use axum::{
    Router,
    routing::{get, post, put, delete},
    extract::{State, Path},
    Json, Extension,
};
use chrono::Utc;
use uuid::Uuid;

use super::model::*;
use crate::{AppState, AppError, auth::middleware::AuthUser};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", post(create_project).get(list_projects))
        .route("/{id}", get(get_project).put(update_project).delete(delete_project))
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
        "INSERT INTO projects (id, name, owner_id, created_at, updated_at) VALUES (?, ?, ?, ?, ?)"
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

    // Create default workspace folders
    for (name, zone) in [("Code", "code"), ("Paper", "paper"), ("Research", "research")] {
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
         ORDER BY p.updated_at DESC"
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
         WHERE p.id = ? AND pm.user_id = ?"
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
    // Verify ownership
    let owner_id: String = sqlx::query_scalar(
        "SELECT owner_id FROM projects WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("project not found".into()))?;

    if owner_id != auth.user_id {
        return Err(AppError::Forbidden("only owner can update project".into()));
    }

    let now = Utc::now().timestamp();
    let name = req.name.unwrap_or_else(|| "Untitled".into());

    sqlx::query("UPDATE projects SET name = ?, updated_at = ? WHERE id = ?")
        .bind(&name)
        .bind(now)
        .bind(&id)
        .execute(&state.pool)
        .await?;

    Ok(Json(Project {
        id,
        name,
        owner_id,
        created_at: 0, // unchanged
        updated_at: now,
    }))
}

async fn delete_project(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let owner_id: String = sqlx::query_scalar(
        "SELECT owner_id FROM projects WHERE id = ?"
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("project not found".into()))?;

    if owner_id != auth.user_id {
        return Err(AppError::Forbidden("only owner can delete project".into()));
    }

    // Cascade delete manually (SQLite foreign keys need PRAGMA enabled)
    sqlx::query("DELETE FROM file_blobs WHERE file_id IN (SELECT id FROM files WHERE project_id = ?)")
        .bind(&id).execute(&state.pool).await?;
    sqlx::query("DELETE FROM crdt_docs WHERE file_id IN (SELECT id FROM files WHERE project_id = ?)")
        .bind(&id).execute(&state.pool).await?;
    sqlx::query("DELETE FROM files WHERE project_id = ?")
        .bind(&id).execute(&state.pool).await?;
    sqlx::query("DELETE FROM project_members WHERE project_id = ?")
        .bind(&id).execute(&state.pool).await?;
    sqlx::query("DELETE FROM invite_codes WHERE project_id = ?")
        .bind(&id).execute(&state.pool).await?;
    sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(&id).execute(&state.pool).await?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn list_members(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<ProjectMember>>, AppError> {
    // Verify membership
    let _: (String,) = sqlx::query_as(
        "SELECT role FROM project_members WHERE project_id = ? AND user_id = ?"
    )
    .bind(&id).bind(&auth.user_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("project not found".into()))?;

    let members = sqlx::query_as(
        "SELECT pm.user_id, u.email, u.display_name, pm.role, pm.joined_at
         FROM project_members pm
         JOIN users u ON pm.user_id = u.id
         WHERE pm.project_id = ?
         ORDER BY pm.joined_at"
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
    // Only owner can remove members
    let (role,): (String,) = sqlx::query_as(
        "SELECT role FROM project_members WHERE project_id = ? AND user_id = ?"
    )
    .bind(&project_id).bind(&auth.user_id)
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
    Json(req): Json<super::model::CreateInviteRequest>,
) -> Result<Json<super::model::InviteCodeResponse>, AppError> {
    // Verify ownership
    let (owner_id,): (String,) = sqlx::query_as(
        "SELECT owner_id FROM projects WHERE id = ?"
    )
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

    Ok(Json(super::model::InviteCodeResponse { code, expires_at }))
}

async fn join_by_code(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<super::model::JoinRequest>,
) -> Result<Json<serde_json::Value>, AppError> {
    let invite: (String, String, i32, i32, Option<i64>) = sqlx::query_as(
        "SELECT id, project_id, max_uses, used_count, expires_at FROM invite_codes WHERE code = ?"
    )
    .bind(&req.code)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("invalid invite code".into()))?;

    let (invite_id, project_id, max_uses, used_count, expires_at) = invite;
    let now = Utc::now().timestamp();

    if used_count >= max_uses {
        return Err(AppError::BadRequest("invite code expired (max uses)".into()));
    }
    if let Some(exp) = expires_at {
        if now > exp {
            return Err(AppError::BadRequest("invite code expired".into()));
        }
    }

    sqlx::query(
        "INSERT OR IGNORE INTO project_members (project_id, user_id, role, joined_at) VALUES (?, ?, 'editor', ?)"
    )
    .bind(&project_id)
    .bind(&auth.user_id)
    .bind(now)
    .execute(&state.pool)
    .await?;

    sqlx::query("UPDATE invite_codes SET used_count = used_count + 1 WHERE id = ?")
        .bind(&invite_id)
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "project_id": project_id })))
}
```

- [ ] **Step 3: Verify compilation**

```bash
cd D:/5_user/mathmodel/server && cargo check
```

Expected: compiles successfully

---

## Task 4: File — CRUD + 上传下载

- [ ] **Step 1: Write file model types**

Write `server/src/file/model.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct FileNode {
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
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
    pub children: Option<Vec<FileTree>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateFileRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String, // "file" or "folder"
    pub parent_id: Option<String>,
    pub zone: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateInviteRequest {
    pub max_uses: Option<i32>,
    pub expires_in_hours: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct InviteCodeResponse {
    pub code: String,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct JoinRequest {
    pub code: String,
}
```

- [ ] **Step 2: Write file handlers**

Write `server/src/file/handlers.rs`:

```rust
use axum::{
    Router,
    routing::{get, post, put, delete},
    extract::{State, Path, Query, Multipart},
    Json,
    body::Bytes,
};
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use super::model::*;
use crate::{AppState, AppError, auth::middleware::AuthUser};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/projects/{project_id}/files", get(list_files).post(create_file))
        .route("/projects/{project_id}/files/upload", post(upload_file))
        .route("/projects/{project_id}/files/{file_id}", get(get_file).delete(delete_file))
        .route("/projects/{project_id}/files/{file_id}/rename", put(rename_file))
        .route("/projects/{project_id}/files/{file_id}/download", get(download_file))
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
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_members WHERE project_id = ? AND user_id = ?)"
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    if !exists {
        Err(AppError::Forbidden("not a member of this project".into()))
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
            "SELECT * FROM files WHERE project_id = ? AND parent_id = ? ORDER BY type DESC, name"
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

    let file_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO files (id, project_id, parent_id, name, type, zone, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&file_id)
    .bind(&project_id)
    .bind(&req.parent_id)
    .bind(&req.name)
    .bind(&req.node_type)
    .bind(req.zone.as_deref().unwrap_or("code"))
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await?;

    // For text files, create an empty CRDT doc
    if req.node_type == "file" {
        let doc = yrs::Doc::new();
        let state = doc.transact();
        let encoded = state.encode_state_as_update_v1(&doc.transact());

        sqlx::query(
            "INSERT INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)"
        )
        .bind(&file_id)
        .bind(encoded.as_ref())
        .bind(now)
        .execute(&state.pool)
        .await?;
    }

    Ok(Json(FileNode {
        id: file_id,
        project_id,
        parent_id: req.parent_id,
        name: req.name,
        node_type: req.node_type,
        mime_type: None,
        size: 0,
        storage_path: None,
        zone: req.zone.unwrap_or_else(|| "code".into()),
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

    if file_name.is_empty() {
        return Err(AppError::BadRequest("no file provided".into()));
    }

    let file_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();
    let mime = mime_guess::from_path(&file_name)
        .first_or_octet_stream()
        .to_string();
    let storage_path = format!("{}/{}", project_id, &file_id);

    // Store file on disk
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

    let file: FileNode = sqlx::query_as("SELECT * FROM files WHERE id = ? AND project_id = ?")
        .bind(&file_id)
        .bind(&project_id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| AppError::NotFound("file not found".into()))?;

    // Remove stored file if on disk
    if let Some(path) = &file.storage_path {
        let full_path = std::path::Path::new(&state.config.data_dir).join(path);
        std::fs::remove_file(full_path).ok();
    }

    sqlx::query("DELETE FROM crdt_docs WHERE file_id = ?")
        .bind(&file_id).execute(&state.pool).await?;
    sqlx::query("DELETE FROM file_blobs WHERE file_id = ?")
        .bind(&file_id).execute(&state.pool).await?;
    // Delete children recursively
    sqlx::query("DELETE FROM files WHERE id = ?")
        .bind(&file_id).execute(&state.pool).await?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn rename_file(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, file_id)): Path<(String, String)>,
    Json(req): Json<RenameRequest>,
) -> Result<Json<FileNode>, AppError> {
    verify_membership(&state.pool, &project_id, &auth.user_id).await?;

    let now = Utc::now().timestamp();
    sqlx::query("UPDATE files SET name = ?, updated_at = ? WHERE id = ? AND project_id = ?")
        .bind(&req.name)
        .bind(now)
        .bind(&file_id)
        .bind(&project_id)
        .execute(&state.pool)
        .await?;

    let file: FileNode = sqlx::query_as("SELECT * FROM files WHERE id = ?")
        .bind(&file_id)
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
            std::fs::read(full_path).map_err(|_| AppError::NotFound("file data not found".into()))?
        }
        None => return Err(AppError::NotFound("no binary content for this file".into())),
    };

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        file.mime_type.unwrap_or_else(|| "application/octet-stream".into())
            .parse()
            .unwrap(),
    );
    headers.insert(
        axum::http::header::CONTENT_DISPOSITION,
        format!("attachment; filename=\"{}\"", file.name).parse().unwrap(),
    );

    Ok((headers, Bytes::from(data)))
}

async fn get_file_tree(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<FileTree>>, AppError> {
    verify_membership(&state.pool, &project_id, &auth.user_id).await?;

    let files: Vec<FileNode> = sqlx::query_as(
        "SELECT * FROM files WHERE project_id = ? ORDER BY type DESC, name"
    )
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
```

- [ ] **Step 3: Add yrs dependency to Cargo.toml**

Append to `server/Cargo.toml`:

```toml
yrs = "0.19"
mime_guess = "2"
```

- [ ] **Step 4: Verify compilation**

```bash
cd D:/5_user/mathmodel/server && cargo check
```

Expected: compiles successfully

---

## Task 5: 更新 main.rs — 注册完整路由

- [ ] **Step 1: Update main.rs with all routes**

Rewrite `server/src/main.rs`:

```rust
mod config;
mod db;
mod error;
mod auth;
mod project;
mod file;

use axum::Router;
use tower_http::cors::{CorsLayer, Any};
use tower_http::trace::TraceLayer;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub config: config::Config,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let cfg = config::Config::from_env();
    std::fs::create_dir_all(&cfg.data_dir).ok();
    let pool = db::init_pool(&cfg.database_url).await;

    let app_state = AppState {
        pool,
        config: cfg.clone(),
    };

    let app = Router::new()
        .nest("/auth", auth::handlers::routes())
        .nest("/projects", project::handlers::routes())
        .nest("/", file::handlers::routes())
        .layer(CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any))
        .layer(TraceLayer::new_for_http())
        .with_state(app_state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    tracing::info!("Server running on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

- [ ] **Step 2: Verify compilation**

```bash
cd D:/5_user/mathmodel/server && cargo check
```

---

## Task 6: 后端测试 — Auth API

- [ ] **Step 1: Start the server**

```bash
cd D:/5_user/mathmodel/server && cargo run
```

Open new terminal:

- [ ] **Step 2: Test register**

```bash
curl -X POST http://localhost:3001/auth/register \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","password":"123456","display_name":"Alice"}'
```

Expected: returns `{"token":"...", "refresh_token":"...", "user":{...}}`

- [ ] **Step 3: Test login**

```bash
curl -X POST http://localhost:3001/auth/login \
  -H "Content-Type: application/json" \
  -d '{"email":"test@example.com","password":"123456"}'
```

Expected: same response shape as register

---

## Task 7: 前端 — API Client 与认证页面

- [ ] **Step 1: Create API client utility**

Create `lib/api.ts`:

```typescript
const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:3001"

let tokenStore: string | null = null
let refreshTokenStore: string | null = null

export function setTokens(token: string, refreshToken: string) {
  tokenStore = token
  refreshTokenStore = refreshToken
  if (typeof window !== "undefined") {
    localStorage.setItem("auth_token", token)
    localStorage.setItem("auth_refresh", refreshToken)
  }
}

export function loadTokens() {
  if (typeof window !== "undefined") {
    tokenStore = localStorage.getItem("auth_token")
    refreshTokenStore = localStorage.getItem("auth_refresh")
  }
}

export function clearTokens() {
  tokenStore = null
  refreshTokenStore = null
  if (typeof window !== "undefined") {
    localStorage.removeItem("auth_token")
    localStorage.removeItem("auth_refresh")
  }
}

export function getToken() {
  if (!tokenStore) loadTokens()
  return tokenStore
}

async function refreshAccessToken(): Promise<boolean> {
  if (!refreshTokenStore) return false
  try {
    const res = await fetch(`${API_BASE}/auth/refresh`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: refreshTokenStore }),
    })
    if (!res.ok) return false
    const data = await res.json()
    setTokens(data.token, data.refresh_token)
    return true
  } catch {
    return false
  }
}

export async function apiFetch<T = unknown>(
  path: string,
  options: RequestInit = {}
): Promise<T> {
  const token = getToken()
  const headers: Record<string, string> = {
    ...(options.headers as Record<string, string> || {}),
  }
  if (token) {
    headers["Authorization"] = `Bearer ${token}`
  }

  let res = await fetch(`${API_BASE}${path}`, { ...options, headers })

  // Auto-refresh on 401
  if (res.status === 401 && refreshTokenStore) {
    const refreshed = await refreshAccessToken()
    if (refreshed) {
      headers["Authorization"] = `Bearer ${getToken()}`
      res = await fetch(`${API_BASE}${path}`, { ...options, headers })
    }
  }

  if (!res.ok) {
    const err = await res.json().catch(() => ({ error: "unknown error" }))
    throw new Error(err.error || `HTTP ${res.status}`)
  }

  return res.json()
}

// Type helpers
export interface UserProfile {
  id: string
  email: string
  display_name: string
}

export interface Project {
  id: string
  name: string
  owner_id: string
  role: string
  created_at: number
  updated_at: number
}
```

- [ ] **Step 2: Create auth hooks**

Create `hooks/use-auth.ts`:

```typescript
"use client"

import { useState, useEffect, useCallback } from "react"
import { apiFetch, setTokens, clearTokens, loadTokens, getToken, UserProfile } from "@/lib/api"

interface AuthState {
  user: UserProfile | null
  loading: boolean
}

export function useAuth() {
  const [state, setState] = useState<AuthState>({ user: null, loading: true })

  useEffect(() => {
    loadTokens()
    const token = getToken()
    if (token) {
      // Verify token by fetching current user info
      // We don't have a /me endpoint yet, so use project list as liveness check
      apiFetch<Array<unknown>>("/projects")
        .then(() => {
          // Token works, we just don't have the user profile cached
          // For now, set a minimal state
          setState({ user: { id: "", email: "", display_name: "User" }, loading: false })
        })
        .catch(() => {
          clearTokens()
          setState({ user: null, loading: false })
        })
    } else {
      setState({ user: null, loading: false })
    }
  }, [])

  const login = useCallback(async (email: string, password: string) => {
    const data = await apiFetch<{ token: string; refresh_token: string; user: UserProfile }>(
      "/auth/login",
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email, password }),
      }
    )
    setTokens(data.token, data.refresh_token)
    setState({ user: data.user, loading: false })
    return data.user
  }, [])

  const register = useCallback(async (email: string, password: string, display_name: string) => {
    const data = await apiFetch<{ token: string; refresh_token: string; user: UserProfile }>(
      "/auth/register",
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email, password, display_name }),
      }
    )
    setTokens(data.token, data.refresh_token)
    setState({ user: data.user, loading: false })
    return data.user
  }, [])

  const logout = useCallback(() => {
    clearTokens()
    setState({ user: null, loading: false })
  }, [])

  return { ...state, login, register, logout }
}
```

- [ ] **Step 3: Create login page**

Create `app/login/page.tsx`:

```tsx
"use client"

import { useState } from "react"
import { useRouter } from "next/navigation"
import Link from "next/link"
import { Sparkles } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { useAuth } from "@/hooks/use-auth"

export default function LoginPage() {
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [error, setError] = useState("")
  const [loading, setLoading] = useState(false)
  const { login } = useAuth()
  const router = useRouter()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError("")
    setLoading(true)
    try {
      await login(email, password)
      router.push("/projects")
    } catch (err) {
      setError(err instanceof Error ? err.message : "Login failed")
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen bg-background flex items-center justify-center">
      <div className="w-full max-w-sm">
        <div className="text-center mb-8">
          <div className="w-12 h-12 rounded-xl bg-primary/20 flex items-center justify-center mx-auto mb-4">
            <Sparkles className="w-6 h-6 text-primary" />
          </div>
          <h1 className="text-xl font-semibold text-foreground">Sign in to Modeler AI</h1>
          <p className="text-sm text-muted-foreground mt-1">
            Collaborative math modeling workspace
          </p>
        </div>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <Input
              type="email"
              placeholder="Email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
              className="bg-input border-border"
            />
          </div>
          <div>
            <Input
              type="password"
              placeholder="Password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              minLength={6}
              className="bg-input border-border"
            />
          </div>

          {error && (
            <p className="text-sm text-destructive">{error}</p>
          )}

          <Button
            type="submit"
            className="w-full bg-primary text-primary-foreground hover:bg-primary/90"
            disabled={loading}
          >
            {loading ? "Signing in..." : "Sign in"}
          </Button>
        </form>

        <p className="text-center text-sm text-muted-foreground mt-6">
          Don&apos;t have an account?{" "}
          <Link href="/register" className="text-primary hover:underline">
            Create one
          </Link>
        </p>
      </div>
    </div>
  )
}
```

- [ ] **Step 4: Create register page**

Create `app/register/page.tsx`:

```tsx
"use client"

import { useState } from "react"
import { useRouter } from "next/navigation"
import Link from "next/link"
import { Sparkles } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { useAuth } from "@/hooks/use-auth"

export default function RegisterPage() {
  const [email, setEmail] = useState("")
  const [password, setPassword] = useState("")
  const [displayName, setDisplayName] = useState("")
  const [error, setError] = useState("")
  const [loading, setLoading] = useState(false)
  const { register } = useAuth()
  const router = useRouter()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError("")
    setLoading(true)
    try {
      await register(email, password, displayName)
      router.push("/projects")
    } catch (err) {
      setError(err instanceof Error ? err.message : "Registration failed")
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen bg-background flex items-center justify-center">
      <div className="w-full max-w-sm">
        <div className="text-center mb-8">
          <div className="w-12 h-12 rounded-xl bg-primary/20 flex items-center justify-center mx-auto mb-4">
            <Sparkles className="w-6 h-6 text-primary" />
          </div>
          <h1 className="text-xl font-semibold text-foreground">Create your account</h1>
          <p className="text-sm text-muted-foreground mt-1">Join your modeling team</p>
        </div>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <Input
              type="text"
              placeholder="Display name"
              value={displayName}
              onChange={(e) => setDisplayName(e.target.value)}
              required
              className="bg-input border-border"
            />
          </div>
          <div>
            <Input
              type="email"
              placeholder="Email"
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              required
              className="bg-input border-border"
            />
          </div>
          <div>
            <Input
              type="password"
              placeholder="Password (min 6 characters)"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              required
              minLength={6}
              className="bg-input border-border"
            />
          </div>

          {error && <p className="text-sm text-destructive">{error}</p>}

          <Button
            type="submit"
            className="w-full bg-primary text-primary-foreground hover:bg-primary/90"
            disabled={loading}
          >
            {loading ? "Creating..." : "Create account"}
          </Button>
        </form>

        <p className="text-center text-sm text-muted-foreground mt-6">
          Already have an account?{" "}
          <Link href="/login" className="text-primary hover:underline">
            Sign in
          </Link>
        </p>
      </div>
    </div>
  )
}
```

- [ ] **Step 5: Verify frontend builds**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit
```

Expected: no type errors

---

## Task 8: 前端 — 项目列表页面

- [ ] **Step 1: Create projects list page**

Create `app/projects/page.tsx`:

```tsx
"use client"

import { useEffect, useState } from "react"
import { useRouter } from "next/navigation"
import Link from "next/link"
import { Plus, Sparkles, LogOut, FolderGit2 } from "lucide-react"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { useAuth } from "@/hooks/use-auth"
import { apiFetch, Project } from "@/lib/api"

export default function ProjectsPage() {
  const { user, loading, logout } = useAuth()
  const router = useRouter()
  const [projects, setProjects] = useState<Project[]>([])
  const [newName, setNewName] = useState("")
  const [creating, setCreating] = useState(false)
  const [fetching, setFetching] = useState(true)

  useEffect(() => {
    if (!loading && !user) {
      router.push("/login")
      return
    }
    if (user) {
      fetchProjects()
    }
  }, [user, loading])

  const fetchProjects = async () => {
    try {
      const data = await apiFetch<Project[]>("/projects")
      setProjects(data)
    } catch (err) {
      console.error("Failed to fetch projects", err)
    } finally {
      setFetching(false)
    }
  }

  const handleCreate = async () => {
    if (!newName.trim()) return
    setCreating(true)
    try {
      const project = await apiFetch<Project>("/projects", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ name: newName.trim() }),
      })
      setProjects((prev) => [project, ...prev])
      setNewName("")
    } catch (err) {
      console.error("Failed to create project", err)
    } finally {
      setCreating(false)
    }
  }

  const handleLogout = () => {
    logout()
    router.push("/login")
  }

  if (loading || fetching) {
    return (
      <div className="min-h-screen bg-background flex items-center justify-center">
        <p className="text-muted-foreground">Loading...</p>
      </div>
    )
  }

  return (
    <div className="min-h-screen bg-background">
      <div className="max-w-2xl mx-auto px-4 py-16">
        <div className="flex items-center justify-between mb-8">
          <div className="flex items-center gap-3">
            <div className="w-10 h-10 rounded-xl bg-primary/20 flex items-center justify-center">
              <Sparkles className="w-5 h-5 text-primary" />
            </div>
            <div>
              <h1 className="text-xl font-semibold text-foreground">Your Projects</h1>
              <p className="text-sm text-muted-foreground">
                {user?.display_name}
              </p>
            </div>
          </div>
          <Button variant="ghost" size="sm" onClick={handleLogout}>
            <LogOut className="w-4 h-4 mr-1.5" />
            Sign out
          </Button>
        </div>

        <div className="flex gap-3 mb-8">
          <Input
            placeholder="New project name..."
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && handleCreate()}
            className="bg-input border-border"
          />
          <Button
            onClick={handleCreate}
            disabled={creating || !newName.trim()}
            className="bg-primary text-primary-foreground hover:bg-primary/90 shrink-0"
          >
            <Plus className="w-4 h-4 mr-1.5" />
            Create
          </Button>
        </div>

        <ScrollArea className="h-[60vh]">
          {projects.length === 0 ? (
            <div className="text-center py-16">
              <FolderGit2 className="w-12 h-12 text-muted-foreground/40 mx-auto mb-4" />
              <p className="text-muted-foreground">No projects yet. Create your first one above.</p>
            </div>
          ) : (
            <div className="space-y-2">
              {projects.map((p) => (
                <Link
                  key={p.id}
                  href={`/projects/${p.id}`}
                  className="block p-4 rounded-lg border border-border bg-card hover:border-primary/30 hover:bg-card/80 transition-all"
                >
                  <div className="flex items-center justify-between">
                    <div>
                      <h3 className="font-medium text-foreground">{p.name}</h3>
                      <p className="text-xs text-muted-foreground mt-0.5">
                        {p.role === "owner" ? "Owner" : "Editor"} ·{" "}
                        {new Date(p.updated_at * 1000).toLocaleDateString()}
                      </p>
                    </div>
                    <span className="text-xs px-2 py-1 rounded bg-primary/10 text-primary">
                      {p.role}
                    </span>
                  </div>
                </Link>
              ))}
            </div>
          )}
        </ScrollArea>
      </div>
    </div>
  )
}
```

- [ ] **Step 2: Update root page to redirect**

Rewrite `app/page.tsx`:

```tsx
"use client"

import { useEffect } from "react"
import { useRouter } from "next/navigation"

export default function Home() {
  const router = useRouter()

  useEffect(() => {
    router.push("/login")
  }, [router])

  return null
}
```

- [ ] **Step 3: Verify build**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit
```

---

## Task 9: 前端 — 项目工作台页面骨架

- [ ] **Step 1: Add CORS env + update files page**

Create `app/projects/[id]/page.tsx`:

```tsx
"use client"

import { useParams } from "next/navigation"
import { Sidebar } from "@/components/dashboard/sidebar"
import { MainWorkspace } from "@/components/dashboard/main-workspace"
import { CodeCanvas } from "@/components/dashboard/code-canvas"
import { useState } from "react"

export default function ProjectPage() {
  const { id } = useParams<{ id: string }>()
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false)

  return (
    <main className="flex h-screen overflow-hidden bg-background">
      <Sidebar
        collapsed={sidebarCollapsed}
        onToggle={() => setSidebarCollapsed(!sidebarCollapsed)}
      />
      <div className="flex flex-1 min-w-0">
        <div className="flex-1 min-w-0">
          <MainWorkspace />
        </div>
        <div className="w-[480px] hidden lg:block">
          <CodeCanvas />
        </div>
      </div>
    </main>
  )
}
```

- [ ] **Step 2: Add .env.local for API URL**

Create `.env.local`:

```
NEXT_PUBLIC_API_URL=http://localhost:3001
```

- [ ] **Step 3: Final build check**

```bash
cd D:/5_user/mathmodel && npx tsc --noEmit
```

Expected: no errors

---

## Task 10: 端到端验证

- [ ] **Step 1: Start backend**

```bash
cd D:/5_user/mathmodel/server && cargo run
```

Keep running in background.

- [ ] **Step 2: Start frontend**

```bash
cd D:/5_user/mathmodel && npm run dev
```

- [ ] **Step 3: Browser test flow**

1. Open `http://localhost:3000` → redirects to `/login`
2. Click "Create one" → go to `/register`
3. Register with email/password/name
4. Should redirect to `/projects`
5. Create a new project → appears in list
6. Click project → opens workspace at `/projects/{id}`
7. Workspace shows existing three-column layout

- [ ] **Step 4: API test — create project via curl**

```bash
TOKEN=$(curl -s -X POST http://localhost:3001/auth/register \
  -H "Content-Type: application/json" \
  -d '{"email":"bob@test.com","password":"123456","display_name":"Bob"}' | jq -r '.token')

curl -X POST http://localhost:3001/projects \
  -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"MCM 2026 Practice"}'

curl http://localhost:3001/projects \
  -H "Authorization: Bearer $TOKEN"

# Get file tree (should have Code/Paper/Research folders created automatically)
PROJECT_ID=$(curl -s http://localhost:3001/projects -H "Authorization: Bearer $TOKEN" | jq -r '.[0].id')
curl "http://localhost:3001/projects/$PROJECT_ID/tree" -H "Authorization: Bearer $TOKEN"
```

Expected: returns file tree with Code, Paper, Research folders.

---

## 阶段检查点

Phase 1 完成时应该具备：

- [ ] 用户可通过 Web 注册/登录
- [ ] 登录后创建/列出/删除项目
- [ ] 每个项目自动创建 Code、Paper、Research 三个区
- [ ] 可在项目内创建文件/文件夹
- [ ] 可上传二进制文件（PDF、数据等）
- [ ] JWT 认证保护所有 API
- [ ] 邀请码机制（创建/使用）
