//! Instagram ingestion runner.
//!
//! Mirrors `bin/ingest.rs` (Quicket). Talks to the Meta Graph API through the
//! secure backend — the System User token stays in the environment and never
//! reaches the browser.
//!
//! Subcommands:
//!   posts    — page /{ig-user-id}/media → upsert `instagram_posts`, and append
//!              a dated snapshot into `instagram_post_metrics`.
//!   account  — snapshot `daily_account_metrics` (followers, reach, profile
//!              visits) for today. This is what makes follower GROWTH computable.
//!   all      — account THEN posts (default). Safe to SCHEDULE nightly.
//!
//! Usage:
//!   cargo run --bin ingest_instagram              # all (account + posts)
//!   cargo run --bin ingest_instagram -- posts     # posts only
//!   cargo run --bin ingest_instagram -- account   # account snapshot only
//!   cargo run --bin ingest_instagram -- all --reset   # clear IG tables first
//!
//! Post row ids are deterministic UUIDv5 from the Instagram media id, so every
//! run is idempotent (re-running refreshes the latest-known numbers in place).

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde_json::Value;
use sqlx::PgPool;
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

use sankayi_intelligence::db;
use sankayi_intelligence::instagram::{field_i64, insight_series, insight_value, InstagramClient};

// Jazz Series start — the earliest date we attempt to backfill reach from.
const SERIES_START: &str = "2026-05-08";

// Same namespace as the Quicket ingest — keeps all deterministic ids consistent.
const NS: Uuid = Uuid::from_u128(0x53414e4b_4159_495a_5f5f_515549434b45_u128);

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
        .unwrap_or_else(|| "all".to_string());
    let reset = args.iter().any(|a| a == "--reset");

    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let pool = db::connect(&db_url).await.context("connecting to PostgreSQL")?;

    if reset {
        log::warn!("--reset: clearing instagram_post_metrics, instagram_posts, daily_account_metrics");
        sqlx::query("DELETE FROM instagram_post_metrics").execute(&pool).await?;
        sqlx::query("DELETE FROM instagram_posts WHERE source = 'instagram'::data_source")
            .execute(&pool)
            .await?;
        sqlx::query("DELETE FROM daily_account_metrics WHERE source = 'instagram'::data_source")
            .execute(&pool)
            .await?;
    }

    let started = Utc::now();
    // Wrap the fallible work so any error is recorded as a 'failed' run — that's
    // what makes an outage (e.g. Meta blocking the app) visible on Data Health.
    let result: Result<(i64, i64)> = async {
        let client = InstagramClient::from_env()?;
        log::info!("Instagram ingest: ig-user-id {}", client.user_id());
        let out = match subcommand.as_str() {
            "all" => {
                let a = account_sync(&pool, &client).await.unwrap_or_else(|e| {
                    log::warn!("account snapshot failed: {e}");
                    0
                });
                let p = posts_sync(&pool, &client).await? + stories_sync(&pool, &client).await?;
                (p, a)
            }
            "posts" => (posts_sync(&pool, &client).await? + stories_sync(&pool, &client).await?, 0),
            "account" => (0, account_sync(&pool, &client).await?),
            "backfill" => (0, backfill(&pool, &client).await?),
            other => anyhow::bail!("unknown subcommand '{other}' (use all | posts | account | backfill)"),
        };
        Ok(out)
    }
    .await;

    let (posts, account_rows) = match result {
        Ok(v) => v,
        Err(e) => {
            db::log_ingest_failure(&pool, "instagram", started, &e).await;
            return Err(e);
        }
    };

    let msg = format!("{subcommand}: {posts} posts, {account_rows} account snapshot(s)");
    sqlx::query(
        "INSERT INTO ingestion_log (source, started_at, finished_at, status, rows_ingested, message)
         VALUES ('instagram'::data_source, $1, now(), 'success', $2, $3)",
    )
    .bind(started)
    .bind(posts as i32)
    .bind(&msg)
    .execute(&pool)
    .await?;

    println!("✅ Instagram {msg}");
    Ok(())
}

// =============================================================================
// posts_sync — /{ig-user-id}/media → instagram_posts (+ dated metric snapshot)
// =============================================================================
async fn posts_sync(pool: &PgPool, client: &InstagramClient) -> Result<i64> {
    let today = Utc::now().date_naive();
    let mut after: Option<String> = None;
    let mut total = 0i64;

    loop {
        let page = client.media(after.as_deref(), 50).await.context("fetching media page")?;
        if page.data.is_empty() {
            break;
        }

        for m in &page.data {
            let Some(mid) = m.get("id").and_then(|x| x.as_str()) else { continue };
            let caption = m.get("caption").and_then(|x| x.as_str()).map(str::to_string);
            let permalink = m.get("permalink").and_then(|x| x.as_str()).map(str::to_string);
            let media_type = m.get("media_type").and_then(|x| x.as_str()).unwrap_or("IMAGE");
            let product_type = m.get("media_product_type").and_then(|x| x.as_str()).unwrap_or("FEED");
            let post_type = map_post_type(media_type, product_type);
            let published_at = parse_dt(m.get("timestamp").and_then(|x| x.as_str()));

            // Likes/comments are plain media fields — no insights call needed.
            let likes = as_i32(m.get("like_count").and_then(|x| x.as_i64()));
            let comments = as_i32(m.get("comments_count").and_then(|x| x.as_i64()));

            // Insights are best-effort: metric availability varies by media type,
            // and one invalid metric fails the whole call — so we fall back to
            // reach-only rather than losing the post entirely.
            let ins = fetch_media_insights(client, mid, post_type).await;
            let reach = as_i32(insight_value(&ins, "reach"));
            let saves = as_i32(insight_value(&ins, "saved"));
            let shares = as_i32(insight_value(&ins, "shares"));
            let video_views = as_i32(insight_value(&ins, "views"));

            let uuid = media_uuid(mid);
            let raw = serde_json::json!({ "media": m, "insights": ins });

            sqlx::query(
                r#"
                INSERT INTO instagram_posts
                    (id, event_id, post_type, caption, permalink, published_at,
                     reach, likes, comments, shares, saves, video_views,
                     source, source_id, source_timestamp, retrieved_at, raw_payload)
                VALUES ($1, NULL, $2::post_type, $3, $4, $5,
                        $6, $7, $8, $9, $10, $11,
                        'instagram'::data_source, $12, $5, now(), $13)
                ON CONFLICT (id) DO UPDATE SET
                    post_type = EXCLUDED.post_type,
                    caption = EXCLUDED.caption,
                    permalink = EXCLUDED.permalink,
                    published_at = EXCLUDED.published_at,
                    reach = EXCLUDED.reach,
                    likes = EXCLUDED.likes,
                    comments = EXCLUDED.comments,
                    shares = EXCLUDED.shares,
                    saves = EXCLUDED.saves,
                    video_views = EXCLUDED.video_views,
                    source_timestamp = EXCLUDED.source_timestamp,
                    retrieved_at = now(),
                    raw_payload = EXCLUDED.raw_payload
                "#,
            )
            .bind(uuid)
            .bind(post_type)
            .bind(&caption)
            .bind(&permalink)
            .bind(published_at)
            .bind(reach)
            .bind(likes)
            .bind(comments)
            .bind(shares)
            .bind(saves)
            .bind(video_views)
            .bind(mid)
            .bind(sqlx::types::Json(&raw))
            .execute(pool)
            .await
            .with_context(|| format!("upsert instagram_post {mid}"))?;

            // Append today's snapshot for the growth time-series.
            sqlx::query(
                r#"
                INSERT INTO instagram_post_metrics
                    (id, post_id, snapshot_date, reach, likes, comments, shares, saves, video_views, retrieved_at, raw_payload)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, now(), $10)
                ON CONFLICT (post_id, snapshot_date) DO UPDATE SET
                    reach = EXCLUDED.reach,
                    likes = EXCLUDED.likes,
                    comments = EXCLUDED.comments,
                    shares = EXCLUDED.shares,
                    saves = EXCLUDED.saves,
                    video_views = EXCLUDED.video_views,
                    retrieved_at = now(),
                    raw_payload = EXCLUDED.raw_payload
                "#,
            )
            .bind(metric_uuid(mid, today))
            .bind(uuid)
            .bind(today)
            .bind(reach)
            .bind(likes)
            .bind(comments)
            .bind(shares)
            .bind(saves)
            .bind(video_views)
            .bind(sqlx::types::Json(&ins))
            .execute(pool)
            .await
            .with_context(|| format!("snapshot instagram_post_metrics {mid}"))?;

            total += 1;
        }

        after = page.paging.cursors.after.clone();
        if after.is_none() {
            break;
        }
        // Be gentle with the Graph API between pages.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    log::info!("posts: upserted {total} media");
    Ok(total)
}

// =============================================================================
// stories_sync — /{ig-user-id}/stories → instagram_posts (post_type = 'story')
// =============================================================================
// Stories are ephemeral (24h) and their insights stay hidden until "enough"
// people have viewed them (Graph error #10). So this is best-effort by nature:
// run it while a story is live to capture engagement; the row persists after the
// story expires with whatever numbers we last saw. Story metrics map onto the
// shared columns — replies→comments, views→video_views — full detail in raw_payload.
async fn stories_sync(pool: &PgPool, client: &InstagramClient) -> Result<i64> {
    let today = Utc::now().date_naive();
    let page = match client.stories().await {
        Ok(p) => p,
        Err(e) => {
            log::warn!("stories list failed: {e}");
            return Ok(0);
        }
    };
    if page.data.is_empty() {
        log::info!("stories: none active");
        return Ok(0);
    }

    let mut n = 0i64;
    for m in &page.data {
        let Some(sid) = m.get("id").and_then(|x| x.as_str()) else { continue };
        let caption = m.get("caption").and_then(|x| x.as_str()).map(str::to_string);
        let permalink = m.get("permalink").and_then(|x| x.as_str()).map(str::to_string);
        let published_at = parse_dt(m.get("timestamp").and_then(|x| x.as_str()));

        // Insights need enough viewers; on #10 we still store the story shell.
        let ins = match client.media_insights(sid, "reach,replies,views,total_interactions").await {
            Ok(v) => v,
            Err(e) => {
                log::info!("story {sid} insights not ready: {e}");
                Value::Null
            }
        };
        let reach = as_i32(insight_value(&ins, "reach"));
        let replies = as_i32(insight_value(&ins, "replies"));
        let views = as_i32(insight_value(&ins, "views"));

        let uuid = media_uuid(sid);
        let raw = serde_json::json!({ "media": m, "insights": ins });

        sqlx::query(
            r#"
            INSERT INTO instagram_posts
                (id, event_id, post_type, caption, permalink, published_at,
                 reach, comments, video_views,
                 source, source_id, source_timestamp, retrieved_at, raw_payload)
            VALUES ($1, NULL, 'story'::post_type, $2, $3, $4,
                    $5, $6, $7,
                    'instagram'::data_source, $8, $4, now(), $9)
            ON CONFLICT (id) DO UPDATE SET
                caption = EXCLUDED.caption,
                permalink = EXCLUDED.permalink,
                published_at = EXCLUDED.published_at,
                reach = EXCLUDED.reach,
                comments = EXCLUDED.comments,
                video_views = EXCLUDED.video_views,
                source_timestamp = EXCLUDED.source_timestamp,
                retrieved_at = now(),
                raw_payload = EXCLUDED.raw_payload
            "#,
        )
        .bind(uuid)
        .bind(&caption)
        .bind(&permalink)
        .bind(published_at)
        .bind(reach)
        .bind(replies)
        .bind(views)
        .bind(sid)
        .bind(sqlx::types::Json(&raw))
        .execute(pool)
        .await
        .with_context(|| format!("upsert story {sid}"))?;

        sqlx::query(
            r#"
            INSERT INTO instagram_post_metrics
                (id, post_id, snapshot_date, reach, comments, video_views, retrieved_at, raw_payload)
            VALUES ($1, $2, $3, $4, $5, $6, now(), $7)
            ON CONFLICT (post_id, snapshot_date) DO UPDATE SET
                reach = EXCLUDED.reach,
                comments = EXCLUDED.comments,
                video_views = EXCLUDED.video_views,
                retrieved_at = now(),
                raw_payload = EXCLUDED.raw_payload
            "#,
        )
        .bind(metric_uuid(sid, today))
        .bind(uuid)
        .bind(today)
        .bind(reach)
        .bind(replies)
        .bind(views)
        .bind(sqlx::types::Json(&ins))
        .execute(pool)
        .await
        .with_context(|| format!("snapshot story metrics {sid}"))?;

        n += 1;
    }
    log::info!("stories: upserted {n}");
    Ok(n)
}

// =============================================================================
// account_sync — followers_count + daily insights → daily_account_metrics
// =============================================================================
async fn account_sync(pool: &PgPool, client: &InstagramClient) -> Result<i64> {
    let today = Utc::now().date_naive();

    let fields = client.account_fields().await.context("fetching account fields")?;
    let followers = as_i32(field_i64(&fields, "followers_count"));

    // Daily insights are best-effort — names/availability/shape shift across API
    // versions, so a failure on one shouldn't sink the whole snapshot.
    // All three are v26 total_value metrics. NB: reach as a time-series returns
    // the current (incomplete) day as its last point — 0 first thing in the
    // morning — so we take the total_value aggregate instead.
    let reach = as_i32(
        client.account_insights_total("reach").await.ok().and_then(|v| insight_value(&v, "reach")),
    );
    let profile_visits = as_i32(
        client
            .account_insights_total("profile_views")
            .await
            .ok()
            .and_then(|v| insight_value(&v, "profile_views")),
    );
    let engagements = as_i32(
        client
            .account_insights_total("accounts_engaged")
            .await
            .ok()
            .and_then(|v| insight_value(&v, "accounts_engaged")),
    );

    let raw = serde_json::json!({ "fields": fields });

    sqlx::query(
        r#"
        INSERT INTO daily_account_metrics
            (id, snapshot_date, followers, reach, profile_visits, engagements,
             source, source_timestamp, retrieved_at, raw_payload)
        VALUES ($1, $2, $3, $4, $5, $6, 'instagram'::data_source, now(), now(), $7)
        ON CONFLICT (snapshot_date) DO UPDATE SET
            followers = EXCLUDED.followers,
            reach = EXCLUDED.reach,
            profile_visits = EXCLUDED.profile_visits,
            engagements = EXCLUDED.engagements,
            retrieved_at = now(),
            raw_payload = EXCLUDED.raw_payload
        "#,
    )
    .bind(account_uuid(today))
    .bind(today)
    .bind(followers)
    .bind(reach)
    .bind(profile_visits)
    .bind(engagements)
    .bind(sqlx::types::Json(&raw))
    .execute(pool)
    .await
    .context("upsert daily_account_metrics")?;

    log::info!(
        "account: {} followers, reach {:?}, profile_visits {:?}, engagements {:?}",
        followers.unwrap_or(0),
        reach,
        profile_visits,
        engagements
    );
    Ok(1)
}

// =============================================================================
// backfill — historical daily_account_metrics (followers ~30d, reach full series)
// =============================================================================
// Honest limits (tested against the live API):
//   * follower_count is capped at the last 30 days — so the absolute follower
//     curve can only be reconstructed for ~30 days (walked back from today's
//     total using daily net-new). Older follower history is NOT API-available.
//   * reach backfills the whole series (110+ days), fetched in 30-day windows.
// We only set followers/reach here and COALESCE on conflict, so we never clobber
// the richer same-day row that `account_sync` writes going forward.
async fn backfill(pool: &PgPool, client: &InstagramClient) -> Result<i64> {
    let today = Utc::now().date_naive();
    let series_start = NaiveDate::parse_from_str(SERIES_START, "%Y-%m-%d").unwrap_or(today);

    // 1) reach per-day across the whole series, in 30-day windows.
    let mut reach_by_day: HashMap<NaiveDate, i32> = HashMap::new();
    let mut win_start = series_start;
    while win_start < today {
        let win_end = (win_start + Duration::days(30)).min(today);
        match client.account_timeseries("reach", to_unix(win_start), to_unix(win_end)).await {
            Ok(env) => {
                for (end_time, val) in insight_series(&env, "reach") {
                    if let Some(d) = end_time_date(&end_time) {
                        reach_by_day.insert(d, val as i32);
                    }
                }
            }
            Err(e) => log::warn!("reach {win_start}..{win_end}: {e}"),
        }
        win_start = win_end;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    // 2) follower_count (last 30 days) → reconstruct absolute followers backward
    //    from today's total. API excludes the current day, so treat the total as
    //    the follower count at the end of the most recent returned day.
    let total = field_i64(&client.account_fields().await?, "followers_count").unwrap_or(0);
    let mut net: Vec<(NaiveDate, i64)> = Vec::new();
    let f_since = to_unix(today - Duration::days(30));
    let f_until = to_unix(today);
    match client.account_timeseries("follower_count", f_since, f_until).await {
        Ok(env) => {
            for (end_time, val) in insight_series(&env, "follower_count") {
                if let Some(d) = end_time_date(&end_time) {
                    net.push((d, val));
                }
            }
        }
        Err(e) => log::warn!("follower_count: {e}"),
    }
    net.sort_by_key(|x| x.0);
    let mut followers_by_day: HashMap<NaiveDate, i32> = HashMap::new();
    let mut running = total;
    for (d, delta) in net.iter().rev() {
        followers_by_day.insert(*d, running as i32); // followers at end of day d
        running -= delta; // ...before that day's net additions
    }

    // 3) upsert one row per date we learned anything about (skip today — that's
    //    account_sync's live row).
    let mut dates: BTreeSet<NaiveDate> = reach_by_day.keys().copied().collect();
    dates.extend(followers_by_day.keys().copied());
    let mut rows = 0i64;
    for d in dates.iter().filter(|d| **d < today) {
        let followers = followers_by_day.get(d).copied();
        let reach = reach_by_day.get(d).copied();
        sqlx::query(
            r#"
            INSERT INTO daily_account_metrics
                (id, snapshot_date, followers, reach, source, retrieved_at)
            VALUES ($1, $2, $3, $4, 'instagram'::data_source, now())
            ON CONFLICT (snapshot_date) DO UPDATE SET
                followers = COALESCE(EXCLUDED.followers, daily_account_metrics.followers),
                reach = COALESCE(EXCLUDED.reach, daily_account_metrics.reach),
                retrieved_at = now()
            "#,
        )
        .bind(account_uuid(*d))
        .bind(*d)
        .bind(followers)
        .bind(reach)
        .execute(pool)
        .await
        .with_context(|| format!("backfill daily_account_metrics {d}"))?;
        rows += 1;
    }

    log::info!(
        "backfill: {rows} days ({} with followers, {} with reach); series from {series_start}",
        followers_by_day.len(),
        reach_by_day.len()
    );
    Ok(rows)
}

// ---- helpers ----

fn to_unix(d: NaiveDate) -> i64 {
    d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
}

/// Insight `end_time` (e.g. `2026-08-25T07:00:00+0000`) → its calendar date.
fn end_time_date(s: &str) -> Option<NaiveDate> {
    if s.is_empty() {
        return None;
    }
    Some(parse_dt(Some(s)).date_naive())
}

/// Which insight metrics to request for a given post type. Reels/video expose
/// `views`; feed posts don't. `total_interactions` is a useful engagement roll-up.
fn insight_metrics_for(post_type: &str) -> &'static str {
    match post_type {
        "reel" | "video" => "reach,saved,shares,views,total_interactions",
        _ => "reach,saved,shares,total_interactions",
    }
}

/// Fetch media insights with a reach-only fallback (see call site).
async fn fetch_media_insights(client: &InstagramClient, mid: &str, post_type: &str) -> Value {
    let metrics = insight_metrics_for(post_type);
    match client.media_insights(mid, metrics).await {
        Ok(v) => v,
        Err(e) => {
            log::warn!("insights {mid} ({metrics}) failed: {e}; retrying reach-only");
            client.media_insights(mid, "reach").await.unwrap_or(Value::Null)
        }
    }
}

/// Map Instagram's media_type/media_product_type onto our `post_type` enum.
fn map_post_type(media_type: &str, product_type: &str) -> &'static str {
    if product_type.eq_ignore_ascii_case("REELS") {
        return "reel";
    }
    match media_type.to_uppercase().as_str() {
        "VIDEO" => "video",
        "CAROUSEL_ALBUM" => "carousel",
        _ => "image",
    }
}

fn media_uuid(media_id: &str) -> Uuid {
    Uuid::new_v5(&NS, format!("instagram:media:{media_id}").as_bytes())
}
fn metric_uuid(media_id: &str, date: NaiveDate) -> Uuid {
    Uuid::new_v5(&NS, format!("instagram:media:{media_id}:{date}").as_bytes())
}
fn account_uuid(date: NaiveDate) -> Uuid {
    Uuid::new_v5(&NS, format!("instagram:account:{date}").as_bytes())
}

/// Graph integers arrive as i64; our columns are INTEGER — narrow safely.
fn as_i32(v: Option<i64>) -> Option<i32> {
    v.map(|x| x as i32)
}

/// Parse Instagram timestamps, e.g. `2026-08-24T13:53:44+0000`.
fn parse_dt(s: Option<&str>) -> DateTime<Utc> {
    let Some(x) = s else { return Utc::now() };
    if let Ok(d) = DateTime::parse_from_rfc3339(x) {
        return d.with_timezone(&Utc);
    }
    // IG uses `+0000` (no colon), which RFC3339 rejects — handle with %z.
    if let Ok(d) = DateTime::parse_from_str(x, "%Y-%m-%dT%H:%M:%S%z") {
        return d.with_timezone(&Utc);
    }
    Utc::now()
}
