# 3. Recovery phrase is the source of truth; no OS keychain

**Status:** accepted · revised twice — keychain-only, then envelope + keychain,
now envelope + optional passphrase with no keychain at all

## Context

The first design stored a random SQLCipher key in the OS keychain. Losing that
entry destroyed the database, and the key could only ever have one holder.

Two problems surfaced in use. The severe one was unrecoverability. The visible
one was that the keychain made the app demand a system password on launch:
macOS treats each build of an unsigned binary as a *different application*, so
every rebuild fell outside the item's access list and re-prompted. The prompt
asks for the keychain password, which is not always the account password — on a
machine whose password had been reset, it was simply rejected, with no way
forward.

## Decision

**Envelope encryption.** The database is encrypted with a random Data
Encryption Key that is derived from nothing:

```text
DEK (32 random bytes) ── encrypts the database
 ├── wrapped by Argon2id(recovery phrase)   always
 └── wrapped by Argon2id(passphrase)        if the user wants one
```

Setting, changing or removing a passphrase rewraps 32 bytes; the database is
never re-encrypted. Forgetting the passphrase is survivable, because the
recovery phrase unwraps the same DEK.

**No OS keychain.** When the user declines a passphrase the DEK is written to
`atlas.localkey` beside the database, mode `0600`. This is a deliberate
simplification for a local-only v1: the protection becomes the account password
and full-disk encryption, which is what protects everything else in the home
folder anyway.

`atlas.localkey` is a *separate file* from `atlas.keyinfo`. The sidecar is
copied next to every backup because the recovery phrase needs its salt; the raw
key must never travel, or each backup folder would decrypt itself. Asserted by
`a_backup_never_carries_the_key_that_decrypts_it`.

## Consequences

- No system password prompt, ever, on any platform.
- The "open automatically" choice is honestly weaker than keychain storage was:
  the key sits beside the data. **Without FileVault it offers little against a
  stolen machine**, which the setup screen says in as many words.
- Choosing a passphrase is now strictly stronger than the old keychain design:
  no key exists on disk at all.
- Key loss is recoverable, which was the original blocking gap.
- BYOK API keys have nowhere to go — see [0005](0005-ai-egress-policy.md).
