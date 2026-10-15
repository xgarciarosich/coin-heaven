# Security review

What was examined before the first public release, what it found, what was
fixed, and what was deliberately left. It is written for someone deciding
whether to trust the app with real statements, and for the next reviewer.

This is a review by the project's own author and an AI coding assistant. It is
not an independent audit. The one part that most deserves outside eyes is the
key handling (`src-tauri/src/security/recovery.rs`, `bootstrap.rs`, `db.rs`);
a review of that by someone who does this for a living is welcome and
requested.

## How it was done

| Layer | What | Where it lives |
|---|---|---|
| Dependency audit | `cargo audit`, `cargo deny` (advisories, licences, sources), `pnpm audit` | CI job `supply-chain`; policy in `src-tauri/deny.toml` |
| Secret scan | `gitleaks` over the tracked tree and the published history | run before release |
| Static guard | `scripts/check-security.mjs`: no `innerHTML`/`eval`/`fetch` in the UI, a strict CSP, a minimal capability file, no install scripts, no risky workflow patterns, no personal paths or addresses | `pnpm check:security`, CI |
| Threat-area review | Code review organised by what can go wrong: IPC boundary, keys and recovery, file import, export and backup paths, network egress, the AI path, SQL and data integrity, data at rest, supply chain | this document |
| Adversarial tests | Hostile statements and workbooks, tampered key files, injection and formula text, delete paths, privacy of outbound requests | `src-tauri/tests/security_*.rs` |
| Fuzzing | Parsers and readers of untrusted text | `src-tauri/fuzz` |

Each reported problem was checked against the code before it was accepted.
Problems that survived were fixed and given a test that fails without the fix.

## Fixed

**Keys and recovery**

- Choosing the data folder as the backup folder zeroed `atlas.keyinfo`
  (copying a file onto itself). The data folder is now refused, each backup
  gets its own key sidecar, and backup files are written whole or not at all.
- First-run setup next to an existing database could replace its keys,
  stranding the data. It now refuses.
- The old-data-folder adoption could leave a database without its key files.
  It is now copy-then-rename.
- The legacy upgrade rewrote the database before saving the new key info. The
  new key info is now written first and renamed into place after.
- Key files were written with default permissions and in place. They are now
  created `0600`, flushed, and replaced whole; the data folder is `0700` and
  database files `0600` (macOS and Linux).
- The key-info file's cost parameters were trusted before authentication. They
  are bounded, and unknown versions or schemes are refused.
- A plaintext key file left beside a passphrase-protected database (a crash
  between two steps) is removed at start-up.

**Hostile input**

- A workbook that inflates to gigabytes (zip bomb) or that has one cell at the
  far corner of a sheet could abort the process. Workbooks are now read cell by
  cell with limits, in a separate copy of the program, with a timeout.
- Text decoding in the search-result reader could panic on non-ASCII input.
- A budget period or a rule or a symbol with unusual characters could panic or
  match everything.
- Spreadsheet formulas in bank descriptions (`=…`) are written to exports as
  text.

**Data integrity**

- Deleting an account, undoing an import or removing the sample failed when a
  refund was linked across. Fixed with a trigger; the same trigger repairs a
  transfer when one leg is deleted.
- Removing the sample deleted anything the person had added to the sample
  account. It now removes only the rows the sample wrote.
- A price typed by hand could be overwritten by a looked-up one.
- London share prices quoted in pence were stored a hundred times too high.
- Net-worth lines created by portfolio sync were labelled in a fixed currency.

**Network and AI**

- Redirects now stay on the same host and never downgrade to http; replies are
  capped at 8 MiB.
- "Answered on this machine" is decided by the address (loopback), not by the
  provider's name; plain http is refused for remote addresses.
- Only one AI provider is enabled at a time (several used to be chosen between
  alphabetically).
- Personal data is masked before text is normalised, so e-mail addresses and
  dashed IBANs are not missed.
- Prices read by a model must be positive and plausible; a model-suggested
  thousands separator must be a real grouping mark.

**Supply chain**

- Updated `rustls`, `quinn-proto` and `quick-xml` past published advisories;
  pinned the build-time `source-map-js`.
- CI actions are pinned to commit hashes.

## Known and accepted

| Item | Why it stands |
|---|---|
| Weak passphrases such as "password123" are accepted; only length (8+) is enforced | A product decision. A stronger gate is easy to add in `rate_passphrase`; the recovery words are the strong secret. |
| In "open automatically" mode the key sits in `atlas.localkey` | Stated plainly in the app and in the threat model; passphrase mode keeps no key on disk. |
| The kind of a key wrap (passphrase vs recovery) is not authenticated | An attacker who can alter the file can already delete it; low impact. |
| Accented text composed vs decomposed (NFC/NFD) is treated as different on re-import | Changing the normalisation would change every stored fingerprint and duplicate existing transactions. |
| Paths for import, export and backup come from the interface | They are chosen in the system file dialog; the interface has no filesystem access of its own. |
| Memory limits for the workbook reader are time-based on Windows | The separate process is killed after 30 s; a hard memory limit needs a platform job object. |
| Windows files inherit the profile's permissions | No explicit ACL is set; the profile folder is owner-only by default. |
| The assistant's requests are logged for what is sent to a model, not for update and price lookups | The threat model lists every request type. |

## Not reviewed

- The Windows build and installer have not been exercised on real machines.
- Third-party code (Tauri, SQLCipher, the PDF reader) was audited only through
  advisories, not read.
- No external penetration test has been done.

## Reporting a problem

See [SECURITY.md](../SECURITY.md). Please do not put statements or balances in
a public issue.
