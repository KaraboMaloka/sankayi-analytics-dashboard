# Sankayi Intelligence — Data Dictionary

The single source of truth for **what every metric means and how it is computed**.
When someone asks _"why does Instagram say 38,000 reach but the dashboard says
41,000?"_, the answer is here.

**Money** is stored and transmitted in **ZAR cents** (integer). `2568000` = R25,680.00.
**Ratios** are decimals (`0.78` = 78%; `4.8` = 4.8×).

---

## Sourced metrics (stored as-is from an external system)

| Metric              | Source     | Table.column                          | Definition |
|---------------------|------------|---------------------------------------|------------|
| Reach               | Instagram  | `instagram_posts.reach`               | Unique accounts that saw the content |
| Impressions         | Instagram  | `instagram_posts.impressions`         | Total times content was displayed |
| Likes / Comments / Shares / Saves | Instagram | `instagram_posts.*`      | Native engagement counts on a post |
| Profile visits      | Instagram  | `daily_account_metrics.profile_visits`| Visits to the profile |
| Followers           | Instagram  | `daily_account_metrics.followers`     | Follower count on a given `snapshot_date` |
| Tickets sold (line) | Quicket    | `ticket_sales.quantity`               | Confirmed ticket quantity for a sale line |
| Ticket price        | Quicket    | `ticket_sales.price_cents`            | Actual price paid per ticket, ZAR cents |
| Ad spend (daily)    | Meta Ads   | `campaign_metrics.spend_cents`        | Amount spent on a campaign on `metric_date` |
| Campaign reach/clicks/engagements | Meta Ads | `campaign_metrics.*`      | As reported by Meta for that day |

Every sourced row also carries provenance: `source`, `source_id`,
`source_timestamp`, `retrieved_at`, and (where kept) `raw_payload`.

---

## Calculated metrics (derived — never stored as source-of-truth)

| Metric               | Formula | Where |
|----------------------|---------|-------|
| **Tickets sold** (event) | `SUM(ticket_sales.quantity)` for the event | `event_sales_summary` view |
| **Revenue** (gross)  | `SUM(quantity × price_cents)` | `event_sales_summary` view |
| **Sell-through**     | `tickets_sold / capacity` | view / API |
| **Followers growth** | `(followers_end − followers_start) / followers_start` over the period | `/api/social`, `/api/dashboard` |
| **Engagement rate** (post) | `(likes + comments + shares + saves) / reach` | `/api/social` |
| **Ad spend** (period)| `SUM(campaign_metrics.spend_cents)` or `campaigns.spend_cents` | `campaign_performance` view |
| **ROAS**             | `attributed_revenue_cents / spend_cents` | `/api/campaigns` |
| **Cost per ticket**  | `spend_cents / attributed_tickets` | `/api/campaigns` |
| **Cost per engagement** | `spend_cents / engagements` | `/api/campaigns` |
| **Ticket conversion**| `attributed_tickets / clicks` | `/api/campaigns` |
| **ROI** (artist)     | `attributed_revenue_cents / spend_cents` across the artist's events | `/api/artists` |

---

## Attribution

A ticket's link to marketing is graded, never assumed:

| Value        | Meaning |
|--------------|---------|
| `direct`     | Purchase arrived through a tracked campaign URL (UTM / campaign tag match) |
| `influenced` | Bought during a campaign window / plausibly influenced, but not tracked end-to-end |
| `unknown`    | Door sales, comps, or no tracking signal |

`attributed_tickets` / `attributed_revenue_cents` for a campaign count sales whose
`attributed_campaign_tag` matches the campaign's `campaign_tag`. Reports should
show `direct` and `influenced` separately from `unknown` rather than blending them.

---

## Artist Performance Score

A weighted 0–100 score to compare artists **fairly** (normalising for venue size,
following and spend). Each component is normalised across artists to 0–100, then
weighted:

| Component            | Weight | Based on |
|----------------------|--------|----------|
| Ticket performance   | 30%    | tickets sold, normalised |
| Revenue              | 20%    | gross revenue |
| Sell-through         | 15%    | tickets / capacity |
| Social engagement    | 15%    | avg post engagement rate |
| Audience growth      | 10%    | follower growth around the event |
| Marketing efficiency | 10%    | inverse cost-per-ticket / ROAS |

`final = 0.30·ticket + 0.20·revenue + 0.15·sellthrough + 0.15·social + 0.10·growth + 0.10·marketing`

> Displayed as **"Artist Performance Score"**, and forecasts as **"Artist
> Performance Forecast"** — deliberately *not* "prediction", until a real trained
> model exists (Phase 5).

---

## Snapshot rule

`daily_account_metrics`, `campaign_metrics`, and `instagram_post_metrics` are
**append-only**. We never overwrite `followers = 14582`; we insert a new dated row.
Growth, velocity and trends are only possible because history is preserved.
