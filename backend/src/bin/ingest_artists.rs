//! Derive artists from the Quicket event lineup and link events to them.
//!
//! Quicket gives us event titles but no artist records, so the Artist Leaderboard
//! and Sales Insight sit empty. This distils each event title down to its act
//! (e.g. "Kyle Shepherd - Solo Piano" -> "Kyle Shepherd"), dedupes across the
//! series (Kyle's two shows share one artist), creates the artist rows, and sets
//! events.artist_id. Deterministic UUIDv5 → idempotent; safe to run nightly.
//!
//! Usage: cargo run --bin ingest_artists

use anyhow::{Context, Result};
use chrono::NaiveDate;
use sqlx::FromRow;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use sankayi_intelligence::db;

const NS: Uuid = Uuid::from_u128(0x53414e4b_4159_495a_5f5f_515549434b45_u128);

#[derive(FromRow)]
struct Ev {
    id: Uuid,
    title: String,
    event_date: NaiveDate,
}

#[derive(FromRow)]
struct Post {
    id: Uuid,
    caption: Option<String>,
    pub_date: NaiveDate,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let pool = db::connect(&db_url).await.context("connecting to PostgreSQL")?;

    let events: Vec<Ev> =
        sqlx::query_as("SELECT id, title, event_date FROM events WHERE source = 'quicket'::data_source")
            .fetch_all(&pool)
            .await?;

    // Canonicalise names so an act's variants collapse to one artist — e.g.
    // "The Kyle Shepherd Xahuri Trio" folds into "Kyle Shepherd".
    let distinct: Vec<String> = {
        let mut seen = HashSet::new();
        events
            .iter()
            .map(|e| core_name(&e.title))
            .filter(|n| !n.is_empty() && seen.insert(n.to_lowercase()))
            .collect()
    };
    let canon = canonicalize(&distinct);

    let mut artists = 0u64;
    let mut linked = 0u64;
    let mut seen: HashSet<Uuid> = HashSet::new();

    for e in &events {
        let derived = core_name(&e.title);
        if derived.is_empty() {
            continue;
        }
        let name = canon.get(&derived.to_lowercase()).cloned().unwrap_or(derived);
        let aid = artist_uuid(&name);

        if seen.insert(aid) {
            sqlx::query(
                "INSERT INTO artists (id, name) VALUES ($1, $2)
                 ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = now()",
            )
            .bind(aid)
            .bind(&name)
            .execute(&pool)
            .await
            .with_context(|| format!("upsert artist {name}"))?;
            artists += 1;
        }

        let res = sqlx::query("UPDATE events SET artist_id = $1 WHERE id = $2")
            .bind(aid)
            .bind(e.id)
            .execute(&pool)
            .await?;
        linked += res.rows_affected();
    }

    // Link Instagram posts to events by caption (populates per-artist social
    // engagement: the score joins posts -> events -> artist). A post about an
    // artist maps to that artist's nearest-dated show.
    sqlx::query("UPDATE instagram_posts SET event_id = NULL WHERE source = 'instagram'::data_source")
        .execute(&pool)
        .await?;
    let posts: Vec<Post> = sqlx::query_as(
        "SELECT id, caption, published_at::date AS pub_date FROM instagram_posts WHERE source = 'instagram'::data_source",
    )
    .fetch_all(&pool)
    .await?;

    let mut posts_linked = 0u64;
    for p in &posts {
        let Some(cap) = p.caption.as_deref() else { continue };
        let cap_l = cap.to_lowercase();
        let mut best: Option<(Uuid, i64)> = None;
        for e in &events {
            if artist_tokens(&e.title).iter().any(|tok| cap_l.contains(tok)) {
                let score = (e.event_date - p.pub_date).num_days().abs();
                if best.map_or(true, |(_, s)| score < s) {
                    best = Some((e.id, score));
                }
            }
        }
        if let Some((eid, _)) = best {
            sqlx::query("UPDATE instagram_posts SET event_id = $1 WHERE id = $2")
                .bind(eid)
                .bind(p.id)
                .execute(&pool)
                .await?;
            posts_linked += 1;
        }
    }

    println!("✅ Artists: {artists} distinct acts, {linked} events linked, {posts_linked} posts linked");
    Ok(())
}

/// Distinctive name tokens from an event title — words ≥5 chars minus generic
/// venue/marketing words — used to spot the act in a post caption.
fn artist_tokens(title: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "sankayi", "series", "music", "jazz", "great", "hearts", "album", "launch",
        "night", "nights", "saturday", "sessions", "friday", "sunday", "women's",
    ];
    core_name(title)
        .to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.len() >= 5 && !STOP.contains(&w.as_str()))
        .collect()
}

fn artist_uuid(name: &str) -> Uuid {
    Uuid::new_v5(&NS, format!("artist:{}", name.to_lowercase()).as_bytes())
}

/// Distinct name-tokens (lowercased, ≥1 char), for subset comparison.
fn name_tokens(name: &str) -> HashSet<String> {
    name.to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Map each act-name to its canonical form: the shortest OTHER name whose tokens
/// are all contained in it (and that carries a distinctive ≥5-char token), so
/// "The Kyle Shepherd Xahuri Trio" → "Kyle Shepherd". Names with no such parent
/// map to themselves.
fn canonicalize(distinct: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for a in distinct {
        let a_tok = name_tokens(a);
        let mut best: Option<&String> = None;
        for b in distinct {
            if a == b {
                continue;
            }
            let b_tok = name_tokens(b);
            let distinctive = b_tok.iter().any(|t| t.len() >= 5);
            if b_tok.len() >= 2 && distinctive && b_tok.is_subset(&a_tok) && b.len() < a.len() {
                if best.map_or(true, |cur| b.len() < cur.len()) {
                    best = Some(b);
                }
            }
        }
        map.insert(a.to_lowercase(), best.unwrap_or(a).clone());
    }
    map
}

/// The act at the heart of an event title — everything before the first
/// structural delimiter (" - ", " with ", " x ", …), original casing kept.
fn core_name(title: &str) -> String {
    let lower = title.to_lowercase();
    let delims = [" - ", " – ", " with ", " x ", " & ", " | ", " plays ", " feat"];
    let mut cut = title.len();
    for d in delims {
        if let Some(i) = lower.find(d) {
            cut = cut.min(i);
        }
    }
    title[..cut].trim().to_string()
}
