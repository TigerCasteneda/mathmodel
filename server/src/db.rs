use sqlx::sqlite::SqlitePool;
use std::path::Path;

pub async fn init_pool(database_url: &str) -> SqlitePool {
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
    run_specific_migration(pool, include_str!("../migrations/001_initial.sql")).await;
    run_specific_migration(pool, include_str!("../migrations/002_ai.sql")).await;
    run_specific_migration(pool, include_str!("../migrations/003_history.sql")).await;
    run_specific_migration(pool, include_str!("../migrations/004_research.sql")).await;
    run_005_migration(pool).await;
}

pub async fn run_specific_migration(pool: &SqlitePool, sql: &str) {
    for statement in sql.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sqlx::query(statement)
            .execute(pool)
            .await
            .expect("Failed to run migration");
    }
}

/// Idempotent column addition: checks PRAGMA table_info before ALTER TABLE.
/// Wraps the check and ALTER in a transaction to prevent races.
/// Callers must only pass trusted, alphanumeric identifiers for `table` and `column`.
pub async fn ensure_column(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    type_sql: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2"
    )
    .bind(table)
    .bind(column)
    .fetch_one(&mut *tx)
    .await?;

    if count == 0 {
        let sql = format!("ALTER TABLE \"{table}\" ADD COLUMN \"{column}\" {type_sql}");
        sqlx::query(&sql).execute(&mut *tx).await?;
        tracing::info!("Migration: added column {table}.{column} {type_sql}");
    }

    tx.commit().await?;

    Ok(())
}

/// 005: extend research_items table with Morphic search columns.
async fn run_005_migration(pool: &SqlitePool) {
    ensure_column(pool, "research_items", "category", "TEXT DEFAULT 'literature'")
        .await
        .expect("005: category");
    ensure_column(pool, "research_items", "authors", "TEXT DEFAULT ''")
        .await
        .expect("005: authors");
    ensure_column(pool, "research_items", "publish_year", "INTEGER")
        .await
        .expect("005: publish_year");
    ensure_column(pool, "research_items", "keywords", "TEXT DEFAULT ''")
        .await
        .expect("005: keywords");
    ensure_column(pool, "research_items", "relevance_score", "REAL DEFAULT 0.0")
        .await
        .expect("005: relevance_score");
    ensure_column(pool, "research_items", "updated_at", "INTEGER NOT NULL DEFAULT 0")
        .await
        .expect("005: updated_at");
    ensure_column(pool, "research_items", "cloud_file_id", "TEXT")
        .await
        .expect("005: cloud_file_id");
}
