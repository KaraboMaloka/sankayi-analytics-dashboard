//! Marketing attribution — links Quicket ticket sales to the Meta Ads campaign
//! that most plausibly drove them, so ROAS / attributed-revenue become real.
//!
//! ⚠️ This is a MODEL, not tracked truth. Quicket exposes no per-sale traffic
//! source (no UTM, no referrer, no promo codes), so attribution is inferred from
//! TIMING + which show a campaign promoted. Attributed sales are marked
//! 'influenced' — never 'direct' — because a click is not proven.
//!
//! Model (two stages):
//!   1. Map each campaign to a specific EVENT when its name contains that show's
//!      artist (e.g. "…welcome Rorisang…" -> Rorisang Sechele). Campaigns with no
//!      recognisable artist stay series-wide (event_id NULL).
//!   2. For each paid ticket (event E, sale date D), credit the best campaign
//!      active on D that is EITHER specific to E or series-wide — event-specific
//!      wins over series-wide, then the --model tie-break (default: most clicks).
//!      This stops a "Kyle Shepherd" ad from being credited a McCoy ticket.
//!
//! Idempotent: resets and recomputes every run. Run after Quicket + Meta Ads.
//!
//! Usage:
//!   cargo run --bin ingest_attribution                     # click-weighted
//!   cargo run --bin ingest_attribution -- --model recent   # last-touch
//!   cargo run --bin ingest_attribution -- --model spend    # biggest spender

use anyhow::{Context, Result};
use chrono::NaiveDate;
use sqlx::FromRow;
use uuid::Uuid;

use sankayi_intelligence::db;

#[derive(FromRow)]
struct Ev {
    id: Uuid,
    title: String,
    event_date: NaiveDate,
}

#[derive(FromRow)]
struct Camp {
    id: Uuid,
    campaign_name: String,
    start_date: Option<NaiveDate>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let args: Vec<String> = std::env::args().collect();
    let model = args
        .windows(2)
        .find(|w| w[0] == "--model")
        .map(|w| w[1].as_str())
        .unwrap_or("clicks");

    let tiebreak = match model {
        "recent" => "c.start_date DESC",
        "spend" => "COALESCE(c.spend_cents,0) DESC, c.start_date DESC",
        "clicks" => {
            "COALESCE((SELECT m.clicks FROM campaign_metrics m \
                       WHERE m.campaign_id = c.id AND m.metric_date = ts.sold_at::date), 0) DESC, \
             COALESCE((SELECT SUM(m.clicks) FROM campaign_metrics m WHERE m.campaign_id = c.id), 0) DESC, \
             c.start_date DESC"
        }
        other => anyhow::bail!("unknown --model '{other}' (use clicks | recent | spend)"),
    };

    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let pool = db::connect(&db_url).await.context("connecting to PostgreSQL")?;
    log::info!("attribution model: {model}");

    // 1. Tag every Meta Ads campaign (stable tag the sales will point at).
    sqlx::query(
        "UPDATE campaigns SET campaign_tag = 'meta:' || source_id
         WHERE source = 'meta_ads'::data_source AND source_id IS NOT NULL
           AND (campaign_tag IS NULL OR campaign_tag = '')",
    )
    .execute(&pool)
    .await?;

    // 2. Map campaigns -> events by artist name (recomputed fresh each run).
    sqlx::query("UPDATE campaigns SET event_id = NULL WHERE source = 'meta_ads'::data_source")
        .execute(&pool)
        .await?;
    let events: Vec<Ev> =
        sqlx::query_as("SELECT id, title, event_date FROM events WHERE source = 'quicket'::data_source")
            .fetch_all(&pool)
            .await?;
    let campaigns: Vec<Camp> = sqlx::query_as(
        "SELECT id, campaign_name, start_date FROM campaigns WHERE source = 'meta_ads'::data_source",
    )
    .fetch_all(&pool)
    .await?;

    let mut mapped = 0u64;
    for c in &campaigns {
        let name_l = c.campaign_name.to_lowercase();
        // Best event whose artist appears in the campaign name; when an artist has
        // several shows, prefer the soonest one on/after the campaign start.
        let mut best: Option<(Uuid, i64)> = None;
        for e in &events {
            let hit = artist_tokens(&e.title).iter().any(|tok| name_l.contains(tok));
            if hit {
                let score = date_score(c.start_date, e.event_date);
                if best.map_or(true, |(_, s)| score < s) {
                    best = Some((e.id, score));
                }
            }
        }
        if let Some((eid, _)) = best {
            sqlx::query("UPDATE campaigns SET event_id = $1 WHERE id = $2")
                .bind(eid)
                .bind(c.id)
                .execute(&pool)
                .await?;
            mapped += 1;
        }
    }
    log::info!("mapped {mapped}/{} campaigns to a specific event", campaigns.len());

    // 2b. Assign promo/tracking codes to campaigns from CAMPAIGN_TRACKING_CODES
    //     ("<campaignSourceId>:<CODE>,..."). These enable DETERMINISTIC tracking:
    //     a Quicket sale that used the code is proven to come from that boost.
    let code_map = parse_codes(std::env::var("CAMPAIGN_TRACKING_CODES").ok());
    // Env is the source of truth — clear then re-apply so removals take effect.
    sqlx::query("UPDATE campaigns SET tracking_code = NULL WHERE source = 'meta_ads'::data_source")
        .execute(&pool)
        .await?;
    for (source_id, code) in &code_map {
        sqlx::query(
            "UPDATE campaigns SET tracking_code = $1 WHERE source = 'meta_ads'::data_source AND source_id = $2",
        )
        .bind(code)
        .bind(source_id)
        .execute(&pool)
        .await?;
    }
    if !code_map.is_empty() {
        log::info!("assigned {} campaign tracking codes", code_map.len());
    }

    // 3. Reset prior attribution (idempotent).
    sqlx::query(
        "UPDATE ticket_sales SET attributed_campaign_tag = NULL, attribution = 'unknown'::attribution_kind
         WHERE source = 'quicket'::data_source",
    )
    .execute(&pool)
    .await?;

    // 3b. DETERMINISTIC pass — a sale whose Quicket DiscountCode matches a
    //     campaign's tracking_code is TRACKED (attribution = 'direct', proven).
    let tracked = sqlx::query(
        r#"
        UPDATE ticket_sales ts SET
            attributed_campaign_tag = c.campaign_tag,
            attribution = 'direct'::attribution_kind
        FROM campaigns c
        WHERE c.source = 'meta_ads'::data_source
          AND c.tracking_code IS NOT NULL AND c.tracking_code <> ''
          AND c.campaign_tag IS NOT NULL
          AND lower(ts.raw_payload->>'DiscountCode') = lower(c.tracking_code)
          AND ts.source = 'quicket'::data_source
        "#,
    )
    .execute(&pool)
    .await?
    .rows_affected();
    if tracked > 0 {
        log::info!("{tracked} tickets attributed DETERMINISTICALLY via promo code");
    }

    // 4. Attribute each PAID ticket. Event-specific campaigns only compete for
    //    their own event; series-wide (event_id NULL) compete for anything.
    let sql = format!(
        r#"
        UPDATE ticket_sales ts SET
            attributed_campaign_tag = (
                SELECT c.campaign_tag
                FROM campaigns c
                WHERE c.source = 'meta_ads'::data_source
                  AND c.campaign_tag IS NOT NULL
                  AND c.start_date IS NOT NULL AND c.end_date IS NOT NULL
                  AND ts.sold_at::date BETWEEN c.start_date AND c.end_date
                  AND (c.event_id = ts.event_id OR c.event_id IS NULL)
                ORDER BY (c.event_id = ts.event_id) DESC NULLS LAST, {tiebreak}
                LIMIT 1
            )
        WHERE ts.source = 'quicket'::data_source
          AND ts.sale_source = 'quicket'::sale_source
          AND ts.attributed_campaign_tag IS NULL  -- leave deterministically-tracked sales alone
        "#
    );
    sqlx::query(&sql).execute(&pool).await?;

    // Mark the MODELED sales 'influenced'; deterministic sales keep 'direct'.
    let attributed = sqlx::query(
        "UPDATE ticket_sales SET attribution = 'influenced'::attribution_kind
         WHERE source = 'quicket'::data_source
           AND attributed_campaign_tag IS NOT NULL
           AND attribution = 'unknown'::attribution_kind",
    )
    .execute(&pool)
    .await?
    .rows_affected();

    let (tickets, revenue_cents): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*)::bigint, COALESCE(SUM(quantity * price_cents),0)::bigint
         FROM ticket_sales WHERE attribution <> 'unknown'::attribution_kind",
    )
    .fetch_one(&pool)
    .await?;

    println!(
        "✅ Attribution ({model}): {mapped} event-mapped campaigns · {attributed} tickets attributed → {tickets} influenced, R{:.2} attributed revenue",
        revenue_cents as f64 / 100.0
    );
    Ok(())
}

/// Parse `CAMPAIGN_TRACKING_CODES` = "sourceId:CODE,sourceId2:CODE2" into pairs.
fn parse_codes(raw: Option<String>) -> Vec<(String, String)> {
    raw.map(|s| {
        s.split(',')
            .filter_map(|kv| {
                let mut it = kv.splitn(2, ':');
                let id = it.next()?.trim();
                let code = it.next()?.trim();
                (!id.is_empty() && !code.is_empty()).then(|| (id.to_string(), code.to_string()))
            })
            .collect()
    })
    .unwrap_or_default()
}

/// The distinctive artist portion of an event title, lowercased — everything
/// before the first structural delimiter (" - ", " with ", " x ", …).
fn artist_key(title: &str) -> String {
    let t = title.replace('–', "-").to_lowercase();
    let delims = [" - ", " with ", " x ", " & ", " plays ", " | ", " feat"];
    let mut cut = t.len();
    for d in delims {
        if let Some(i) = t.find(d) {
            cut = cut.min(i);
        }
    }
    t[..cut].trim().to_string()
}

/// Distinctive name tokens from an event title — words ≥5 chars, minus generic
/// venue/marketing words — so "…welcome Rorisang…" still matches Rorisang Sechele
/// without "jazz"/"series"/"music" causing false hits.
fn artist_tokens(title: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "sankayi", "series", "music", "jazz", "great", "hearts", "album", "launch",
        "night", "nights", "saturday", "sessions", "friday", "sunday", "welcome",
        "excited", "introducing", "restaurant", "lounge", "instagram", "promoting",
    ];
    artist_key(title)
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.len() >= 5 && !STOP.contains(&w.as_str()))
        .collect()
}

/// Rank an event for a campaign by how close its date is to the campaign — a
/// boost runs near its show, before or just after. Nearest wins (lower is
/// better); ties break toward the upcoming show.
fn date_score(start: Option<NaiveDate>, ev: NaiveDate) -> i64 {
    match start {
        Some(s) => {
            let d = (ev - s).num_days();
            d.abs() * 2 + if d < 0 { 1 } else { 0 } // tie-break: upcoming over past
        }
        None => (ev - NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()).num_days(),
    }
}
