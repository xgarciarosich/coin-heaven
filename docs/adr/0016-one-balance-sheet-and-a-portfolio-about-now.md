# 16. One balance sheet per date; a portfolio that is about now

**Status:** accepted. Supersedes the account and loan parts of ADR 15.

## Decision

**Net worth is a balance sheet you correct, not a snapshot you compose.** The
screen shows one date at a time as a sheet grouped by asset class: the items,
what each was last time, what it is now, and the move. A new date starts as a
copy of the last one, because between two dates most things have not changed.
Dates are picked from a strip of cards that each carry the total and its
change, which replaces the separate "movement" and "snapshots" lists. The word
"snapshot" is gone from the interface.

**Only the portfolio writes lines on its own.** Accounts and loans no longer
derive lines (migration 0012 turns the ones already written into ordinary
editable lines, so no past total changes). A mortgage is typed against its
property like any other figure. The tables are left in place, unused.

**Portfolio lines are never stale.** Anything that changes what a holding was
worth — a movement, a recurring buy, a price — re-derives the portfolio lines
of every recorded date (`resync_all`). There is no "update" button. Saving a
price does not create a date; it only revalues dates on or after it.

**The portfolio is valued as of today.** The price check asks for the current
price, not a half-year close, and the per-date selector is gone. The past
lives in a chart: value against money put in, at each month end and on every
date a price was recorded.

**Units nobody typed are worked out.** A recurring buy knows its cost, not its
units. `fill_units` shares a later reconciliation over the blind buys before
it in proportion to cost, and prices any buy no reconciliation covers at the
nearest price on or before it (else the first after). The count is then
marked ≈ rather than "at least", and reads at cost rather than at zero.

**Categories and rules live in Cash flow**, laid out like asset classes.

**Independence and the simulator lead with the answer.** Independence opens
with a sentence, a progress bar with its milestones, and three controls in
monthly euros; tax, pension, rent and withdrawal rates are in sections that
state their current setting while shut. The plan saves itself. The simulator
orders its controls as a person thinks — me, the property, the mortgage, the
market — folds away the notary's and the bank's numbers, and adds a strip
showing the year of each purchase and, where one was refused, why.

## Why

Feedback from use: the snapshot editor was not understandable, the accounts
and loans screen was a second place to keep numbers the balance already asks
for, and a portfolio pinned to 30 June answered a question nobody was asking.

## Consequences

- A holding with one price and no unit counts reads exactly at cost: neutral,
  and wrong by whatever the market did. One reconciliation fixes it.
- History before a holding's first price is carried at cost.
- Editing a holding changes the portfolio lines of past dates. They are
  derived data; the hand-typed lines beside them are never touched.
