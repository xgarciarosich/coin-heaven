-- BYOK API keys move into the encrypted database.
--
-- The original design put them in the OS keychain. There is no keychain any
-- more (docs/adr/0003), and storing them here is consistent rather than weaker:
-- the database is AES-256 encrypted, and when the user has set a passphrase it
-- is protected by a secret that exists nowhere on disk. One protection
-- boundary instead of two that differ.
ALTER TABLE ai_provider_config ADD COLUMN api_key TEXT;

-- keychain_ref is now dead. SQLite cannot drop a column with a CHECK on it in
-- older versions, so it is left in place and simply unused.
UPDATE ai_provider_config SET keychain_ref = NULL;
