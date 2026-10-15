# Contributing

Thank you. Coin Heaven is a local-first personal finance app: the data lives in an
encrypted file on the person's own computer, and that shapes what a good
change looks like.

## Run it

```bash
pnpm install
pnpm dev        # the interface alone, with fixture data — fastest for UI work
pnpm app:dev    # the real app, Rust backend included
pnpm verify     # everything CI runs
```

Needs Node 22+, pnpm, Rust 1.88+ and the platform build tools (Xcode command
line tools on macOS, the MSVC build tools on Windows). The first Rust build
compiles SQLCipher and OpenSSL from source and takes several minutes.

## The easiest, most useful contribution: your country

Everything Coin Heaven assumes about *where you live* is one small JSON file in
[`regions/`](regions/): currency, how numbers and dates are written, tax on
investment gains, pension age, property defaults. There is no code to write.

1. Copy `regions/us.json` to `regions/<your country code>.json`.
2. Change the values, in plain percentages and amounts.
3. `pnpm check:regions` says in plain words what is wrong with it.
4. Open a pull request, with where you read the rates (the tax authority's
   page) and the year they are for.

[regions/README.md](regions/README.md) lists every field.

## Translating

The interface is in `src/locales/<language>/`. English is the source; the
compiler fails the build if another language is missing a phrase, so a
translation is complete or it does not build. Backend messages are in
`src-tauri/src/i18n.rs`, and a test fails if one has no translation.

## Ground rules

- **Money is never a float.** It is an integer count of minor units; see
  [ADR 1](docs/adr/0001-money-as-integer-minor-units.md).
- **Nothing leaves the machine unasked.** Every network request starts in
  Rust, is logged, and is shown before it is sent. New outbound calls need a
  strong reason and a place in
  [docs/THREAT-MODEL.md](docs/THREAT-MODEL.md).
- **No country, currency or rate in the code.** It belongs in a region file.
- **Test against the real thing.** Database code is tested against a migrated
  schema, not a mock. A bug fix comes with the test that would have caught it.
- **Say why.** Non-obvious decisions get a short ADR in `docs/adr/`.

## Reporting a bug

Please do **not** attach statements or screenshots with real amounts. A
description of the shape of the problem (the columns, the date format, what
you expected) is enough, and sample data you made up is better.

For security issues, see [SECURITY.md](SECURITY.md).
