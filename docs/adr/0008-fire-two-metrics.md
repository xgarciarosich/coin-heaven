# 8. FIRE reports two metrics and never blends them

**Status:** accepted · implements decision 2.6

## Context
The spec computed real-estate equity and *also* subtracted the rent that equity
produces from the spending target — counting one asset twice.

## Decision
Two figures, never added together:

1. **Liquid FIRE target** = `(annual_spend − net_rental_income) / SWR`,
   compared strictly against liquid assets (cash, equities, crypto, pensions).
   Real-estate equity is excluded from the assets side because its contribution
   is already counted as the rental income on the target side.
2. **Total wealth milestone** = net worth including real-estate equity. Context,
   not a finish line.

The SWR is treated as a **real** rate, so both the spending target and the
rental income must be in today's money. `fire::to_todays_money` deflates a
nominal projection figure before it meets the target.

## Consequences
- `real_estate_equity_is_never_added_to_the_liquid_side` asserts that a large
  property portfolio moves neither the liquid target nor liquid progress.
- `using_an_undeflated_rent_would_understate_the_target` guards the real-rate
  contract, which is the most common error in FIRE calculators.
- Pensions count as liquid, per the decision. The UI should still note that
  pension capital is usually locked until a statutory age — a liquid FIRE number
  leaning on it is not the same as being able to stop working today.
