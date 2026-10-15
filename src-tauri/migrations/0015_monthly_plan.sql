-- The monthly plan: a worksheet of boxes, income and spending, for a month.
--
-- It is deliberately not tied to the ledger or to budgets. A budget is a
-- target per category that Coin Heaven measures what actually happened against; a
-- plan is the sum someone writes on paper before the month — "salary 2.400,
-- rent 900, …" — in the order they think about it, ending in what is left.
-- Keeping it free-form is the point: any name, any order, no category to
-- choose.
--
-- One row per month that has a plan, so the history of past plans is simply
-- the list of rows. Items are kept in the order the person arranged them.
CREATE TABLE plan_month (
    period      TEXT PRIMARY KEY
                CHECK (period GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
) WITHOUT ROWID;

CREATE TABLE plan_item (
    id            INTEGER PRIMARY KEY,
    period        TEXT    NOT NULL REFERENCES plan_month(period) ON DELETE CASCADE,
    kind          TEXT    NOT NULL CHECK (kind IN ('income','expense')),
    name          TEXT    NOT NULL DEFAULT '',
    -- A magnitude: the kind says which way it goes.
    amount_minor  INTEGER NOT NULL CHECK (amount_minor >= 0),
    sort_order    INTEGER NOT NULL
);
CREATE INDEX ix_plan_item_period ON plan_item (period, sort_order);
