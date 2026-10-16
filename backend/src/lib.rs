//! Sankayi Intelligence — Rust data service & REST API.
//!
//! Phase 1 delivered the **type layer**: domain models mirroring the Postgres
//! schema, JSON DTOs mirroring `docs/API_CONTRACTS.md`, and the `DataProvider`
//! trait with stubs.
//!
//! Phase 2 adds the **live API**: an Actix Web server (`src/main.rs`) with
//! `sqlx` handlers that serve the contracts as real JSON from PostgreSQL.

pub mod auth;
pub mod db;
pub mod error;
pub mod handlers;
pub mod instagram;
pub mod metaads;
pub mod models;
pub mod providers;
pub mod quicket;
pub mod redact;
pub mod yoco;

/// Money on the wire is ZAR **cents** (see the data dictionary). This alias
/// documents intent wherever a cent-denominated integer is used.
pub type Cents = i64;
