//! External data providers.
//!
//! Every external system (Instagram/Meta, Quicket, Meta Ads) is reached through
//! the same `DataProvider` trait. This is the seam that keeps the rest of the
//! codebase honest: analytics code depends on the trait, not on any one API.
//!
//! Phase 1 ships the trait + **stub** implementations returning empty data, so
//! the API and dashboard can be built end-to-end. Phase 2 fills the stubs with
//! real HTTP calls (behind the secure Rust backend — keys never reach the
//! browser).

use async_trait::async_trait;
use chrono::Utc;
use thiserror::Error;

use crate::models::{
    Campaign, DailyAccountMetric, DataSource, InstagramPost, Provenance, TicketSale,
};

/// Errors any provider can raise. Surfaces as `upstream_error` (502) in the API.
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("authentication failed for {0:?}")]
    Auth(DataSource),
    #[error("rate limited by {0:?}")]
    RateLimited(DataSource),
    #[error("network/transport error: {0}")]
    Transport(String),
    // NB: `thiserror` reserves a field literally named `source` as the error
    // cause (must impl std::error::Error). `DataSource` is a plain enum, so we
    // name the field `provider` to avoid that special-casing.
    #[error("unexpected response from {provider:?}: {detail}")]
    Unexpected { provider: DataSource, detail: String },
    #[error("provider not yet implemented: {0:?}")]
    NotImplemented(DataSource),
}

pub type ProviderResult<T> = Result<T, ProviderError>;

/// What a data source can fetch. A provider implements only the methods that
/// make sense for it and returns `NotImplemented` for the rest.
///
/// `since` lets ingestion pull incrementally; `None` means a full backfill.
#[async_trait]
pub trait DataProvider: Send + Sync {
    /// Which external system this provider talks to.
    fn source(&self) -> DataSource;

    async fn fetch_ticket_sales(
        &self,
        _since: Option<chrono::DateTime<Utc>>,
    ) -> ProviderResult<Vec<TicketSale>> {
        Err(ProviderError::NotImplemented(self.source()))
    }

    async fn fetch_instagram_posts(
        &self,
        _since: Option<chrono::DateTime<Utc>>,
    ) -> ProviderResult<Vec<InstagramPost>> {
        Err(ProviderError::NotImplemented(self.source()))
    }

    async fn fetch_daily_account_metrics(
        &self,
        _since: Option<chrono::DateTime<Utc>>,
    ) -> ProviderResult<Vec<DailyAccountMetric>> {
        Err(ProviderError::NotImplemented(self.source()))
    }

    async fn fetch_campaigns(
        &self,
        _since: Option<chrono::DateTime<Utc>>,
    ) -> ProviderResult<Vec<Campaign>> {
        Err(ProviderError::NotImplemented(self.source()))
    }
}

/// Helper to stamp provenance on freshly-fetched rows.
pub fn provenance_now(source: DataSource, source_id: Option<String>) -> Provenance {
    Provenance {
        source,
        source_id,
        source_timestamp: None,
        retrieved_at: Utc::now(),
        raw_payload: None,
    }
}

// -----------------------------------------------------------------------------
// Stubs — Phase 1. Each returns empty data and records its intended shape.
// Real implementations (reqwest against the Graph API / Quicket REST API) land
// in Phase 2. Config (tokens, account ids) will come from env/secrets, never
// hard-coded and never sent to the frontend.
// -----------------------------------------------------------------------------

/// Instagram / Meta Graph API. Requires a Business/Creator account + approved
/// permissions on the Sankayi account.
pub struct InstagramProvider {
    // pub access_token: String,
    // pub ig_business_account_id: String,
    // pub http: reqwest::Client,
}

#[async_trait]
impl DataProvider for InstagramProvider {
    fn source(&self) -> DataSource {
        DataSource::Instagram
    }

    async fn fetch_instagram_posts(
        &self,
        _since: Option<chrono::DateTime<Utc>>,
    ) -> ProviderResult<Vec<InstagramPost>> {
        // TODO(phase2): GET /{ig-user-id}/media then /{media-id}/insights
        Ok(Vec::new())
    }

    async fn fetch_daily_account_metrics(
        &self,
        _since: Option<chrono::DateTime<Utc>>,
    ) -> ProviderResult<Vec<DailyAccountMetric>> {
        // TODO(phase2): GET /{ig-user-id}/insights (follower_count, reach, ...)
        Ok(Vec::new())
    }
}

/// Quicket ticketing REST API. Keys stay server-side (Quicket's own docs warn
/// against exposing tokens in public-facing code).
pub struct QuicketProvider {
    // pub api_key: String,
    // pub http: reqwest::Client,
}

#[async_trait]
impl DataProvider for QuicketProvider {
    fn source(&self) -> DataSource {
        DataSource::Quicket
    }

    async fn fetch_ticket_sales(
        &self,
        _since: Option<chrono::DateTime<Utc>>,
    ) -> ProviderResult<Vec<TicketSale>> {
        // TODO(phase2): Quicket sales/orders endpoint -> TicketSale rows.
        Ok(Vec::new())
    }
}

/// Meta Ads (Marketing API) — campaign spend & performance.
pub struct MetaAdsProvider {
    // pub access_token: String,
    // pub ad_account_id: String,
    // pub http: reqwest::Client,
}

#[async_trait]
impl DataProvider for MetaAdsProvider {
    fn source(&self) -> DataSource {
        DataSource::MetaAds
    }

    async fn fetch_campaigns(
        &self,
        _since: Option<chrono::DateTime<Utc>>,
    ) -> ProviderResult<Vec<Campaign>> {
        // TODO(phase2): GET /act_{ad-account-id}/campaigns + insights.
        Ok(Vec::new())
    }
}
