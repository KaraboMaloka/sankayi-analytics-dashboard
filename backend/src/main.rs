//! Sankayi Intelligence API server.
//!
//! Reads config from the environment (see `.env.example`), connects to
//! PostgreSQL, and serves the endpoints in `docs/API_CONTRACTS.md`.

use actix_cors::Cors;
use actix_web::{middleware::Logger, web, App, HttpServer};
use sankayi_intelligence::{db, handlers};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Load .env if present (never committed — holds DATABASE_URL and secrets).
    dotenvy::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set (copy .env.example to .env and fill it in)");
    let bind_addr = std::env::var("BIND_ADDR").unwrap_or_else(|_| "127.0.0.1:8080".to_string());

    let pool = db::connect(&database_url)
        .await
        .expect("failed to connect to PostgreSQL — is the server running and DATABASE_URL correct?");

    log::info!("connected to PostgreSQL; starting server on http://{bind_addr}");

    HttpServer::new(move || {
        // Dev CORS: allow the Vite frontend (Phase 2/3) to call the API.
        let cors = Cors::default()
            .allow_any_origin()
            .allow_any_method()
            .allow_any_header()
            .max_age(3600);

        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap(Logger::default())
            .wrap(cors)
            .configure(handlers::configure)
    })
    .bind(&bind_addr)?
    .run()
    .await
}
