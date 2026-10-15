# 2. rusqlite, one connection, custom-free migrations

**Status:** accepted · resolves the spec's "SQLx or rusqlite"

## Context
The spec left the driver open. SQLCipher needs `PRAGMA key` as the first
statement on every connection, and `PRAGMA foreign_keys` is per-connection.

## Decision
`rusqlite` with `bundled-sqlcipher-vendored-openssl` — SQLCipher and OpenSSL
compile into the binary, so no system crypto library is required on either
platform. `rusqlite_migration` drives versioning via `user_version`; migrations
are plain `.sql` embedded with `include_str!`.

One connection behind a `Mutex`, not a pool: a pool multiplies re-keying and
pragma setup for no gain in a single-user app with sub-millisecond queries.

## Consequences
- SQLx's compile-time query checking is given up. Acceptable: SQLx's SQLite
  driver has no first-class SQLCipher support, which is the harder requirement.
- Longer first build (OpenSSL from source), then cached.
- `rust-version` is 1.85. Setting it lower silently downgrades transitive crates
  — it pinned `rusqlite_migration` to 1.3 and caused a `libsqlite3-sys`
  conflict during scaffolding.
