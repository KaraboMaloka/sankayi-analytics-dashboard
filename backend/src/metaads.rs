//! Meta Ads (Marketing API) client.
//!
//! Reads the boosted-post ad account through the secure backend — the System
//! User token (shared with the Instagram client) and the ad account id come from
//! the environment and never reach the browser.
//!
//! Verified against act_90078902 (ZAR, v26.0): boosted IG posts appear as
//! campaigns (objective LINK_CLICKS). Endpoints used:
//!   * GET /{ad-account}/campaigns          — campaign list (name, objective, …)
//!   * GET /{campaign-id}/insights          — per-day spend/reach/impressions/…
//!
//! Money caution (a real Meta quirk): insights `spend` is a decimal in the
//! account CURRENCY (e.g. "235.23"), while budget fields (daily_budget /
//! lifetime_budget) are integer MINOR units (cents). The ingest converts each
//! accordingly. Responses are kept as raw JSON and read defensively, like the
//! Quicket/Instagram clients.

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

pub struct MetaAdsClient {
    http: reqwest::Client,
    ad_account_id: String,
    token: String,
    base: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct Page {
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

impl MetaAdsClient {
    pub fn from_env() -> Result<Self> {
        let ad_account_id =
            std::env::var("META_AD_ACCOUNT_ID").context("META_AD_ACCOUNT_ID not set")?;
        // Same System User token as the Instagram client (needs ads_read scope).
        let token = std::env::var("META_ACCESS_TOKEN")
            .or_else(|_| std::env::var("IG_ACCESS_TOKEN"))
            .context("META_ACCESS_TOKEN / IG_ACCESS_TOKEN not set")?;
        let version = std::env::var("META_GRAPH_VERSION").unwrap_or_else(|_| "v26.0".to_string());
        let base = format!("https://graph.facebook.com/{version}");
        let http = reqwest::Client::builder()
            .user_agent("SankayiIntelligence/0.1 (+https://sankayi.co.za)")
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Self { http, ad_account_id, token, base })
    }

    pub fn ad_account_id(&self) -> &str {
        &self.ad_account_id
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, params: &[(&str, String)]) -> Result<T> {
        let url = format!("{}/{}", self.base, path);
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
            let snippet = &text[..text.len().min(300)];
            anyhow::bail!("{path} returned HTTP {status}: {snippet}");
        }
        serde_json::from_str(&text).with_context(|| format!("failed to parse {path} response"))
    }

    /// One page of campaigns. `after` is the paging cursor (`None` = first page).
    pub async fn campaigns(&self, after: Option<&str>) -> Result<Page> {
        let mut params = vec![
            (
                "fields",
                "id,name,objective,status,start_time,stop_time,daily_budget,lifetime_budget"
                    .to_string(),
            ),
            ("limit", "100".to_string()),
        ];
        if let Some(a) = after {
            params.push(("after", a.to_string()));
        }
        let path = format!("{}/campaigns", self.ad_account_id);
        self.get(&path, &params).await
    }

    /// The Instagram permalink of the post a campaign boosted (its first ad's
    /// creative). `None` if the boost isn't an Instagram post or has no creative.
    pub async fn campaign_permalink(&self, campaign_id: &str) -> Result<Option<String>> {
        let path = format!("{campaign_id}/ads");
        // Nested field expansion works on this edge; reqwest URL-encodes the braces.
        let v: Value = self
            .get(
                &path,
                &[
                    ("fields", "creative{instagram_permalink_url}".to_string()),
                    ("limit", "1".to_string()),
                ],
            )
            .await?;
        Ok(v.get("data")
            .and_then(|d| d.as_array())
            .and_then(|a| a.first())
            .and_then(|ad| ad.get("creative"))
            .and_then(|c| c.get("instagram_permalink_url"))
            .and_then(|u| u.as_str())
            .map(|s| s.to_string()))
    }

    /// Per-day insights for one campaign over its full lifetime. Raw envelope;
    /// each `data` row carries spend/reach/impressions/clicks/actions + dates.
    pub async fn campaign_daily_insights(&self, campaign_id: &str) -> Result<Value> {
        let path = format!("{campaign_id}/insights");
        self.get(
            &path,
            &[
                (
                    "fields",
                    "spend,reach,impressions,clicks,inline_link_clicks,actions".to_string(),
                ),
                ("time_increment", "1".to_string()),
                ("date_preset", "maximum".to_string()),
            ],
        )
        .await
    }
}

// ---- defensive extractors ----

/// Read a numeric field that Meta returns as a string (e.g. spend "235.23").
pub fn num_str(v: &Value, key: &str) -> Option<f64> {
    match v.get(key) {
        Some(Value::String(s)) => s.parse().ok(),
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
}

/// Pull one action's value out of the `actions` array (values are strings).
pub fn action_value(v: &Value, action_type: &str) -> Option<i64> {
    let arr = v.get("actions")?.as_array()?;
    for a in arr {
        if a.get("action_type").and_then(|x| x.as_str()) == Some(action_type) {
            return a.get("value").and_then(|x| match x {
                Value::String(s) => s.parse::<f64>().ok().map(|f| f as i64),
                Value::Number(n) => n.as_i64(),
                _ => None,
            });
        }
    }
    None
}
