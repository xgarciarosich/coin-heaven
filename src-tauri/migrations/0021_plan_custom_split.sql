-- A couple can split what they share by any proportion they agree on, not
-- only half each or by income. `custom_a_bps` is person A's share in basis
-- points (6000 = 60 %), used when `split` is 'custom'.
--
-- SQLite cannot widen a CHECK in place, so the month table is rebuilt; foreign
-- keys are off while migrations run, and the boxes keep pointing at it by name.
CREATE TABLE plan_month_new (
    period       TEXT PRIMARY KEY
                 CHECK (period GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]'),
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    mode         TEXT NOT NULL DEFAULT 'individual' CHECK (mode IN ('individual', 'couple')),
    person_a     TEXT NOT NULL DEFAULT '',
    person_b     TEXT NOT NULL DEFAULT '',
    split        TEXT NOT NULL DEFAULT 'equal' CHECK (split IN ('equal', 'income', 'custom')),
    custom_a_bps INTEGER NOT NULL DEFAULT 5000 CHECK (custom_a_bps BETWEEN 0 AND 10000)
) WITHOUT ROWID;

INSERT INTO plan_month_new (period, created_at, mode, person_a, person_b, split)
SELECT period, created_at, mode, person_a, person_b, split FROM plan_month;

DROP TABLE plan_month;
ALTER TABLE plan_month_new RENAME TO plan_month;
