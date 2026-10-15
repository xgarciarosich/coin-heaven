-- v1 currency support, match decisions, and backup bookkeeping.

------------------------------------------------------------------------------
-- Currencies
--
-- Existing money columns already carry a `currency` code; this table gives the
-- UI a list to offer and records the minor-unit exponent, which is NOT 2 for
-- every currency (JPY/KRW = 0, KWD = 3). Existing tables are not retrofitted
-- with foreign keys — SQLite cannot add one via ALTER — so Rust validates
-- against `currency::CURRENCIES`, which this table mirrors.
------------------------------------------------------------------------------
CREATE TABLE currency (
    code       TEXT PRIMARY KEY CHECK (length(code) = 3),
    name       TEXT    NOT NULL,
    exponent   INTEGER NOT NULL CHECK (exponent BETWEEN 0 AND 3),
    symbol     TEXT    NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 100
);

INSERT INTO currency (code, name, exponent, symbol, sort_order) VALUES
    ('EUR','Euro',2,'€',1),                ('USD','US Dollar',2,'$',2),
    ('GBP','Pound Sterling',2,'£',3),      ('CHF','Swiss Franc',2,'CHF',4),
    ('JPY','Japanese Yen',0,'¥',5),        ('CAD','Canadian Dollar',2,'CA$',6),
    ('AUD','Australian Dollar',2,'A$',7),  ('SEK','Swedish Krona',2,'kr',8),
    ('NOK','Norwegian Krone',2,'kr',9),    ('DKK','Danish Krone',2,'kr',10),
    ('PLN','Polish Zloty',2,'zł',11),      ('CZK','Czech Koruna',2,'Kč',12),
    ('MXN','Mexican Peso',2,'MX$',13),     ('BRL','Brazilian Real',2,'R$',14),
    ('CNY','Chinese Yuan',2,'CN¥',15),     ('HKD','Hong Kong Dollar',2,'HK$',16),
    ('SGD','Singapore Dollar',2,'S$',17),  ('NZD','New Zealand Dollar',2,'NZ$',18),
    ('ZAR','South African Rand',2,'R',19), ('TRY','Turkish Lira',2,'₺',20),
    ('KRW','South Korean Won',0,'₩',21),   ('INR','Indian Rupee',2,'₹',22),
    ('AED','UAE Dirham',2,'AED',23),       ('KWD','Kuwaiti Dinar',3,'KD',24);

-- Rates are entered by hand. An offline-first app has no FX feed, and inventing
-- one would be the only outbound network call the app makes without asking.
CREATE TABLE fx_rate (
    id       INTEGER PRIMARY KEY,
    base     TEXT NOT NULL REFERENCES currency(code) ON DELETE RESTRICT,
    quote    TEXT NOT NULL REFERENCES currency(code) ON DELETE RESTRICT,
    -- Parts per million, integer: 1.0834 EUR/USD -> 1083400. No float rates.
    rate_ppm INTEGER NOT NULL CHECK (rate_ppm > 0),
    as_of    TEXT NOT NULL,
    source   TEXT NOT NULL DEFAULT 'manual' CHECK (source IN ('manual','import')),
    UNIQUE (base, quote, as_of),
    CHECK (base <> quote),
    CHECK (as_of GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]')
);
CREATE INDEX ix_fx_rate_pair ON fx_rate (base, quote, as_of);

------------------------------------------------------------------------------
-- Match decisions
--
-- Transfer pairs and split-bill reimbursements are proposed, never applied
-- silently. A rejected pair is remembered so the drawer does not keep
-- re-proposing it after every import.
------------------------------------------------------------------------------
CREATE TABLE match_decision (
    id         INTEGER PRIMARY KEY,
    kind       TEXT    NOT NULL CHECK (kind IN ('transfer','split')),
    -- Always stored with the lower id first, so a pair has one canonical row.
    txn_lo_id  INTEGER NOT NULL REFERENCES txn(id) ON DELETE CASCADE,
    txn_hi_id  INTEGER NOT NULL REFERENCES txn(id) ON DELETE CASCADE,
    decision   TEXT    NOT NULL CHECK (decision IN ('confirmed','rejected')),
    decided_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    UNIQUE (kind, txn_lo_id, txn_hi_id),
    CHECK (txn_lo_id < txn_hi_id)
);
CREATE INDEX ix_match_decision_lo ON match_decision (txn_lo_id);
CREATE INDEX ix_match_decision_hi ON match_decision (txn_hi_id);

------------------------------------------------------------------------------
-- Backups
------------------------------------------------------------------------------
CREATE TABLE backup_log (
    id         INTEGER PRIMARY KEY,
    created_at TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    path       TEXT    NOT NULL,
    bytes      INTEGER NOT NULL DEFAULT 0,
    trigger    TEXT    NOT NULL CHECK (trigger IN
                   ('interval','import','shutdown','manual')),
    outcome    TEXT    NOT NULL CHECK (outcome IN ('ok','failed')),
    detail     TEXT
);
CREATE INDEX ix_backup_log_time ON backup_log (created_at);

INSERT INTO app_setting (key, value_json) VALUES
    -- Backup: coalesced rather than literally per-write. See docs/adr/0007.
    ('backup.enabled',              'true'),
    ('backup.directory',            'null'),
    ('backup.min_interval_seconds', '300'),
    ('backup.retain_copies',        '14'),
    ('recovery.phrase_confirmed',   'false'),
    -- Transfer matcher
    ('match.transfer.window_days',       '3'),
    ('match.transfer.tolerance_minor',   '200'),
    -- Split-bill matcher
    ('match.split.window_hours',         '72'),
    ('match.split.min_parent_minor',     '3000'),
    ('ui.currency.display',              '"EUR"');
