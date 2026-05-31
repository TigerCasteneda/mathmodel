use crate::agent_bridge::registry::AgentRegistry;
use crate::error::AppError;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;
use yrs::ReadTxn;
use yrs::Transact;
use yrs::Text;

use super::model::SaveItemInput;

/// Generate a references/<slug>.md file body from a search result.
pub fn render_md(input: &SaveItemInput) -> String {
    let category_label = category_label(&input.category);
    let summary = input.summary.as_deref().unwrap_or("");
    let authors = input.authors.as_deref().unwrap_or("");
    let publish_year = input
        .publish_year
        .map(|y| y.to_string())
        .unwrap_or_default();
    let keywords = input.keywords.as_deref().unwrap_or("");
    let date = Utc::now().format("%Y-%m-%d");

    format!(
        "# {title}\n\
         - **URL**: {url}\n\
         - **Category**: {category}\n\
         - **Authors**: {authors}\n\
         - **Year**: {year}\n\
         - **Keywords**: {keywords}\n\
         - **Saved**: {date}\n\n\
         ## Abstract\n\
         {summary}\n\n\
         ## Notes\n\
         <!-- Add your notes here -->\n",
        title = input.title,
        url = input.url,
        category = category_label,
        authors = authors,
        year = publish_year,
        keywords = keywords,
        date = date,
        summary = summary,
    )
}

fn category_label(cat: &str) -> &str {
    match cat {
        "literature" => "📄 Literature",
        "dataset" => "📊 Dataset",
        "code" => "🧮 Code",
        "formula" => "📐 Formula",
        "competition" => "🏆 Competition",
        _ => "📄 Literature",
    }
}

/// Derive a filesystem-safe slug from a title.
pub fn title_to_slug(title: &str) -> String {
    let slug = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if slug.len() > 64 {
        slug[..64].to_string()
    } else {
        slug
    }
}

/// Create a cloud file entry using existing project file + CRDT storage path.
///
/// Returns the created file's UUID.
pub async fn create_cloud_md_file(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    filename: &str,
    md_content: &str,
) -> Result<String, AppError> {
    let file_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    // 1. Insert into files table (zone=research)
    sqlx::query(
        "INSERT INTO files (id, project_id, parent_id, name, type, zone, created_at, updated_at)
         VALUES (?, ?, NULL, ?, 'file', 'research', ?, ?)",
    )
    .bind(&file_id)
    .bind(project_id)
    .bind(filename)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    // 2. Encode the markdown as a Yrs CRDT update and store in crdt_docs
    let ydoc = yrs::Doc::new();
    let text = ydoc.get_or_insert_text("content");
    {
        let mut txn = ydoc.transact_mut();
        text.insert(&mut txn, 0, md_content);
    }
    let state = {
        let txn = ydoc.transact();
        txn.encode_state_as_update_v1(&yrs::StateVector::default())
    };

    sqlx::query("INSERT INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)")
        .bind(&file_id)
        .bind(&state)
        .bind(now)
        .execute(pool)
        .await?;

    Ok(file_id)
}

/// Send create_file to Agent (best-effort, errors are logged not returned).
/// Returns 1 if sent, 0 if Agent not connected or bridge missing.
pub async fn notify_agent_create_file(
    agent_registry: &Arc<AgentRegistry>,
    project_id: &str,
    relative_path: &str,
    content: &str,
) -> i32 {
    let Some(bridge) = agent_registry.get(project_id).await else {
        tracing::info!(
            "No agent bridge for project {project_id}, skipping local file creation"
        );
        return 0;
    };

    let msg = serde_json::json!({
        "type": "create_file",
        "path": relative_path,
        "content": content,
    });

    match bridge.send_to_agent(msg).await {
        Ok(()) => {
            tracing::info!("Sent create_file to agent: {relative_path}");
            1
        }
        Err(()) => {
            tracing::info!(
                "Agent not connected for project {project_id}, skipping local file"
            );
            0
        }
    }
}
