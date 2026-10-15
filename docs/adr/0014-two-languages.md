# 14. English and Spanish, with completeness enforced by the build

**Status:** accepted

## Decision

**Interface text lives in typed dictionaries.** `src/locales/en` is the source
of truth; `src/locales/es` is typed as its shape, so a phrase missing in
Spanish is a compile error rather than a blank label. Phrases that carry a
value are functions, which is also where plurals are decided — per language,
because Spanish and English do not agree on them.

**Backend text is written in English where it is raised and translated on the
way out.** Errors go through one catalogue at `AppError`'s serialisation;
sentences assembled from fragments (the matcher's reasons, the importer's
notes) choose inline with `i18n::pick`. A test scans the Rust source for every
error message and fails if one has no Spanish. It found two real problems on
its first run, one of them a user-visible message carrying 23 stray spaces.

**Starter names are data, and follow the language only while untouched.**
Switching language renames seeded categories and asset classes that still
carry their original name in the other language. Anything the user renamed is
never touched; a rename that would collide with an existing name is skipped.

**Numbers use the Spanish convention in both languages**, as they already did
for money: decimals are commas. "10.6 años" beside "€184.500" read as two
countries on one screen.

## Consequences

* The first launch writes the system language as the choice; it is visible and
  changeable in Settings. The last choice is cached outside the database so the
  unlock screen can use it before anything is decrypted.
* AI prompts stay in English, which models follow most reliably, with an
  instruction to reply in Spanish when that is the interface language.
* Descriptions, category names the user wrote, and account names are never
  translated.
