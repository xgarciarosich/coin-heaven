# 20. A monthly plan that is a worksheet

**Status:** accepted.

## Decision

Cash flow gains a **Monthly plan** tab: one list of boxes per month, each an
income or a spending amount with any name, in whatever order the person
drags them into, and under them what is left. Each month that has a plan is
kept, and the tab lists them so a plan from two years ago opens exactly as it
was left.

- **Not a budget.** A budget is a figure per category that Coin Heaven measures
  transactions against. A plan is the sum written before the month; it has
  no categories, no link to the ledger and nothing else has to agree with it.
  Keeping them apart is what keeps the plan this simple.
- **Order is data.** Boxes are stored with their position, and each shows
  what is left after it, so arranging them is arranging how the month is
  spent — which is the reason order matters at all.
- **Whole-month writes.** A month is saved and read as one list (`plan_save`
  replaces, `plan_list` returns every month). The list is a few dozen rows,
  and replacing keeps what is stored identical to what is on screen, order
  included.
- **No Save button.** Edits are written 0.7 s after typing stops, a drag or a
  delete at once, and leaving the month flushes. Same rule as the balance
  sheet.
- **A new month starts from the nearest earlier plan**, or the nearest later
  one when filling in the past, and from nothing on request.
- **Reordering uses pointer events**, not the browser's drag-and-drop, which
  some platforms' windows intercept and which a keyboard cannot drive. The
  handle also takes the arrow keys.

Schema: migration 0015 (`plan_month`, `plan_item`).

## Consequences

- The plan is not read by FIRE or the assistant. If it should be (a planned
  surplus as the savings figure, say), that is a separate decision about
  which of two figures wins.
- Amounts are shown with cents only when they have some, so a rent of 900 is
  "€900" and a subscription of 45,50 is not rounded to 46. The same field now
  serves the net worth sheet.
