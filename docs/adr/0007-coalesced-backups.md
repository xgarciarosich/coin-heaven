# 7. Backups are coalesced, and the backup format is the database itself

**Status:** accepted · implements decision 1.4, with one deviation

## Context
The decision was "auto-scheduled encrypted backup (.enc) exported to a
user-selected folder on **every database write**".

## Decision
Two departures from the literal wording, both to serve the same goal.

**Coalesced, not per-write.** Importing a 5,000-row statement is 5,000 writes.
A backup per write would produce 5,000 full-database copies, turn a two-second
import into minutes of I/O, and serialise the import behind its own backups
(each `VACUUM INTO` holds a read lock). Instead:

- every mutating command marks the database dirty;
- a backup runs when dirty **and** `backup.min_interval_seconds` (default 300)
  has elapsed;
- **import** and **shutdown** force one immediately, ignoring the interval;
- a manual "Back up now" is always available.

The two moments where meaningful work could be lost are therefore always
covered. Worst case is one idle interval of edits.

**No separate `.enc` format.** The database is already SQLCipher-encrypted and
`VACUUM INTO` writes its output through the same cipher, so the copy is
ciphertext by construction and opens with the same recovery phrase. Inventing a
second encrypted container would add a crypto stack to implement, review and get
wrong, for no additional protection. `backup_is_encrypted_and_openable_with_the_same_key`
asserts both halves — unreadable on disk, and a complete working database under
the same key. The `.db.enc` extension is kept so the files are obviously not
plaintext.

## Consequences
- Backups are consistent and defragmented, not raw file copies mid-WAL.
- `atlas.keyinfo` is copied next to every backup. Without the salt the phrase
  cannot derive the key, so a backup without the sidecar is only half a backup.
- Retention keeps the newest `backup.retain_copies` (default 14); filenames are
  sortable UTC timestamps, so pruning needs no filesystem metadata.
- Every attempt is recorded in `backup_log`, successes and failures alike.
