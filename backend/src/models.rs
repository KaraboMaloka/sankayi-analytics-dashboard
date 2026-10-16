//! Domain models and API DTOs for Sankayi Intelligence.
//!
//! Two layers live here on purpose:
//!   * **Domain structs** map 1:1 onto the Postgres tables in `db/schema.sql`.
//!   * **`api` DTOs** map 1:1 onto the JSON in `docs/API_CONTRACTS.md`.
//!
//! Keeping them separate means the wire format can stay "beautifully boring"
//! and stable even as the storage layer evolves.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use uuid::Uuid;

use crate::Cents;

// =============================================================================
// Enums (mirror the Postgres enum types)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "data_source", rename_all = "snake_case")]
pub enum DataSource {
    Instagram,
    Quicket,
    MetaAds,
    Yoco,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "event_status", rename_all = "snake_case")]
pub enum EventStatus {
    Draft,
    Announced,
    OnSale,
    SoldOut,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "sale_source", rename_all = "snake_case")]
pub enum SaleSource {
    Quicket,
    Door,
    Complimentary,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "post_type", rename_all = "snake_case")]
pub enum PostType {
    Image,
    Carousel,
    Reel,
    Video,
    Story,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "campaign_platform", rename_all = "snake_case")]
pub enum CampaignPlatform {
    MetaAds,
    Instagram,
    Facebook,
    Google,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[serde(rename_all = "snake_case")]
#[sqlx(type_name = "attribution_kind", rename_all = "snake_case")]
pub enum AttributionKind {
    Direct,
    Influenced,
    Unknown,
}

// =============================================================================
// Provenance — embedded on every ingested domain struct
// =============================================================================

/// The audit trail attached to any row pulled from an external API. Lets us
/// trace a number back to the exact response that produced it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub source: DataSource,
    pub source_id: Option<String>,
    pub source_timestamp: Option<DateTime<Utc>>,
    pub retrieved_at: DateTime<Utc>,
    /// Original API response, kept where practical for later investigation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_payload: Option<Json>,
}

// =============================================================================
// Domain structs (mirror db/schema.sql)
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artist {
    pub id: Uuid,
    pub name: String,
    pub genre: Option<String>,
    pub agency: Option<String>,
    pub instagram_handle: Option<String>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Uuid,
    pub artist_id: Option<Uuid>,
    pub title: String,
    pub event_date: NaiveDate,
    pub doors_time: Option<NaiveTime>,
    pub venue: String,
    pub capacity: i32,
    pub ticket_price_cents: Cents,
    pub status: EventStatus,
    pub campaign_tag: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketSale {
    pub id: Uuid,
    pub event_id: Uuid,
    pub ticket_type: String,
    pub quantity: i32,
    pub price_cents: Cents,
    pub sold_at: DateTime<Utc>,
    pub sale_source: SaleSource,
    pub attributed_campaign_tag: Option<String>,
    pub attribution: AttributionKind,
    #[serde(flatten)]
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstagramPost {
    pub id: Uuid,
    pub event_id: Option<Uuid>,
    pub post_type: PostType,
    pub caption: Option<String>,
    pub permalink: Option<String>,
    pub published_at: DateTime<Utc>,
    pub reach: Option<i32>,
    pub impressions: Option<i32>,
    pub likes: Option<i32>,
    pub comments: Option<i32>,
    pub shares: Option<i32>,
    pub saves: Option<i32>,
    pub profile_visits: Option<i32>,
    pub video_views: Option<i32>,
    #[serde(flatten)]
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Campaign {
    pub id: Uuid,
    pub event_id: Option<Uuid>,
    pub campaign_name: String,
    pub campaign_tag: Option<String>,
    pub platform: CampaignPlatform,
    pub budget_cents: Option<Cents>,
    pub spend_cents: Option<Cents>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    #[serde(flatten)]
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CampaignMetric {
    pub id: Uuid,
    pub campaign_id: Uuid,
    pub metric_date: NaiveDate,
    pub reach: Option<i32>,
    pub impressions: Option<i32>,
    pub clicks: Option<i32>,
    pub engagements: Option<i32>,
    pub spend_cents: Option<Cents>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyAccountMetric {
    pub id: Uuid,
    pub snapshot_date: NaiveDate,
    pub followers: Option<i32>,
    pub reach: Option<i32>,
    pub impressions: Option<i32>,
    pub profile_visits: Option<i32>,
    pub engagements: Option<i32>,
}

// =============================================================================
// API DTOs (mirror docs/API_CONTRACTS.md). Money as *_cents, ratios as f64.
// =============================================================================

pub mod api {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Period {
        pub from: Option<NaiveDate>,
        pub to: Option<NaiveDate>,
    }

    /// `GET /api/dashboard`
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct DashboardResponse {
        pub period: Period,
        pub tickets_sold: i64,
        pub revenue_cents: Cents,
        pub sell_through: f64,
        pub followers: i64,
        pub followers_growth: f64,
        pub ad_spend_cents: Cents,
        pub roas: f64,
        pub generated_at: DateTime<Utc>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ArtistRef {
        pub id: Uuid,
        pub name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub genre: Option<String>,
    }

    /// One row in `GET /api/events`
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EventSummary {
        pub id: Uuid,
        pub title: String,
        pub event_date: NaiveDate,
        pub artist: Option<ArtistRef>,
        pub capacity: i32,
        pub ticket_price_cents: Cents,
        pub status: EventStatus,
        pub tickets_sold: i64,
        pub revenue_cents: Cents,
        pub sell_through: f64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EventsResponse {
        pub events: Vec<EventSummary>,
        pub count: usize,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SalesByType {
        pub ticket_type: String,
        pub quantity: i64,
        pub revenue_cents: Cents,
    }

    /// Tickets grouped by how they were sold (`sale_source`): quicket / door /
    /// complimentary / manual. The frontend labels `quicket` as "Online".
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SalesByChannel {
        pub channel: String,
        pub quantity: i64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct VelocityPoint {
        pub date: NaiveDate,
        pub cumulative_tickets: i64,
    }

    /// `GET /api/events/:id`
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct EventDetail {
        pub id: Uuid,
        pub title: String,
        pub event_date: NaiveDate,
        pub artist: Option<ArtistRef>,
        pub capacity: i32,
        pub ticket_price_cents: Cents,
        pub status: EventStatus,
        pub tickets_sold: i64,
        pub revenue_cents: Cents,
        pub sell_through: f64,
        pub sales_by_type: Vec<SalesByType>,
        pub sales_by_channel: Vec<SalesByChannel>,
        pub sales_velocity: Vec<VelocityPoint>,
        pub campaigns: Vec<CampaignSummary>,
        pub posts: Vec<PostSummary>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CampaignSummary {
        pub id: Uuid,
        pub campaign_name: String,
        pub spend_cents: Cents,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PostSummary {
        pub id: Uuid,
        pub post_type: PostType,
        pub reach: Option<i32>,
        pub engagement_rate: Option<f64>,
    }

    /// One row in `GET /api/artists`
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ArtistLeaderboardRow {
        pub id: Uuid,
        pub name: String,
        pub genre: Option<String>,
        pub events_count: i64,
        pub tickets_sold: i64,
        pub revenue_cents: Cents,
        pub avg_sell_through: f64,
        pub avg_engagement_rate: f64,
        pub roi: f64,
        pub performance_score: f64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ArtistsResponse {
        pub artists: Vec<ArtistLeaderboardRow>,
    }

    /// Weighted 0–100 components — see the Artist Score table in the dictionary.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ScoreBreakdown {
        pub ticket_performance: f64,
        pub revenue: f64,
        pub sell_through: f64,
        pub social_engagement: f64,
        pub audience_growth: f64,
        pub marketing_efficiency: f64,
    }

    /// `GET /api/social`
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct AccountSeriesPoint {
        pub date: NaiveDate,
        pub followers: Option<i32>,
        pub reach: Option<i32>,
        pub engagements: Option<i32>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SocialResponse {
        pub account_series: Vec<AccountSeriesPoint>,
        pub followers_growth: f64,
        pub top_posts: Vec<PostSummary>,
    }

    /// One row in `GET /api/campaigns`
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct CampaignPerformanceRow {
        pub id: Uuid,
        pub campaign_name: String,
        pub campaign_tag: Option<String>,
        pub platform: CampaignPlatform,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub permalink: Option<String>,
        pub spend_cents: Cents,
        pub reach: i64,
        pub clicks: i64,
        pub engagements: i64,
        pub attributed_tickets: i64,
        pub attributed_revenue_cents: Cents,
        pub cost_per_ticket_cents: Option<Cents>,
        pub cost_per_engagement_cents: Option<Cents>,
        pub ticket_conversion: Option<f64>,
        pub roas: Option<f64>,
    }

    /// `GET /api/forecasts` (Phase 5; stub for now)
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Forecast {
        pub artist_id: Uuid,
        pub artist_name: String,
        pub expected_tickets: i64,
        pub range_low: i64,
        pub range_high: i64,
        pub confidence: f64,
        pub expected_revenue_cents: Cents,
        pub expected_sell_through: f64,
        pub recommended_budget_cents: Cents,
        pub rationale: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SeasonPoint {
        pub month: String,
        pub shows: i64,
        pub avg_sell_through: f64,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ForecastsResponse {
        pub model: String,
        pub forecasts: Vec<Forecast>,
        /// Series-level demand by month — the "when to perform" signal.
        #[serde(default)]
        pub seasonality: Vec<SeasonPoint>,
    }

    /// `GET /api/health`
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct SourceHealth {
        pub source: DataSource,
        pub status: String,
        pub last_synced: Option<DateTime<Utc>>,
        pub rows: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub message: Option<String>,
        /// Statuses of the most recent runs, oldest→newest (for a health strip).
        #[serde(default)]
        pub recent: Vec<String>,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct HealthResponse {
        pub sources: Vec<SourceHealth>,
        pub database: String,
    }

    /// Shared error envelope for non-2xx responses.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ApiError {
        pub error: ApiErrorBody,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct ApiErrorBody {
        pub code: String,
        pub message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub request_id: Option<String>,
    }
}
