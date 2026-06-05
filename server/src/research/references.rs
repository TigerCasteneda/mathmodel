use crate::error::AppError;
use chrono::Utc;
use yrs::ReadTxn;
use yrs::Text;
use yrs::Transact;

use super::model::SaveItemInput;

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
    let slug = title_to_slug(title);
    let suffix = file_id.chars().take(8).collect::<String>();
    let file_name = format!("{slug}-{suffix}.{extension}");
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
}
