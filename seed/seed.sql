-- =============================================================================
-- Sankayi Intelligence — Seed data
--
-- Realistic mock data so the API/dashboard can be built and demoed BEFORE the
-- real Instagram / Quicket / Meta Ads credentials exist. All figures are
-- illustrative. Money is in ZAR cents.
--
-- Run AFTER db/schema.sql against the same database.
-- =============================================================================

BEGIN;

-- -----------------------------------------------------------------------------
-- Artists
-- -----------------------------------------------------------------------------
INSERT INTO artists (id, name, genre, agency, instagram_handle) VALUES
    ('11111111-1111-1111-1111-111111111111', 'Nomsa Dlamini Quartet', 'Vocal Jazz',   'Blue Note ZA',  'nomsadlaminijazz'),
    ('22222222-2222-2222-2222-222222222222', 'Kagiso Mabe Trio',      'Afro Jazz',     'Independent',   'kagisomabe'),
    ('33333333-3333-3333-3333-333333333333', 'The Sipho Collective',  'Fusion',        'Groove Agency', 'siphocollective');

-- -----------------------------------------------------------------------------
-- Events  (capacity 220 at Sankayi Restaurant and Lounge)
-- ticket_price_cents: R250.00 = 25000
-- -----------------------------------------------------------------------------
INSERT INTO events (id, artist_id, title, event_date, venue, capacity, ticket_price_cents, status, campaign_tag) VALUES
    ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', '11111111-1111-1111-1111-111111111111',
        'Sankayi Jazz Series: Nomsa Dlamini Quartet', '2026-07-19', 'Sankayi Restaurant and Lounge', 220, 25000, 'completed', 'SJ-JUL-NOMSA'),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', '22222222-2222-2222-2222-222222222222',
        'Sankayi Jazz Series: Kagiso Mabe Trio',      '2026-08-16', 'Sankayi Restaurant and Lounge', 220, 25000, 'completed', 'SJ-AUG-KAGISO'),
    ('cccccccc-cccc-cccc-cccc-cccccccccccc', '33333333-3333-3333-3333-333333333333',
        'Sankayi Jazz Series: The Sipho Collective',  '2026-09-20', 'Sankayi Restaurant and Lounge', 220, 28000, 'on_sale', 'SJ-SEP-SIPHO');

-- -----------------------------------------------------------------------------
-- Ticket sales  (aggregated line-items; realistic spread)
--   July event: strong seller ~ 201 tickets
--   Aug event:  moderate     ~ 158 tickets
--   Sep event:  on sale now  ~ 96 tickets so far
-- -----------------------------------------------------------------------------
INSERT INTO ticket_sales (event_id, ticket_type, quantity, price_cents, sold_at, sale_source, attributed_campaign_tag, attribution) VALUES
    -- July / Nomsa
    ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Early Bird', 60, 22000, '2026-06-20 10:00:00+02', 'quicket', 'SJ-JUL-NOMSA', 'direct'),
    ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'General',    118, 25000, '2026-07-05 18:30:00+02', 'quicket', 'SJ-JUL-NOMSA', 'influenced'),
    ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'General',    15, 25000, '2026-07-19 19:00:00+02', 'door',    NULL,           'unknown'),
    ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Comp',        8, 0,     '2026-07-19 19:10:00+02', 'complimentary', NULL,     'unknown'),
    -- August / Kagiso
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Early Bird', 45, 22000, '2026-07-18 09:00:00+02', 'quicket', 'SJ-AUG-KAGISO', 'direct'),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'General',    98, 25000, '2026-08-02 12:00:00+02', 'quicket', 'SJ-AUG-KAGISO', 'influenced'),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'General',    15, 25000, '2026-08-16 19:00:00+02', 'door',    NULL,            'unknown'),
    -- September / Sipho (on sale)
    ('cccccccc-cccc-cccc-cccc-cccccccccccc', 'Early Bird', 40, 24000, '2026-08-10 08:00:00+02', 'quicket', 'SJ-SEP-SIPHO', 'direct'),
    ('cccccccc-cccc-cccc-cccc-cccccccccccc', 'General',    56, 28000, '2026-08-19 15:00:00+02', 'quicket', 'SJ-SEP-SIPHO', 'influenced');

-- -----------------------------------------------------------------------------
-- Campaigns  (boosted posts per event)
--   spend R3,500 / R6,000 / R4,200 = 350000 / 600000 / 420000 cents
-- -----------------------------------------------------------------------------
INSERT INTO campaigns (id, event_id, campaign_name, campaign_tag, platform, budget_cents, spend_cents, start_date, end_date) VALUES
    ('dddddddd-dddd-dddd-dddd-dddddddddddd', 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'Nomsa Boost',  'SJ-JUL-NOMSA',  'meta_ads', 400000, 350000, '2026-06-25', '2026-07-18'),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee', 'bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'Kagiso Boost', 'SJ-AUG-KAGISO', 'meta_ads', 600000, 600000, '2026-07-20', '2026-08-15'),
    ('ffffffff-ffff-ffff-ffff-ffffffffffff', 'cccccccc-cccc-cccc-cccc-cccccccccccc', 'Sipho Boost',  'SJ-SEP-SIPHO',  'meta_ads', 500000, 420000, '2026-08-08', '2026-09-19');

INSERT INTO campaign_metrics (campaign_id, metric_date, reach, impressions, clicks, engagements, spend_cents) VALUES
    ('dddddddd-dddd-dddd-dddd-dddddddddddd', '2026-07-01', 12000, 18500, 340, 1450, 175000),
    ('dddddddd-dddd-dddd-dddd-dddddddddddd', '2026-07-10', 14500, 21000, 410, 1720, 175000),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee', '2026-08-01', 21000, 31000, 520, 1980, 300000),
    ('eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee', '2026-08-10', 24000, 35500, 610, 2100, 300000),
    ('ffffffff-ffff-ffff-ffff-ffffffffffff', '2026-08-12', 9000,  13000, 220, 880,  210000),
    ('ffffffff-ffff-ffff-ffff-ffffffffffff', '2026-08-19', 11500, 16800, 300, 1120, 210000);

-- -----------------------------------------------------------------------------
-- Instagram posts
-- -----------------------------------------------------------------------------
INSERT INTO instagram_posts (event_id, post_type, caption, published_at, reach, impressions, likes, comments, shares, saves, profile_visits, video_views, source_id) VALUES
    ('aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'reel',  'Nomsa Dlamini live at Sankayi 🎷', '2026-06-28 17:00:00+02', 15200, 22800, 640, 48, 92, 210, 380, 18400, 'ig_media_001'),
    ('bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb', 'reel',  'Kagiso Mabe Trio — this Saturday',  '2026-07-30 17:00:00+02', 24800, 36200, 910, 71, 130, 340, 520, 27600, 'ig_media_002'),
    ('cccccccc-cccc-cccc-cccc-cccccccccccc', 'carousel', 'The Sipho Collective is coming', '2026-08-15 17:00:00+02', 11800, 17400, 505, 33, 61, 180, 290, NULL,   'ig_media_003');

-- -----------------------------------------------------------------------------
-- Daily account metrics  (follower growth series)
-- -----------------------------------------------------------------------------
INSERT INTO daily_account_metrics (snapshot_date, followers, reach, profile_visits, engagements) VALUES
    ('2026-07-01', 12100, 18000, 320, 1400),
    ('2026-07-15', 12580, 21000, 410, 1650),
    ('2026-08-01', 13120, 26000, 520, 1980),
    ('2026-08-15', 13740, 31000, 610, 2210),
    ('2026-08-20', 14020, 28500, 560, 2050);

-- -----------------------------------------------------------------------------
-- Ingestion log  (for the Data Health page)
-- -----------------------------------------------------------------------------
INSERT INTO ingestion_log (source, started_at, finished_at, status, rows_ingested, message) VALUES
    ('quicket',  '2026-08-20 21:05:00+02', '2026-08-20 21:05:12+02', 'success', 9,  NULL),
    ('instagram','2026-08-20 21:00:00+02', '2026-08-20 21:00:20+02', 'partial', 3,  '2 posts missing insights'),
    ('meta_ads', '2026-08-20 18:00:00+02', '2026-08-20 18:00:30+02', 'success', 6,  NULL);

COMMIT;
