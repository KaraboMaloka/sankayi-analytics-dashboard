//! Database connection pool.

use chrono::{DateTime, Utc};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::Duration;

/// Build a Postgres connection pool from `DATABASE_URL`.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await
}

/// Record a FAILED ingestion run so the Data Health page flags it immediately
/// (red dot + "Attention required") instead of silently showing the last success.
/// Best-effort: a logging failure must never mask the original error.
pub async fn log_ingest_failure(
    pool: &PgPool,
    source: &str,
    started: DateTime<Utc>,
    err: &anyhow::Error,
) {
    // Scrub any credential (e.g. an access_token embedded in a failed request
    // URL) before it reaches the DB and the Data Health page.
    let msg: String = crate::redact::scrub_secrets(&format!("{err:#}"))
        .chars()
        .take(300)
        .collect();
    let res = sqlx::query(
        "INSERT INTO ingestion_log (source, started_at, finished_at, status, rows_ingested, message)
         VALUES ($1::data_source, $2, now(), 'failed', 0, $3)",
    )
    .bind(source)
    .bind(started)
    .bind(&msg)
    .execute(pool)
    .await;
    if let Err(e) = res {
        log::error!("could not record ingest failure for {source}: {e}");
    }
}
