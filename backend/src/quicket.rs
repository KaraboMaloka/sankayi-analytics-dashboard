//! Quicket API client.
//!
//! Talks to `https://api.quicket.co.za/api` as the "secure middleman": the API
//! key (query param) and usertoken (header) come from the environment and never
//! leave the backend. Verified endpoints:
//!   * GET users/me/orders   — order-level records; used to DISCOVER event IDs
//!   * GET events/{id}        — event details (name, dates, venue, ticket types)
//!   * GET events/{id}/guests — per-ticket rows (PII-free); the sales detail
//!
//! `users/me/events` is intentionally NOT used — it times out on Quicket's side.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

const BASE: &str = "https://api.quicket.co.za/api";
// The organiser dashboard's internal API — richer (full event list + capacity)
// but needs a short-lived Bearer JWT captured from a logged-in browser session.
const INTERNAL_BASE: &str = "https://www.quicket.co.za/api";

pub struct QuicketClient {
    http: reqwest::Client,
    api_key: String,
    user_token: String,
}

// ---- response shapes (only the fields we use) ----

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrdersPage {
    #[serde(default)]
    pub results: Vec<OrderRow>,
    #[serde(default)]
    pub pages: i64,
    #[serde(default)]
    pub records: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderRow {
    pub event_id: i64,
    #[serde(default)]
    pub event_name: String,
}

// Event detail is parsed as raw JSON (not a struct) — Quicket returns explicit
// `null` for many fields (e.g. organiser.name), which defeats serde defaults.
// The ingest layer extracts the few fields we need with the defensive helpers.

/// Guests come back with loose/PascalCase typing (docs show several fields as
/// `<string>`), so we keep them as raw JSON and extract defensively at map time.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestsPage {
    #[serde(default)]
    pub results: Vec<Value>,
    #[serde(default)]
    pub pages: i64,
    #[serde(default)]
    pub records: i64,
}

// ---- internal (Bearer) API shapes ----

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    payload: T,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProductsPayload {
    #[serde(default)]
    pub user_products: Vec<UserProduct>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserProduct {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    pub date: Option<String>,
    #[serde(default)]
    pub ticket_count: i64,
    pub organiser_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardPayload {
    pub total_guests: Option<TotalGuests>,
    #[serde(default)]
    pub schedule_items: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TotalGuests {
    pub max_capacity: Option<i64>,
    pub seating_capacity: Option<i64>,
}

impl QuicketClient {
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("QUICKET_API_KEY").context("QUICKET_API_KEY not set")?;
        let user_token =
            std::env::var("QUICKET_USER_TOKEN").context("QUICKET_USER_TOKEN not set")?;
        // A User-Agent is required — Quicket sits behind a WAF that 403s requests
        // without one (reqwest sends none by default).
        let http = reqwest::Client::builder()
            .user_agent("SankayiIntelligence/0.1 (+https://sankayi.co.za)")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self { http, api_key, user_token })
    }

    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(&str, String)],
    ) -> Result<T> {
        let url = format!("{BASE}/{path}");
        let mut query: Vec<(&str, String)> = vec![("api_key", self.api_key.clone())];
        query.extend(params.iter().cloned());

        let resp = self
            .http
            .get(&url)
            .query(&query)
            .header("usertoken", &self.user_token)
            .send()
            .await
            // reqwest's transport error embeds the full URL (api_key and all), so
            // scrub the whole cause chain before it can be logged or stored.
            .map_err(|e| {
                anyhow::anyhow!("request to {path} failed: {}", crate::redact::scrub_error_chain(&e))
            })?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            let snippet = &text[..text.len().min(200)];
            anyhow::bail!("{path} returned HTTP {status}: {snippet}");
        }
        serde_json::from_str(&text).with_context(|| format!("failed to parse {path} response"))
    }

    /// Resolve the authenticated user's id from the usertoken.
    pub async fn me(&self) -> Result<i64> {
        let v: Value = self.get("users/me", &[]).await?;
        v.get(0)
            .and_then(|o| o.get("UserId"))
            .and_then(|x| x.as_i64())
            .or_else(|| v.get("UserId").and_then(|x| x.as_i64()))
            .context("could not read UserId from users/me")
    }

    pub async fn orders(&self, page: i64, page_size: i64) -> Result<OrdersPage> {
        self.get(
            "users/me/orders",
            &[("page", page.to_string()), ("pageSize", page_size.to_string())],
        )
        .await
    }

    pub async fn event(&self, id: i64) -> Result<Value> {
        self.get(&format!("events/{id}"), &[]).await
    }

    pub async fn guests(&self, id: i64, page: i64, page_size: i64) -> Result<GuestsPage> {
        self.get(
            &format!("events/{id}/guests"),
            &[("page", page.to_string()), ("pageSize", page_size.to_string())],
        )
        .await
    }

    // ---- internal API (Bearer JWT) ----

    async fn get_internal<T: serde::de::DeserializeOwned>(&self, path: &str, bearer: &str) -> Result<T> {
        let url = format!("{INTERNAL_BASE}/{path}");
        let resp = self
            .http
            .get(&url)
            .header("Authorization", bearer)
            .send()
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "internal request to {path} failed: {}",
                    crate::redact::scrub_error_chain(&e)
                )
            })?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            let snippet = &text[..text.len().min(160)];
            anyhow::bail!("{path} returned HTTP {status}: {snippet}");
        }
        serde_json::from_str(&text).with_context(|| format!("failed to parse {path}"))
    }

    /// Full list of the organiser's own events (all statuses).
    pub async fn internal_products(&self, user_id: i64, bearer: &str) -> Result<ProductsPayload> {
        let env: Envelope<ProductsPayload> = self
            .get_internal(
                &format!("users/{user_id}/products?type=1&pageSize=300&pageNumber=1&search=&sortOrder=0&sortBy=1&statusFilter=null&organiserFilter=null"),
                bearer,
            )
            .await?;
        Ok(env.payload)
    }

    /// Per-event dashboard — carries capacity (totalGuests) and schedule items.
    pub async fn internal_dashboard(&self, event_id: i64, bearer: &str) -> Result<DashboardPayload> {
        let env: Envelope<DashboardPayload> =
            self.get_internal(&format!("product/{event_id}/dashboard"), bearer).await?;
        Ok(env.payload)
    }
}

/// Resolve a real capacity from a dashboard payload, or None if not determinable.
/// seatingCapacity wins; else maxCapacity (ignoring the "no limit" sentinel);
/// multiplied by the number of schedule items (nights).
pub fn resolve_capacity(d: &DashboardPayload) -> Option<i32> {
    let tg = d.total_guests.as_ref()?;
    let base = match tg.seating_capacity {
        Some(s) if s > 0 => s,
        _ => match tg.max_capacity {
            Some(m) if m > 0 && m < 100_000 => m,
            _ => return None,
        },
    };
    let nights = d.schedule_items.len().max(1) as i64;
    Some((base * nights) as i32)
}

// ---- defensive JSON field extractors (guests are loosely typed) ----

pub fn v_str(v: &Value, key: &str) -> Option<String> {
    match v.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

pub fn v_i64(v: &Value, key: &str) -> Option<i64> {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

pub fn v_f64(v: &Value, key: &str) -> Option<f64> {
    match v.get(key) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

pub fn v_bool(v: &Value, key: &str) -> bool {
    match v.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true") || s == "1",
        Some(Value::Number(n)) => n.as_i64().map(|i| i != 0).unwrap_or(false),
        _ => false,
    }
}
