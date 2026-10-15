-- Net worth that updates itself, and an investment portfolio.
--
-- Until now every snapshot line was typed by hand, including numbers Coin Heaven
-- already knows: a current account's balance is its ledger, a mortgage's
-- balance is its amortisation schedule, and a holding's value is units times
-- a price. Lines can now say where they came from, and those lines are
-- recomputed rather than retyped. Hand-typed lines are never touched.

------------------------------------------------------------------------------
-- Accounts: an anchor balance, so the ledger can carry it forward and back.
------------------------------------------------------------------------------
-- "The balance was X on date D", read off the bank app. Balance on any other
-- date is X plus (or minus) the ledger between the two dates. An anchor rather
-- than an opening balance because nobody knows what their account held on the
-- day it was opened, and everybody can read today's balance.
ALTER TABLE account ADD COLUMN anchor_balance_minor INTEGER;
ALTER TABLE account ADD COLUMN anchor_date TEXT;
ALTER TABLE account ADD COLUMN asset_class TEXT REFERENCES asset_class(key);
ALTER TABLE account ADD COLUMN in_net_worth INTEGER NOT NULL DEFAULT 1
    CHECK (in_net_worth IN (0,1));

------------------------------------------------------------------------------
-- Loans: the outstanding balance follows from the terms.
------------------------------------------------------------------------------
CREATE TABLE loan (
    id               INTEGER PRIMARY KEY,
    name             TEXT    NOT NULL,
    principal_minor  INTEGER NOT NULL CHECK (principal_minor > 0),
    annual_rate_bps  INTEGER NOT NULL CHECK (annual_rate_bps BETWEEN 0 AND 5000),
    term_months      INTEGER NOT NULL CHECK (term_months BETWEEN 1 AND 600),
    -- First instalment is due one month after this date.
    start_date       TEXT    NOT NULL,
    asset_class      TEXT    NOT NULL REFERENCES asset_class(key),
    is_archived      INTEGER NOT NULL DEFAULT 0 CHECK (is_archived IN (0,1))
);

------------------------------------------------------------------------------
-- Holdings, what happened to them, and what they were worth.
------------------------------------------------------------------------------
CREATE TABLE holding (
    id           INTEGER PRIMARY KEY,
    name         TEXT    NOT NULL,
    -- What a price lookup searches for: ticker, ISIN or coin symbol.
    symbol       TEXT,
    asset_class  TEXT    NOT NULL REFERENCES asset_class(key),
    is_archived  INTEGER NOT NULL DEFAULT 0 CHECK (is_archived IN (0,1))
);

-- A recurring buy: a fixed amount every N days, or on a day of each month.
CREATE TABLE holding_schedule (
    id            INTEGER PRIMARY KEY,
    holding_id    INTEGER NOT NULL REFERENCES holding(id) ON DELETE CASCADE,
    amount_minor  INTEGER NOT NULL CHECK (amount_minor > 0),
    every_days    INTEGER CHECK (every_days BETWEEN 1 AND 366),
    day_of_month  INTEGER CHECK (day_of_month BETWEEN 1 AND 31),
    start_date    TEXT    NOT NULL,
    end_date      TEXT,
    CHECK ((every_days IS NULL) <> (day_of_month IS NULL))
);

-- Units are stored ×1e8 so a fraction of a bitcoin survives exactly.
-- `units_e8` is NULL when a buy is known by its amount only — a scheduled
-- buy of €50 does not say how many units €50 bought that day.
-- A 'units' row is a reconciliation: "I held exactly this many on this date",
-- read off the broker; it resets the count and later buys add to it.
CREATE TABLE holding_txn (
    id            INTEGER PRIMARY KEY,
    holding_id    INTEGER NOT NULL REFERENCES holding(id) ON DELETE CASCADE,
    on_date       TEXT    NOT NULL,
    kind          TEXT    NOT NULL CHECK (kind IN ('buy','sell','units')),
    amount_minor  INTEGER NOT NULL DEFAULT 0 CHECK (amount_minor >= 0),
    units_e8      INTEGER CHECK (units_e8 IS NULL OR units_e8 >= 0),
    schedule_id   INTEGER REFERENCES holding_schedule(id) ON DELETE SET NULL,
    -- A scheduled buy that did not happen. Kept rather than deleted, so the
    -- schedule does not regenerate it.
    skipped       INTEGER NOT NULL DEFAULT 0 CHECK (skipped IN (0,1)),
    UNIQUE (schedule_id, on_date)
);
CREATE INDEX ix_holding_txn ON holding_txn (holding_id, on_date);

-- Price per unit in millionths of a euro, so a €0.000123 token is not zero.
CREATE TABLE holding_price (
    holding_id   INTEGER NOT NULL REFERENCES holding(id) ON DELETE CASCADE,
    on_date      TEXT    NOT NULL,
    price_micro  INTEGER NOT NULL CHECK (price_micro >= 0),
    -- 'manual' or 'assistant'; an assistant price carries the page it came
    -- from so it can be checked.
    source       TEXT    NOT NULL DEFAULT 'manual' CHECK (source IN ('manual','assistant')),
    source_url   TEXT,
    recorded_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    PRIMARY KEY (holding_id, on_date)
);

------------------------------------------------------------------------------
-- Snapshot lines remember where they came from.
------------------------------------------------------------------------------
ALTER TABLE wealth_line ADD COLUMN source TEXT NOT NULL DEFAULT 'manual'
    CHECK (source IN ('manual','account','loan','holding'));
ALTER TABLE wealth_line ADD COLUMN source_id INTEGER;

------------------------------------------------------------------------------
-- The egress log learns a new purpose: looking up prices.
------------------------------------------------------------------------------
-- Same rebuild as 0006, for the same reason: a CHECK cannot be altered, and
-- the audit log's history must survive the change.
CREATE TABLE ai_egress_log_new (
    id               INTEGER PRIMARY KEY,
    occurred_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    provider         TEXT    NOT NULL,
    model            TEXT    NOT NULL,
    purpose          TEXT    NOT NULL CHECK (purpose IN
                         ('categorize','split_match','nl_query','connection_test',
                          'detect_format','price_check')),
    payload_sha256   TEXT    NOT NULL,
    payload_preview  TEXT    NOT NULL,
    row_count        INTEGER,
    approved_by_user INTEGER NOT NULL DEFAULT 0 CHECK (approved_by_user IN (0,1)),
    request_bytes    INTEGER,
    response_bytes   INTEGER,
    outcome          TEXT    NOT NULL CHECK (outcome IN ('sent','failed','declined'))
);
INSERT INTO ai_egress_log_new
    (id, occurred_at, provider, model, purpose, payload_sha256, payload_preview,
     row_count, approved_by_user, request_bytes, response_bytes, outcome)
SELECT id, occurred_at, provider, model, purpose, payload_sha256, payload_preview,
       row_count, approved_by_user, request_bytes, response_bytes, outcome
FROM ai_egress_log;
DROP TABLE ai_egress_log;
ALTER TABLE ai_egress_log_new RENAME TO ai_egress_log;
CREATE INDEX ix_egress_time ON ai_egress_log (occurred_at);
