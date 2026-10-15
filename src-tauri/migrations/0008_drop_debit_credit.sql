-- Remove separate debit/credit column support from bank mappings.
--
-- The table is rebuilt rather than altered because its CHECK constraint names
-- the columns being dropped, and SQLite cannot alter a CHECK. Existing
-- mappings are carried across with their ids intact so `import_batch.mapping_id`
-- keeps pointing at the right row.
--
-- A mapping that relied on a debit/credit pair has no amount column, so it
-- cannot survive the change; those rows are dropped and the user re-picks the
-- column. Silently leaving a broken mapping in the list would be worse.
CREATE TABLE bank_mapping_new (
    id                  INTEGER PRIMARY KEY,
    name                TEXT    NOT NULL UNIQUE,
    delimiter           TEXT    NOT NULL DEFAULT ',' CHECK (length(delimiter) = 1),
    encoding            TEXT    NOT NULL DEFAULT 'utf-8',
    has_header          INTEGER NOT NULL DEFAULT 1 CHECK (has_header IN (0,1)),
    skip_rows           INTEGER NOT NULL DEFAULT 0 CHECK (skip_rows >= 0),
    date_col            INTEGER NOT NULL CHECK (date_col >= 0),
    date_format         TEXT    NOT NULL,
    description_col     INTEGER NOT NULL CHECK (description_col >= 0),
    amount_col          INTEGER NOT NULL CHECK (amount_col >= 0),
    balance_col         INTEGER,
    counterparty_col    INTEGER,
    decimal_separator   TEXT    NOT NULL DEFAULT ',' CHECK (length(decimal_separator) = 1),
    thousands_separator TEXT    NOT NULL DEFAULT '.',
    invert_sign         INTEGER NOT NULL DEFAULT 0 CHECK (invert_sign IN (0,1)),
    currency            TEXT    NOT NULL DEFAULT 'EUR' CHECK (length(currency) = 3),
    created_at          TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now'))
);

INSERT INTO bank_mapping_new
    (id, name, delimiter, encoding, has_header, skip_rows, date_col, date_format,
     description_col, amount_col, balance_col, counterparty_col,
     decimal_separator, thousands_separator, invert_sign, currency, created_at)
SELECT id, name, delimiter, encoding, has_header, skip_rows, date_col, date_format,
       description_col, amount_col, balance_col, counterparty_col,
       decimal_separator, thousands_separator, invert_sign, currency, created_at
FROM bank_mapping
WHERE amount_col IS NOT NULL;

DROP TABLE bank_mapping;
ALTER TABLE bank_mapping_new RENAME TO bank_mapping;
