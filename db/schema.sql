-- =============================================================================
-- Sankayi Intelligence — PostgreSQL Schema
-- Phase 1: Data model
--
-- Design principles (from the project brief):
--   1. HISTORICAL SNAPSHOTS, never overwrite. Account & campaign metrics are
--      append-only so growth is always computable.
--   2. PROVENANCE on everything ingested from an external API:
--        source, source_id, source_timestamp, retrieved_at, raw_payload.
--      If a calculation looks wrong six months from now, we can trace it back
--      to the exact API response that produced it.
--   3. CALCULATED metrics are NOT stored as source-of-truth. They are derived
--      in the API/analytics layer (or in views) so definitions stay in one place.
--   4. Currency is ZAR. Money is stored in CENTS (BIGINT) to avoid float error.
--
-- Target: PostgreSQL 14+
-- =============================================================================

BEGIN;

-- -----------------------------------------------------------------------------
-- Extensions
-- -----------------------------------------------------------------------------
CREATE EXTENSION IF NOT EXISTS "pgcrypto";   -- gen_random_uuid()

-- -----------------------------------------------------------------------------
-- Enumerated types
-- -----------------------------------------------------------------------------
-- Where a piece of data originally came from. Stamped on every ingested row.
CREATE TYPE data_source AS ENUM ('instagram', 'quicket', 'meta_ads', 'yoco', 'manual');

-- Lifecycle of an event.
CREATE TYPE event_status AS ENUM ('draft', 'announced', 'on_sale', 'sold_out', 'completed', 'cancelled');

-- How a specific ticket sale reached us / was attributed.
CREATE TYPE sale_source AS ENUM ('quicket', 'door', 'complimentary', 'manual');

-- Instagram media formats we track.
CREATE TYPE post_type AS ENUM ('image', 'carousel', 'reel', 'video', 'story');

-- Advertising platforms for paid campaigns.
CREATE TYPE campaign_platform AS ENUM ('meta_ads', 'instagram', 'facebook', 'google', 'other');

-- Attribution strength for a tracked ticket purchase.
CREATE TYPE attribution_kind AS ENUM ('direct', 'influenced', 'unknown');

-- -----------------------------------------------------------------------------
-- Provenance: shared columns pattern
-- -----------------------------------------------------------------------------
-- Postgres has no table inheritance we want to rely on here, so provenance
-- columns are repeated explicitly on each ingested table. The convention:
--   source            data_source   -- which system it came from
--   source_id         TEXT          -- the external primary key (keep it!)
--   source_timestamp  TIMESTAMPTZ   -- when the source says the event happened
--   retrieved_at      TIMESTAMPTZ   -- when WE pulled it
--   raw_payload       JSONB         -- the original API response (nullable)
-- -----------------------------------------------------------------------------


-- =============================================================================
-- USERS + SESSIONS  (dashboard authentication)
-- =============================================================================
CREATE TABLE users (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username      TEXT NOT NULL UNIQUE,          -- lowercased login handle
    display_name  TEXT NOT NULL,
    password_hash TEXT NOT NULL,                 -- Argon2 hash (never plaintext)
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE sessions (
    token      TEXT PRIMARY KEY,                 -- opaque random 256-bit token
    user_id    UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX sessions_user_idx ON sessions (user_id);


-- =============================================================================
-- ARTISTS
-- =============================================================================
CREATE TABLE artists (
    id               UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name             TEXT        NOT NULL,
    genre            TEXT,
    agency           TEXT,
    instagram_handle TEXT,                 -- without the leading '@'
    notes            TEXT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT artists_name_not_blank CHECK (length(trim(name)) > 0)
);

CREATE UNIQUE INDEX artists_instagram_handle_key
    ON artists (lower(instagram_handle))
    WHERE instagram_handle IS NOT NULL;


-- =============================================================================
-- EVENTS  (one Jazz Series night)
-- =============================================================================
CREATE TABLE events (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    artist_id     UUID         REFERENCES artists(id) ON DELETE SET NULL,
    title         TEXT         NOT NULL,
    event_date    DATE         NOT NULL,
    doors_time    TIME,
    venue         TEXT         NOT NULL DEFAULT 'Sankayi Restaurant and Lounge',
    capacity      INTEGER      NOT NULL,
    -- Face-value ticket price in ZAR cents. Per-sale price is captured again in
    -- ticket_sales because tiers/discounts can differ from face value.
    ticket_price_cents BIGINT  NOT NULL,
    status        event_status NOT NULL DEFAULT 'draft',
    -- Campaign tag used in tracked URLs, e.g. 'SJ-AUG-ARTIST-X'. Ties Instagram
    -- posts, campaigns and attributed sales together.
    campaign_tag  TEXT,
    -- Provenance for events discovered from an external system (e.g. Quicket).
    -- source_id is the external event id; discovery upserts by (source, source_id).
    source            data_source,
    source_id         TEXT,
    retrieved_at      TIMESTAMPTZ,
    created_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),

    CONSTRAINT events_capacity_positive     CHECK (capacity > 0),
    CONSTRAINT events_ticket_price_nonneg    CHECK (ticket_price_cents >= 0)
);

CREATE INDEX events_artist_id_idx  ON events (artist_id);
CREATE INDEX events_event_date_idx ON events (event_date);
CREATE UNIQUE INDEX events_campaign_tag_key
    ON events (campaign_tag) WHERE campaign_tag IS NOT NULL;
CREATE UNIQUE INDEX events_source_key
    ON events (source, source_id) WHERE source_id IS NOT NULL;


-- =============================================================================
-- TICKET_SALES  (individual sale line-items, sourced from Quicket)
-- =============================================================================
CREATE TABLE ticket_sales (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id      UUID        NOT NULL REFERENCES events(id) ON DELETE CASCADE,
    ticket_type   TEXT        NOT NULL DEFAULT 'General',
    quantity      INTEGER     NOT NULL,
    -- Actual price paid per ticket, ZAR cents (may differ from event face value).
    price_cents   BIGINT      NOT NULL,
    sold_at       TIMESTAMPTZ NOT NULL,
    sale_source   sale_source NOT NULL DEFAULT 'quicket',
    -- Attribution: if this purchase came through a tracked URL we know which
    -- campaign, and how confidently we can attribute it.
    attributed_campaign_tag TEXT,
    attribution   attribution_kind NOT NULL DEFAULT 'unknown',

    -- Provenance
    source            data_source NOT NULL DEFAULT 'quicket',
    source_id         TEXT,
    source_timestamp  TIMESTAMPTZ,
    retrieved_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    raw_payload       JSONB,

    CONSTRAINT ticket_sales_quantity_positive CHECK (quantity > 0),
    CONSTRAINT ticket_sales_price_nonneg       CHECK (price_cents >= 0)
);

CREATE INDEX ticket_sales_event_id_idx ON ticket_sales (event_id);
CREATE INDEX ticket_sales_sold_at_idx  ON ticket_sales (sold_at);
CREATE INDEX ticket_sales_campaign_idx ON ticket_sales (attributed_campaign_tag);
-- Guard against ingesting the same Quicket record twice.
CREATE UNIQUE INDEX ticket_sales_source_key
    ON ticket_sales (source, source_id) WHERE source_id IS NOT NULL;


-- =============================================================================
-- INSTAGRAM_POSTS  (per-post performance snapshot)
-- =============================================================================
-- Note: insight numbers (reach, likes...) change over time. If you want the
-- full time-series of a post's growth, snapshot into instagram_post_metrics
-- (below). This table holds the latest-known values for convenience.
CREATE TABLE instagram_posts (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id       UUID      REFERENCES events(id) ON DELETE SET NULL,
    post_type      post_type NOT NULL,
    caption        TEXT,
    permalink      TEXT,
    published_at   TIMESTAMPTZ NOT NULL,
    reach          INTEGER,
    impressions    INTEGER,
    likes          INTEGER,
    comments       INTEGER,
    shares         INTEGER,
    saves          INTEGER,
    profile_visits INTEGER,
    video_views    INTEGER,

    -- Provenance
    source            data_source NOT NULL DEFAULT 'instagram',
    source_id         TEXT,                 -- Instagram media id
    source_timestamp  TIMESTAMPTZ,
    retrieved_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    raw_payload       JSONB
);

CREATE INDEX instagram_posts_event_id_idx     ON instagram_posts (event_id);
CREATE INDEX instagram_posts_published_at_idx ON instagram_posts (published_at);
CREATE UNIQUE INDEX instagram_posts_source_key
    ON instagram_posts (source, source_id) WHERE source_id IS NOT NULL;


-- =============================================================================
-- INSTAGRAM_POST_METRICS  (append-only snapshots of a post's insights)
-- =============================================================================
CREATE TABLE instagram_post_metrics (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    post_id       UUID NOT NULL REFERENCES instagram_posts(id) ON DELETE CASCADE,
    snapshot_date DATE NOT NULL,
    reach          INTEGER,
    impressions    INTEGER,
    likes          INTEGER,
    comments       INTEGER,
    shares         INTEGER,
    saves          INTEGER,
    video_views    INTEGER,

    retrieved_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    raw_payload   JSONB,

    CONSTRAINT ig_post_metrics_unique UNIQUE (post_id, snapshot_date)
);

CREATE INDEX ig_post_metrics_post_id_idx ON instagram_post_metrics (post_id);


-- =============================================================================
-- CAMPAIGNS  (paid marketing pushes / boosted posts)
-- =============================================================================
CREATE TABLE campaigns (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id      UUID         REFERENCES events(id) ON DELETE SET NULL,
    campaign_name TEXT         NOT NULL,
    -- Matches events.campaign_tag / ticket_sales.attributed_campaign_tag.
    campaign_tag  TEXT,
    platform      campaign_platform NOT NULL DEFAULT 'meta_ads',
    budget_cents  BIGINT,          -- planned spend, ZAR cents
    spend_cents   BIGINT,          -- actual spend to date, ZAR cents
    start_date    DATE,
    end_date      DATE,
    permalink     TEXT,            -- link to the boosted post (Instagram URL)
    tracking_code TEXT,            -- Quicket promo code for deterministic attribution

    -- Provenance
    source            data_source NOT NULL DEFAULT 'meta_ads',
    source_id         TEXT,
    source_timestamp  TIMESTAMPTZ,
    retrieved_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    raw_payload       JSONB,

    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    CONSTRAINT campaigns_budget_nonneg CHECK (budget_cents IS NULL OR budget_cents >= 0),
    CONSTRAINT campaigns_spend_nonneg  CHECK (spend_cents  IS NULL OR spend_cents  >= 0),
    CONSTRAINT campaigns_date_order     CHECK (end_date IS NULL OR start_date IS NULL OR end_date >= start_date)
);

CREATE INDEX campaigns_event_id_idx ON campaigns (event_id);
CREATE INDEX campaigns_tag_idx      ON campaigns (campaign_tag);
CREATE UNIQUE INDEX campaigns_source_key
    ON campaigns (source, source_id) WHERE source_id IS NOT NULL;


-- =============================================================================
-- CAMPAIGN_METRICS  (append-only daily performance of a campaign)
-- =============================================================================
CREATE TABLE campaign_metrics (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    campaign_id  UUID NOT NULL REFERENCES campaigns(id) ON DELETE CASCADE,
    metric_date  DATE NOT NULL,
    reach        INTEGER,
    impressions  INTEGER,
    clicks       INTEGER,
    engagements  INTEGER,
    spend_cents  BIGINT,          -- spend on THIS day, ZAR cents

    -- Provenance
    retrieved_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    raw_payload  JSONB,

    CONSTRAINT campaign_metrics_unique       UNIQUE (campaign_id, metric_date),
    CONSTRAINT campaign_metrics_spend_nonneg  CHECK (spend_cents IS NULL OR spend_cents >= 0)
);

CREATE INDEX campaign_metrics_campaign_id_idx ON campaign_metrics (campaign_id);
CREATE INDEX campaign_metrics_date_idx        ON campaign_metrics (metric_date);


-- =============================================================================
-- DAILY_ACCOUNT_METRICS  (append-only account-level Instagram snapshots)
-- =============================================================================
-- This is the table that makes follower GROWTH computable. One row per day.
CREATE TABLE daily_account_metrics (
    id             UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    snapshot_date  DATE NOT NULL,
    followers      INTEGER,
    reach          INTEGER,
    impressions    INTEGER,
    profile_visits INTEGER,
    engagements    INTEGER,

    -- Provenance
    source            data_source NOT NULL DEFAULT 'instagram',
    source_timestamp  TIMESTAMPTZ,
    retrieved_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    raw_payload       JSONB,

    CONSTRAINT daily_account_metrics_unique UNIQUE (snapshot_date)
);

CREATE INDEX daily_account_metrics_date_idx ON daily_account_metrics (snapshot_date);


-- =============================================================================
-- INGESTION_LOG  (powers the "Data Health" page)
-- =============================================================================
CREATE TABLE ingestion_log (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source        data_source NOT NULL,
    started_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at   TIMESTAMPTZ,
    -- 'success' | 'partial' | 'failed'
    status        TEXT        NOT NULL DEFAULT 'success',
    rows_ingested INTEGER     NOT NULL DEFAULT 0,
    message       TEXT
);

CREATE INDEX ingestion_log_source_idx  ON ingestion_log (source, started_at DESC);


-- -----------------------------------------------------------------------------
-- updated_at trigger
-- -----------------------------------------------------------------------------
CREATE OR REPLACE FUNCTION set_updated_at()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER artists_set_updated_at   BEFORE UPDATE ON artists
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER events_set_updated_at    BEFORE UPDATE ON events
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();
CREATE TRIGGER campaigns_set_updated_at BEFORE UPDATE ON campaigns
    FOR EACH ROW EXECUTE FUNCTION set_updated_at();


-- =============================================================================
-- CONVENIENCE VIEWS  (calculated metrics live HERE, not in base tables)
-- =============================================================================

-- Tickets sold + gross revenue per event, from ticket_sales.
CREATE VIEW event_sales_summary AS
SELECT
    e.id                                            AS event_id,
    e.title,
    e.event_date,
    e.capacity,
    COALESCE(SUM(ts.quantity), 0)                   AS tickets_sold,
    COALESCE(SUM(ts.quantity * ts.price_cents), 0)  AS revenue_cents,
    CASE WHEN e.capacity > 0
         THEN ROUND(COALESCE(SUM(ts.quantity), 0)::numeric / e.capacity, 4)
         ELSE NULL END                              AS sell_through
FROM events e
LEFT JOIN ticket_sales ts ON ts.event_id = e.id
GROUP BY e.id;

-- Campaign spend + performance rollup, joined to attributed sales.
CREATE VIEW campaign_performance AS
SELECT
    c.id                                            AS campaign_id,
    c.campaign_name,
    c.campaign_tag,
    c.event_id,
    COALESCE(c.spend_cents, SUM(cm.spend_cents))    AS spend_cents,
    SUM(cm.reach)                                   AS reach,
    SUM(cm.impressions)                             AS impressions,
    SUM(cm.clicks)                                  AS clicks,
    SUM(cm.engagements)                             AS engagements
FROM campaigns c
LEFT JOIN campaign_metrics cm ON cm.campaign_id = c.id
GROUP BY c.id;

COMMIT;

-- =============================================================================
-- End of schema. See ../docs/DATA_DICTIONARY.md for metric definitions and
-- ../docs/API_CONTRACTS.md for the REST shapes the Rust layer serves.
-- =============================================================================
