# 21. Tabs by job, and set-up beside its use

**Status:** accepted. Amends the tab layout of ADRs 12 and 20.

## Decision

A tab is a place someone goes to do a job; setting something up is not a job
they go to, it is something they do while doing another. So:

- **Cash flow** has four tabs: **Overview**, **Monthly plan**,
  **Transaction manager**, **Import**.
  - *Overview* is the analysis of a year or a month, with a switch between
    **What happened** (the category-by-month view) and **Against budget**
    (budgets, paced against the calendar). They share one year selector
    because they are two readings of the same year.
  - *Monthly plan* is a tab of its own. It is the quick sheet for "what is
    left this month", written before the month; budgets are the slower
    analysis of what actually happened against a target. Different goals,
    different places — not one screen with a switch.
  - *Categories & rules* and *New event* moved into the Transaction manager,
    where categories are applied and events are filled; the manager opens as
    a panel above the list.
- **Net worth** has two tabs: **Balance** and **Portfolio**. *Asset classes*
  opens from the Balance header, because classes are chosen while writing
  items down.
- The "Module n" label above page titles is gone: it was the app's own
  numbering, not something a user could use.
- The filter panel offers only the year on the Overview. The category,
  event, account and search filters acted on the transaction list alone and
  were being shown over screens that ignore them.

## Not decided here

The monthly plan and budgets can still disagree (a plan of 350 for food, a
budget of 600). Linking a plan box to a category, so one figure serves both,
is a separate change.
