use crate::error::AppError;
use chrono::Utc;
use yrs::ReadTxn;
use yrs::Text;
use yrs::Transact;

use super::model::SaveItemInput;

#[derive(Debug, Clone)]
pub struct PdfAttachment {
    pub url: String,
    pub filename: String,
}

/// Generate a references/<slug>.md file body from an AI-saved source.
pub fn render_md(input: &SaveItemInput) -> String {
    let category_label = category_label(&input.category);
    let summary = input.summary.as_deref().unwrap_or("");
    let authors = input.authors.as_deref().unwrap_or("");
    let publish_year = input
        .publish_year
        .map(|y| y.to_string())
        .unwrap_or_default();
    let keywords = input.keywords.as_deref().unwrap_or("");
    let methodology = input.methodology.as_deref().unwrap_or("");
    let key_parameters = input.key_parameters.as_deref().unwrap_or("");
    let ai_relevance = input.ai_relevance.as_deref().unwrap_or("");
    let bibtex = input.bibtex.as_deref().unwrap_or("");
    let date = Utc::now().format("%Y-%m-%d");

    format!(
        "# {title}\n\
         - **URL**: {url}\n\
         - **Category**: {category}\n\
         - **Authors**: {authors}\n\
         - **Year**: {year}\n\
         - **Keywords**: {keywords}\n\
         - **Saved**: {date}\n\n\
         ## AI Summary\n\
         {summary}\n\n\
         ## Methodology\n\
         {methodology}\n\n\
         ## Key Parameters\n\
         {key_parameters}\n\n\
         ## Relevance to Project\n\
         {ai_relevance}\n\n\
         ## BibTeX\n\
         ```bibtex\n\
         {bibtex}\n\
         ```\n\n\
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
        methodology = methodology,
        key_parameters = key_parameters,
        ai_relevance = ai_relevance,
        bibtex = bibtex,
    )
}

fn category_label(cat: &str) -> &str {
    match cat {
        "literature" => "Literature",
        "dataset" => "Dataset",
        "code" => "Code",
        "formula" => "Formula",
        "competition" => "Competition",
        _ => "Literature",
    }
}

pub fn pdf_attachment_from_input(input: &SaveItemInput) -> Option<PdfAttachment> {
    let raw_json = input.raw_json.as_ref()?;
    let url = raw_json
        .get("pdf_url")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| value.starts_with("http://") || value.starts_with("https://"))?;
    let filename = raw_json
        .get("attachment_filename")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&input.title);
    Some(PdfAttachment {
        url: url.to_string(),
        filename: sanitize_attachment_filename(filename),
    })
}

fn sanitize_attachment_filename(filename: &str) -> String {
    let mut cleaned = filename
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    if cleaned.is_empty() {
        cleaned = "research-paper.pdf".to_string();
    }
    if !cleaned.to_ascii_lowercase().ends_with(".pdf") {
        cleaned.push_str(".pdf");
    }
    if cleaned.len() > 96 {
        let mut truncated = cleaned.chars().take(92).collect::<String>();
        truncated.push_str(".pdf");
        truncated
    } else {
        cleaned
    }
}

/// Derive a filesystem-safe slug from a title.
pub fn title_to_slug(title: &str) -> String {
    let slug = title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    let slug = if slug.is_empty() {
        "research-item".to_string()
    } else {
        slug
    };
    if slug.len() > 64 {
        slug[..64].to_string()
    } else {
        slug
    }
}

/// Compute the cloud-side file_name that `create_cloud_text_file` writes for a
/// given title + file_id + extension. Centralized so callers (the save_items
/// handler and tests) can reference the same naming without duplicating the
/// slug derivation.
pub fn cloud_file_name(file_id: &str, title: &str, extension: &str) -> String {
    let slug = title_to_slug(title);
    let suffix: String = file_id.chars().take(8).collect();
    format!("{slug}-{suffix}.{extension}")
}

/// Create a cloud file entry using existing project file + CRDT storage path.
pub async fn create_cloud_text_file(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    file_id: &str,
    title: &str,
    extension: &str,
    content: &str,
) -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    let file_name = cloud_file_name(file_id, title, extension);
    let parent_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM files
         WHERE project_id = ?
           AND parent_id IS NULL
           AND name = 'Research'
           AND type = 'folder'
           AND zone = 'research'
         LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;

    sqlx::query(
        "INSERT INTO files (id, project_id, parent_id, name, type, zone, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'file', 'research', ?, ?)",
    )
    .bind(file_id)
    .bind(project_id)
    .bind(&parent_id)
    .bind(&file_name)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    let ydoc = yrs::Doc::new();
    let text = ydoc.get_or_insert_text("content");
    {
        let mut txn = ydoc.transact_mut();
        text.insert(&mut txn, 0, content);
    }
    let state = {
        let txn = ydoc.transact();
        txn.encode_state_as_update_v1(&yrs::StateVector::default())
    };

    sqlx::query("INSERT INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)")
        .bind(file_id)
        .bind(&state)
        .bind(now)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn create_cloud_binary_file(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    file_id: &str,
    file_name: &str,
    mime_type: &str,
    content: &[u8],
) -> Result<(), AppError> {
    let now = Utc::now().timestamp();
    let suffix = file_id.chars().take(8).collect::<String>();
    let file_stem = file_name
        .trim_end_matches(".pdf")
        .trim_end_matches(".PDF")
        .trim_matches('_');
    let file_name = if file_stem.is_empty() {
        format!("research-paper-{suffix}.pdf")
    } else {
        format!("{file_stem}-{suffix}.pdf")
    };
    let parent_id: Option<String> = sqlx::query_scalar(
        "SELECT id FROM files
         WHERE project_id = ?
           AND parent_id IS NULL
           AND name = 'Research'
           AND type = 'folder'
           AND zone = 'research'
         LIMIT 1",
    )
    .bind(project_id)
    .fetch_optional(pool)
    .await?;

    sqlx::query(
        "INSERT INTO files (id, project_id, parent_id, name, type, mime_type, size, zone, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'file', ?, ?, 'research', ?, ?)",
    )
    .bind(file_id)
    .bind(project_id)
    .bind(&parent_id)
    .bind(&file_name)
    .bind(mime_type)
    .bind(content.len() as i64)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    sqlx::query("INSERT INTO file_blobs (file_id, content) VALUES (?, ?)")
        .bind(file_id)
        .bind(content)
        .execute(pool)
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_md_includes_phase9_ai_sections() {
        let input = SaveItemInput {
            title: "Bayesian SIR".to_string(),
            url: "https://example.com/paper".to_string(),
            content: "body".to_string(),
            category: "literature".to_string(),
            summary: Some("summary".to_string()),
            authors: Some("A. Author".to_string()),
            publish_year: Some(2026),
            keywords: Some("MCMC,SIR".to_string()),
            methodology: Some("Bayesian inference".to_string()),
            key_parameters: Some("{\"beta\":0.3}".to_string()),
            ai_relevance: Some("Useful for parameter estimation".to_string()),
            relevance_score: Some(0.9),
            bibtex: Some("@article{bayesian_sir,title={Bayesian SIR}}".to_string()),
            raw_json: None,
        };

        let md = render_md(&input);

        assert!(md.contains("## AI Summary"));
        assert!(md.contains("## Methodology"));
        assert!(md.contains("Bayesian inference"));
        assert!(md.contains("## Key Parameters"));
        assert!(md.contains("## Relevance to Project"));
        assert!(md.contains("## BibTeX"));
    }

    #[test]
    fn cloud_file_name_matches_create_cloud_text_file() {
        // The cloud-side file_name is consumed by the host agent to mirror
        // files locally; it must match what create_cloud_text_file writes.
        let name = cloud_file_name("7f3a8c12-aaaa-bbbb-cccc-111122223333", "Bayesian SIR", "md");
        assert_eq!(name, "bayesian_sir-7f3a8c12.md");

        // Empty title falls back to the placeholder slug.
        let name = cloud_file_name("abcdef01-xxxx", "!!!", "bib");
        assert_eq!(name, "research-item-abcdef01.bib");

        // Long titles truncate to 64-char slug + suffix + extension.
        let long_title = "a".repeat(100);
        let name = cloud_file_name("12345678", &long_title, "md");
        assert_eq!(name.len(), 64 + 1 + 8 + 1 + 2);
        assert!(name.starts_with(&"a".repeat(64)));
        assert!(name.ends_with("-12345678.md"));
    }

    #[test]
    fn pdf_attachment_metadata_reads_raw_json() {
        let input = SaveItemInput {
            title: "Traffic Flow Paper".to_string(),
            url: "https://example.edu/paper".to_string(),
            content: "body".to_string(),
            category: "literature".to_string(),
            summary: None,
            authors: None,
            publish_year: None,
            keywords: None,
            methodology: None,
            key_parameters: None,
            ai_relevance: None,
            relevance_score: None,
            bibtex: None,
            raw_json: Some(serde_json::json!({
                "pdf_url": "https://example.edu/paper.pdf",
                "attachment_filename": "Traffic Flow Paper.pdf"
            })),
        };

        let attachment = pdf_attachment_from_input(&input).unwrap();

        assert_eq!(attachment.url, "https://example.edu/paper.pdf");
        assert_eq!(attachment.filename, "Traffic_Flow_Paper.pdf");
    }
}
