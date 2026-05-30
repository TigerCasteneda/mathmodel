# Phase 5: History — Version Snapshots + Timeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development

**Goal:** 文件编辑历史 — 自动快照、手动保存点、时间线回退、版本对比

**Architecture:** 利用已有 `crdt_docs` 和 `snapshots` 表的 `ydoc_state` BLOB，存储 CRDT 全量状态作为快照；提供 REST API 浏览和恢复

---

## 非目标 (Phase 5a)

- 不支持可视化 diff 渲染（只返回文本 diff）
- 不实现快照合并策略（先只保留最近 100 个，超过就删旧的不合并）
- 不暴露 Git 概念

---

## 文件结构

```
server/src/history/
├── mod.rs
├── model.rs
├── handlers.rs
```

---

## Task 1: 数据模型 + 模块骨架

**Files:**
- Create: `server/src/history/mod.rs`
- Create: `server/src/history/model.rs`
- Update: `server/src/main.rs` (mod + route)

- [ ] **Step 1: `server/src/history/mod.rs`**

```rust
pub mod model;
pub mod handlers;
```

- [ ] **Step 2: `server/src/history/model.rs`**

```rust
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
pub struct SnapshotDetail {
    pub id: String,
    pub file_id: String,
    pub label: Option<String>,
    pub created_by: String,
    pub display_name: String,
    pub created_at: i64,
    pub content: String,
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
    pub from_label: Option<String>,
    pub to_label: Option<String>,
    pub from_time: i64,
    pub to_time: i64,
    pub diff: String,
}
```

- [ ] **Step 3: Update `server/src/main.rs`**

```rust
mod history;

// router 添加:
.merge(history::handlers::routes())
```

---

## Task 2: History Handlers

**Files:**
- Create: `server/src/history/handlers.rs`

端点：

```
GET  /projects/{project_id}/history/{file_id}       — 获取文件的时间线
POST /projects/{project_id}/history/{file_id}/checkpoint — 手动创建保存点
POST /projects/{project_id}/history/{file_id}/restore/{snapshot_id} — 回退
GET  /projects/{project_id}/history/{file_id}/diff/{from_id}/{to_id} — 对比
```

```rust
use axum::{
    Router, Json,
    routing::{get, post},
    extract::{State, Path},
};
use chrono::Utc;
use uuid::Uuid;

use super::model::*;
use crate::{AppState, AppError};
use crate::auth::middleware::AuthUser;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/projects/{project_id}/history/{file_id}", get(get_timeline))
        .route("/projects/{project_id}/history/{file_id}/checkpoint", post(create_checkpoint))
        .route("/projects/{project_id}/history/{file_id}/restore/{snapshot_id}", post(restore_snapshot))
        .route("/projects/{project_id}/history/{file_id}/diff/{from_id}/{to_id}", get(get_diff))
}

async fn verify_file_member(
    pool: &sqlx::SqlitePool,
    file_id: &str,
    user_id: &str,
) -> Result<(String, String), AppError> {
    let row: (String, String, String) = sqlx::query_as(
        "SELECT f.id, f.name, f.project_id FROM files f
         JOIN project_members pm ON pm.project_id = f.project_id
         WHERE f.id = ? AND f.type = 'file' AND pm.user_id = ?"
    )
    .bind(file_id).bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::NotFound("file not found".into()))?;

    Ok((row.0, row.2))
}

/// Get snapshot timeline for a file
async fn get_timeline(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_project_id, file_id)): Path<(String, String)>,
) -> Result<Json<TimelineResponse>, AppError> {
    verify_file_member(&state.pool, &file_id, &auth.user_id).await?;

    let snapshots: Vec<Snapshot> = sqlx::query_as(
        "SELECT id, file_id, project_id, label, created_by, created_at
         FROM snapshots WHERE file_id = ? ORDER BY created_at DESC LIMIT 100"
    )
    .bind(&file_id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(TimelineResponse { file_id, snapshots }))
}

/// Manually create a named checkpoint
async fn create_checkpoint(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_project_id, file_id)): Path<(String, String)>,
    Json(req): Json<CreateCheckpointRequest>,
) -> Result<Json<Snapshot>, AppError> {
    verify_file_member(&state.pool, &file_id, &auth.user_id).await?;

    // Copy current CRDT state into snapshots
    let crdt_state: Option<(Vec<u8>,)> = sqlx::query_as(
        "SELECT ydoc_state FROM crdt_docs WHERE file_id = ?"
    )
    .bind(&file_id)
    .fetch_optional(&state.pool)
    .await?
    .map(|(s,)| Some((s,))).unwrap_or(None);

    let state = crdt_state.map(|(s,)| s).unwrap_or_default();
    let snapshot_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO snapshots (id, file_id, project_id, label, ydoc_state, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&snapshot_id)
    .bind(&file_id)
    .bind(&_project_id)
    .bind(&req.label)
    .bind(&state)
    .bind(&auth.user_id)
    .bind(now)
    .execute(&state.pool)
    .await?;

    // Enforce 100-snapshot limit
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM snapshots WHERE file_id = ?")
        .bind(&file_id).fetch_one(&state.pool).await?;

    if count > 100 {
        sqlx::query(
            "DELETE FROM snapshots WHERE id IN (
                SELECT id FROM snapshots WHERE file_id = ?
                ORDER BY created_at ASC LIMIT ?
            )"
        )
        .bind(&file_id)
        .bind(count - 100)
        .execute(&state.pool)
        .await?;
    }

    Ok(Json(Snapshot {
        id: snapshot_id, file_id: file_id.clone(),
        project_id: _project_id, label: req.label, created_by: auth.user_id, created_at: now,
    }))
}

/// Get snapshot content (as text decoded from CRDT)
async fn get_snapshot_content(pool: &sqlx::SqlitePool, snapshot_id: &str) -> Result<String, AppError> {
    let state: (Vec<u8>,) = sqlx::query_as("SELECT ydoc_state FROM snapshots WHERE id = ?")
        .bind(snapshot_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("snapshot not found".into()))?;

    decode_crdt_to_text(&state.0)
}

/// Decode CRDT binary to plain text
fn decode_crdt_to_text(data: &[u8]) -> Result<String, AppError> {
    if data.is_empty() {
        return Ok(String::new());
    }
    let doc = yrs::Doc::new();
    let mut txn = doc.transact_mut();
    let update = yrs::Update::decode_v1(data)
        .map_err(|e| AppError::Internal(format!("decode: {}", e)))?;
    txn.apply_update(update);

    // Extract text from Y.Text named "content"
    let text = doc.get_or_insert_text("content");
    Ok(text.get_string(&txn))
}

/// Get diff between two snapshots
async fn get_diff(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_project_id, file_id, from_id, to_id)): Path<(String, String, String, String)>,
) -> Result<Json<DiffResponse>, AppError> {
    verify_file_member(&state.pool, &file_id, &auth.user_id).await?;

    let from_snap: (String, Option<String>, i64) = sqlx::query_as(
        "SELECT id, label, created_at FROM snapshots WHERE id = ? AND file_id = ?"
    )
    .bind(&from_id).bind(&file_id)
    .fetch_optional(&state.pool).await?
    .ok_or_else(|| AppError::NotFound("from snapshot not found".into()))?;

    let to_snap: (String, Option<String>, i64) = sqlx::query_as(
        "SELECT id, label, created_at FROM snapshots WHERE id = ? AND file_id = ?"
    )
    .bind(&to_id).bind(&file_id)
    .fetch_optional(&state.pool).await?
    .ok_or_else(|| AppError::NotFound("to snapshot not found".into()))?;

    let from_text = get_snapshot_content(&state.pool, &from_id).await?;
    let to_text = get_snapshot_content(&state.pool, &to_id).await?;

    // Simple line diff
    let diff = compute_line_diff(&from_text, &to_text);

    Ok(Json(DiffResponse {
        from_id: from_snap.0,
        to_id: to_snap.0,
        from_label: from_snap.1,
        to_label: to_snap.1,
        from_time: from_snap.2,
        to_time: to_snap.2,
        diff,
    }))
}

fn compute_line_diff(old_text: &str, new_text: &str) -> String {
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();
    let mut result = String::new();

    let max_len = old_lines.len().max(new_lines.len());
    for i in 0..max_len {
        match (old_lines.get(i), new_lines.get(i)) {
            (Some(old), Some(new)) if old != new => {
                result.push_str(&format!("  {} | - {}\n  {} | + {}\n", i + 1, old, i + 1, new));
            }
            (Some(old), None) => {
                result.push_str(&format!("  {} | - {}\n", i + 1, old));
            }
            (None, Some(new)) => {
                result.push_str(&format!("  {} | + {}\n", i + 1, new));
            }
            _ => {} // identical lines are skipped
        }
    }

    if result.is_empty() {
        "No differences".to_string()
    } else {
        result
    }
}
```

---

## Task 3: 自动快照 — sync 模块回写

**Files:**
- Modify: `server/src/sync/handlers.rs`

每当客户端断开 WebSocket 连接时（即用户关闭文件），自动创建快照：

在 `handle_socket` 函数的断开处理部分（`persist` 调用之后），添加：

```rust
// Auto-snapshot on disconnect if doc has content
auto_snapshot(&pool, &file_id).await;
```

新增辅助函数：

```rust
async fn auto_snapshot(pool: &sqlx::SqlitePool, file_id: &str) {
    // Get CRDT state
    let state: Option<(Vec<u8>,)> = sqlx::query_as(
        "SELECT ydoc_state FROM crdt_docs WHERE file_id = ?"
    )
    .bind(file_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|(s,)| (s,));

    let Some((state,)) = state else { return };
    if state.is_empty() { return; }

    // Get project_id from file
    let project_id: Option<(String,)> = sqlx::query_as(
        "SELECT project_id FROM files WHERE id = ?"
    )
    .bind(file_id)
    .fetch_optional(pool)
    .await
    .ok()
    .flatten()
    .map(|(p,)| (p,));

    let Some((project_id,)) = project_id else { return };

    let snapshot_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    sqlx::query(
        "INSERT INTO snapshots (id, file_id, project_id, label, ydoc_state, created_by, created_at) VALUES (?, ?, ?, NULL, ?, ?, ?)"
    )
    .bind(&snapshot_id)
    .bind(file_id)
    .bind(&project_id)
    .bind(&state)
    .bind("system")
    .bind(now)
    .execute(pool)
    .await
    .ok();

    // Enforce limit
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM snapshots WHERE file_id = ?")
        .bind(file_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    if count > 100 {
        sqlx::query(
            "DELETE FROM snapshots WHERE id IN (
                SELECT id FROM snapshots WHERE file_id = ? ORDER BY created_at ASC LIMIT ?
            )"
        )
        .bind(file_id)
        .bind(count - 100)
        .execute(pool)
        .await
        .ok();
    }
}
```

---

## Task 4: Restore 端点

在 handlers 中添加 `restore_snapshot`:

```rust
/// Restore a file to a snapshot's state
async fn restore_snapshot(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((_project_id, file_id, snapshot_id)): Path<(String, String, String)>,
) -> Result<Json<serde_json::Value>, AppError> {
    verify_file_member(&state.pool, &file_id, &auth.user_id).await?;

    let snap_state: (Vec<u8>,) = sqlx::query_as("SELECT ydoc_state FROM snapshots WHERE id = ? AND file_id = ?")
        .bind(&snapshot_id).bind(&file_id)
        .fetch_optional(&state.pool).await?
        .ok_or_else(|| AppError::NotFound("snapshot not found".into()))?;

    let now = Utc::now().timestamp();

    // Save current state as auto-snapshot before restore (safety net)
    let current: Option<(Vec<u8>,)> = sqlx::query_as("SELECT ydoc_state FROM crdt_docs WHERE file_id = ?")
        .bind(&file_id).fetch_optional(&state.pool).await?;

    if let Some((current_state,)) = current {
        if !current_state.is_empty() {
            sqlx::query(
                "INSERT INTO snapshots (id, file_id, project_id, label, ydoc_state, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)"
            )
            .bind(Uuid::new_v4().to_string()).bind(&file_id).bind(&_project_id)
            .bind("auto-before-restore").bind(&current_state).bind(&auth.user_id).bind(now)
            .execute(&state.pool).await?;
        }
    }

    // Replace current CRDT state with snapshot state
    sqlx::query(
        "INSERT OR REPLACE INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)"
    )
    .bind(&file_id).bind(&snap_state.0).bind(now)
    .execute(&state.pool).await?;

    Ok(Json(serde_json::json!({ "restored": true, "snapshot_id": snapshot_id })))
}
```

---

## Task 5: 编译验证

```bash
cargo check
```

Expected: zero errors.

db.rs 的 `run_migrations` 中，`snapshots` 表已在 `001_initial.sql` 定义，无需新 migration。

---

## Task 6: E2E 测试

```bash
# Get timeline
curl http://localhost:3001/projects/{pid}/history/{fid} -H "Authorization: Bearer $TOKEN"

# Create checkpoint
curl -X POST http://localhost:3001/projects/{pid}/history/{fid}/checkpoint \
  -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{"label":"before refactor"}'

# Get diff
curl "http://localhost:3001/projects/{pid}/history/{fid}/diff/{snap1}/{snap2}" \
  -H "Authorization: Bearer $TOKEN"

# Restore
curl -X POST "http://localhost:3001/projects/{pid}/history/{fid}/restore/{snap_id}" \
  -H "Authorization: Bearer $TOKEN"
```
