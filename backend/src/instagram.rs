//! Instagram Graph API client.
//!
//! Talks to `https://graph.facebook.com/{version}` as the "secure middleman":
//! the access token (a Meta System User token — never expires) and the IG
//! Business Account id come from the environment and never leave the backend.
//!
//! Verified working (Graph API Explorer, v26.0, sankayijazz / 17841431955485624):
//!   * GET /{ig-user-id}/media          — posts (id, caption, type, permalink, …)
//!   * GET /{media-id}/insights         — per-post reach / saves / shares / views
//!   * GET /{ig-user-id}?fields=followers_count  — absolute follower total
//!   * GET /{ig-user-id}/insights       — account-level reach / profile_views
//!
//! Mirrors the Quicket client's shape on purpose: a thin typed client here, the
//! DB-writing logic in `bin/ingest_instagram.rs`. Insight numbers come back
//! loosely shaped (metric availability varies by media type), so — like the
//! Quicket guests — we keep responses as raw JSON and extract defensively.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

pub struct InstagramClient {
    http: reqwest::Client,
    user_id: String,
    token: String,
    base: String,
}

// ---- response shapes (only the envelope; rows stay as raw Value) ----

#[derive(Debug, Deserialize, Default)]
pub struct MediaPage {
    #[serde(default)]
    pub data: Vec<Value>,
    #[serde(default)]
    pub paging: Paging,
}

#[derive(Debug, Deserialize, Default)]
pub struct Paging {
    #[serde(default)]
    pub cursors: Cursors,
}

#[derive(Debug, Deserialize, Default)]
pub struct Cursors {
    #[serde(default)]
    pub after: Option<String>,
}

impl InstagramClient {
    pub fn from_env() -> Result<Self> {
        let user_id = std::env::var("IG_USER_ID").context("IG_USER_ID not set")?;
        let token = std::env::var("IG_ACCESS_TOKEN").context("IG_ACCESS_TOKEN not set")?;
        let version = std::env::var("META_GRAPH_VERSION").unwrap_or_else(|_| "v26.0".to_string());
        let base = format!("https://graph.facebook.com/{version}");
        let http = reqwest::Client::builder()
            .user_agent("SankayiIntelligence/0.1 (+https://sankayi.co.za)")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self { http, user_id, token, base })
    }

    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, params: &[(&str, String)]) -> Result<T> {
        let url = format!("{}/{}", self.base, path);
        // access_token as a query param (not a header) — that's how Graph wants it.
        let mut query: Vec<(&str, String)> = vec![("access_token", self.token.clone())];
        query.extend(params.iter().cloned());

        let resp = self
            .http
            .get(&url)
            .query(&query)
            .send()
            .await
            // reqwest's transport error embeds the full URL (token and all), so
            // scrub the whole cause chain before it can be logged or stored.
            .map_err(|e| {
                anyhow::anyhow!("request to {path} failed: {}", crate::redact::scrub_error_chain(&e))
            })?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            // Graph errors are JSON; surface a trimmed snippet (never the token).
            let snippet = &text[..text.len().min(300)];
            anyhow::bail!("{path} returned HTTP {status}: {snippet}");
        }
        serde_json::from_str(&text).with_context(|| format!("failed to parse {path} response"))
    }

    /// One page of media. `after` is the paging cursor (`None` for the first page).
    pub async fn media(&self, after: Option<&str>, page_size: i64) -> Result<MediaPage> {
        let mut params = vec![
            (
                "fields",
                "id,caption,media_type,media_product_type,permalink,timestamp,like_count,comments_count"
                    .to_string(),
            ),
            ("limit", page_size.to_string()),
        ];
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        let path = format!("{}/media", self.user_id);
        self.get(&path, &params).await
    }

    /// Insights for a single media object. Returns the raw envelope (its `data`
    /// array may be empty or absent — extract with [`insight_value`]).
    pub async fn media_insights(&self, media_id: &str, metrics: &str) -> Result<Value> {
        let path = format!("{media_id}/insights");
        self.get(&path, &[("metric", metrics.to_string())]).await
    }

    /// Absolute account snapshot fields (followers_count, media_count). This is
    /// the total follower count — what makes day-over-day growth computable.
    pub async fn account_fields(&self) -> Result<Value> {
        let path = self.user_id.clone();
        self.get(&path, &[("fields", "followers_count,media_count".to_string())]).await
    }

    /// Account-level daily insight in the classic time-series shape (e.g. `reach`).
    pub async fn account_insights(&self, metric: &str, period: &str) -> Result<Value> {
        let path = format!("{}/insights", self.user_id);
        self.get(&path, &[("metric", metric.to_string()), ("period", period.to_string())])
            .await
    }

    /// Account-level insight that v26 requires as `metric_type=total_value`
    /// (e.g. `profile_views`, `accounts_engaged`). Value lands under `total_value`.
    pub async fn account_insights_total(&self, metric: &str) -> Result<Value> {
        let path = format!("{}/insights", self.user_id);
        self.get(
            &path,
            &[
                ("metric", metric.to_string()),
                ("period", "day".to_string()),
                ("metric_type", "total_value".to_string()),
            ],
        )
        .await
    }

    /// Per-day time-series for an account metric over a range (max 30 days per
    /// call — the caller chunks). Used for historical backfill of `reach`,
    /// `follower_count`, etc. `since`/`until` are unix seconds.
    pub async fn account_timeseries(&self, metric: &str, since: i64, until: i64) -> Result<Value> {
        let path = format!("{}/insights", self.user_id);
        self.get(
            &path,
            &[
                ("metric", metric.to_string()),
                ("period", "day".to_string()),
                ("since", since.to_string()),
                ("until", until.to_string()),
            ],
        )
        .await
    }

    /// Currently-active stories (they vanish after 24h). Same envelope as media.
    pub async fn stories(&self) -> Result<MediaPage> {
        let path = format!("{}/stories", self.user_id);
        self.get(
            &path,
            &[(
                "fields",
                "id,caption,media_type,media_product_type,permalink,timestamp".to_string(),
            )],
        )
        .await
    }
}

// ---- defensive extractors (insight shapes vary by media type / API version) ----

/// Pull the value for a named metric out of an insights envelope. Handles BOTH
/// v26 shapes: the classic time-series `values:[{value}]` (takes the newest) and
/// the newer `total_value:{value}` (required by profile_views, story metrics, …).
pub fn insight_value(env: &Value, name: &str) -> Option<i64> {
    let arr = env.get("data")?.as_array()?;
    for m in arr {
        if m.get("name").and_then(|n| n.as_str()) == Some(name) {
            // Newer aggregate shape first.
            if let Some(tv) = m.get("total_value").and_then(|t| t.get("value")).and_then(Value::as_i64) {
                return Some(tv);
            }
            // Classic time-series: last entry is the most recent.
            if let Some(values) = m.get("values").and_then(Value::as_array) {
                return values.last()?.get("value")?.as_i64();
            }
        }
    }
    None
}

/// Read a top-level integer field from a node response (e.g. followers_count).
pub fn field_i64(v: &Value, key: &str) -> Option<i64> {
    v.get(key).and_then(|x| x.as_i64())
}

/// Extract a time-series as `(end_time, value)` pairs from a `period=day`
/// insights envelope. Empty if the metric/shape isn't present.
pub fn insight_series(env: &Value, name: &str) -> Vec<(String, i64)> {
    let mut out = Vec::new();
    let Some(arr) = env.get("data").and_then(Value::as_array) else { return out };
    for m in arr {
        if m.get("name").and_then(|n| n.as_str()) == Some(name) {
            if let Some(values) = m.get("values").and_then(Value::as_array) {
                for v in values {
                    let et = v.get("end_time").and_then(|x| x.as_str()).unwrap_or("").to_string();
                    if let Some(val) = v.get("value").and_then(Value::as_i64) {
                        out.push((et, val));
                    }
                }
            }
        }
    }
    out
}
