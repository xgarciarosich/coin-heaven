# 6. Imports are idempotent and reversible

**Status:** accepted · fills a gap in the spec

## Context
The spec covers parsing and validation but never re-imports. Bank exports
overlap constantly; as specified, every overlapping row would be double-counted
and the P&L silently wrong.

## Decision
- `txn.dedupe_hash` with `UNIQUE (account_id, dedupe_hash)`.
- `import_batch` records the file SHA-256 and inserted/duplicate/rejected counts;
  `txn.import_batch_id` makes a batch reversible as a unit.
- Each import runs in one transaction.

## Consequences
- Re-importing an overlapping statement is a no-op, enforced by the database
  rather than by application care. Test:
  `the_dedupe_index_blocks_a_repeated_import_row`.
- The hash composition is still open — it must include an intra-day ordinal so
  two genuine identical purchases on the same day are not collapsed.
