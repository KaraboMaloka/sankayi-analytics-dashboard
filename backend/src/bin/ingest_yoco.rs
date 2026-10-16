//! Yoco door-sales ingestion runner.
//!
//! Pulls in-person card-machine ticket sales from Yoco's Orders API and writes
//! them into `ticket_sales` as `sale_source = 'door'`, mapped to the event on
//! that night (the venue runs one show per night). Ticket line items are matched
//! by product name (`YOCO_TICKET_PRODUCT`, default "Sankayi Jazz Series"), so
//! restaurant bar/food on the same Yoco account is ignored automatically.
//!
//! Usage:
//!   cargo run --bin ingest_yoco   # sync door sales for every event night in the DB
//!
//! Door tickets sell on the night, so rather than paging the restaurant's entire
//! order history (and tripping Yoco's rate limit), we fetch only a window around
//! each event night and keep the "Sankayi Jazz Series" line items.
//!
//! Idempotent: each ticket row's id is a deterministic UUIDv5 of the Yoco order
//! id, so re-runs upsert in place.

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, FixedOffset, NaiveDate, TimeZone, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use sankayi_intelligence::db;
use sankayi_intelligence::yoco::{is_in_person, money_cents, order_timestamp, ticket_line, YocoClient};

const NS: Uuid = Uuid::from_u128(0x53414e4b_4159_495a_5f5f_515549434b45_u128);
const SAST_OFFSET_SECS: i32 = 2 * 3600;
// Be a good citizen with Yoco's (unpublished) rate limit: pause between requests.
const REQUEST_DELAY_MS: u64 = 400;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let pool = db::connect(&db_url).await.context("connecting to PostgreSQL")?;

    let started = Utc::now();
    let result = run(&pool).await;

    let tickets = match result {
        Ok(t) => t,
        Err(e) => {
            db::log_ingest_failure(&pool, "yoco", started, &e).await;
            return Err(e);
        }
    };

    let msg = format!("door sales: {tickets} tickets");
    sqlx::query(
        "INSERT INTO ingestion_log (source, started_at, finished_at, status, rows_ingested, message)
         VALUES ('yoco'::data_source, $1, now(), 'success', $2, $3)",
    )
    .bind(started)
    .bind(tickets as i32)
    .bind(&msg)
    .execute(&pool)
    .await?;

    println!("✅ Yoco {msg}");
    Ok(())
}

async fn run(pool: &PgPool) -> Result<i64> {
    let client = YocoClient::from_env()?;
    log::info!("Yoco door ingest: matching product '{}'", client.ticket_product());

    // One window per event night — door tickets sell on the night.
    let events: Vec<(Uuid, NaiveDate)> =
        sqlx::query_as("SELECT id, event_date FROM events ORDER BY event_date")
            .fetch_all(pool)
            .await?;
    if events.is_empty() {
        log::warn!("no events in DB — run the Quicket ingest first so door sales can be mapped");
        return Ok(0);
    }

    let sast = FixedOffset::east_opt(SAST_OFFSET_SECS).unwrap();
    let product = client.ticket_product().to_string();
    let now = Utc::now();
    let mut total = 0i64;

    for (event_id, event_date) in &events {
        // Night window: event day 00:00 SAST → next day 06:00 SAST (late/after-midnight taps).
        let gte = sast.from_local_datetime(&event_date.and_hms_opt(0, 0, 0).unwrap()).unwrap();
        let lte = sast
            .from_local_datetime(&(*event_date + Duration::days(1)).and_hms_opt(6, 0, 0).unwrap())
            .unwrap();
        if gte.with_timezone(&Utc) > now {
            continue; // event is in the future — no door sales yet
        }

        let mut cursor: Option<String> = None;
        let mut event_tickets = 0i64;
        let mut orders_seen = 0usize;
        let mut names_seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(REQUEST_DELAY_MS)).await;
            let page = client
                .orders(&gte.to_rfc3339(), &lte.to_rfc3339(), cursor.as_deref())
                .await
                .with_context(|| format!("fetching orders for event night {event_date}"))?;

            for order in &page.data {
                orders_seen += 1;
                // Diagnostic: remember product names so a 0-match night is explainable.
                if let Some(items) = order.get("line_items").and_then(|v| v.as_array()) {
                    for it in items {
                        if let Some(n) = it.get("name").and_then(|v| v.as_str()) {
                            names_seen.insert(n.to_string());
                        }
                    }
                }
                if !is_in_person(order) {
                    continue; // online order carrying the product — not a door sale
                }
                let Some((qty, mut unit_cents)) = ticket_line(order, &product) else { continue };

                let ts = order_timestamp(order)
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|d| d.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now);

                // Fall back to the order's per-ticket gross if unit_price was absent.
                if unit_cents <= 0 {
                    let gross = money_cents(order.get("amounts").and_then(|a| a.get("gross_amount")))
                        .or_else(|| money_cents(order.get("total_amount")))
                        .unwrap_or(0);
                    if qty > 0 {
                        unit_cents = gross / qty;
                    }
                }

                sqlx::query(
                    r#"
                    INSERT INTO ticket_sales
                        (id, event_id, ticket_type, quantity, price_cents, sold_at,
                         sale_source, attribution, source, source_id, retrieved_at, raw_payload)
                    VALUES ($1, $2, 'Door', $3, $4, $5, 'door'::sale_source, 'unknown'::attribution_kind,
                            'yoco'::data_source, $6, now(), $7)
                    ON CONFLICT (id) DO UPDATE SET
                        event_id    = EXCLUDED.event_id,
                        quantity    = EXCLUDED.quantity,
                        price_cents = EXCLUDED.price_cents,
                        sold_at     = EXCLUDED.sold_at,
                        retrieved_at = now(),
                        raw_payload = EXCLUDED.raw_payload
                    "#,
                )
                .bind(order_uuid(order))
                .bind(event_id)
                .bind(qty as i32)
                .bind(unit_cents)
                .bind(ts)
                .bind(format!("order:{}", order_id(order)))
                .bind(sqlx::types::Json(sanitize_order(order)))
                .execute(pool)
                .await
                .with_context(|| format!("upsert door order {}", order_id(order)))?;

                event_tickets += qty;
            }

            match page.next_cursor {
                Some(c) if !page.data.is_empty() => cursor = Some(c),
                _ => break,
            }
        }
        if event_tickets > 0 {
            log::info!("door: event {event_date} → {event_tickets} ticket(s)");
        } else if orders_seen > 0 {
            let sample: Vec<&String> = names_seen.iter().take(20).collect();
            log::info!(
                "event {event_date}: {orders_seen} order(s), 0 matched '{product}'. Products seen: {sample:?}"
            );
        }
        total += event_tickets;
    }

    Ok(total)
}

fn order_id(order: &serde_json::Value) -> String {
    order.get("id").and_then(|v| v.as_str()).unwrap_or("unknown").to_string()
}

fn order_uuid(order: &serde_json::Value) -> Uuid {
    Uuid::new_v5(&NS, format!("yoco:order:{}", order_id(order)).as_bytes())
}

/// Keep only non-PII fields from the order for provenance.
fn sanitize_order(order: &serde_json::Value) -> serde_json::Value {
    use serde_json::{Map, Value};
    const SAFE: &[&str] = &["id", "created_at", "closed_at", "status", "amounts", "line_items", "payments"];
    let mut out = Map::new();
    for key in SAFE {
        if let Some(v) = order.get(*key) {
            out.insert((*key).to_string(), v.clone());
        }
    }
    Value::Object(out)
}
