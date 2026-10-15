# 12. Budgets are plans with a pace, and categorising is a bulk decision

**Status:** accepted

## Context

Module 2 had a transaction list and a set of charts. Both broke down at real
volume. A year of statements is a few thousand rows, and a flat list of those
gets scrolled past rather than read; categorising them meant one decision per
row, which nobody does twice. Neither the list nor the charts could answer the
question a household spreadsheet is usually built to answer — what did each
category cost, month by month, against what I meant to spend.

## Decision

**A budget is one category, one period, one positive amount.** The period is
either a year (`2026`) or a month (`2026-03`), stored as the text prefix of
`booked_on`, so "what happened against this plan?" is a `substr()` comparison
and not calendar arithmetic. The amount is a magnitude regardless of the
category's kind: a budget that changed meaning when its category was edited
from expense to income would be a trap.

**Every budget shows pace, not just a total.** Three quarters of the grocery
budget spent is unremarkable in October and alarming in April, so each line
carries the elapsed share of its period alongside the spent share, and the bar
marks today. Reading the fill without the marker is what makes progress bars
useless.

**Categorising is done per merchant, not per transaction.** Each description is
reduced to one token — the longest word that is not statement boilerplate — and
everything sharing that token is one decision. The key is a literal substring of
the description by construction, so the optional rule written from it (`contains
<key>`) is guaranteed to match everything the key grouped.

Three things can propose a category, in descending order of trust: a rule the
user wrote; what the user themselves filed the same merchant under before; or
nothing, in which case the group is still shown so it can be done by hand. The
assistant is deliberately not among them — it runs separately behind its own
consent gate, on whatever this local, free, instant pass could not clear.

A rule only speaks for a group if it matches *every* row in it. A rule covering
half a group would categorise the other half by accident the moment the group
was accepted, which is exactly what a preview exists to prevent.

## Consequences

* One new table (`budget`) and one new derived read (`pl_matrix`).
* Merchant grouping is a heuristic, and the noise-word list is Spanish and
  English only. It is always previewed and never applied on import.
* Averages in the matrix divide by months that have data rather than by twelve,
  so a half-imported year does not read as half as expensive.
* Accepting a group writes `categorized_by = 'rule'`, so a later pass can tell
  these apart from decisions made one row at a time.
