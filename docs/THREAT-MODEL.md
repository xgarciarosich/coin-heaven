# Threat model

Written to be honest rather than reassuring. The original spec called the design
"zero-knowledge"; it is not, and an open-source project that overstates its
guarantees invites a reviewer to make the correction publicly.

## What this app protects

There are two modes, and the table says which one each row depends on.
**Passphrase**: the database key is never written to disk. **Open
automatically**: the key sits in `atlas.localkey` (`0600`) beside the database.

| Threat | Passphrase | Open automatically | How |
|---|---|---|---|
| Laptop stolen, disk **not** encrypted | ✅ | ❌ | The database is SQLCipher AES-256 with no readable header, but in automatic mode the key file is next to it |
| Laptop stolen, FileVault/BitLocker **on** | ✅ | ✅ | Full-disk encryption is what protects the key file |
| Backup folder or cloud copy taken on its own | ✅ | ✅ | Backups are ciphertext and never contain `atlas.localkey` |
| A backup of your whole home folder (Time Machine, a cloud drive) | ✅ | ❌ | In automatic mode it captures the data **and** the key file together |
| Another user account on the same machine | ✅ | ✅ | Data folder `0700`, files `0600` on macOS and Linux. On Windows the folder inherits your profile's owner-only permissions |
| A dependency in the interface exfiltrating data | ✅ | ✅ | No HTTP, filesystem or shell capability for the webview; a strict content policy; every request starts in Rust |
| SQL injection from imported content | ✅ | ✅ | Prepared statements only; verified by tests with hostile text |
| A hostile file (CSV, XLSX, PDF) | ✅ | ✅ | Size and shape checks before parsing; PDFs are read in a separate process; fuzz targets in `src-tauri/fuzz` |
| A spreadsheet formula in a bank description | ✅ | ✅ | Exports write such cells as text |
| Leak of transaction detail to an AI service | ⚠️ partial | ⚠️ partial | Off by default; totals first; every payload previewed and logged; masking is a second line of defence |
| Malware running as your user | ❌ | ❌ | Can read the open database and process memory (and, in automatic mode, the key file) |
| A malicious OS or hardware implant | ❌ | ❌ | Out of scope |
| Forgetting the passphrase | ✅ | n/a | The twelve recovery words open the data |
| Losing the words **and** the passphrase **and** every key file | ❌ | ❌ | Nothing can open the database — by design |

## Why it is not zero-knowledge

In automatic mode the key is read from a file at startup without asking you,
so anything running as your user can do the same. In passphrase mode the key
exists only in memory while the app is open. Either way, encryption at rest
defends the files when the app is not running; it does not defend a running
system against code that already has your privileges.

That is an appropriate model for a personal finance app on a machine you
control. It should be described accurately.

## Key loss — addressed

```text
DEK (32 random bytes) ── encrypts the database
 ├── wrapped by Argon2id(recovery phrase)   always
 └── wrapped by Argon2id(passphrase)        if chosen
```

The recovery phrase always unwraps the key, **given `atlas.keyinfo`**: the
twelve words are the secret, but the file holds the salt they are stretched
with. A new machine, a deleted key file, or a forgotten passphrase is a
recoverable inconvenience as long as that file (copied into every backup
folder, once per copy and as the latest) survives.

**There is no OS keychain.** It cost a system password prompt on every launch of
an unsigned build, for protection that an account password and full-disk
encryption already provide on a local-only app. When no passphrase is set the
key is written to `atlas.localkey` (`0600`) beside the database.

`atlas.keyinfo` holds the Argon2 parameters, the salt and a verifier. It is not
secret, but the salt is *required* for derivation — so it is copied next to
every backup automatically. A backup folder without it is only half a backup.

What is still unrecoverable, deliberately: losing the phrase **and** every copy
of the sidecar. There is no vendor reset, because there is no vendor. That is
the trade the design makes in exchange for nobody else being able to read your
finances.

The first-run UI blocks until the phrase has been shown *and* typed back — a
phrase generated but never written down would have left the original risk
exactly where it was.

## AI egress

Off by default (`privacy.ai.enabled = false`). When enabled, the intended
contract:

1. Aggregates by default — monthly category totals and schema, never raw rows.
2. Raw descriptions require `privacy.ai.allow_raw_descriptions` **and** a
   per-request preview the user approves.
3. Every payload is hashed and previewed into `ai_egress_log` before it is sent,
   so "what has this app ever sent?" is answerable.
4. Redaction (`security/pii.rs`) strips IBANs, cards, emails and phone numbers —
   as a second line of defence, not the first.

Redaction's limits are explicit and tested: `documents_its_own_blind_spot`
asserts that a personal name survives masking. Merchant strings are themselves
disclosive — a clinic name reveals health data no regex will catch, which under
GDPR is special-category data. The control that works is the policy, not the
filter.

The configuration that is fully consistent with this app's claims is a local
endpoint (Ollama), which the schema already supports as a provider.

## The other requests Coin Heaven can make

Everything above is about AI. There are three more outbound requests, and
each is the person's choice:

| Request | When | What it carries | What it does not |
| --- | --- | --- | --- |
| **Update check** | Only if "Check for a new version" is ticked (once a day), or when the button is pressed | `Coin Heaven/<version>` as the user agent, to the project's public GitHub release page | No identifier, no setting, no data. It installs nothing; it shows the address. |
| **Web search** for a price | Only when a price lookup is run and a search engine is chosen (what is sent to the model afterwards depends on the model: a hosted one sees the results and the asset names) | The asset's name, plus the person's IP address, to the engine chosen (DuckDuckGo, Exa, or their own SearXNG) | No amount, balance or account. The queries are listed before they are sent. |
| **Market prices** | Only when "Look up the prices" is pressed, after a preview of which holdings and dates | A ticker (or the holding's name, to search) and a date range, to Yahoo Finance's public chart data, plus the person's IP address | No amount, unit count or balance. A price typed by hand is never replaced. |
| **Export** | Only when the button is pressed | Nothing: it writes files to a folder on the same machine | — |

Export deserves a word: the files it writes are **not encrypted**. They are the
person's data, in a form a spreadsheet opens. That is the purpose, and the
interface says so where the button is.

## Notes for implementers

- **The automatic-unlock option depends on FileVault.** With no passphrase, the
  key file is readable by anything that can read the home folder, and by anyone
  who images the disk. FileVault is what makes that acceptable; the setup screen
  says so before the user chooses.
- `PRAGMA cipher_memory_security` is **off**. It has a real performance cost and
  defends only against an attacker who already has your privileges.
- `zeroize` is used everywhere a key is held, but it is best-effort: `keyring`
  returns an owned `String` that Rust may already have moved or copied.
- SQLCipher keeps the key in its own page cache for the life of the connection.
  Zeroizing our copy does not evict its copy.
- The key is a 32-byte CSPRNG value passed as raw hex, so SQLCipher's KDF is
  deliberately bypassed — there is no low-entropy passphrase to stretch. If you
  move to a user passphrase, that changes: use Argon2id.
- Nothing prompts for a system password any more. If you see one, it is not
  Coin Heaven.
