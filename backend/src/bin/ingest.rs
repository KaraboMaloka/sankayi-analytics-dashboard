//! Quicket ingestion runner.
//!
//! Two responsibilities, deliberately split by what auth they need:
//!
//!   discover  — full event list + real capacities from Quicket's INTERNAL API.
//!               Needs a short-lived Bearer JWT (QUICKET_BEARER) captured from a
//!               logged-in browser session. Upserts rows into `events`. Run this
//!               when shows are added/changed. Picks up new events automatically.
//!
//!   sales     — ticket sales for the events already in the DB, via the PUBLIC
//!               API (stable QUICKET_API_KEY + QUICKET_USER_TOKEN). No browser
//!               token needed, so this is safe to SCHEDULE (e.g. nightly).
//!
//! Usage:
//!   cargo run --bin ingest -- refresh          # discover (needs QUICKET_BEARER) THEN sales
//!   cargo run --bin ingest -- sales            # sales only (schedulable)
//!   cargo run --bin ingest -- sales --reset    # clear events+sales first
//!   cargo run --bin ingest -- set-capacities   # apply QUICKET_CAPACITIES only (no API)
//!
//! Event & ticket row ids are deterministic UUIDv5 from the Quicket ids, so
//! every run is idempotent.

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, NaiveDate, NaiveDateTime, Utc};
use serde_json::{Map, Value};
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

use sankayi_intelligence::db;
use sankayi_intelligence::quicket::{resolve_capacity, v_f64, v_i64, v_str, QuicketClient};

const NS: Uuid = Uuid::from_u128(0x53414e4b_4159_495a_5f5f_515549434b45_u128);
const SAST_OFFSET_SECS: i32 = 2 * 3600;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let args: Vec<String> = std::env::args().collect();
    let subcommand = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| "sales".to_string());
    let reset = args.iter().any(|a| a == "--reset");

    let organiser_filter: Option<i64> =
        match std::env::var("QUICKET_ORGANISER_ID").ok().map(|s| s.trim().to_lowercase()) {
            None => Some(91748),
            Some(s) if s.is_empty() || s == "all" || s == "0" || s == "none" => None,
            Some(s) => s.parse().ok().or(Some(91748)),
        };
    // Only ingest events on/after this date (the Jazz Series start).
    let start_date = std::env::var("QUICKET_START_DATE").unwrap_or_else(|_| "2026-05-08".to_string());
    let default_capacity: i32 = std::env::var("QUICKET_DEFAULT_CAPACITY")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(220);
    let capacity_overrides = parse_capacities(std::env::var("QUICKET_CAPACITIES").ok());

    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let pool = db::connect(&db_url).await.context("connecting to PostgreSQL")?;

    // Capacity-only mode: apply QUICKET_CAPACITIES without any Quicket API calls.
    if subcommand == "set-capacities" {
        let mut updated = 0u64;
        for (eid, cap) in &capacity_overrides {
            let res = sqlx::query("UPDATE events SET capacity = $1, updated_at = now() WHERE id = $2")
                .bind(*cap)
                .bind(event_uuid(*eid))
                .execute(&pool)
                .await?;
            updated += res.rows_affected();
        }
        println!("✅ Applied {updated} capacities from {} overrides (no API calls).", capacity_overrides.len());
        return Ok(());
    }

    if reset {
        log::warn!("--reset: clearing existing events and ticket_sales");
        sqlx::query("DELETE FROM ticket_sales").execute(&pool).await?;
        sqlx::query("DELETE FROM events").execute(&pool).await?;
    }

    let started = Utc::now();
    // Record any failure (auth/network) so it surfaces on Data Health, not silently.
    let result: Result<(i64, i64)> = async {
        let client = QuicketClient::from_env()?;
        match subcommand.as_str() {
            "refresh" => {
                let bearer = std::env::var("QUICKET_BEARER").context(
                    "refresh needs QUICKET_BEARER (a browser Bearer JWT). Grab it from a logged-in \
                     Quicket session, or run 'sales' to sync sales for already-discovered events.",
                )?;
                let discovered = discover(&pool, &client, &bearer, organiser_filter, &start_date, &capacity_overrides, default_capacity).await?;
                log::info!("discover: upserted {discovered} events");
                let t = sales_sync(&pool, &client).await?;
                Ok((discovered as i64, t))
            }
            "sales" => {
                let t = sales_sync(&pool, &client).await?;
                let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM events WHERE source = 'quicket'::data_source")
                    .fetch_one(&pool).await.unwrap_or(0);
                Ok((n, t))
            }
            other => anyhow::bail!("unknown subcommand '{other}' (use refresh | sales | set-capacities)"),
        }
    }
    .await;
    let (events_touched, tickets) = match result {
        Ok(v) => v,
        Err(e) => {
            db::log_ingest_failure(&pool, "quicket", started, &e).await;
            return Err(e);
        }
    };

    let msg = format!("{subcommand}: {events_touched} events, {tickets} tickets");
    sqlx::query(
        "INSERT INTO ingestion_log (source, started_at, finished_at, status, rows_ingested, message)
         VALUES ('quicket'::data_source, $1, now(), 'success', $2, $3)",
    )
    .bind(started)
    .bind(tickets as i32)
    .bind(&msg)
    .execute(&pool)
    .await?;

    println!("✅ Quicket {msg}");
    Ok(())
}

// =============================================================================
// discover — internal API → events (+ real capacities)
// =============================================================================
async fn discover(
    pool: &PgPool,
    client: &QuicketClient,
    bearer: &str,
    organiser_filter: Option<i64>,
    start_date: &str,
    overrides: &HashMap<i64, i32>,
    default_capacity: i32,
) -> Result<usize> {
    let user_id = client.me().await.context("resolving user id")?;
    let products = client.internal_products(user_id, bearer).await.context("fetching event list")?;

    let mut count = 0;
    for p in &products.user_products {
        if let Some(f) = organiser_filter {
            if p.organiser_id != Some(f) {
                continue;
            }
        }
        let date_str = p.date.clone().unwrap_or_default();
        if date_str.as_str() < start_date {
            continue;
        }

        // Capacity: manual override wins; else fetch from the dashboard; else default.
        let capacity = if let Some(c) = overrides.get(&p.id) {
            *c
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            match client.internal_dashboard(p.id, bearer).await {
                Ok(d) => resolve_capacity(&d).unwrap_or(default_capacity),
                Err(e) => {
                    log::warn!("capacity for {} unavailable: {e}", p.id);
                    default_capacity
                }
            }
        };

        let uuid = event_uuid(p.id);
        let event_date = parse_date(p.date.as_deref());
        let status = if parse_dt(p.date.as_deref()) < Utc::now() { "completed" } else { "on_sale" };

        sqlx::query(
            r#"
            INSERT INTO events (id, artist_id, title, event_date, venue, capacity, ticket_price_cents,
                                status, source, source_id, retrieved_at)
            VALUES ($1, NULL, $2, $3, 'Sankayi Restaurant and Lounge', $4, 0,
                    $5::event_status, 'quicket'::data_source, $6, now())
            ON CONFLICT (id) DO UPDATE SET
                title = EXCLUDED.title,
                event_date = EXCLUDED.event_date,
                capacity = EXCLUDED.capacity,
                status = EXCLUDED.status,
                source = EXCLUDED.source,
                source_id = EXCLUDED.source_id,
                retrieved_at = now(),
                updated_at = now()
            "#,
        )
        .bind(uuid)
        .bind(&p.name)
        .bind(event_date)
        .bind(capacity)
        .bind(status)
        .bind(p.id.to_string())
        .execute(pool)
        .await
        .with_context(|| format!("upsert event {}", p.id))?;
        count += 1;
        log::info!("discovered event {} '{}' — capacity {capacity}", p.id, p.name);
    }
    Ok(count)
}

// =============================================================================
// sales_sync — public API → ticket_sales for events already in the DB
// =============================================================================
async fn sales_sync(pool: &PgPool, client: &QuicketClient) -> Result<i64> {
    let source_ids: Vec<String> = sqlx::query_scalar(
        "SELECT source_id FROM events WHERE source = 'quicket'::data_source AND source_id IS NOT NULL ORDER BY event_date",
    )
    .fetch_all(pool)
    .await?;

    if source_ids.is_empty() {
        log::warn!("no discovered Quicket events in DB — run 'refresh' first");
        return Ok(0);
    }

    let mut total = 0i64;
    for sid in &source_ids {
        let Ok(eid) = sid.parse::<i64>() else { continue };
        let uuid = event_uuid(eid);
        let mut gpage = 1;
        let mut event_tickets = 0i64;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            let gp = match client.guests(eid, gpage, 200).await {
                Ok(g) => g,
                Err(e) => {
                    log::warn!("guests for event {eid} page {gpage}: {e}");
                    break;
                }
            };
            if gp.results.is_empty() {
                break;
            }
            for g in &gp.results {
                let Some(tid) = v_i64(g, "TicketId").or_else(|| v_i64(g, "Barcode")) else { continue };
                let ticket_type = v_str(g, "TicketType").unwrap_or_else(|| "General".to_string());
                let is_comp = v_str(g, "Complimentary").map(|s| s.eq_ignore_ascii_case("yes")).unwrap_or(false);
                let price = (v_f64(g, "Price").unwrap_or(0.0) - v_f64(g, "DiscountAmount").unwrap_or(0.0)).max(0.0);
                let price_cents = if is_comp { 0 } else { (price * 100.0).round() as i64 };
                let sold_at = parse_dt(v_str(g, "DateAdded").as_deref());
                let sale_source = if is_comp { "complimentary" } else { "quicket" };

                sqlx::query(
                    r#"
                    INSERT INTO ticket_sales
                        (id, event_id, ticket_type, quantity, price_cents, sold_at,
                         sale_source, attribution, source, source_id, retrieved_at, raw_payload)
                    VALUES ($1, $2, $3, 1, $4, $5, $6::sale_source, 'unknown'::attribution_kind,
                            'quicket'::data_source, $7, now(), $8)
                    ON CONFLICT (id) DO UPDATE SET
                        ticket_type = EXCLUDED.ticket_type,
                        price_cents = EXCLUDED.price_cents,
                        sold_at = EXCLUDED.sold_at,
                        sale_source = EXCLUDED.sale_source,
                        retrieved_at = now(),
                        raw_payload = EXCLUDED.raw_payload
                    "#,
                )
                .bind(ticket_uuid(tid))
                .bind(uuid)
                .bind(&ticket_type)
                .bind(price_cents)
                .bind(sold_at)
                .bind(sale_source)
                .bind(format!("ticket:{tid}"))
                .bind(sqlx::types::Json(sanitize_guest(g)))
                .execute(pool)
                .await
                .with_context(|| format!("upsert ticket {tid}"))?;
                event_tickets += 1;
            }
            if gpage >= gp.pages || gpage >= 50 {
                break;
            }
            gpage += 1;
        }
        // Set the event's face price from the highest observed ticket price.
        sqlx::query(
            "UPDATE events SET ticket_price_cents = COALESCE((SELECT MAX(price_cents) FROM ticket_sales WHERE event_id = $1), ticket_price_cents) WHERE id = $1",
        )
        .bind(uuid)
        .execute(pool)
        .await?;
        total += event_tickets;
        log::info!("sales: event {eid} — {event_tickets} tickets");
    }
    Ok(total)
}

// ---- helpers ----
fn event_uuid(quicket_id: i64) -> Uuid {
    Uuid::new_v5(&NS, format!("quicket:event:{quicket_id}").as_bytes())
}
fn ticket_uuid(quicket_ticket_id: i64) -> Uuid {
    Uuid::new_v5(&NS, format!("quicket:ticket:{quicket_ticket_id}").as_bytes())
}

fn parse_capacities(raw: Option<String>) -> HashMap<i64, i32> {
    raw.map(|s| {
        s.split(',')
            .filter_map(|kv| {
                let mut it = kv.split(':');
                Some((it.next()?.trim().parse().ok()?, it.next()?.trim().parse().ok()?))
            })
            .collect()
    })
    .unwrap_or_default()
}

fn sanitize_guest(g: &Value) -> Value {
    const SAFE: &[&str] = &[
        "OrderId", "TicketId", "Barcode", "DateAdded", "TicketType", "TicketTypeId",
        "SeatNumber", "DiscountCode", "DiscountAmount", "CheckedIn", "CheckinDate",
        "Complimentary", "Price", "AmountPaid", "Valid", "Frozen",
    ];
    let mut out = Map::new();
    for key in SAFE {
        if let Some(v) = g.get(*key) {
            out.insert((*key).to_string(), v.clone());
        }
    }
    Value::Object(out)
}

fn parse_date(s: Option<&str>) -> NaiveDate {
    s.and_then(|x| {
        DateTime::parse_from_rfc3339(x)
            .ok()
            .map(|d| d.date_naive())
            .or_else(|| NaiveDateTime::parse_from_str(x, "%Y-%m-%dT%H:%M:%S%.f").ok().map(|d| d.date()))
            .or_else(|| NaiveDate::parse_from_str(&x[..x.len().min(10)], "%Y-%m-%d").ok())
    })
    .unwrap_or_else(|| Utc::now().date_naive())
}

fn parse_dt(s: Option<&str>) -> DateTime<Utc> {
    let Some(x) = s else { return Utc::now() };
    if let Ok(d) = DateTime::parse_from_rfc3339(x) {
        return d.with_timezone(&Utc);
    }
    let off = FixedOffset::east_opt(SAST_OFFSET_SECS).unwrap();
    for fmt in ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(nd) = NaiveDateTime::parse_from_str(x, fmt) {
            if let Some(d) = nd.and_local_timezone(off).single() {
                return d.with_timezone(&Utc);
            }
        }
    }
    Utc::now()
}
