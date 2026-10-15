-- Net worth history, real property register, simulator scenarios, app/AI config.

------------------------------------------------------------------------------
-- Real property register (absent from the original spec, but the simulator
-- cannot be compared against reality without it)
------------------------------------------------------------------------------
CREATE TABLE property (
    id                     INTEGER PRIMARY KEY,
    label                  TEXT    NOT NULL,
    address_note           TEXT,                      -- free text, never geocoded
    status                 TEXT    NOT NULL DEFAULT 'owned'
                               CHECK (status IN ('prospect','under_offer','owned','sold')),
    purchased_on           TEXT,
    sold_on                TEXT,
    purchase_price_minor   INTEGER CHECK (purchase_price_minor   IS NULL OR purchase_price_minor   >= 0),
    acquisition_costs_minor INTEGER NOT NULL DEFAULT 0 CHECK (acquisition_costs_minor >= 0),
    renovation_minor       INTEGER NOT NULL DEFAULT 0 CHECK (renovation_minor >= 0),
    current_value_minor    INTEGER CHECK (current_value_minor IS NULL OR current_value_minor >= 0),
    valued_on              TEXT,
    monthly_rent_minor     INTEGER NOT NULL DEFAULT 0 CHECK (monthly_rent_minor >= 0),
    monthly_costs_minor    INTEGER NOT NULL DEFAULT 0 CHECK (monthly_costs_minor >= 0),
    currency               TEXT    NOT NULL DEFAULT 'EUR' CHECK (length(currency) = 3),
    notes                  TEXT,
    CHECK (purchased_on IS NULL OR purchased_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    CHECK (sold_on IS NULL OR sold_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]')
);

CREATE TABLE mortgage (
    id               INTEGER PRIMARY KEY,
    property_id      INTEGER NOT NULL REFERENCES property(id) ON DELETE CASCADE,
    lender           TEXT,
    principal_minor  INTEGER NOT NULL CHECK (principal_minor > 0),
    -- Rates are basis points, not floats: 2.8% -> 280.
    annual_rate_bps  INTEGER NOT NULL CHECK (annual_rate_bps >= 0),
    term_months      INTEGER NOT NULL CHECK (term_months > 0),
    started_on       TEXT    NOT NULL,
    kind             TEXT    NOT NULL DEFAULT 'fixed' CHECK (kind IN ('fixed','variable','mixed')),
    is_closed        INTEGER NOT NULL DEFAULT 0 CHECK (is_closed IN (0,1)),
    CHECK (started_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]')
);
CREATE INDEX ix_mortgage_property ON mortgage (property_id);

------------------------------------------------------------------------------
-- Net-worth snapshots
--
-- The spec asked for fixed bi-annual rows with fixed asset columns. Both are
-- relaxed here: any date is allowed (is_scheduled marks the Jun-30/Dec-31 ones
-- the UI prompts for), and buckets are rows rather than columns so a new asset
-- class never needs a migration.
------------------------------------------------------------------------------
CREATE TABLE wealth_snapshot (
    id           INTEGER PRIMARY KEY,
    taken_on     TEXT    NOT NULL UNIQUE,
    label        TEXT,
    is_scheduled INTEGER NOT NULL DEFAULT 0 CHECK (is_scheduled IN (0,1)),
    currency     TEXT    NOT NULL DEFAULT 'EUR' CHECK (length(currency) = 3),
    notes        TEXT,
    created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    CHECK (taken_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]')
);

CREATE TABLE wealth_line (
    id                INTEGER PRIMARY KEY,
    snapshot_id       INTEGER NOT NULL REFERENCES wealth_snapshot(id) ON DELETE CASCADE,
    bucket            TEXT    NOT NULL CHECK (bucket IN
                          ('cash','equities','crypto','pension','real_estate',
                           'other_asset','mortgage_debt','other_debt')),
    label             TEXT    NOT NULL,
    account_id        INTEGER REFERENCES account(id)  ON DELETE SET NULL,
    property_id       INTEGER REFERENCES property(id) ON DELETE SET NULL,
    gross_value_minor INTEGER NOT NULL DEFAULT 0,
    debt_minor        INTEGER NOT NULL DEFAULT 0 CHECK (debt_minor >= 0),
    currency          TEXT    NOT NULL DEFAULT 'EUR' CHECK (length(currency) = 3)
);
CREATE INDEX ix_wealth_line_snapshot ON wealth_line (snapshot_id);
CREATE INDEX ix_wealth_line_bucket   ON wealth_line (bucket);

CREATE VIEW v_net_worth AS
SELECT s.id            AS snapshot_id,
       s.taken_on,
       s.label,
       SUM(l.gross_value_minor)                        AS gross_assets_minor,
       SUM(l.debt_minor)                               AS total_debt_minor,
       SUM(l.gross_value_minor) - SUM(l.debt_minor)    AS net_worth_minor,
       SUM(CASE WHEN l.bucket IN ('cash','equities','crypto')
                THEN l.gross_value_minor ELSE 0 END)   AS liquid_minor,
       SUM(CASE WHEN l.bucket = 'real_estate'
                THEN l.gross_value_minor - l.debt_minor ELSE 0 END) AS re_equity_minor
FROM wealth_snapshot s
LEFT JOIN wealth_line l ON l.snapshot_id = s.id
GROUP BY s.id;

------------------------------------------------------------------------------
-- Simulator: scenarios are stored as a versioned parameter blob, checkpoints
-- as real observations to overlay on the projection.
------------------------------------------------------------------------------
CREATE TABLE sim_scenario (
    id             INTEGER PRIMARY KEY,
    name           TEXT    NOT NULL UNIQUE,
    params_version INTEGER NOT NULL DEFAULT 1,
    params_json    TEXT    NOT NULL,
    is_default     INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0,1)),
    created_at     TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at     TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE TABLE sim_checkpoint (
    id                      INTEGER PRIMARY KEY,
    scenario_id             INTEGER NOT NULL REFERENCES sim_scenario(id) ON DELETE CASCADE,
    observed_on             TEXT    NOT NULL,
    cash_minor              INTEGER NOT NULL DEFAULT 0,
    property_count          INTEGER NOT NULL DEFAULT 0 CHECK (property_count >= 0),
    net_rent_monthly_minor  INTEGER NOT NULL DEFAULT 0,
    net_worth_minor         INTEGER,
    notes                   TEXT,
    UNIQUE (scenario_id, observed_on),
    CHECK (observed_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]')
);
CREATE INDEX ix_sim_checkpoint_date ON sim_checkpoint (scenario_id, observed_on);

------------------------------------------------------------------------------
-- Application + AI configuration.
-- API keys live in the OS keychain. This table stores only a *reference*.
------------------------------------------------------------------------------
CREATE TABLE app_setting (
    key        TEXT PRIMARY KEY,
    value_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

CREATE TABLE ai_provider_config (
    id                INTEGER PRIMARY KEY,
    provider          TEXT    NOT NULL UNIQUE CHECK (provider IN
                          ('anthropic','openai','gemini','deepseek','qwen',
                           'moonshot','zai','openai_compatible','ollama')),
    display_name      TEXT,
    model             TEXT    NOT NULL,
    base_url          TEXT,
    is_enabled        INTEGER NOT NULL DEFAULT 0 CHECK (is_enabled IN (0,1)),
    -- Keychain *account* name, never the key itself; the CHECK below is a
    -- cheap tripwire against a coding mistake that writes a real key here.
    keychain_ref      TEXT    CHECK (keychain_ref IS NULL
                                     OR keychain_ref NOT LIKE 'sk-%'),
    max_output_tokens INTEGER CHECK (max_output_tokens IS NULL OR max_output_tokens > 0),
    temperature       REAL    CHECK (temperature IS NULL OR (temperature >= 0.0 AND temperature <= 2.0)),
    system_prompt     TEXT
);

-- Every byte that leaves the machine is recorded here, before it is sent.
CREATE TABLE ai_egress_log (
    id               INTEGER PRIMARY KEY,
    occurred_at      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    provider         TEXT    NOT NULL,
    model            TEXT    NOT NULL,
    purpose          TEXT    NOT NULL CHECK (purpose IN
                         ('categorize','split_match','nl_query','connection_test')),
    payload_sha256   TEXT    NOT NULL,
    payload_preview  TEXT    NOT NULL,   -- exactly what the user was shown
    row_count        INTEGER,
    approved_by_user INTEGER NOT NULL DEFAULT 0 CHECK (approved_by_user IN (0,1)),
    request_bytes    INTEGER,
    response_bytes   INTEGER,
    outcome          TEXT    NOT NULL CHECK (outcome IN ('sent','failed','declined'))
);
CREATE INDEX ix_egress_time ON ai_egress_log (occurred_at);
