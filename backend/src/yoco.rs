//! Yoco API client — reads in-person card-machine sales for door tickets.
//!
//! The venue's Yoco account is the restaurant POS too, so a raw payment feed
//! mixes bar/food with tickets. The clean signal is the **Orders API**: each
//! order carries `line_items` with a product `name`, so we match the ticket
//! product ("Sankayi Jazz Series") and read its exact `quantity` and
//! `unit_price` — bar/food are simply other line items and ignored.
//!
//!   * GET /v1/orders  — completed orders with line_items[] + payments[]
//!
//! Auth: an API key from the Yoco Developer Console (scope `business/orders:read`),
//! sent as `Authorization: Bearer <key>`. The key comes from the environment and
//! never leaves the backend. Amounts are ZAR **cents** (integer minor units),
//! matching the rest of the platform. Responses are kept as raw JSON and read
//! defensively, like the Quicket/Instagram/Meta clients.

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;

const BASE: &str = "https://api.yoco.com/v1";
// If Yoco's Retry-After exceeds this, stop rather than block for minutes.
const MAX_BACKOFF_SECS: u64 = 90;

pub struct YocoClient {
    http: reqwest::Client,
    token: String,
    /// Product name that identifies a door ticket line item (case-insensitive
    /// "contains" match, so per-night variants still catch).
    ticket_product: String,
}

/// One page of orders. Rows stay as raw `Value` (line_items / payments vary).
#[derive(Debug, Deserialize, Default)]
pub struct OrdersPage {
    #[serde(default)]
    pub data: Vec<Value>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

impl YocoClient {
    pub fn from_env() -> Result<Self> {
        let token = std::env::var("YOCO_API_KEY").context("YOCO_API_KEY not set")?;
        let ticket_product =
            std::env::var("YOCO_TICKET_PRODUCT").unwrap_or_else(|_| "Sankayi Jazz Series".to_string());
        let http = reqwest::Client::builder()
            .user_agent("SankayiIntelligence/0.1 (+https://sankayi.co.za)")
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self { http, token, ticket_product })
    }

    pub fn ticket_product(&self) -> &str {
        &self.ticket_product
    }

    /// One page of completed orders in `[gte, lte]` (ISO 8601, max 31-day span).
    /// `cursor` is `None` for the first page. Yoco rate-limits aggressively and
    /// publishes no numbers, so on HTTP 429 we honour the `Retry-After` header
    /// and retry (a few times) rather than failing the whole sync.
    pub async fn orders(&self, gte: &str, lte: &str, cursor: Option<&str>) -> Result<OrdersPage> {
        let query: Vec<(&str, String)> = {
            let mut q = vec![
                ("status", "completed".to_string()),
                ("created_at__gte", gte.to_string()),
                ("created_at__lte", lte.to_string()),
                ("limit", "100".to_string()),
            ];
            if let Some(c) = cursor {
                q.push(("cursor", c.to_string()));
            }
            q
        };

        let mut attempt = 0;
        loop {
            let resp = self
                .http
                .get(format!("{BASE}/orders/"))
                .bearer_auth(&self.token)
                .query(&query)
                .send()
                .await
                // Token is a header, not in the URL, but scrub defensively anyway.
                .map_err(|e| anyhow::anyhow!("request to /orders failed: {}", crate::redact::scrub_error_chain(&e)))?;

            let status = resp.status();

            if status.as_u16() == 429 {
                attempt += 1;
                // Respect Retry-After (seconds); fall back to exponential backoff.
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.trim().parse::<u64>().ok())
                    .unwrap_or_else(|| 2u64.pow(attempt.min(6)));
                // A long cooldown means the account's quota is spent — don't hang
                // the process (or the nightly job) for many minutes. Bail clearly;
                // the next scheduled run picks up where we left off (idempotent).
                if wait > MAX_BACKOFF_SECS || attempt > 5 {
                    anyhow::bail!(
                        "/orders rate-limited (429); Yoco asked to wait {wait}s. Try again later — \
                         the sync is idempotent and will resume."
                    );
                }
                log::warn!("/orders 429 — waiting {wait}s (attempt {attempt})");
                tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                continue;
            }

            let text = resp.text().await?;
            if !status.is_success() {
                let snippet = &text[..text.len().min(300)];
                anyhow::bail!("/orders returned HTTP {status}: {snippet}");
            }
            return serde_json::from_str(&text).with_context(|| "failed to parse /orders response");
        }
    }
}

// ---- defensive extractors (Yoco order/line-item shapes read loosely) ----

/// Is this an in-person (door) order rather than an online checkout? This
/// account labels in-person card taps with `payment_source` "other" or
/// "card_machine" (not just "card_machine"), so we identify door sales by
/// *excluding* online payments (checkout / payment_link) instead. Online ticket
/// sales go through Quicket anyway, so anything here with the ticket product is a
/// door sale.
pub fn is_in_person(order: &Value) -> bool {
    let Some(ps) = order.get("payments").and_then(Value::as_array) else { return false };
    ps.iter().any(|p| {
        let src = p.get("payment_source").and_then(Value::as_str).unwrap_or("");
        let is_online = matches!(src, "checkout" | "payment_link")
            || p.get("checkout_id").map(Value::is_string).unwrap_or(false);
        !is_online
    })
}

/// Sum quantity and capture unit price (cents) for line items whose product
/// `name` contains `product` (case-insensitive). Returns `(quantity, unit_price_cents)`
/// or `None` if the ticket product isn't on this order.
pub fn ticket_line(order: &Value, product: &str) -> Option<(i64, i64)> {
    let needle = product.to_ascii_lowercase();
    let items = order.get("line_items").and_then(Value::as_array)?;
    let mut qty = 0i64;
    let mut unit_cents = 0i64;
    for it in items {
        let name = it.get("name").and_then(Value::as_str).unwrap_or("");
        if !name.to_ascii_lowercase().contains(&needle) {
            continue;
        }
        let q = flex_i64(it.get("quantity")).unwrap_or(0);
        if q <= 0 {
            continue;
        }
        qty += q;
        // Prefer the newest non-zero unit_price we see.
        if let Some(u) = money_cents(it.get("unit_price")) {
            unit_cents = u;
        }
    }
    if qty > 0 {
        Some((qty, unit_cents))
    } else {
        None
    }
}

/// The order's completion/creation timestamp (RFC3339), for event-night mapping.
pub fn order_timestamp(order: &Value) -> Option<String> {
    for key in ["closed_at", "created_at"] {
        if let Some(s) = order.get(key).and_then(Value::as_str) {
            return Some(s.to_string());
        }
    }
    None
}

/// Read a money value as integer cents. Yoco returns a `{amount, currency}`
/// object (amount already in minor units); this also tolerates a bare integer.
pub fn money_cents(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::Object(o)) => o.get("amount").and_then(Value::as_i64),
        _ => None,
    }
}

/// Read an integer that Yoco may encode as a JSON string (e.g. `quantity: "2"`).
fn flex_i64(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => {
            let s = s.trim();
            s.parse::<i64>().ok().or_else(|| s.parse::<f64>().ok().map(|f| f as i64))
        }
        _ => None,
    }
}
