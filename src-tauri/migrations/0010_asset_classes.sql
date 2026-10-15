-- Asset classes become rows the user owns, not a CHECK constraint.
--
-- The eight hardcoded buckets were a guess at what one person holds. Someone
-- with a business stake, a classic car, a stock-option grant or a private
-- pension in another country had nowhere to put it except "Other asset",
-- which is the same as not recording it.
--
-- What cannot be user-defined is the *meaning*. Net worth, the FIRE liquid
-- target and property equity all depend on knowing whether a line is spendable
-- capital, a property with a mortgage against it, or a debt. So a class carries
-- a `kind` from a fixed set, and the maths keys off the kind rather than off
-- the name. A user can call a class anything; they cannot invent a fifth way
-- for it to behave.

CREATE TABLE asset_class (
    -- Stable slug. `wealth_line.bucket` stores this, so renaming a class for
    -- display never rewrites history.
    key         TEXT PRIMARY KEY,
    name        TEXT    NOT NULL,
    kind        TEXT    NOT NULL CHECK (kind IN
                    ('liquid','property','other_asset','liability')),
    color       TEXT,
    sort_order  INTEGER NOT NULL DEFAULT 100,
    is_archived INTEGER NOT NULL DEFAULT 0 CHECK (is_archived IN (0,1))
);

INSERT INTO asset_class (key, name, kind, color, sort_order) VALUES
    ('cash',          'Cash',          'liquid',      '#94a3b8', 10),
    ('equities',      'Equities',      'liquid',      '#22c55e', 20),
    ('crypto',        'Crypto',        'liquid',      '#a855f7', 30),
    ('pension',       'Pension',       'liquid',      '#3b82f6', 40),
    ('real_estate',   'Real estate',   'property',    '#d97706', 50),
    ('other_asset',   'Other asset',   'other_asset', '#64748b', 60),
    ('mortgage_debt', 'Mortgage debt', 'liability',   '#ef4444', 70),
    ('other_debt',    'Other debt',    'liability',   '#f87171', 80);

-- The CHECK constraint named the eight buckets, so the table has to be rebuilt
-- rather than altered. Ids are preserved.
CREATE TABLE wealth_line_new (
    id                INTEGER PRIMARY KEY,
    snapshot_id       INTEGER NOT NULL REFERENCES wealth_snapshot(id) ON DELETE CASCADE,
    bucket            TEXT    NOT NULL REFERENCES asset_class(key),
    label             TEXT    NOT NULL,
    gross_value_minor INTEGER NOT NULL DEFAULT 0,
    debt_minor        INTEGER NOT NULL DEFAULT 0 CHECK (debt_minor >= 0),
    currency          TEXT    NOT NULL DEFAULT 'EUR'
);
INSERT INTO wealth_line_new
       (id, snapshot_id, bucket, label, gross_value_minor, debt_minor, currency)
SELECT  id, snapshot_id, bucket, label, gross_value_minor, debt_minor, currency
FROM wealth_line;

DROP VIEW  v_net_worth;
DROP TABLE wealth_line;
ALTER TABLE wealth_line_new RENAME TO wealth_line;
CREATE INDEX ix_wealth_line_snapshot ON wealth_line (snapshot_id);
CREATE INDEX ix_wealth_line_bucket   ON wealth_line (bucket);

-- One definition of "liquid".
--
-- The old view counted cash, equities and crypto; the FIRE command counted
-- those plus pensions. Two definitions of the same word in one app is a bug
-- waiting to be reported as a discrepancy, so both now read the class kind.
-- A user who does not consider a pension spendable reclassifies it, which is
-- the whole point of the classes being editable.
CREATE VIEW v_net_worth AS
SELECT s.id         AS snapshot_id,
       s.taken_on,
       s.label,
       COALESCE(SUM(CASE WHEN k.kind = 'liability' THEN 0
                         ELSE l.gross_value_minor END), 0)  AS gross_assets_minor,
       COALESCE(SUM(CASE WHEN k.kind = 'liability'
                         THEN l.gross_value_minor + l.debt_minor
                         ELSE l.debt_minor END), 0)         AS total_debt_minor,
       COALESCE(SUM(CASE WHEN k.kind = 'liability'
                         THEN -(l.gross_value_minor + l.debt_minor)
                         ELSE l.gross_value_minor - l.debt_minor END), 0)
                                                            AS net_worth_minor,
       COALESCE(SUM(CASE WHEN k.kind = 'liquid'
                         THEN l.gross_value_minor - l.debt_minor ELSE 0 END), 0)
                                                            AS liquid_minor,
       COALESCE(SUM(CASE WHEN k.kind = 'property'
                         THEN l.gross_value_minor - l.debt_minor ELSE 0 END), 0)
                                                            AS re_equity_minor
FROM wealth_snapshot s
LEFT JOIN wealth_line  l ON l.snapshot_id = s.id
LEFT JOIN asset_class  k ON k.key = l.bucket
GROUP BY s.id;
