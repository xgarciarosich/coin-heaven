-- Transactions someone deleted on purpose.
--
-- A statement is usually imported more than once: this month's file overlaps
-- last month's. The dedupe hash makes that a no-op for rows that are still
-- there — but a row that was deleted is no longer there, so the next
-- overlapping import would quietly bring it back. Remembering its hash is
-- what makes "delete" mean delete.
--
-- Only the hash is kept, not the transaction: nothing about what was removed
-- can be read back out of this table.
CREATE TABLE txn_removed (
    account_id   INTEGER NOT NULL REFERENCES account(id) ON DELETE CASCADE,
    dedupe_hash  TEXT    NOT NULL,
    removed_at   TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ','now')),
    PRIMARY KEY (account_id, dedupe_hash)
) WITHOUT ROWID;
