<p align="center"><img src="branding/logo-full.png" width="128" alt="Coin Heaven logo"></p>

<h1 align="center">Coin Heaven</h1>

<p align="center"><strong>Private personal finance on your own computer.</strong></p>

Coin Heaven is a desktop app for people who keep their money in a spreadsheet
and would like something better without handing their bank statements to a
cloud service. You import your statements, see where the money goes, track what
you own and owe, plan when work could become optional, and try out a
buy-to-let plan before you sign anything.

Everything is stored in an encrypted file on your machine. There is no account,
no sync server and no analytics. It works with the network switched off.

> **Not financial advice.** The projections are arithmetic on the assumptions
> you type in. They are a way to think, not a forecast.

## Why I built it

Most finance apps want a login, a bank connection and a subscription, and they
keep your data. Spreadsheets keep your data private but fall apart after a year.
Coin Heaven is the middle path: a real app with your numbers kept on your own
computer, in plain files you can export whenever you like, and with the source
open so you can see exactly what it does.

## What you can do with it

- **Cash flow.** Drop in a `.csv`, `.xlsx` or PDF statement from your bank.
  Coin Heaven works out the layout by reading it, so there is nothing to
  configure. Importing an overlapping statement only adds what is new.
  Categorise a merchant once and it remembers. Budgets, recurring payments and
  a month-by-category view sit next to your transactions.
- **Monthly plan.** The spreadsheet most people already keep: boxes of income
  and spending in any order, and what is left at the bottom.
- **Net worth.** A balance sheet per date, a portfolio with recurring buys, and
  a return that accounts for when you put money in. Buys known only by their
  amount can be priced on their own day, in one click.
- **Independence (FIRE).** When work becomes optional, in a sentence: tax on
  withdrawals, a pension bridge, and what would bring the date forward.
- **Property simulator.** Each property with its own price, rent, purchase tax
  and mortgage, checked against your cash and what a bank would lend.
- **An assistant, if you want one.** It can answer questions about your own
  finances using a model on your own computer (through
  [Ollama](https://ollama.com)). It is off until you turn it on, it is only
  sent totals, never transactions, and every request it makes can be read.

Screens are available in English and Spanish, and the tax and currency rules
for a country live in one small file you can copy and edit
([see below](#make-it-yours)).

## Get it running

New to this? Follow the step-by-step guide:
**[docs/GETTING-STARTED.md](docs/GETTING-STARTED.md)**. It covers macOS and
Windows, what the first launch asks you, and how to bring in your first
statement.

The short version, for developers:

```bash
pnpm install
pnpm app:dev      # the real app, Rust backend included
```

You need Node 22+, pnpm, Rust 1.88+ and the platform build tools described in
the guide. The first build compiles SQLCipher and OpenSSL from source and takes
several minutes.

Installers will appear on the [Releases](../../releases) page. They are not
signed with a paid developer certificate, so macOS and Windows will warn you the
first time you open one; the guide explains why and how to proceed safely.

## One thing to do on day one

**Write down the twelve words shown when you first open the app.** They are the
only way back into your data if you forget your passphrase, lose the key file,
or move to a new computer. Nobody, including this project, can reset them. That
is the price of nobody else being able to read your finances.

## What is protected, and what is not

| | |
|---|---|
| Database | SQLCipher, AES-256 |
| Key | random, wrapped by a 12-word recovery phrase and optionally a passphrase (Argon2id) |
| Backups | encrypted copies to a folder you choose, after each import and when you close the app |
| Interface | no direct network or file access from the UI; a strict content policy |
| Network | every request starts in Rust, and the ones that exist are listed below |
| AI | off by default; sees totals, not transactions; every payload is logged |

Coin Heaven makes only these requests, and only when you ask for them: a check
for a new version, web search for prices (through the search engine you pick),
market prices for tickers you hold, and your chosen AI model. The threat model,
including what it does *not* defend against, is in
[docs/THREAT-MODEL.md](docs/THREAT-MODEL.md).

## Make it yours

Nothing about any one country is in the code. Currency, number and date format,
tax brackets, retirement defaults and the wording banks put on statements live
in one small file per country in [`regions/`](regions/), written in plain
percentages and amounts. Copy `us.json`, change the values, run
`pnpm check:regions`, and your country shows up in Settings. Languages work the
same way in [`src/locales/`](src/locales/). Details are in
[regions/README.md](regions/README.md). A pull request adding your country is
very welcome.

## Documentation

| | |
|---|---|
| [GETTING-STARTED.md](docs/GETTING-STARTED.md) | install and first steps, for everyone |
| [RECOVERY.md](docs/RECOVERY.md) | moving to a new computer, and what to do if one is stolen |
| [THREAT-MODEL.md](docs/THREAT-MODEL.md) | what is and is not protected |
| [SECURITY-REVIEW.md](docs/SECURITY-REVIEW.md) | what was examined, what was fixed, what was accepted |
| [LOCAL-AI.md](docs/LOCAL-AI.md) | running the assistant fully offline |
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | boundaries and why they are where they are |
| [SIMULATOR-MATH.md](docs/SIMULATOR-MATH.md) | every formula, and its limits |
| [DISTRIBUTION.md](docs/DISTRIBUTION.md) | signing, notarisation, SmartScreen |
| [docs/adr/](docs/adr/) | the decisions behind the design, one short file each |

## Contributing

Open source and meant to be forked. Run `pnpm verify` before a pull request: it
runs the typechecker, the region check, the frontend build, `cargo fmt`,
`clippy` and the Rust tests. Decisions live in [docs/adr/](docs/adr/); if you
change one, add a new note rather than editing history. See
[CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

Built with Tauri v2, Rust, React 19 and TypeScript, with SQLCipher for storage.
Written by Xavier Garcia Rosich with Claude (Anthropic) as a coding partner.

## Licence

MIT. See [LICENSE](LICENSE).
