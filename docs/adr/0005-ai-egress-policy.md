# 5. AI egress is opt-in, aggregated, previewed and logged

**Status:** accepted · strengthens the spec's PII-masking approach

## Context
The spec relies on a regex PII filter before sending data to LLM providers. That
catches structured identifiers but not names — and merchant strings are
themselves disclosive. "Clínica Dermatológica X" is health data under GDPR and
no masker will strip it.

## Decision
Policy first, redaction second:

1. `privacy.ai.enabled` defaults to `false`. The app is fully usable without it.
2. Aggregates by default — monthly category totals and schema, never raw rows.
3. Raw descriptions require a second flag **and** a per-request preview the user
   approves.
4. Every payload is hashed and previewed into `ai_egress_log` before sending.
5. `security/pii.rs` redacts IBANs, cards, emails and phones as a second layer.
6. A deterministic rule engine runs first, so the AI only sees what rules could
   not classify.

## Where API keys live

Superseding the original "keys go in the OS keychain": there is no keychain
here any more ([0003](0003-key-storage-and-recovery.md)). Provider keys will be
stored **in the encrypted database itself**.

That is consistent rather than weaker. The database is AES-256 encrypted, and
when the user has set a passphrase it is protected by a secret that exists
nowhere on disk — better than a keychain item unlocked at login. When they have
not, an API key is exactly as exposed as the financial data beside it, which is
the honest position: one protection boundary, not two that differ.

The `ai_provider_config.keychain_ref` column is now misnamed. It is left in
place because nothing writes to it yet; Module 5 should rename it in a
migration when it lands.

## Consequences
- "What has this app ever sent?" is answerable from the database.
- The masker's limits are asserted, not assumed: `documents_its_own_blind_spot`
  proves a personal name survives redaction, so nobody mistakes it for complete.
- A local provider (Ollama) is the only configuration fully consistent with the
  app's privacy claims, and is a supported provider value.
