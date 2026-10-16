//! Meta Ads ingestion runner.
//!
//! Mirrors the Quicket/Instagram ingests. Pulls boosted-post campaigns and their
//! per-day performance from the Marketing API → `campaigns` + `campaign_metrics`.
//! The System User token stays server-side.
//!
//! Usage:
//!   cargo run --bin ingest_metaads             # full sync (schedulable)
//!   cargo run --bin ingest_metaads -- --reset  # clear meta_ads rows first
//!
//! Campaign/metric ids are deterministic UUIDv5 from the Meta ids, so re-running
//! is idempotent (refreshes spend/metrics in place).
//!
//! Money: insights `spend` is a decimal in ACCOUNT CURRENCY (ZAR) → ×100 = cents.
//! Budget fields are already integer MINOR units (cents) → stored as-is.

use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use sankayi_intelligence::db;
use sankayi_intelligence::metaads::{action_value, num_str, MetaAdsClient};

const NS: Uuid = Uuid::from_u128(0x53414e4b_4159_495a_5f5f_515549434b45_u128);

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let reset = std::env::args().any(|a| a == "--reset");

    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let pool = db::connect(&db_url).await.context("connecting to PostgreSQL")?;

    if reset {
        log::warn!("--reset: clearing meta_ads campaigns and their metrics");
        // campaign_metrics FK-cascades from campaigns, but be explicit.
        sqlx::query(
            "DELETE FROM campaign_metrics WHERE campaign_id IN (SELECT id FROM campaigns WHERE source = 'meta_ads'::data_source)",
        )
        .execute(&pool)
        .await?;
        sqlx::query("DELETE FROM campaigns WHERE source = 'meta_ads'::data_source")
            .execute(&pool)
            .await?;
    }

    // Only ingest 2026+ activity — the Jazz Series started May 2026, so older
    // boosts (2020–2022) would skew the spend totals. Override with META_ADS_START_DATE.
    let cutoff = std::env::var("META_ADS_START_DATE")
        .ok()
        .and_then(|s| NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
        .unwrap_or_else(|| NaiveDate::from_ymd_opt(2026, 1, 1).unwrap());

    let started = Utc::now();
    // Record any failure (e.g. Meta blocking API access) so Data Health flags it.
    let result: Result<(i64, i64)> = async {
        let client = MetaAdsClient::from_env()?;
        log::info!("Meta Ads ingest: ad account {} (from {cutoff})", client.ad_account_id());
        sync(&pool, &client, cutoff).await
    }
    .await;
    let (campaigns, metric_rows) = match result {
        Ok(v) => v,
        Err(e) => {
            db::log_ingest_failure(&pool, "meta_ads", started, &e).await;
            return Err(e);
        }
    };

    let msg = format!("{campaigns} campaigns, {metric_rows} daily metric rows");
    sqlx::query(
        "INSERT INTO ingestion_log (source, started_at, finished_at, status, rows_ingested, message)
         VALUES ('meta_ads'::data_source, $1, now(), 'success', $2, $3)",
    )
    .bind(started)
    .bind(campaigns as i32)
    .bind(&msg)
    .execute(&pool)
    .await?;

    println!("✅ Meta Ads: {msg}");
    Ok(())
}

async fn sync(pool: &PgPool, client: &MetaAdsClient, cutoff: NaiveDate) -> Result<(i64, i64)> {
    let mut after: Option<String> = None;
    let mut n_campaigns = 0i64;
    let mut n_metrics = 0i64;

    loop {
        let page = client.campaigns(after.as_deref()).await.context("fetching campaigns")?;
        if page.data.is_empty() {
            break;
        }

        for c in &page.data {
            let Some(cid) = c.get("id").and_then(|x| x.as_str()) else { continue };
            let name = c.get("name").and_then(|x| x.as_str()).unwrap_or("Untitled boost").to_string();
            let budget_cents = c
                .get("lifetime_budget")
                .or_else(|| c.get("daily_budget"))
                .and_then(|x| x.as_str())
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|b| *b > 0);

            let start_date = parse_date(c.get("start_time").and_then(|x| x.as_str()));
            let end_date = parse_date(c.get("stop_time").and_then(|x| x.as_str()));

            // Per-day insights → campaign_metrics, accumulating the totals.
            let ins = match client.campaign_daily_insights(cid).await {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("insights for campaign {cid}: {e}");
                    Value::Null
                }
            };
            let all_days = ins.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
            // Keep only days on/after the cutoff (clips campaigns spanning the boundary).
            let days: Vec<Value> = all_days
                .into_iter()
                .filter(|d| {
                    parse_date(d.get("date_start").and_then(|x| x.as_str()))
                        .map_or(false, |dt| dt >= cutoff)
                })
                .collect();

            // Skip campaigns with no in-range activity that also started earlier —
            // i.e. the pre-Jazz-Series (2020–2022) boosts.
            let starts_in_range = start_date.map_or(false, |d| d >= cutoff);
            if days.is_empty() && !starts_in_range {
                log::info!("skip pre-{cutoff} campaign {cid}");
                continue;
            }

            // Link to the boosted post (best-effort — some old boosts lack one).
            let permalink = client.campaign_permalink(cid).await.unwrap_or(None);

            let uuid = campaign_uuid(cid);
            let mut total_spend_cents = 0i64;
            let mut min_date: Option<NaiveDate> = None;
            let mut max_date: Option<NaiveDate> = None;
            sqlx::query(
                r#"
                INSERT INTO campaigns
                    (id, event_id, campaign_name, platform, budget_cents, spend_cents,
                     start_date, end_date, permalink, source, source_id, retrieved_at, raw_payload)
                VALUES ($1, NULL, $2, 'instagram'::campaign_platform, $3, 0,
                        $4, $5, $6, 'meta_ads'::data_source, $7, now(), $8)
                ON CONFLICT (id) DO UPDATE SET
                    campaign_name = EXCLUDED.campaign_name,
                    budget_cents = EXCLUDED.budget_cents,
                    start_date = EXCLUDED.start_date,
                    end_date = EXCLUDED.end_date,
                    permalink = EXCLUDED.permalink,
                    retrieved_at = now(),
                    raw_payload = EXCLUDED.raw_payload,
                    updated_at = now()
                "#,
            )
            .bind(uuid)
            .bind(&name)
            .bind(budget_cents)
            .bind(start_date)
            .bind(end_date)
            .bind(&permalink)
            .bind(cid)
            .bind(sqlx::types::Json(c))
            .execute(pool)
            .await
            .with_context(|| format!("upsert campaign {cid}"))?;

            for d in &days {
                let date = match parse_date(d.get("date_start").and_then(|x| x.as_str())) {
                    Some(dt) => dt,
                    None => continue,
                };
                let spend_cents = (num_str(d, "spend").unwrap_or(0.0) * 100.0).round() as i64;
                total_spend_cents += spend_cents;
                min_date = Some(min_date.map_or(date, |m| m.min(date)));
                max_date = Some(max_date.map_or(date, |m| m.max(date)));

                let reach = as_i32(num_str(d, "reach"));
                let impressions = as_i32(num_str(d, "impressions"));
                let clicks = as_i32(num_str(d, "clicks"));
                let engagements = action_value(d, "post_engagement").map(|x| x as i32);

                sqlx::query(
                    r#"
                    INSERT INTO campaign_metrics
                        (id, campaign_id, metric_date, reach, impressions, clicks, engagements, spend_cents, retrieved_at, raw_payload)
                    VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now(), $9)
                    ON CONFLICT (campaign_id, metric_date) DO UPDATE SET
                        reach = EXCLUDED.reach,
                        impressions = EXCLUDED.impressions,
                        clicks = EXCLUDED.clicks,
                        engagements = EXCLUDED.engagements,
                        spend_cents = EXCLUDED.spend_cents,
                        retrieved_at = now(),
                        raw_payload = EXCLUDED.raw_payload
                    "#,
                )
                .bind(metric_uuid(cid, date))
                .bind(uuid)
                .bind(date)
                .bind(reach)
                .bind(impressions)
                .bind(clicks)
                .bind(engagements)
                .bind(spend_cents)
                .bind(sqlx::types::Json(d))
                .execute(pool)
                .await
                .with_context(|| format!("upsert campaign_metric {cid} {date}"))?;
                n_metrics += 1;
            }

            // Patch the campaign's total spend + tighten dates to observed range.
            sqlx::query(
                "UPDATE campaigns SET spend_cents = $1,
                    start_date = COALESCE($2, start_date),
                    end_date = COALESCE($3, end_date)
                 WHERE id = $4",
            )
            .bind(total_spend_cents)
            .bind(min_date)
            .bind(max_date)
            .bind(uuid)
            .execute(pool)
            .await?;

            n_campaigns += 1;
            log::info!("campaign {cid} '{name}' — R{:.2} over {} days", total_spend_cents as f64 / 100.0, days.len());
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        }

        after = page.paging.cursors.after.clone();
        if after.is_none() {
            break;
        }
    }

    Ok((n_campaigns, n_metrics))
}

// ---- helpers ----
fn campaign_uuid(id: &str) -> Uuid {
    Uuid::new_v5(&NS, format!("metaads:campaign:{id}").as_bytes())
}
fn metric_uuid(id: &str, date: NaiveDate) -> Uuid {
    Uuid::new_v5(&NS, format!("metaads:campaign:{id}:{date}").as_bytes())
}
fn as_i32(v: Option<f64>) -> Option<i32> {
    v.map(|x| x as i32)
}
/// Parse Meta timestamps ("2026-08-17T00:16:04-0700") or dates ("2026-08-17").
fn parse_date(s: Option<&str>) -> Option<NaiveDate> {
    let x = s?;
    if let Ok(d) = DateTime::parse_from_rfc3339(x) {
        return Some(d.date_naive());
    }
    if let Ok(d) = DateTime::parse_from_str(x, "%Y-%m-%dT%H:%M:%S%z") {
        return Some(d.date_naive());
    }
    NaiveDate::parse_from_str(&x[..x.len().min(10)], "%Y-%m-%d").ok()
}
