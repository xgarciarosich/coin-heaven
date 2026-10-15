# 15. Net worth lines are derived where they can be; the portfolio is dated facts

**Status:** accepted

## Decision

**A snapshot line can say where it came from** — `manual`, `account`, `loan`
or `holding` — and every non-manual line is recomputed, never edited. The
snapshot editor shows automatic lines read-only and only ever replaces the
hand-typed ones. `snapshot_sync(date)` rebuilds a date's automatic lines and is
idempotent; it runs when a new snapshot is saved, when prices are recorded, and
from an Update button.

- **Accounts** carry one known balance ("X on date D", read off the bank app)
  forward and back through the ledger. An anchor rather than an opening
  balance, because nobody knows what an account held the day it was opened.
- **Loans** are stored by their terms; the outstanding principal on any date
  comes from the same French-annuity code the simulator uses.
- **Holdings** are buys, sells and unit reconciliations, plus dated prices.
  Value is units × price; money put in is the sum of buys less sells.

**Recurring buys write real transactions.** A schedule (every N days, or a day
of the month, clamped to short months) materialises its buys idempotently up
to today. A skipped buy is marked, not deleted, or the schedule would write it
again. A fixed-amount buy does not say how many units it bought, so its units
are unknown; one reconciliation ("I held exactly this many on this date")
makes the count exact again from that point on, and until then the count is
reported as a floor rather than presented as the truth.

**Returns are money-weighted (XIRR).** With half-yearly prices a time-weighted
return would need valuations that do not exist. XIRR is suppressed under three
months of history, where an annualised figure is noise.

**Prices can be looked up by the assistant, and are checked before they are
saved.** Only asset names, symbols and the date are sent. The request uses the
provider's own web search (Claude `web_search_20260209` on current models,
Gemini Google Search, OpenAI search models); providers without search are
refused rather than asked to recall a price. Every proposal carries the page it
came from, and that page is checked against the pages the search actually
returned: a source the search never touched starts unticked with a warning.
Prices are stored on the date asked about, even when the close is the previous
trading day's, because it is that date's snapshot they are for.

**The FIRE page grosses withdrawals up for Spanish savings tax**, with the gain
share taken from the portfolio when there is one, and can model a state
pension as a bridge. **Privacy mode** masks amounts at the formatter, the one
place money becomes text.

## Consequences

- The Anthropic request no longer sends `temperature`, which current models
  reject.
- Year-end checkpoints stay on 31 December — the same moment as 1 January, and
  a day with a closing price.
- A price the assistant finds is still a claim; the design makes it cheap to
  check, not unnecessary.
