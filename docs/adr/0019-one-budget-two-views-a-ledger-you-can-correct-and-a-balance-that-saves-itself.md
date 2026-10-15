# 19. One budget in two views, a ledger you can correct, a balance that saves itself

**Status:** accepted. Supersedes the "amounts are not editable" rule of the
ledger and the split-into-months action of ADR 12.

## Decision

**A budget is one plan, read by the year or by the month.** The table still
holds a figure per category and period. What changed is how they relate: a
year's figure spreads evenly over the months that have none of their own; a
month with its own figure keeps it and the others share what the year has
left, to the cent; with no figure for the year, the year is what its months
add up to. `budget_plan` returns every budgeted category as its year plus its
twelve months, each marked as written or derived, and the screen switches
between the two without changing a number. Writing "the same every month"
drops the year's figure; writing a year over twelve written months replaces
them. Splitting a year into twelve stored rows is gone — the months are
derived, so there is nothing to keep in step.

**Every field of a transaction can be corrected, and a transaction can be
deleted.** Amount and account join date, description, category, event, notes
and the transfer flag. The dedupe hash is left as imported, so loading the
same statement again still recognises a corrected row and leaves it alone. A
deleted row's hash goes to `txn_removed` (migration 0014), which the importer
checks, so an overlapping statement does not bring it back; moving a row to
another account records the same for the account it left. Anything linked to
a deleted row is unlinked first.

**The balance sheet has no Save button.** A new date is recorded the moment
it is asked for, as a copy of the one before it. After that an amount is
written 700 ms after typing stops and a name when its field is left; leaving
the sheet flushes what is pending. A rename is applied to every date, as
before, except when another item of the same class already has the new name —
then the history is left alone rather than merging two items irreversibly.

**The sheet reads as what you own, then what you owe.** Each class is a line
with last time, now and the change, and opens to the items inside it; each
group has its total and the footer shows the subtraction. The item-by-item
chart is the same shape: classes first, each opening to a chart of its items.

**A region file is written in the units a person uses.** Percent and whole
currency instead of basis points and cents; only identity fields are
required; `regionSchema.ts` validates and converts, `pnpm check:regions`
reports problems in plain words, and a file that cannot be used is listed in
Settings with the reason. The FIRE tax table gains a country picker fed by
those files. Which country is selected is not stored: it is whichever one the
plan's rates still equal.

## Why

Year-or-month was a choice the old screen forced once per category and then
made awkward to revisit. People who think in months and people who think in
years were both half-served, and a split was a one-way conversion.

An amount that cannot be corrected protects the statement at the expense of
the ledger's owner; the hash already protects against the real risk, which is
double-counting on re-import.

## Consequences

- Deleting is permanent and the tombstone holds only a hash, so a deleted row
  cannot be restored except from a backup.
- Autosave means there is no "discard changes" on a balance. Deleting the
  date is the way back from a new one started by mistake.
- Region files written for the previous format (basis points, cents) are
  rejected with messages saying what each field should be.
