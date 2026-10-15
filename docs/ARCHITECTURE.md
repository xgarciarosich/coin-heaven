# Architecture

```
┌──────────────────────────────────────────────────────────────────────┐
│ WEBVIEW  (React 19 · TypeScript · Vite · Tailwind 4 · Recharts)      │
│  No fetch. No filesystem. No shell. No HTTP capability.             │
│  CSP: connect-src 'self' ipc: http://ipc.localhost                  │
└──────────────────────────────────────────────────────────────────────┘
                              │ Tauri IPC — 7 named, typed commands
                              ▼
┌──────────────────────────────────────────────────────────────────────┐
│ RUST CORE  (src-tauri)                                              │
│  commands.rs   the entire IPC surface, one file                     │
│  sim/          pure projection engine — no I/O, no clock            │
│  money.rs      i64 minor units; the only numeric type for money     │
│  security/     recovery (envelope keys) · pii (egress redaction)   │
│  db.rs         one connection, prepared statements only             │
└──────────────────────────────────────────────────────────────────────┘
              │                                    │
              ▼                                    ▼
┌───────────────────────────┐      ┌──────────────────────────────────┐
│ KEY MATERIAL              │      │ SQLCipher (AES-256)              │
│ atlas.keyinfo — wrapped   │      │ WAL · foreign_keys=ON            │
│   under phrase/passphrase │      │ 20 tables · 3 views              │
│ atlas.localkey (0600) —   │      │ ~/Library/Application Support/…  │
│   only when no passphrase │      │                                  │
└───────────────────────────┘      └──────────────────────────────────┘

No OS keychain is involved; see docs/adr/0003.
```

## Boundaries that carry weight

**The webview cannot reach the network or the disk.** Not by convention — by
`capabilities/default.json`, which grants only `core:default` and two dialog
permissions. The CSP is a second layer over the same property. Any future AI
call is a Rust command, which is why "what can this app send?" is answerable by
reading one module rather than auditing a React tree.

**Money never becomes a float.** `Money(i64)` serialises as a bare integer, so
IPC payloads contain no decimal points at all — asserted by a test
(`params_round_trip_through_json_as_camel_case`). The frontend formats for
display in `lib/money.ts` and does no arithmetic.

**The simulator is pure.** `sim::run(&params)` takes parameters and returns a
projection. No database, no clock, no randomness. It is called on every slider
change and is trivially testable, which is why it was the first thing built.

## Choices worth knowing about

**One database connection behind a mutex, not a pool.** `PRAGMA foreign_keys`
is per-connection, and every SQLCipher connection must be re-keyed. A pool
multiplies both concerns for no gain in a single-user desktop app whose queries
are sub-millisecond.

**The simulation engine lives in Rust, not TypeScript.** A 120-month run
completes in well under a millisecond, so an 80 ms debounce is all the slider
smoothing the UI needs. The alternative — reimplementing the model in TS for
"instant" feedback — means two copies of the same financial maths, which is a
guaranteed divergence bug.

**Views, not stored aggregates.** `v_txn_effective` resolves category
precedence (a transaction's own override beats its event's default) and
`v_txn_net` nets reimbursements against their parent. Defining these once in
SQL keeps every report consistent; caching them would create staleness bugs the
moment a reimbursement is edited.

**Rates are basis points.** `2.8% -> 280`. Integers throughout, so scenario
parameters compare and hash exactly.

## Layout

```
src/                        React app
  lib/{ipc,money,types}.ts  the only IPC surface + formatting
  features/simulator/       the built module
src-tauri/
  migrations/*.sql          embedded via include_str!, versioned by user_version
  src/sim/                  engine + mortgage maths + tests
  src/security/             keystore + pii
  capabilities/default.json the webview's permission grant
docs/adr/                   decisions and their reasons
```

## Not built yet

Modules 1, 2, 3 and 5 have schema, indexes and constraints in place but no Rust
logic.
