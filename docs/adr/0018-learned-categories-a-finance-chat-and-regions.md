# 18. Categories that are learned, an assistant that is a chat, and regions as files

**Status:** accepted.

## Decision

**A category set by hand is a decision about the merchant.** After every
import and every manual categorisation, uncategorised transactions from a
merchant the person has filed before take that category, provided at least
80% of their past decisions for it agree. Nothing set is ever changed, and a
merchant filed two ways is left for a person.

**The model is asked like a bookkeeper would be briefed.** Auto-categorise
sends one line per *merchant* (key word, a masked example, direction, a
rounded typical amount), the category list with each category's kind, and a
sample of the household's own past filing, under standing instructions that
say what each kind means, to follow the household's habits, and to answer
`null` rather than guess. Suggestions land in the same reviewable list as
local ones. The assistant's separate "categorise what needs review" is
removed.

**The assistant is a chat with a fixed remit.** It answers questions about
personal finance and the person's own figures and declines everything else.
Each turn carries aggregates only — totals by month and category, net worth by
class, the portfolio by holding, the FIRE plan — chosen by scope toggles, and
what the person types is masked for bank identifiers. The system prompt
restricts scope, forbids inventing figures, disclaims advice and treats the
context as data. Provider, privacy and the egress log move to Settings → AI.

**A price lookup also asks for the past.** Holdings with buys recorded by cost
alone are asked for month-end closes (up to 18, thinned evenly), so each buy
can be valued at the price nearest its date.

**A model that cannot search can be given a search engine.** With a SearXNG
address configured, Coin Heaven runs the queries there and gives the results to the
model to read. Both the queries and what the model is sent are logged.

**Ollama is addressed natively, with a bounded context window.** The
OpenAI-compatible route cannot set `num_ctx`; a model whose default is a
quarter of a million tokens took 18 GB and stalled a 24 GB machine during
testing. Requests ask for 16k and for no extended reasoning, retrying without
that switch for models that lack it.

**PDF statements are read in a child process.** The app re-launches itself
with `--pdf-text`; a crash or hang in the parser cannot touch the app or the
database. Lines that start with a date and end in an amount become rows for
the ordinary importer, which still previews before storing.

**A country is a file.** `regions/<id>.json` holds locale, currency, symbol
placement, tax brackets and plan defaults; the frontend finds every file at
build time and Settings lists them. Formatting reads the active region.

## Consequences

- Rust `Default` impls still carry Spanish mid-case numbers as a last resort;
  the region file overrides them when a plan is new.
- PDF layouts vary without limit. The reader handles one-line-per-transaction
  statements with text in them; scans and multi-column layouts will not parse,
  and say so.
- The chat's guardrails are instructions, not enforcement. A determined
  person can talk a model out of them; the only thing that can leak is the
  aggregate context the person themselves chose to send.
