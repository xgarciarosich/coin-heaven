-- Coin Heaven schema v1.
--
-- Conventions enforced across every table:
--   * Money  -> INTEGER minor units (cents). NEVER REAL. See docs/adr/0001.
--   * Dates  -> TEXT 'YYYY-MM-DD' (ISO-8601), guarded by GLOB CHECKs.
--   * Stamps -> TEXT RFC3339 UTC.
--   * Booleans -> INTEGER 0/1 with a CHECK.
--   * Every FK states its ON DELETE behaviour explicitly.

------------------------------------------------------------------------------
-- Accounts
------------------------------------------------------------------------------
CREATE TABLE account (
    id           INTEGER PRIMARY KEY,
    name         TEXT    NOT NULL,
    institution  TEXT,
    kind         TEXT    NOT NULL CHECK (kind IN
                     ('checking','savings','credit_card','brokerage',
                      'crypto','pension','mortgage','cash','other')),
    currency     TEXT    NOT NULL DEFAULT 'EUR' CHECK (length(currency) = 3),
    -- Deliberately NOT the full IBAN: only enough to recognise the account.
    iban_last4   TEXT    CHECK (iban_last4 IS NULL OR length(iban_last4) = 4),
    is_archived  INTEGER NOT NULL DEFAULT 0 CHECK (is_archived IN (0,1)),
    created_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    UNIQUE (name)
);

------------------------------------------------------------------------------
-- P&L category hierarchy
------------------------------------------------------------------------------
CREATE TABLE pl_category (
    id          INTEGER PRIMARY KEY,
    parent_id   INTEGER REFERENCES pl_category(id) ON DELETE RESTRICT,
    name        TEXT    NOT NULL,
    kind        TEXT    NOT NULL CHECK (kind IN ('income','expense','neutral')),
    color       TEXT,
    icon        TEXT,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    is_archived INTEGER NOT NULL DEFAULT 0 CHECK (is_archived IN (0,1)),
    CHECK (parent_id IS NULL OR parent_id <> id)
);
-- SQLite treats NULLs as distinct in UNIQUE, so a plain UNIQUE(parent_id,name)
-- would allow duplicate root categories. COALESCE closes that hole.
CREATE UNIQUE INDEX ux_pl_category_name
    ON pl_category (COALESCE(parent_id, -1), name);

------------------------------------------------------------------------------
-- Event-first project containers
------------------------------------------------------------------------------
CREATE TABLE project_event (
    id                  INTEGER PRIMARY KEY,
    name                TEXT    NOT NULL UNIQUE,
    -- The roll-up target: every attached txn without its own override lands here.
    default_category_id INTEGER NOT NULL REFERENCES pl_category(id) ON DELETE RESTRICT,
    starts_on           TEXT,
    ends_on             TEXT,
    budget_minor        INTEGER,
    currency            TEXT    NOT NULL DEFAULT 'EUR' CHECK (length(currency) = 3),
    notes               TEXT,
    is_closed           INTEGER NOT NULL DEFAULT 0 CHECK (is_closed IN (0,1)),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    CHECK (starts_on IS NULL OR starts_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    CHECK (ends_on   IS NULL OR ends_on   GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    CHECK (starts_on IS NULL OR ends_on IS NULL OR ends_on >= starts_on)
);

------------------------------------------------------------------------------
-- CSV import: per-bank column mapping presets
------------------------------------------------------------------------------
CREATE TABLE bank_mapping (
    id                  INTEGER PRIMARY KEY,
    name                TEXT    NOT NULL UNIQUE,
    delimiter           TEXT    NOT NULL DEFAULT ',' CHECK (length(delimiter) = 1),
    encoding            TEXT    NOT NULL DEFAULT 'utf-8',
    has_header          INTEGER NOT NULL DEFAULT 1 CHECK (has_header IN (0,1)),
    skip_rows           INTEGER NOT NULL DEFAULT 0 CHECK (skip_rows >= 0),
    date_col            INTEGER NOT NULL CHECK (date_col >= 0),
    date_format         TEXT    NOT NULL,              -- chrono strftime, e.g. '%d/%m/%Y'
    description_col     INTEGER NOT NULL CHECK (description_col >= 0),
    -- Either one signed amount column, or a debit/credit pair. Never neither.
    amount_col          INTEGER CHECK (amount_col IS NULL OR amount_col >= 0),
    debit_col           INTEGER CHECK (debit_col  IS NULL OR debit_col  >= 0),
    credit_col          INTEGER CHECK (credit_col IS NULL OR credit_col >= 0),
    balance_col         INTEGER,
    counterparty_col    INTEGER,
    decimal_separator   TEXT    NOT NULL DEFAULT ',' CHECK (length(decimal_separator) = 1),
    thousands_separator TEXT    NOT NULL DEFAULT '.',
    invert_sign         INTEGER NOT NULL DEFAULT 0 CHECK (invert_sign IN (0,1)),
    currency            TEXT    NOT NULL DEFAULT 'EUR' CHECK (length(currency) = 3),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    CHECK (amount_col IS NOT NULL
           OR (debit_col IS NOT NULL AND credit_col IS NOT NULL))
);

-- One row per import run: makes an import auditable and undoable.
CREATE TABLE import_batch (
    id              INTEGER PRIMARY KEY,
    account_id      INTEGER NOT NULL REFERENCES account(id)     ON DELETE CASCADE,
    mapping_id      INTEGER          REFERENCES bank_mapping(id) ON DELETE SET NULL,
    source_filename TEXT,
    source_sha256   TEXT    NOT NULL,
    imported_at     TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    row_count       INTEGER NOT NULL DEFAULT 0,
    inserted_count  INTEGER NOT NULL DEFAULT 0,
    duplicate_count INTEGER NOT NULL DEFAULT 0,
    rejected_count  INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX ix_import_batch_account ON import_batch (account_id, imported_at);

------------------------------------------------------------------------------
-- Transactions
------------------------------------------------------------------------------
CREATE TABLE txn (
    id                   INTEGER PRIMARY KEY,
    account_id           INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    booked_on            TEXT    NOT NULL,
    value_on             TEXT,
    amount_minor         INTEGER NOT NULL,          -- signed: <0 outflow, >0 inflow
    currency             TEXT    NOT NULL DEFAULT 'EUR' CHECK (length(currency) = 3),
    description_raw      TEXT    NOT NULL,
    description_norm     TEXT    NOT NULL,          -- folded: dedupe + rule matching
    counterparty         TEXT,

    project_id           INTEGER REFERENCES project_event(id) ON DELETE SET NULL,
    category_override_id INTEGER REFERENCES pl_category(id)   ON DELETE SET NULL,

    -- Self-referential link for split bills / reimbursements / transfer pairs.
    parent_txn_id        INTEGER REFERENCES txn(id) ON DELETE SET NULL,
    link_kind            TEXT CHECK (link_kind IN
                             ('reimbursement','split_child','transfer_pair')),

    is_neutral_transfer  INTEGER NOT NULL DEFAULT 0 CHECK (is_neutral_transfer IN (0,1)),
    transfer_group_id    TEXT,

    categorized_by       TEXT    NOT NULL DEFAULT 'none'
                             CHECK (categorized_by IN ('none','rule','ai','user')),
    ai_confidence        REAL    CHECK (ai_confidence IS NULL
                                        OR (ai_confidence >= 0.0 AND ai_confidence <= 1.0)),
    needs_review         INTEGER NOT NULL DEFAULT 0 CHECK (needs_review IN (0,1)),

    import_batch_id      INTEGER REFERENCES import_batch(id) ON DELETE SET NULL,
    dedupe_hash          TEXT    NOT NULL,
    notes                TEXT,
    created_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    updated_at           TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),

    CHECK (booked_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    CHECK (value_on IS NULL OR value_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    CHECK (parent_txn_id IS NULL OR parent_txn_id <> id),
    CHECK ((parent_txn_id IS NULL) = (link_kind IS NULL))
);

-- Re-importing an overlapping statement must be a no-op, not a double count.
CREATE UNIQUE INDEX ux_txn_dedupe        ON txn (account_id, dedupe_hash);
CREATE        INDEX ix_txn_booked_on     ON txn (booked_on);
CREATE        INDEX ix_txn_account_date  ON txn (account_id, booked_on);
CREATE        INDEX ix_txn_project       ON txn (project_id)           WHERE project_id IS NOT NULL;
CREATE        INDEX ix_txn_category      ON txn (category_override_id) WHERE category_override_id IS NOT NULL;
CREATE        INDEX ix_txn_parent        ON txn (parent_txn_id)        WHERE parent_txn_id IS NOT NULL;
CREATE        INDEX ix_txn_review        ON txn (needs_review)         WHERE needs_review = 1;
CREATE        INDEX ix_txn_transfer_grp  ON txn (transfer_group_id)    WHERE transfer_group_id IS NOT NULL;
CREATE        INDEX ix_txn_batch         ON txn (import_batch_id)      WHERE import_batch_id IS NOT NULL;

CREATE TRIGGER trg_txn_touch AFTER UPDATE ON txn
FOR EACH ROW WHEN NEW.updated_at = OLD.updated_at
BEGIN
    UPDATE txn SET updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now') WHERE id = NEW.id;
END;

------------------------------------------------------------------------------
-- Deterministic categorisation rules (the local, AI-free path)
------------------------------------------------------------------------------
CREATE TABLE categorization_rule (
    id                  INTEGER PRIMARY KEY,
    name                TEXT    NOT NULL,
    priority            INTEGER NOT NULL DEFAULT 100,
    match_kind          TEXT    NOT NULL CHECK (match_kind IN
                             ('contains','starts_with','exact','regex')),
    pattern             TEXT    NOT NULL,
    account_id          INTEGER REFERENCES account(id)       ON DELETE CASCADE,
    min_amount_minor    INTEGER,
    max_amount_minor    INTEGER,
    set_category_id     INTEGER REFERENCES pl_category(id)   ON DELETE CASCADE,
    set_project_id      INTEGER REFERENCES project_event(id) ON DELETE CASCADE,
    set_neutral_transfer INTEGER NOT NULL DEFAULT 0 CHECK (set_neutral_transfer IN (0,1)),
    is_enabled          INTEGER NOT NULL DEFAULT 1 CHECK (is_enabled IN (0,1)),
    CHECK (set_category_id IS NOT NULL
           OR set_project_id IS NOT NULL
           OR set_neutral_transfer = 1)
);
CREATE INDEX ix_rule_enabled ON categorization_rule (is_enabled, priority);

------------------------------------------------------------------------------
-- Derived views: one definition of "effective category" and "net amount"
------------------------------------------------------------------------------
-- A txn's own override beats its event's default category.
CREATE VIEW v_txn_effective AS
SELECT t.id,
       t.account_id,
       t.booked_on,
       t.amount_minor,
       t.currency,
       t.description_raw,
       t.project_id,
       t.is_neutral_transfer,
       COALESCE(t.category_override_id, p.default_category_id) AS effective_category_id
FROM txn t
LEFT JOIN project_event p ON p.id = t.project_id;

-- Net expense = the parent charge plus every reimbursement booked against it.
-- Reimbursement children are excluded from the top-level ledger so they are
-- never counted twice.
CREATE VIEW v_txn_net AS
SELECT e.id,
       e.account_id,
       e.booked_on,
       e.currency,
       e.project_id,
       e.effective_category_id,
       e.amount_minor AS gross_minor,
       e.amount_minor + COALESCE(
           (SELECT SUM(c.amount_minor) FROM txn c
             WHERE c.parent_txn_id = e.id AND c.link_kind = 'reimbursement'), 0
       ) AS net_minor
FROM v_txn_effective e
JOIN txn t ON t.id = e.id
WHERE t.is_neutral_transfer = 0
  AND NOT (t.parent_txn_id IS NOT NULL AND t.link_kind = 'reimbursement');
