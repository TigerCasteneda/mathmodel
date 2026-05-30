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
}

pub async fn run_specific_migration(pool: &SqlitePool, sql: &str) {
    for statement in sql.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sqlx::query(statement)
            .execute(pool)
            .await
            .expect("Failed to run migration");
    }
}
