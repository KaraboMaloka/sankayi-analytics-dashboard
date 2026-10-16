# Sankayi Intelligence — API Contracts (Phase 1)

The Rust + Actix Web backend is the **only** thing that talks to Instagram, Quicket
and Meta Ads. The browser talks only to this API. No third-party keys ever reach
the frontend.

**Conventions**
- Base path: `/api`
- All responses `Content-Type: application/json`
- **Money is returned in ZAR cents** (`revenue_cents: 25680000` = R256,800.00). The
  frontend formats to `R` — keeping cents on the wire avoids float rounding.
- Ratios (`sell_through`, `roas`, growth) are decimals: `0.78` = 78%, `4.8` = 4.8x.
- Timestamps are ISO 8601 with timezone (`2026-08-20T21:00:00+02:00`).
- Dates are `YYYY-MM-DD`.
- Errors use a common envelope (see bottom).

> **Calculated vs stored.** Any field marked _(calculated)_ is derived in the API/
> analytics layer or a SQL view — never stored as source-of-truth. See
> [DATA_DICTIONARY.md](DATA_DICTIONARY.md) for exact formulas.

---

## `GET /api/dashboard`

Executive overview KPIs. Optional `eventId` scopes to one event; omitted = whole series.

**Query params**

| param     | type   | required | notes                                   |
|-----------|--------|----------|-----------------------------------------|
| `eventId` | uuid   | no       | scope to a single event                 |
| `from`    | date   | no       | period start (default: all time)        |
| `to`      | date   | no       | period end                              |

**Response**
```json
{
  "period": { "from": "2026-08-01", "to": "2026-08-31" },
  "tickets_sold": 1284,
  "revenue_cents": 25680000,
  "sell_through": 0.78,
  "followers": 14020,
  "followers_growth": 0.084,
  "ad_spend_cents": 1245000,
  "roas": 4.8,
  "generated_at": "2026-08-21T08:00:00+02:00"
}
```
`roas`, `sell_through`, `followers_growth` are _(calculated)_.

---

## `GET /api/events`

List events with a sales summary. Newest first.

**Query params:** `status` (enum), `from`, `to`, `artistId` — all optional.

**Response**
```json
{
  "events": [
    {
      "id": "cccccccc-cccc-cccc-cccc-cccccccccccc",
      "title": "Sankayi Jazz Series: The Sipho Collective",
      "event_date": "2026-09-20",
      "artist": { "id": "3333...", "name": "The Sipho Collective" },
      "capacity": 220,
      "ticket_price_cents": 28000,
      "status": "on_sale",
      "tickets_sold": 96,
      "revenue_cents": 2528000,
      "sell_through": 0.436
    }
  ],
  "count": 1
}
```

---

## `GET /api/events/:id`

Full detail for one event: sales breakdown, linked campaigns, linked posts,
and a sales-velocity series.

**Response**
```json
{
  "id": "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb",
  "title": "Sankayi Jazz Series: Kagiso Mabe Trio",
  "event_date": "2026-08-16",
  "artist": { "id": "2222...", "name": "Kagiso Mabe Trio", "genre": "Afro Jazz" },
  "capacity": 220,
  "ticket_price_cents": 25000,
  "status": "completed",
  "tickets_sold": 158,
  "revenue_cents": 3910000,
  "sell_through": 0.718,
  "sales_by_type": [
    { "ticket_type": "Early Bird", "quantity": 45, "revenue_cents": 990000 },
    { "ticket_type": "General",    "quantity": 113, "revenue_cents": 2825000 }
  ],
  "sales_velocity": [
    { "date": "2026-07-18", "cumulative_tickets": 45 },
    { "date": "2026-08-02", "cumulative_tickets": 143 },
    { "date": "2026-08-16", "cumulative_tickets": 158 }
  ],
  "campaigns": [ { "id": "eeee...", "campaign_name": "Kagiso Boost", "spend_cents": 600000 } ],
  "posts": [ { "id": "...", "post_type": "reel", "reach": 24800, "engagement_rate": 0.0489 } ]
}
```

---

## `GET /api/artists`

Artist leaderboard with normalised performance. `sort` controls ranking.

**Query params:** `sort` = `score` (default) | `tickets` | `revenue` | `sell_through` | `engagement`.

**Response**
```json
{
  "artists": [
    {
      "id": "1111...",
      "name": "Nomsa Dlamini Quartet",
      "genre": "Vocal Jazz",
      "events_count": 1,
      "tickets_sold": 201,
      "revenue_cents": 4795000,
      "avg_sell_through": 0.914,
      "avg_engagement_rate": 0.082,
      "roi": 5.1,
      "performance_score": 88.4
    }
  ]
}
```
`performance_score`, `roi`, `avg_*` are _(calculated)_ — see the Artist Score
weighting in the data dictionary.

---

## `GET /api/artists/:id`

One artist across all their Sankayi events: event history, score breakdown by
component (ticket performance, revenue, sell-through, social, growth, marketing
efficiency).

```json
{
  "id": "1111...",
  "name": "Nomsa Dlamini Quartet",
  "performance_score": 88.4,
  "score_breakdown": {
    "ticket_performance": 92,
    "revenue": 88,
    "sell_through": 94,
    "social_engagement": 79,
    "audience_growth": 81,
    "marketing_efficiency": 91
  },
  "events": [
    { "id": "aaaa...", "event_date": "2026-07-19", "tickets_sold": 201, "sell_through": 0.914 }
  ]
}
```

---

## `GET /api/social`

Account-level Instagram growth + post performance.

**Query params:** `from`, `to` (optional).

```json
{
  "account_series": [
    { "date": "2026-08-01", "followers": 13120, "reach": 26000, "engagements": 1980 },
    { "date": "2026-08-20", "followers": 14020, "reach": 28500, "engagements": 2050 }
  ],
  "followers_growth": 0.084,
  "top_posts": [
    {
      "id": "...",
      "post_type": "reel",
      "published_at": "2026-07-30T17:00:00+02:00",
      "reach": 24800,
      "engagement_rate": 0.0489
    }
  ]
}
```

---

## `GET /api/campaigns`

Marketing performance with attribution + ROAS. Optional `eventId`.

```json
{
  "campaigns": [
    {
      "id": "eeee...",
      "campaign_name": "Kagiso Boost",
      "campaign_tag": "SJ-AUG-KAGISO",
      "platform": "meta_ads",
      "spend_cents": 600000,
      "reach": 45000,
      "clicks": 1130,
      "engagements": 4080,
      "attributed_tickets": 143,
      "attributed_revenue_cents": 3815000,
      "cost_per_ticket_cents": 4196,
      "cost_per_engagement_cents": 147,
      "ticket_conversion": 0.1265,
      "roas": 6.36
    }
  ]
}
```
`attributed_*`, `cost_per_*`, `ticket_conversion`, `roas` are _(calculated)_.

---

## `GET /api/sales`

Raw-ish ticket sales feed for grids / export. Supports paging + filters that map
onto Syncfusion DataGrid remote binding.

**Query params:** `eventId`, `from`, `to`, `page` (default 1), `pageSize` (default 50).

```json
{
  "sales": [
    {
      "id": "...",
      "event_id": "bbbb...",
      "ticket_type": "General",
      "quantity": 98,
      "price_cents": 25000,
      "sold_at": "2026-08-02T12:00:00+02:00",
      "sale_source": "quicket",
      "attribution": "influenced"
    }
  ],
  "page": 1,
  "page_size": 50,
  "total": 9
}
```

---

## `GET /api/forecasts`  *(Phase 4/5 — contract defined now, returns stub)*

Artist/event demand forecast. In Phase 1 this returns `model: "stub"` with
heuristic numbers so the UI can be built; the real model lands in Phase 5.

```json
{
  "model": "stub",
  "forecasts": [
    {
      "artist_id": "3333...",
      "artist_name": "The Sipho Collective",
      "expected_tickets": 172,
      "range_low": 150,
      "range_high": 195,
      "confidence": 0.62,
      "expected_revenue_cents": 4816000,
      "expected_sell_through": 0.78,
      "recommended_budget_cents": 480000,
      "rationale": "Heuristic from prior events; not a trained model."
    }
  ]
}
```

---

## `GET /api/health`  — Data Health page

```json
{
  "sources": [
    { "source": "quicket",   "status": "success", "last_synced": "2026-08-20T21:05:12+02:00", "rows": 9 },
    { "source": "instagram", "status": "partial", "last_synced": "2026-08-20T21:00:20+02:00", "rows": 3, "message": "2 posts missing insights" },
    { "source": "meta_ads",  "status": "success", "last_synced": "2026-08-20T18:00:30+02:00", "rows": 6 }
  ],
  "database": "healthy"
}
```

---

## Error envelope

Non-2xx responses share one shape:
```json
{
  "error": {
    "code": "not_found",
    "message": "Event not found",
    "request_id": "req_01H..."
  }
}
```
Codes: `bad_request` (400), `unauthorized` (401), `not_found` (404),
`upstream_error` (502, an external API failed), `internal` (500).
