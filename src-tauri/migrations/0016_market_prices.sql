-- A price can now come from a market-data source as well as from a person or
-- the assistant.
--
-- The CHECK on `source` names the allowed values, and a CHECK cannot be
-- altered, so the (small) table is rebuilt. Nothing is lost: every row is
-- copied across as it was.
CREATE TABLE holding_price_new (
    holding_id   INTEGER NOT NULL REFERENCES holding(id) ON DELETE CASCADE,
    on_date      TEXT    NOT NULL,
    price_micro  INTEGER NOT NULL CHECK (price_micro >= 0),
    -- 'manual', 'assistant' or 'market'. A price that was not typed carries
    -- the page it came from, so it can be checked.
    source       TEXT    NOT NULL DEFAULT 'manual'
                 CHECK (source IN ('manual','assistant','market')),
    source_url   TEXT,
    recorded_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    PRIMARY KEY (holding_id, on_date)
);
INSERT INTO holding_price_new
       (holding_id, on_date, price_micro, source, source_url, recorded_at)
SELECT  holding_id, on_date, price_micro, source, source_url, recorded_at
FROM holding_price;
DROP TABLE holding_price;
ALTER TABLE holding_price_new RENAME TO holding_price;
