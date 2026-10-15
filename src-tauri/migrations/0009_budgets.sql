-- Budgets: a planned amount for one category over one period.
--
-- A period is either a year ('2026') or a month ('2026-03'). Storing the
-- period as the text prefix of `booked_on` means "what did I actually spend
-- against this?" is a substr() comparison against the ledger — no calendar
-- arithmetic, and a month budget and a year budget are read the same way.
--
-- Amounts are stored as a positive magnitude regardless of the category kind.
-- "I plan to spend 600" and "I plan to earn 40000" are both plans, and making
-- the sign depend on the category would mean a budget silently flips meaning
-- when the category is edited.
CREATE TABLE budget (
    id            INTEGER PRIMARY KEY,
    category_id   INTEGER NOT NULL REFERENCES pl_category(id) ON DELETE CASCADE,
    period        TEXT    NOT NULL CHECK (length(period) IN (4, 7)),
    amount_minor  INTEGER NOT NULL CHECK (amount_minor >= 0),
    notes         TEXT,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    UNIQUE (category_id, period)
);
CREATE INDEX ix_budget_period ON budget (period);
