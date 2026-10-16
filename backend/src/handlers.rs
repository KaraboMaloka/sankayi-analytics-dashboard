//! HTTP handlers. Each serves one endpoint from `docs/API_CONTRACTS.md`.
//!
//! Strategy:
//!   * SQL returns *integer* aggregates (money in cents as BIGINT, counts as
//!     BIGINT). Postgres `SUM()` yields NUMERIC, so we cast `::bigint`.
//!   * Ratios (sell-through, ROAS, growth, engagement rate) are computed in
//!     Rust from those integers — one place, easy to reason about, no float
//!     surprises from SQL.
//!   * Runtime `query_as` (not the compile-time macros) so the project builds
//!     without a live DB connection at compile time.

use actix_web::{get, web};
use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;
use sqlx::{FromRow, PgPool};
use uuid::Uuid;

use crate::error::AppError;
use crate::models::{
    api, AttributionKind, CampaignPlatform, DataSource, EventStatus, PostType, SaleSource,
};

/// Register every route under `/api`. Auth routes `login`/`forgot-password` are
/// public; everything else sits behind a bearer-token middleware.
pub fn configure(cfg: &mut web::ServiceConfig) {
    use actix_web_httpauth::middleware::HttpAuthentication;
    cfg.service(
        web::scope("/api")
            // Public (no session required).
            .configure(crate::auth::public)
            // Protected: require a valid session token.
            .service(
                web::scope("")
                    .wrap(HttpAuthentication::bearer(crate::auth::validator))
                    .configure(crate::auth::protected)
                    .service(health)
                    .service(dashboard)
                    .service(events)
                    .service(event_detail)
                    .service(artist_detail)
                    .service(artists)
                    .service(social)
                    .service(campaigns)
                    .service(sales)
                    .service(forecasts),
            ),
    );
}

// Safe ratio: returns 0.0 when the denominator is 0.
fn ratio(num: i64, den: i64) -> f64 {
    if den == 0 {
        0.0
    } else {
        num as f64 / den as f64
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

// =============================================================================
// GET /api/health
// =============================================================================

#[derive(FromRow)]
struct HealthRow {
    source: DataSource,
    status: String,
    last_synced: Option<DateTime<Utc>>,
    rows: i64,
    message: Option<String>,
}

#[get("/health")]
async fn health(pool: web::Data<PgPool>) -> Result<web::Json<api::HealthResponse>, AppError> {
    // Latest ingestion run per source, but `rows` reports the ACTUAL data volume
    // that source has landed (live table counts) rather than the last run's
    // delta — otherwise an incremental run (e.g. `account`/`backfill`, which add
    // 0 posts) would make the source look empty.
    let rows = sqlx::query_as::<_, HealthRow>(
        r#"
        SELECT DISTINCT ON (source)
               source,
               status,
               finished_at AS last_synced,
               CASE source
                   WHEN 'instagram'::data_source THEN (SELECT count(*) FROM instagram_posts)
                   WHEN 'quicket'::data_source   THEN (SELECT count(*) FROM ticket_sales WHERE source = 'quicket'::data_source)
                   WHEN 'meta_ads'::data_source  THEN (SELECT count(*) FROM campaigns)
                   WHEN 'yoco'::data_source      THEN (SELECT count(*) FROM ticket_sales WHERE source = 'yoco'::data_source)
                   ELSE 0
               END::bigint AS rows,
               message
        FROM ingestion_log
        ORDER BY source, started_at DESC
        "#,
    )
    .fetch_all(pool.get_ref())
    .await?;

    // The last 8 run statuses per source (oldest→newest), for the health strip.
    let recent_rows = sqlx::query_as::<_, (DataSource, String)>(
        r#"
        SELECT source, status FROM (
            SELECT source, status, started_at,
                   ROW_NUMBER() OVER (PARTITION BY source ORDER BY started_at DESC) AS rn
            FROM ingestion_log
        ) t
        WHERE rn <= 8
        ORDER BY source, started_at ASC
        "#,
    )
    .fetch_all(pool.get_ref())
    .await?;

    let sources = rows
        .into_iter()
        .map(|r| {
            let recent = recent_rows
                .iter()
                .filter(|(src, _)| *src == r.source)
                .map(|(_, st)| st.clone())
                .collect();
            api::SourceHealth {
                source: r.source,
                status: r.status,
                last_synced: r.last_synced,
                rows: r.rows,
                message: r.message,
                recent,
            }
        })
        .collect();

    Ok(web::Json(api::HealthResponse {
        sources,
        database: "healthy".to_string(),
    }))
}

// =============================================================================
// GET /api/dashboard
// =============================================================================

#[derive(FromRow)]
struct DashboardRow {
    tickets_sold: i64,
    revenue_cents: i64,
    capacity_total: i64,
    attributed_revenue: i64,
    ad_spend: i64,
    followers_latest: Option<i64>,
    followers_earliest: Option<i64>,
}

#[get("/dashboard")]
async fn dashboard(pool: web::Data<PgPool>) -> Result<web::Json<api::DashboardResponse>, AppError> {
    let r = sqlx::query_as::<_, DashboardRow>(
        r#"
        SELECT
          (SELECT COALESCE(SUM(quantity),0) FROM ticket_sales)::bigint                       AS tickets_sold,
          (SELECT COALESCE(SUM(quantity*price_cents),0) FROM ticket_sales)::bigint           AS revenue_cents,
          (SELECT COALESCE(SUM(capacity),0) FROM events)::bigint                             AS capacity_total,
          (SELECT COALESCE(SUM(quantity*price_cents),0) FROM ticket_sales
             WHERE attribution <> 'unknown')::bigint                                         AS attributed_revenue,
          (SELECT COALESCE(SUM(spend_cents),0) FROM campaigns)::bigint                       AS ad_spend,
          (SELECT followers FROM daily_account_metrics ORDER BY snapshot_date DESC LIMIT 1)::bigint AS followers_latest,
          (SELECT followers FROM daily_account_metrics ORDER BY snapshot_date ASC  LIMIT 1)::bigint AS followers_earliest
        "#,
    )
    .fetch_one(pool.get_ref())
    .await?;

    let followers = r.followers_latest.unwrap_or(0);
    let followers_growth = match (r.followers_earliest, r.followers_latest) {
        (Some(first), Some(last)) if first > 0 => ratio(last - first, first),
        _ => 0.0,
    };

    Ok(web::Json(api::DashboardResponse {
        period: api::Period { from: None, to: None },
        tickets_sold: r.tickets_sold,
        revenue_cents: r.revenue_cents,
        sell_through: round2(ratio(r.tickets_sold, r.capacity_total)),
        followers,
        followers_growth: round2(followers_growth),
        ad_spend_cents: r.ad_spend,
        roas: round2(ratio(r.attributed_revenue, r.ad_spend)),
        generated_at: Utc::now(),
    }))
}

// =============================================================================
// GET /api/events
// =============================================================================

#[derive(FromRow)]
struct EventRow {
    id: Uuid,
    title: String,
    event_date: NaiveDate,
    status: EventStatus,
    capacity: i32,
    ticket_price_cents: i64,
    artist_id: Option<Uuid>,
    artist_name: Option<String>,
    artist_genre: Option<String>,
    tickets_sold: i64,
    revenue_cents: i64,
}

impl EventRow {
    fn artist_ref(&self) -> Option<api::ArtistRef> {
        match (self.artist_id, &self.artist_name) {
            (Some(id), Some(name)) => Some(api::ArtistRef {
                id,
                name: name.clone(),
                genre: self.artist_genre.clone(),
            }),
            _ => None,
        }
    }
}

const EVENT_SELECT: &str = r#"
    SELECT e.id, e.title, e.event_date, e.status, e.capacity, e.ticket_price_cents,
           a.id   AS artist_id,
           a.name AS artist_name,
           a.genre AS artist_genre,
           COALESCE(s.tickets_sold, 0)::bigint  AS tickets_sold,
           COALESCE(s.revenue_cents, 0)::bigint AS revenue_cents
    FROM events e
    LEFT JOIN artists a ON a.id = e.artist_id
    LEFT JOIN (
        SELECT event_id,
               SUM(quantity)               AS tickets_sold,
               SUM(quantity * price_cents) AS revenue_cents
        FROM ticket_sales GROUP BY event_id
    ) s ON s.event_id = e.id
"#;

#[get("/events")]
async fn events(pool: web::Data<PgPool>) -> Result<web::Json<api::EventsResponse>, AppError> {
    let sql = format!("{EVENT_SELECT} ORDER BY e.event_date DESC");
    let rows = sqlx::query_as::<_, EventRow>(&sql)
        .fetch_all(pool.get_ref())
        .await?;

    let events: Vec<api::EventSummary> = rows
        .iter()
        .map(|r| api::EventSummary {
            id: r.id,
            title: r.title.clone(),
            event_date: r.event_date,
            artist: r.artist_ref(),
            capacity: r.capacity,
            ticket_price_cents: r.ticket_price_cents,
            status: r.status,
            tickets_sold: r.tickets_sold,
            revenue_cents: r.revenue_cents,
            sell_through: round2(ratio(r.tickets_sold, r.capacity as i64)),
        })
        .collect();

    Ok(web::Json(api::EventsResponse {
        count: events.len(),
        events,
    }))
}

// =============================================================================
// GET /api/events/:id
// =============================================================================

#[derive(FromRow)]
struct SalesByTypeRow {
    ticket_type: String,
    quantity: i64,
    revenue_cents: i64,
}

#[derive(FromRow)]
struct ChannelRow {
    channel: String,
    quantity: i64,
}

#[derive(FromRow)]
struct VelocityRow {
    date: NaiveDate,
    cumulative_tickets: i64,
}

#[derive(FromRow)]
struct CampaignMiniRow {
    id: Uuid,
    campaign_name: String,
    spend_cents: i64,
}

#[derive(FromRow)]
struct PostMiniRow {
    id: Uuid,
    post_type: PostType,
    reach: Option<i32>,
    engagement_rate: Option<f64>,
}

#[get("/events/{id}")]
async fn event_detail(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<web::Json<api::EventDetail>, AppError> {
    let id = path.into_inner();
    let pool = pool.get_ref();

    let sql = format!("{EVENT_SELECT} WHERE e.id = $1");
    let ev = sqlx::query_as::<_, EventRow>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound("Event not found".to_string()))?;

    let sales_by_type = sqlx::query_as::<_, SalesByTypeRow>(
        r#"
        SELECT ticket_type,
               SUM(quantity)::bigint               AS quantity,
               SUM(quantity * price_cents)::bigint AS revenue_cents
        FROM ticket_sales WHERE event_id = $1
        GROUP BY ticket_type ORDER BY ticket_type
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| api::SalesByType {
        ticket_type: r.ticket_type,
        quantity: r.quantity,
        revenue_cents: r.revenue_cents,
    })
    .collect();

    let sales_by_channel = sqlx::query_as::<_, ChannelRow>(
        r#"
        SELECT sale_source::text AS channel,
               SUM(quantity)::bigint AS quantity
        FROM ticket_sales WHERE event_id = $1
        GROUP BY sale_source ORDER BY sale_source
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| api::SalesByChannel {
        channel: r.channel,
        quantity: r.quantity,
    })
    .collect();

    let sales_velocity = sqlx::query_as::<_, VelocityRow>(
        r#"
        SELECT sold_at::date AS date,
               SUM(SUM(quantity)) OVER (ORDER BY sold_at::date)::bigint AS cumulative_tickets
        FROM ticket_sales WHERE event_id = $1
        GROUP BY sold_at::date ORDER BY date
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| api::VelocityPoint {
        date: r.date,
        cumulative_tickets: r.cumulative_tickets,
    })
    .collect();

    // NB: local is `event_campaigns`, not `campaigns` — the `#[get("/campaigns")]`
    // handler macro generates a module-level type named `campaigns` that would
    // otherwise collide with the inference on this `.collect()`.
    let event_campaigns: Vec<api::CampaignSummary> = sqlx::query_as::<_, CampaignMiniRow>(
        r#"
        SELECT id, campaign_name, COALESCE(spend_cents, 0)::bigint AS spend_cents
        FROM campaigns WHERE event_id = $1 ORDER BY campaign_name
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| api::CampaignSummary {
        id: r.id,
        campaign_name: r.campaign_name,
        spend_cents: r.spend_cents,
    })
    .collect();

    let posts = sqlx::query_as::<_, PostMiniRow>(
        r#"
        SELECT id, post_type, reach,
               CASE WHEN reach > 0
                    THEN (COALESCE(likes,0)+COALESCE(comments,0)+COALESCE(shares,0)+COALESCE(saves,0))::double precision / reach
               END AS engagement_rate
        FROM instagram_posts WHERE event_id = $1
        ORDER BY published_at
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|r| api::PostSummary {
        id: r.id,
        post_type: r.post_type,
        reach: r.reach,
        engagement_rate: r.engagement_rate.map(round4),
    })
    .collect();

    Ok(web::Json(api::EventDetail {
        id: ev.id,
        title: ev.title.clone(),
        event_date: ev.event_date,
        artist: ev.artist_ref(),
        capacity: ev.capacity,
        ticket_price_cents: ev.ticket_price_cents,
        status: ev.status,
        tickets_sold: ev.tickets_sold,
        revenue_cents: ev.revenue_cents,
        sell_through: round2(ratio(ev.tickets_sold, ev.capacity as i64)),
        sales_by_type,
        sales_by_channel,
        sales_velocity,
        campaigns: event_campaigns,
        posts,
    }))
}

fn round4(x: f64) -> f64 {
    (x * 10000.0).round() / 10000.0
}

// =============================================================================
// GET /api/artists
// =============================================================================

#[derive(FromRow)]
struct ArtistAggRow {
    id: Uuid,
    name: String,
    genre: Option<String>,
    instagram_handle: Option<String>,
    events_count: i64,
    tickets_sold: i64,
    revenue_cents: i64,
    capacity_total: i64,
    attributed_revenue: i64,
    spend_cents: i64,
    avg_engagement_rate: f64,
}

const ARTIST_AGG_SQL: &str = r#"
    SELECT a.id, a.name, a.genre, a.instagram_handle,
      (SELECT COUNT(*) FROM events e WHERE e.artist_id = a.id)::bigint AS events_count,
      (SELECT COALESCE(SUM(ts.quantity),0) FROM ticket_sales ts
         JOIN events e ON e.id = ts.event_id WHERE e.artist_id = a.id)::bigint AS tickets_sold,
      (SELECT COALESCE(SUM(ts.quantity*ts.price_cents),0) FROM ticket_sales ts
         JOIN events e ON e.id = ts.event_id WHERE e.artist_id = a.id)::bigint AS revenue_cents,
      (SELECT COALESCE(SUM(e.capacity),0) FROM events e WHERE e.artist_id = a.id)::bigint AS capacity_total,
      (SELECT COALESCE(SUM(ts.quantity*ts.price_cents),0) FROM ticket_sales ts
         JOIN events e ON e.id = ts.event_id WHERE e.artist_id = a.id
         AND ts.attribution <> 'unknown')::bigint AS attributed_revenue,
      (SELECT COALESCE(SUM(c.spend_cents),0) FROM campaigns c
         JOIN events e ON e.id = c.event_id WHERE e.artist_id = a.id)::bigint AS spend_cents,
      (SELECT COALESCE(AVG(CASE WHEN p.reach > 0
                THEN (COALESCE(p.likes,0)+COALESCE(p.comments,0)+COALESCE(p.shares,0)+COALESCE(p.saves,0))::double precision / p.reach
              END), 0) FROM instagram_posts p
         JOIN events e ON e.id = p.event_id WHERE e.artist_id = a.id) AS avg_engagement_rate
    FROM artists a
"#;

/// The leaderboard row plus its component score breakdown, computed together so
/// the list and detail endpoints share one scoring definition.
struct Scored {
    row: api::ArtistLeaderboardRow,
    breakdown: api::ScoreBreakdown,
}

/// Provisional performance score. The full event-window-aware model is a Phase 4
/// deliverable; here each measurable component is normalised across artists to
/// 0-100 and weighted per the data dictionary. audience_growth is a placeholder
/// (0.0) until per-artist follower attribution exists.
fn score_artists(rows: &[ArtistAggRow]) -> Vec<Scored> {
    let max_tickets = rows.iter().map(|r| r.tickets_sold).max().unwrap_or(0);
    let max_revenue = rows.iter().map(|r| r.revenue_cents).max().unwrap_or(0);
    let max_roas = rows
        .iter()
        .map(|r| ratio(r.attributed_revenue, r.spend_cents))
        .fold(0.0_f64, f64::max);
    let max_engagement = rows.iter().map(|r| r.avg_engagement_rate).fold(0.0_f64, f64::max);

    rows.iter()
        .map(|r| {
            let sell_through = ratio(r.tickets_sold, r.capacity_total);
            let roi = ratio(r.attributed_revenue, r.spend_cents);

            let c_ticket = ratio(r.tickets_sold, max_tickets) * 100.0;
            let c_revenue = ratio(r.revenue_cents, max_revenue) * 100.0;
            let c_sell = (sell_through * 100.0).min(100.0);
            let c_social = if max_engagement > 0.0 { r.avg_engagement_rate / max_engagement * 100.0 } else { 0.0 };
            // Per-artist audience growth isn't obtainable (Instagram reports
            // followers only at the account level, never per show), so its weight
            // is dropped and the remaining five components are rescaled to 100.
            let c_growth = 0.0; // kept for the breakdown display only (always 0)
            let c_marketing = if max_roas > 0.0 { roi / max_roas * 100.0 } else { 0.0 };

            let score = 0.3333 * c_ticket + 0.2222 * c_revenue + 0.1667 * c_sell
                + 0.1667 * c_social + 0.1111 * c_marketing;

            Scored {
                row: api::ArtistLeaderboardRow {
                    id: r.id,
                    name: r.name.clone(),
                    genre: r.genre.clone(),
                    events_count: r.events_count,
                    tickets_sold: r.tickets_sold,
                    revenue_cents: r.revenue_cents,
                    avg_sell_through: round4(sell_through),
                    avg_engagement_rate: round4(r.avg_engagement_rate),
                    roi: round2(roi),
                    performance_score: round2(score),
                },
                breakdown: api::ScoreBreakdown {
                    ticket_performance: round2(c_ticket),
                    revenue: round2(c_revenue),
                    sell_through: round2(c_sell),
                    social_engagement: round2(c_social),
                    audience_growth: round2(c_growth),
                    marketing_efficiency: round2(c_marketing),
                },
            }
        })
        .collect()
}

#[get("/artists")]
async fn artists(pool: web::Data<PgPool>) -> Result<web::Json<api::ArtistsResponse>, AppError> {
    let rows = sqlx::query_as::<_, ArtistAggRow>(ARTIST_AGG_SQL)
        .fetch_all(pool.get_ref())
        .await?;

    let mut leaderboard: Vec<api::ArtistLeaderboardRow> =
        score_artists(&rows).into_iter().map(|s| s.row).collect();
    leaderboard.sort_by(|a, b| {
        b.performance_score
            .partial_cmp(&a.performance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(web::Json(api::ArtistsResponse { artists: leaderboard }))
}

#[derive(FromRow)]
struct EventHistRow {
    id: Uuid,
    title: String,
    event_date: NaiveDate,
    status: EventStatus,
    capacity: i32,
    tickets_sold: i64,
    revenue_cents: i64,
}

#[get("/artists/{id}")]
async fn artist_detail(
    pool: web::Data<PgPool>,
    path: web::Path<Uuid>,
) -> Result<web::Json<serde_json::Value>, AppError> {
    let id = path.into_inner();
    let pool = pool.get_ref();

    // Score across ALL artists (normalisation needs the full field), then pick one.
    let rows = sqlx::query_as::<_, ArtistAggRow>(ARTIST_AGG_SQL)
        .fetch_all(pool)
        .await?;
    let scored = score_artists(&rows);
    let idx = rows
        .iter()
        .position(|r| r.id == id)
        .ok_or_else(|| AppError::NotFound("Artist not found".to_string()))?;
    let agg = &rows[idx];
    let s = &scored[idx];

    let event_rows = sqlx::query_as::<_, EventHistRow>(
        r#"
        SELECT e.id, e.title, e.event_date, e.status, e.capacity,
               COALESCE(SUM(ts.quantity),0)::bigint               AS tickets_sold,
               COALESCE(SUM(ts.quantity*ts.price_cents),0)::bigint AS revenue_cents
        FROM events e
        LEFT JOIN ticket_sales ts ON ts.event_id = e.id
        WHERE e.artist_id = $1
        GROUP BY e.id
        ORDER BY e.event_date
        "#,
    )
    .bind(id)
    .fetch_all(pool)
    .await?;

    let events_json: Vec<serde_json::Value> = event_rows
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "title": e.title,
                "event_date": e.event_date,
                "status": e.status,
                "tickets_sold": e.tickets_sold,
                "revenue_cents": e.revenue_cents,
                "capacity": e.capacity,
                "sell_through": round4(ratio(e.tickets_sold, e.capacity as i64)),
            })
        })
        .collect();

    let avg_ticket_price = if agg.tickets_sold > 0 { agg.revenue_cents / agg.tickets_sold } else { 0 };

    Ok(web::Json(serde_json::json!({
        "id": agg.id,
        "name": agg.name,
        "genre": agg.genre,
        "instagram_handle": agg.instagram_handle,
        "performance_score": s.row.performance_score,
        "score_breakdown": s.breakdown,
        "events_count": agg.events_count,
        "tickets_sold": agg.tickets_sold,
        "revenue_cents": agg.revenue_cents,
        "avg_sell_through": s.row.avg_sell_through,
        "avg_engagement_rate": s.row.avg_engagement_rate,
        "roi": s.row.roi,
        "avg_ticket_price_cents": avg_ticket_price,
        "spend_cents": agg.spend_cents,
        "attributed_revenue_cents": agg.attributed_revenue,
        "events": events_json,
    })))
}

// =============================================================================
// GET /api/social
// =============================================================================

#[derive(FromRow)]
struct AccountSeriesRow {
    date: NaiveDate,
    followers: Option<i32>,
    reach: Option<i32>,
    impressions: Option<i32>,
    engagements: Option<i32>,
    profile_visits: Option<i32>,
}

#[derive(FromRow)]
struct TopPostRow {
    id: Uuid,
    event_title: Option<String>,
    post_type: PostType,
    caption: Option<String>,
    published_at: DateTime<Utc>,
    reach: Option<i32>,
    impressions: Option<i32>,
    likes: Option<i32>,
    comments: Option<i32>,
    shares: Option<i32>,
    saves: Option<i32>,
    engagement_rate: Option<f64>,
}

// Returns serde_json (richer than the base PostSummary DTO) so the Social page
// has the full per-post engagement breakdown and account-level profile visits.
#[get("/social")]
async fn social(pool: web::Data<PgPool>) -> Result<web::Json<serde_json::Value>, AppError> {
    let pool = pool.get_ref();

    let series = sqlx::query_as::<_, AccountSeriesRow>(
        r#"
        SELECT snapshot_date AS date, followers, reach, impressions, engagements, profile_visits
        FROM daily_account_metrics ORDER BY snapshot_date
        "#,
    )
    .fetch_all(pool)
    .await?;

    // Anchor growth on the first/last days that actually have a follower count.
    // Historical reach-only rows (backfilled before the 30-day follower window)
    // carry NULL followers and must not zero this out.
    let first_followers = series.iter().find_map(|r| r.followers);
    let last_followers = series.iter().rev().find_map(|r| r.followers);
    let followers_growth = match (first_followers, last_followers) {
        (Some(f), Some(l)) if f > 0 => ratio((l - f) as i64, f as i64),
        _ => 0.0,
    };

    let latest = series.last();
    let latest_json = latest.map(|r| {
        let eng_rate = match (r.engagements, r.reach) {
            (Some(e), Some(re)) if re > 0 => round4(e as f64 / re as f64),
            _ => 0.0,
        };
        serde_json::json!({
            "followers": r.followers,
            "reach": r.reach,
            "impressions": r.impressions,
            "engagements": r.engagements,
            "profile_visits": r.profile_visits,
            "engagement_rate": eng_rate,
        })
    });

    let account_series: Vec<serde_json::Value> = series
        .iter()
        .map(|r| {
            serde_json::json!({
                "date": r.date,
                "followers": r.followers,
                "reach": r.reach,
                "impressions": r.impressions,
                "engagements": r.engagements,
                "profile_visits": r.profile_visits,
            })
        })
        .collect();

    let posts = sqlx::query_as::<_, TopPostRow>(
        r#"
        SELECT p.id, e.title AS event_title, p.post_type, p.caption, p.published_at,
               p.reach, p.impressions, p.likes, p.comments, p.shares, p.saves,
               CASE WHEN p.reach > 0
                    THEN (COALESCE(p.likes,0)+COALESCE(p.comments,0)+COALESCE(p.shares,0)+COALESCE(p.saves,0))::double precision / p.reach
               END AS engagement_rate
        FROM instagram_posts p
        LEFT JOIN events e ON e.id = p.event_id
        ORDER BY engagement_rate DESC NULLS LAST
        "#,
    )
    .fetch_all(pool)
    .await?;

    let avg_post_engagement = if posts.is_empty() {
        0.0
    } else {
        round4(posts.iter().filter_map(|p| p.engagement_rate).sum::<f64>() / posts.len() as f64)
    };

    let top_posts: Vec<serde_json::Value> = posts
        .iter()
        .map(|p| {
            serde_json::json!({
                "id": p.id,
                "event_title": p.event_title,
                "post_type": p.post_type,
                "caption": p.caption,
                "published_at": p.published_at,
                "reach": p.reach,
                "impressions": p.impressions,
                "likes": p.likes,
                "comments": p.comments,
                "shares": p.shares,
                "saves": p.saves,
                "engagement_rate": p.engagement_rate.map(round4),
            })
        })
        .collect();

    Ok(web::Json(serde_json::json!({
        "account_series": account_series,
        "followers_growth": round4(followers_growth),
        "latest": latest_json,
        "avg_post_engagement": avg_post_engagement,
        "top_posts": top_posts,
    })))
}

// =============================================================================
// GET /api/campaigns
// =============================================================================

#[derive(FromRow)]
struct CampaignPerfRow {
    id: Uuid,
    campaign_name: String,
    campaign_tag: Option<String>,
    platform: CampaignPlatform,
    permalink: Option<String>,
    spend_cents: i64,
    reach: i64,
    clicks: i64,
    engagements: i64,
    attributed_tickets: i64,
    attributed_revenue_cents: i64,
}

#[get("/campaigns")]
async fn campaigns(pool: web::Data<PgPool>) -> Result<web::Json<serde_json::Value>, AppError> {
    let rows = sqlx::query_as::<_, CampaignPerfRow>(
        r#"
        SELECT c.id, c.campaign_name, c.campaign_tag, c.platform, c.permalink,
          COALESCE(c.spend_cents,
                   (SELECT SUM(spend_cents) FROM campaign_metrics m WHERE m.campaign_id = c.id), 0)::bigint AS spend_cents,
          COALESCE((SELECT SUM(reach)       FROM campaign_metrics m WHERE m.campaign_id = c.id), 0)::bigint AS reach,
          COALESCE((SELECT SUM(clicks)      FROM campaign_metrics m WHERE m.campaign_id = c.id), 0)::bigint AS clicks,
          COALESCE((SELECT SUM(engagements) FROM campaign_metrics m WHERE m.campaign_id = c.id), 0)::bigint AS engagements,
          COALESCE((SELECT SUM(ts.quantity) FROM ticket_sales ts
                    WHERE ts.attributed_campaign_tag = c.campaign_tag), 0)::bigint AS attributed_tickets,
          COALESCE((SELECT SUM(ts.quantity*ts.price_cents) FROM ticket_sales ts
                    WHERE ts.attributed_campaign_tag = c.campaign_tag), 0)::bigint AS attributed_revenue_cents
        FROM campaigns c ORDER BY c.campaign_name
        "#,
    )
    .fetch_all(pool.get_ref())
    .await?;

    let campaigns: Vec<api::CampaignPerformanceRow> = rows
        .into_iter()
        .map(|r| {
            let cost_per_ticket_cents = if r.attributed_tickets > 0 {
                Some(r.spend_cents / r.attributed_tickets)
            } else {
                None
            };
            let cost_per_engagement_cents = if r.engagements > 0 {
                Some(r.spend_cents / r.engagements)
            } else {
                None
            };
            let ticket_conversion = if r.clicks > 0 {
                Some(round4(ratio(r.attributed_tickets, r.clicks)))
            } else {
                None
            };
            let roas = if r.spend_cents > 0 {
                Some(round2(ratio(r.attributed_revenue_cents, r.spend_cents)))
            } else {
                None
            };
            api::CampaignPerformanceRow {
                id: r.id,
                campaign_name: r.campaign_name,
                campaign_tag: r.campaign_tag,
                platform: r.platform,
                permalink: r.permalink,
                spend_cents: r.spend_cents,
                reach: r.reach,
                clicks: r.clicks,
                engagements: r.engagements,
                attributed_tickets: r.attributed_tickets,
                attributed_revenue_cents: r.attributed_revenue_cents,
                cost_per_ticket_cents,
                cost_per_engagement_cents,
                ticket_conversion,
                roas,
            }
        })
        .collect();

    Ok(web::Json(serde_json::json!({ "campaigns": campaigns })))
}

// =============================================================================
// GET /api/sales
// =============================================================================

#[derive(Deserialize)]
struct SalesQuery {
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
    #[serde(rename = "eventId")]
    event_id: Option<Uuid>,
}

#[derive(FromRow, serde::Serialize)]
struct SaleRow {
    id: Uuid,
    event_id: Uuid,
    ticket_type: String,
    quantity: i32,
    price_cents: i64,
    sold_at: DateTime<Utc>,
    sale_source: SaleSource,
    attribution: AttributionKind,
}

#[get("/sales")]
async fn sales(
    pool: web::Data<PgPool>,
    q: web::Query<SalesQuery>,
) -> Result<web::Json<serde_json::Value>, AppError> {
    let pool = pool.get_ref();
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(50).clamp(1, 20000);
    let offset = (page - 1) * page_size;

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM ticket_sales WHERE ($1::uuid IS NULL OR event_id = $1)",
    )
    .bind(q.event_id)
    .fetch_one(pool)
    .await?;

    let rows = sqlx::query_as::<_, SaleRow>(
        r#"
        SELECT id, event_id, ticket_type, quantity, price_cents, sold_at, sale_source, attribution
        FROM ticket_sales
        WHERE ($1::uuid IS NULL OR event_id = $1)
        ORDER BY sold_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(q.event_id)
    .bind(page_size)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok(web::Json(serde_json::json!({
        "sales": rows,
        "page": page,
        "page_size": page_size,
        "total": total,
    })))
}

// =============================================================================
// GET /api/forecasts  (Phase 5 — heuristic stub)
// =============================================================================

#[derive(FromRow)]
struct ForecastSeedRow {
    artist_id: Uuid,
    artist_name: String,
    n_events: i64,
    avg_tickets: Option<f64>,
    avg_price_cents: Option<f64>,
    avg_capacity: Option<f64>,
    revenue_cents: i64,
    engagement: Option<f64>,
    ad_spend_cents: i64,
}

#[get("/forecasts")]
async fn forecasts(pool: web::Data<PgPool>) -> Result<web::Json<api::ForecastsResponse>, AppError> {
    // A transparent signal-based projection (not a trained model): each artist's
    // completed-event history, adjusted for sell-out demand, with a confidence
    // that reflects how much corroborating signal we actually have.
    let rows = sqlx::query_as::<_, ForecastSeedRow>(
        r#"
        SELECT a.id AS artist_id, a.name AS artist_name,
               COALESCE(COUNT(DISTINCT t.event_id), 0)::bigint       AS n_events,
               AVG(t.tickets)::double precision                       AS avg_tickets,
               AVG(t.avg_price)::double precision                     AS avg_price_cents,
               AVG(t.capacity)::double precision                      AS avg_capacity,
               COALESCE(SUM(t.revenue), 0)::bigint                    AS revenue_cents,
               (SELECT AVG(CASE WHEN p.reach > 0
                   THEN (COALESCE(p.likes,0)+COALESCE(p.comments,0)+COALESCE(p.shares,0)+COALESCE(p.saves,0))::double precision / p.reach
                 END) FROM instagram_posts p JOIN events e2 ON e2.id = p.event_id
                 WHERE e2.artist_id = a.id)                           AS engagement,
               (SELECT COALESCE(SUM(c.spend_cents),0) FROM campaigns c
                 JOIN events e3 ON e3.id = c.event_id WHERE e3.artist_id = a.id)::bigint AS ad_spend_cents
        FROM artists a
        LEFT JOIN (
            SELECT e.id AS event_id, e.artist_id, e.capacity,
                   SUM(ts.quantity)              AS tickets,
                   AVG(ts.price_cents)           AS avg_price,
                   SUM(ts.quantity*ts.price_cents) AS revenue
            FROM events e JOIN ticket_sales ts ON ts.event_id = e.id
            WHERE e.status = 'completed'
            GROUP BY e.id
        ) t ON t.artist_id = a.id
        GROUP BY a.id, a.name
        ORDER BY avg_tickets DESC NULLS LAST
        "#,
    )
    .fetch_all(pool.get_ref())
    .await?;

    let max_revenue = rows.iter().map(|r| r.revenue_cents).max().unwrap_or(0) as f64;

    let forecasts = rows
        .into_iter()
        .map(|r| {
            let base = r.avg_tickets.unwrap_or(0.0);
            let price = r.avg_price_cents.unwrap_or(30000.0);
            let capacity = r.avg_capacity.unwrap_or(90.0).max(1.0);
            let hist_sell = (base / capacity).min(2.0);
            let engagement = r.engagement.unwrap_or(0.0);

            // Demand: a sold-out act was capacity-constrained, so true demand sat
            // above what they could sell — nudge the expectation up a little.
            let uplift = if hist_sell >= 0.95 { 1.10 } else { 1.0 };
            let expected = base * uplift;
            let expected_tickets = expected.round() as i64;

            // Confidence from real signals — never a flat number, capped at 0.85.
            let mut conf = 0.30; // one completed event is thin evidence
            conf += ((r.n_events - 1).max(0) as f64 * 0.15).min(0.30); // more history
            if engagement > 0.0 { conf += 0.12; } // social demand signal present
            if r.ad_spend_cents > 0 { conf += 0.08; } // marketing reach known
            if hist_sell >= 0.9 { conf += 0.15; } // demand proven (sold out)
            if max_revenue > 0.0 { conf += (r.revenue_cents as f64 / max_revenue) * 0.10; } // scale
            let confidence = conf.min(0.85);

            // Range widens as confidence falls (honest uncertainty).
            let half = 0.10 + 0.25 * (1.0 - confidence);

            // Sold-out acts barely need paid support; soft ones need more.
            let budget_pct = if hist_sell >= 0.95 { 0.08 } else if hist_sell < 0.7 { 0.16 } else { 0.12 };
            let expected_revenue = expected * price;

            api::Forecast {
                artist_id: r.artist_id,
                artist_name: r.artist_name,
                expected_tickets,
                range_low: (expected * (1.0 - half)).round() as i64,
                range_high: (expected * (1.0 + half)).round() as i64,
                confidence: round2(confidence),
                expected_revenue_cents: expected_revenue.round() as i64,
                expected_sell_through: round2((expected / capacity).min(1.0)),
                recommended_budget_cents: (expected_revenue * budget_pct).round() as i64,
                rationale: format!(
                    "{} completed event(s); {:.0}% historical sell-through; {} engagement; {}.",
                    r.n_events,
                    hist_sell * 100.0,
                    if engagement >= 0.06 { "above-average" } else { "modest" },
                    if r.ad_spend_cents > 0 { "ad-supported" } else { "organic draw" }
                ),
            }
        })
        .collect();

    // Seasonality — series demand by month (the "when to perform" signal).
    let season = sqlx::query_as::<_, (String, i64, f64)>(
        r#"
        SELECT to_char(m, 'Mon') AS month, shows, avg_sell FROM (
            SELECT date_trunc('month', e.event_date) AS m,
                   COUNT(*)::bigint AS shows,
                   AVG(sub.st)::double precision AS avg_sell
            FROM events e
            JOIN (
                SELECT event_id, SUM(quantity)::double precision / NULLIF(MAX(capacity),0) AS st
                FROM ticket_sales ts JOIN events ev ON ev.id = ts.event_id
                GROUP BY event_id
            ) sub ON sub.event_id = e.id
            WHERE e.status = 'completed'
            GROUP BY 1
        ) q ORDER BY m
        "#,
    )
    .fetch_all(pool.get_ref())
    .await?;
    let seasonality = season
        .into_iter()
        .map(|(month, shows, avg)| api::SeasonPoint { month, shows, avg_sell_through: round2(avg) })
        .collect();

    Ok(web::Json(api::ForecastsResponse {
        model: "signal-based v1".to_string(),
        forecasts,
        seasonality,
    }))
}
