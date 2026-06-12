use axum::{
    extract::{Path, State},
    routing::{get, post, put},
    Json, Router,
};
use chrono::{Local, Utc};
use uuid::Uuid;
use yrs::updates::decoder::Decode;
use yrs::{GetString, ReadTxn, Text, Transact};

use super::markdown::{parse_arena_markdown, render_arena_markdown};
use super::model::{
    build_arena_index, AppendArenaLogRequest, AppendArenaLogResponse, ArenaCard, ArenaIndex,
    CreateArenaCardRequest, UpdateArenaCardRequest,
};
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

const ARENA_ROOT: &str = "Arena";
const ARENA_CARDS: &str = "Cards";
const ARENA_LOGS: &str = "Logs";
const ARENA_ZONE: &str = "research";

#[derive(sqlx::FromRow)]
struct ArenaFileRow {
    id: String,
    name: String,
    updated_at: i64,
    ydoc_state: Option<Vec<u8>>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/projects/{project_id}/arena/cards",
            get(list_cards).post(create_card),
        )
        .route(
            "/projects/{project_id}/arena/cards/{file_id}",
            put(update_card),
        )
        .route("/projects/{project_id}/arena/index", get(get_index))
        .route("/projects/{project_id}/arena/log", post(append_log))
        .merge(super::chat::routes())
}

fn default_capabilities(role: &str) -> Vec<String> {
    match role {
        "owner" => [
            "files.read",
            "files.write",
            "ai.read",
            "ai.write",
            "workspace.sync",
            "members.manage",
            "invites.manage",
            "screen.share",
            "screen.view",
        ]
        .iter()
        .map(|cap| cap.to_string())
        .collect(),
        "editor" => [
            "files.read",
            "files.write",
            "ai.read",
            "ai.write",
            "screen.share",
        ]
        .iter()
        .map(|cap| cap.to_string())
        .collect(),
        "viewer" => ["files.read", "ai.read"]
            .iter()
            .map(|cap| cap.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

async fn member_capabilities(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    user_id: &str,
) -> Result<Vec<String>, AppError> {
    let row: (String, Option<String>) = sqlx::query_as(
        "SELECT role, capabilities FROM project_members WHERE project_id = ? AND user_id = ? LIMIT 1",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| AppError::Forbidden("not a member of this project".into()))?;

    let (role, capabilities) = row;
    match capabilities {
        Some(raw) if !raw.trim().is_empty() => serde_json::from_str::<Vec<String>>(&raw)
            .map_err(|e| AppError::Internal(format!("capabilities decode: {e}"))),
        _ => Ok(default_capabilities(&role)),
    }
}

async fn ensure_capability(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    user_id: &str,
    capability: &str,
) -> Result<(), AppError> {
    let capabilities = member_capabilities(pool, project_id, user_id).await?;
    if capabilities.iter().any(|cap| cap == capability) {
        Ok(())
    } else {
        Err(AppError::Forbidden(format!("{capability} required")))
    }
}

fn decode_crdt_to_text(data: &[u8]) -> Result<String, AppError> {
    if data.is_empty() {
        return Ok(String::new());
    }

    let doc = yrs::Doc::new();
    let mut txn = doc.transact_mut();
    let update = yrs::Update::decode_v1(data)
        .map_err(|e| AppError::Internal(format!("crdt decode: {e}")))?;
    txn.apply_update(update);
    drop(txn);

    let text = doc.get_or_insert_text("content");
    let txn = doc.transact();
    Ok(text.get_string(&txn))
}

fn encode_text_as_crdt(content: &str) -> Vec<u8> {
    let doc = yrs::Doc::new();
    let text = doc.get_or_insert_text("content");
    {
        let mut txn = doc.transact_mut();
        text.insert(&mut txn, 0, content);
    }
    let txn = doc.transact();
    txn.encode_state_as_update_v1(&yrs::StateVector::default())
}

fn validate_title(title: &str) -> Result<String, AppError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::BadRequest("card title required".into()));
    }
    if title.contains('\0') || title.contains('\r') || title.contains('\n') {
        return Err(AppError::BadRequest("invalid card title".into()));
    }
    Ok(title.to_string())
}

fn validate_card_type(card_type: &str) -> Result<String, AppError> {
    match card_type.trim() {
        "formula" | "finding" | "assumption" | "decision" | "note" => {
            Ok(card_type.trim().to_string())
        }
        other => Err(AppError::BadRequest(format!("invalid card type: {other}"))),
    }
}

fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            slug.push('-');
            previous_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "card".to_string()
    } else {
        slug.to_string()
    }
}

async fn find_child_folder(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    parent_id: Option<&str>,
    name: &str,
) -> Result<Option<String>, AppError> {
    let row = if let Some(parent_id) = parent_id {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM files WHERE project_id = ? AND parent_id = ? AND name = ? AND type = 'folder' LIMIT 1",
        )
        .bind(project_id)
        .bind(parent_id)
        .bind(name)
        .fetch_optional(pool)
        .await?
    } else {
        sqlx::query_scalar::<_, String>(
            "SELECT id FROM files WHERE project_id = ? AND parent_id IS NULL AND name = ? AND type = 'folder' LIMIT 1",
        )
        .bind(project_id)
        .bind(name)
        .fetch_optional(pool)
        .await?
    };
    Ok(row)
}

async fn create_folder(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    parent_id: Option<&str>,
    name: &str,
) -> Result<String, AppError> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT INTO files (id, project_id, parent_id, name, type, zone, created_at, updated_at) VALUES (?, ?, ?, ?, 'folder', ?, ?, ?)",
    )
    .bind(&id)
    .bind(project_id)
    .bind(parent_id)
    .bind(name)
    .bind(ARENA_ZONE)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(id)
}

async fn ensure_child_folder(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    parent_id: Option<&str>,
    name: &str,
) -> Result<String, AppError> {
    if let Some(id) = find_child_folder(pool, project_id, parent_id, name).await? {
        return Ok(id);
    }
    create_folder(pool, project_id, parent_id, name).await
}

async fn ensure_arena_folders(
    pool: &sqlx::SqlitePool,
    project_id: &str,
) -> Result<(String, String, String), AppError> {
    let root_id = ensure_child_folder(pool, project_id, None, ARENA_ROOT).await?;
    let cards_id = ensure_child_folder(pool, project_id, Some(&root_id), ARENA_CARDS).await?;
    let logs_id = ensure_child_folder(pool, project_id, Some(&root_id), ARENA_LOGS).await?;
    Ok((root_id, cards_id, logs_id))
}

async fn unique_markdown_name(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    parent_id: &str,
    title: &str,
) -> Result<String, AppError> {
    let base = slugify(title);
    for suffix in 0..1000 {
        let name = if suffix == 0 {
            format!("{base}.md")
        } else {
            format!("{base}-{suffix}.md")
        };
        let exists: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM files WHERE project_id = ? AND parent_id = ? AND name = ?)",
        )
        .bind(project_id)
        .bind(parent_id)
        .bind(&name)
        .fetch_one(pool)
        .await?;
        if exists == 0 {
            return Ok(name);
        }
    }
    Err(AppError::Conflict(
        "too many cards with similar names".into(),
    ))
}

async fn load_card_rows(
    pool: &sqlx::SqlitePool,
    project_id: &str,
) -> Result<Vec<ArenaFileRow>, AppError> {
    let (_, cards_id, _) = ensure_arena_folders(pool, project_id).await?;
    let rows = sqlx::query_as::<_, ArenaFileRow>(
        "SELECT f.id, f.name, f.updated_at, c.ydoc_state
         FROM files f
         LEFT JOIN crdt_docs c ON c.file_id = f.id
         WHERE f.project_id = ? AND f.parent_id = ? AND f.type = 'file' AND f.name LIKE '%.md'
         ORDER BY f.updated_at DESC, f.name ASC",
    )
    .bind(project_id)
    .bind(cards_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

fn card_from_row(row: ArenaFileRow) -> Result<ArenaCard, AppError> {
    let content = decode_crdt_to_text(&row.ydoc_state.unwrap_or_default())?;
    let fallback_title = row.name.strip_suffix(".md").unwrap_or(&row.name);
    let parsed = parse_arena_markdown(&content, fallback_title);
    Ok(ArenaCard {
        file_id: row.id,
        title: parsed.frontmatter.title,
        card_type: parsed.frontmatter.card_type,
        tags: parsed.frontmatter.tags,
        aliases: parsed.frontmatter.aliases,
        status: parsed.frontmatter.status,
        links: parsed.links,
        backlinks: Vec::new(),
        unresolved_links: Vec::new(),
        content,
        updated_at: row.updated_at,
    })
}

async fn load_index(pool: &sqlx::SqlitePool, project_id: &str) -> Result<ArenaIndex, AppError> {
    let rows = load_card_rows(pool, project_id).await?;
    let cards = rows
        .into_iter()
        .map(card_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(build_arena_index(cards))
}

async fn list_cards(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<ArenaCard>>, AppError> {
    ensure_capability(&state.pool, &project_id, &auth.user_id, "files.read").await?;
    Ok(Json(load_index(&state.pool, &project_id).await?.cards))
}

async fn get_index(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
) -> Result<Json<ArenaIndex>, AppError> {
    ensure_capability(&state.pool, &project_id, &auth.user_id, "files.read").await?;
    Ok(Json(load_index(&state.pool, &project_id).await?))
}

async fn create_card(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
    Json(req): Json<CreateArenaCardRequest>,
) -> Result<Json<ArenaCard>, AppError> {
    ensure_capability(&state.pool, &project_id, &auth.user_id, "files.write").await?;

    let card_type = validate_card_type(&req.card_type)?;
    let title = validate_title(&req.title)?;
    let tags = req.tags.unwrap_or_default();
    let body = req.body.unwrap_or_else(|| format!("# {title}\n\n"));
    let content = render_arena_markdown(&card_type, &title, &tags, &body);
    let (_, cards_id, _) = ensure_arena_folders(&state.pool, &project_id).await?;
    let name = unique_markdown_name(&state.pool, &project_id, &cards_id, &title).await?;
    let file_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp_millis();
    let ydoc_state = encode_text_as_crdt(&content);

    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT INTO files (id, project_id, parent_id, name, type, mime_type, size, zone, created_at, updated_at) VALUES (?, ?, ?, ?, 'file', 'text/markdown', ?, ?, ?, ?)",
    )
    .bind(&file_id)
    .bind(&project_id)
    .bind(&cards_id)
    .bind(&name)
    .bind(content.len() as i64)
    .bind(ARENA_ZONE)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("INSERT INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)")
        .bind(&file_id)
        .bind(&ydoc_state)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let parsed = parse_arena_markdown(&content, &title);
    Ok(Json(ArenaCard {
        file_id,
        title: parsed.frontmatter.title,
        card_type: parsed.frontmatter.card_type,
        tags: parsed.frontmatter.tags,
        aliases: parsed.frontmatter.aliases,
        status: parsed.frontmatter.status,
        links: parsed.links,
        backlinks: Vec::new(),
        unresolved_links: Vec::new(),
        content,
        updated_at: now,
    }))
}

async fn update_card(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((project_id, file_id)): Path<(String, String)>,
    Json(req): Json<UpdateArenaCardRequest>,
) -> Result<Json<ArenaCard>, AppError> {
    ensure_capability(&state.pool, &project_id, &auth.user_id, "files.write").await?;

    let (_, cards_id, _) = ensure_arena_folders(&state.pool, &project_id).await?;
    let file: Option<(String, i64)> = sqlx::query_as(
        "SELECT name, updated_at FROM files WHERE id = ? AND project_id = ? AND parent_id = ? AND type = 'file'",
    )
    .bind(&file_id)
    .bind(&project_id)
    .bind(&cards_id)
    .fetch_optional(&state.pool)
    .await?;
    let (name, updated_at) =
        file.ok_or_else(|| AppError::NotFound("arena card not found".into()))?;
    if let Some(expected_updated_at) = req.expected_updated_at {
        if updated_at != expected_updated_at {
            return Err(AppError::Conflict(
                "card changed since it was opened".into(),
            ));
        }
    }

    let now = Utc::now().timestamp_millis();
    let ydoc_state = encode_text_as_crdt(&req.content);
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT OR REPLACE INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)",
    )
    .bind(&file_id)
    .bind(&ydoc_state)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE files SET size = ?, updated_at = ? WHERE id = ? AND project_id = ?")
        .bind(req.content.len() as i64)
        .bind(now)
        .bind(&file_id)
        .bind(&project_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let parsed = parse_arena_markdown(&req.content, name.strip_suffix(".md").unwrap_or(&name));
    Ok(Json(ArenaCard {
        file_id,
        title: parsed.frontmatter.title,
        card_type: parsed.frontmatter.card_type,
        tags: parsed.frontmatter.tags,
        aliases: parsed.frontmatter.aliases,
        status: parsed.frontmatter.status,
        links: parsed.links,
        backlinks: Vec::new(),
        unresolved_links: Vec::new(),
        content: req.content,
        updated_at: now,
    }))
}

async fn append_log(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(project_id): Path<String>,
    Json(req): Json<AppendArenaLogRequest>,
) -> Result<Json<AppendArenaLogResponse>, AppError> {
    ensure_capability(&state.pool, &project_id, &auth.user_id, "files.write").await?;

    let message = req.message.trim();
    if message.is_empty() {
        return Err(AppError::BadRequest("log message required".into()));
    }

    let (_, _, logs_id) = ensure_arena_folders(&state.pool, &project_id).await?;
    let now_local = Local::now();
    let name = format!("{}.md", now_local.format("%Y-%m-%d"));
    let now = Utc::now().timestamp_millis();
    let existing: Option<(String, i64, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT f.id, f.updated_at, c.ydoc_state
         FROM files f
         LEFT JOIN crdt_docs c ON c.file_id = f.id
         WHERE f.project_id = ? AND f.parent_id = ? AND f.name = ? AND f.type = 'file'
         LIMIT 1",
    )
    .bind(&project_id)
    .bind(&logs_id)
    .bind(&name)
    .fetch_optional(&state.pool)
    .await?;

    let entry = format!(
        "- {} **{}**: {}\n",
        now_local.format("%H:%M"),
        auth.user_id,
        message
    );

    let (file_id, mut content) = if let Some((file_id, _, ydoc_state)) = existing {
        (
            file_id,
            decode_crdt_to_text(&ydoc_state.unwrap_or_default())?,
        )
    } else {
        (
            Uuid::new_v4().to_string(),
            format!("# Daily Log {}\n\n", now_local.format("%Y-%m-%d")),
        )
    };
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&entry);

    let ydoc_state = encode_text_as_crdt(&content);
    let mut tx = state.pool.begin().await?;
    sqlx::query(
        "INSERT OR IGNORE INTO files (id, project_id, parent_id, name, type, mime_type, size, zone, created_at, updated_at) VALUES (?, ?, ?, ?, 'file', 'text/markdown', ?, ?, ?, ?)",
    )
    .bind(&file_id)
    .bind(&project_id)
    .bind(&logs_id)
    .bind(&name)
    .bind(content.len() as i64)
    .bind(ARENA_ZONE)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT OR REPLACE INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)",
    )
    .bind(&file_id)
    .bind(&ydoc_state)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE files SET size = ?, updated_at = ? WHERE id = ? AND project_id = ?")
        .bind(content.len() as i64)
        .bind(now)
        .bind(&file_id)
        .bind(&project_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Json(AppendArenaLogResponse {
        file_id,
        content,
        updated_at: now,
    }))
}
