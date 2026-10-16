//! Seed the three dashboard users with generated passwords (Argon2-hashed).
//!
//! Idempotent: existing users are left untouched (their passwords are NOT reset),
//! so re-running is safe. Prints the credentials for any user it creates.
//!
//! Usage: cargo run --bin seed_users

use anyhow::{Context, Result};
use argon2::password_hash::rand_core::{OsRng, RngCore};

use sankayi_intelligence::{auth, db};

const USERS: &[(&str, &str)] = &[
    ("alex", "Alex Hitzeroth"),
    ("khotso", "Khotso Moleta"),
    ("karabo", "Karabo Maloka"),
];

// Readable-but-random passwords: Word-word-#### (easy to type once, then change).
const WORDS: &[&str] = &[
    "amber", "brass", "swing", "tempo", "encore", "velvet", "cadence", "rhythm",
    "sonata", "treble", "harbor", "marimba", "quartz", "meadow", "cobalt", "ember",
    "lyric", "verve", "octave", "ballad", "improv", "reprise", "crescent", "nocturne",
];

fn word() -> &'static str {
    WORDS[(OsRng.next_u32() as usize) % WORDS.len()]
}

fn gen_password() -> String {
    let mut a = word();
    let mut b = word();
    while b == a {
        b = word();
    }
    a = Box::leak(capitalize(a).into_boxed_str());
    let digits = OsRng.next_u32() % 10_000;
    format!("{a}-{b}-{digits:04}")
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let db_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let pool = db::connect(&db_url).await.context("connecting to PostgreSQL")?;

    println!("\n=== Sankayi dashboard users ===");
    for (username, display) in USERS {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE username = $1)")
                .bind(username)
                .fetch_one(&pool)
                .await?;
        if exists {
            println!("  {username:8}  (already exists — password unchanged)");
            continue;
        }
        let password = gen_password();
        let hash = auth::hash_password(&password)?;
        sqlx::query("INSERT INTO users (username, display_name, password_hash) VALUES ($1, $2, $3)")
            .bind(username)
            .bind(display)
            .bind(&hash)
            .execute(&pool)
            .await
            .with_context(|| format!("creating user {username}"))?;
        println!("  {username:8}  {password:20}  ({display})");
    }
    println!("\nShare these with each person; they can change their password in Settings.\n");
    Ok(())
}
