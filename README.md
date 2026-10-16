# Sankayi Intelligence

A business + marketing intelligence platform for the **Sankayi Jazz Series** (in
partnership with the Sankayi Restaurant and Lounge). It combines ticketing,
social media and marketing-spend data to answer the question that matters:

> **What is driving ticket sales, and what should Sankayi do next?**

Not just *"how many tickets did we sell"* — but which artists perform, whether
boosted posts actually pay off, and (eventually) which artist to book next.

---

## Status — Phase 1: Data model & API contracts ✅

This is the foundation. It deliberately contains **no UI code and no live API
credentials** — per the plan, we get the data model and API contracts right
first, on realistic seed data, then the dashboard becomes the fun part.

| Area | File |
|------|------|
| PostgreSQL schema | [db/schema.sql](db/schema.sql) |
| Seed data (mock, ZAR) | [seed/seed.sql](seed/seed.sql) |
| REST API contracts | [docs/API_CONTRACTS.md](docs/API_CONTRACTS.md) |
| Data dictionary (metric definitions) | [docs/DATA_DICTIONARY.md](docs/DATA_DICTIONARY.md) |
| Rust models (domain + API DTOs) | [backend/src/models.rs](backend/src/models.rs) |
| Provider trait + stubs | [backend/src/providers.rs](backend/src/providers.rs) |

### Design principles baked in
- **Historical snapshots, never overwrite** — account/campaign metrics are
  append-only so growth is always computable.
- **Provenance on everything ingested** — `source`, `source_id`,
  `source_timestamp`, `retrieved_at`, `raw_payload`.
- **Calculated ≠ stored** — ROAS, sell-through, scores are derived in views/API,
  defined once in the data dictionary.
- **Money in ZAR cents** as integers on the wire; formatted to `R` in the UI.
- **Secure middleman** — the Rust backend is the only thing holding API keys;
  the browser never sees them.

---

## Architecture

```
Instagram / Meta ─┐
Quicket ──────────┼─▶ Rust Data Service ─▶ PostgreSQL ─▶ Rust REST API ─▶ JS Dashboard
Meta Ads ─────────┘   (validate/normalise)   (snapshots)   (/api/*)        (bespoke)
```

**Stack:** Rust + Actix Web · PostgreSQL · Vite + vanilla JS (bespoke UI, no
component framework). Charts are hand-built SVG/CSS in the Sankayi brand
(orange + deep indigo, Fraunces + Oswald + Manrope).

---

## Roadmap

- **Phase 1 — Data model & API contracts** ✅ schema, contracts, models, provider trait, seed data
- **Phase 2 — Live API** ✅ Actix routes + sqlx queries; all endpoints serving verified JSON.
  **Quicket integration is live** — real ticket sales ingested via `cargo run --bin ingest -- quicket`.
  Instagram/Meta and Meta Ads integrations still pending credentials.
- **Phase 3 — Frontend** ✅ bespoke Vite dashboard wired to the live API. All ten
  pages built: Overview, Events (+ detail), Ticket Sales (paged/filterable),
  Artists (+ profile & score breakdown), Social, Marketing (attribution funnel),
  Forecasts, Reports (live CSV export), Data Health, Settings.
- **Phase 4 — Intelligence** — full artist Performance Score, sales velocity, event comparison
- **Phase 5 — Predictive** — trained demand/revenue forecasts, recommendations, confidence intervals

---

## Try the data model locally

```bash
createdb sankayi
psql sankayi -f db/schema.sql
psql sankayi -f seed/seed.sql
```

Then explore, e.g. tickets sold + revenue per event:

```bash
psql sankayi -c "SELECT title, tickets_sold, revenue_cents/100.0 AS revenue_zar, sell_through FROM event_sales_summary ORDER BY event_date;"
```

The Rust type layer compiles independently:

```bash
cd backend && cargo check
```

---

## Run the full stack (dev)

**1. Database** — create `sankayi` and load schema + seed (see above, or via pgAdmin).

**2. Backend** — copy `backend/.env.example` to `backend/.env` and set `DATABASE_URL`
(percent-encode special characters in the password, e.g. `@` → `%40`), then:

```bash
cd backend && cargo run
```

Serves the API on http://127.0.0.1:8080.

**3. Frontend** — in a second terminal:

```bash
cd frontend && npm install && npm run dev
```

Opens http://localhost:5173. Vite proxies `/api` → `:8080`, so the browser makes
same-origin requests and the dashboard shows live data from your database.

### Pull real Quicket data (live integration)

The database is the source of truth for events; ingestion has two modes:

```bash
# DISCOVER — full event list + real capacities (auto-picks up new shows).
# Needs a short-lived Bearer JWT from a logged-in Quicket browser session.
cd backend && QUICKET_BEARER="Bearer eyJ..." cargo run --bin ingest -- refresh

# SALES — sync ticket sales for events already in the DB. Stable creds only,
# so this is safe to schedule (e.g. nightly).
cd backend && cargo run --bin ingest -- sales
```

Sales are stored as privacy-preserving aggregates (no buyer PII). Everything is
idempotent (deterministic UUIDv5 ids). Why the split? Quicket's public API can't
list an organiser's events or expose capacity — only its internal, browser-auth'd
API can — while per-event sales work fine on the stable public creds. See
`src/quicket.rs` and `src/bin/ingest.rs`.

> The backend has two binaries — the API (`sankayi-api`, the default) and the
> ingestion runner (`ingest`, with `refresh` / `sales` / `set-capacities`).

---

## Notes & caveats (carried from the design discussion)
- **Instagram data is via authorised Meta Graph APIs only** — no scraping. The
  Sankayi account must be a Business/Creator account with approved permissions.
- **Quicket exposes a REST API**; keys must stay server-side.
- **Hosting:** Vercel Hobby is non-commercial and its cron is once-daily — fine
  for a portfolio/demo build, but a real production Sankayi system needs hosting
  and a database chosen for its actual commercial usage.
- **Frontend is bespoke** (no Syncfusion / component framework) — total visual
  control in the Sankayi brand, no licensing.
- Forecasting is labelled **"Forecast"**, not "prediction", until a real trained
  model exists.
