# Simulator maths

Every formula the engine uses, and where it departs from the original spec.
Source: [`src-tauri/src/sim/`](../src-tauri/src/sim/). All rates are basis
points; all money is integer cents.

## Time step

Monthly, over `horizonYears × 12` periods. Monthly rather than annual because
purchases are gated on a cash balance that crosses its threshold mid-year, and
an annual step would systematically delay every purchase by up to eleven months.

## Cash required per purchase

```
cash_required = price × (1 − financing)      deposit
              + price × transfer_tax          ITP
              + price × other_closing         notary + registry + gestoría + AJD
              + renovation
```

The spec omitted `other_closing`. On the €120k default that is €1,800 per unit.

Default scenario: `24,000 + 12,000 + 1,800 + 15,000 = €52,800`, asserted by
`upfront_cash_matches_the_documented_formula`.

## Mortgage

French system, constant annuity:

```
        i
P = L ─────────────         i = annual_rate / 12,  n = term_months
      1 − (1+i)^(−n)
```

Zero rate falls back to `L / n`. The final instalment absorbs rounding residue
so the balance lands on exactly zero — verified for four rate/term combinations
by `amortisation_fully_repays_the_principal_and_lands_on_zero`.

Reference points, computed independently:

| Loan | Rate | Term | Payment |
|---|---|---|---|
| €200,000 | 2.80% | 30y | €821.79 |
| €96,000 | 2.80% | 25y | €445.32 |

**Term is a slider.** The spec listed a rate but no term, despite term moving
monthly cash flow ~12% between 25 and 30 years — which compounds into how fast
the next deposit is reached.

**First payment falls the month after completion.** A unit bought in month *m*
makes its first instalment in month *m+1*, as in reality. Over a 120-month run a
month-1 purchase therefore sees 119 periods, asserted by
`reporting_does_not_advance_the_simulation`.

## Rent and value: indexed, not compounded

Each holding stores an immutable `base_value` and `base_monthly_rent` fixed at
purchase. Current figures are derived:

```
value(m) = base_value        × (1 + appreciation) ^ (years_held)
rent(m)  = base_monthly_rent × (1 + rent_cpi)     ^ (years_held)
```

Deriving from the base rather than multiplying a rounded integer month over
month is what keeps 120 iterations free of accumulated rounding drift. Verified
by `a_zero_appreciation_zero_cpi_run_holds_value_flat`, which requires the value
to be *exactly* equal across all ten years.

## Monthly net rent

```
collected = rent × occupancy
opex      = collected × (mgmt_fee + maintenance) + fixed_costs_annual / 12
NOI       = collected − opex
taxable   = max(0, NOI − mortgage_interest)        principal is not deductible
tax       = taxable × rental_tax
net_rent  = NOI − mortgage_payment − tax
```

Departures from the spec, which had
`Net Rent = (Gross × Occupancy) − Mortgage − Taxes`:

- **`gross_yield`, not "net yield".** The spec's own formula derives *gross*
  rent from it. Calling it net overstates income by roughly a third.
- **Management, maintenance/capex and fixed costs** (IBI, insurance, community)
  were missing. On a €120k Spanish unit they run €150–250/month.
- **Occupancy is applied once.** Occupancy and vacancy are the same adjustment.
- **Interest is deducted before tax, principal is not** — matching Spanish IRPF
  treatment. `rental_tax_bps` defaults to 19%, an effective rate standing in for
  the long-term-residential reduction. Set it to your own marginal reality.

## Affordability (the "CIRBE check")

```
computable_income = net_salary + gross_rent × computable_rent
debt_service      = existing_debt + Σ active mortgage payments
DTI               = debt_service / computable_income
ok                = DTI ≤ dti_max
```

Renamed from the spec's "Max CIRBE Limit %". CIRBE is the Bank of Spain credit
register — the lender's *data source*, not a percentage. The threshold is the
bank's debt-to-income rule, typically ~35%, and `computable_rent` is the haircut
lenders apply to rental income (70–80%).

The test is applied **prospectively**: before buying, the engine adds the
hypothetical mortgage *and* the new unit's rent, then re-checks — which is how a
lender assesses the deal in front of them.

## Purchase rule

The person plans each purchase: a month, a price, a renovation, a starting
rent, the transfer tax of its region, and optionally its own mortgage terms
(anything left blank takes the general ones). The engine never chooses a
purchase. Each month, after savings and rent have arrived, it takes the
planned purchases that are due, in order of their planned month, and for each:

1. **Cash**: `cash − buffer ≥ deposit + transfer tax + other closing + renovation`.
2. **Bank**: debt service including the new mortgage, over salary plus the
   computable share of rent including the new unit's, is within the limit.

If both hold it is bought that month. If either fails, the purchase waits and
is tried again every month after; the planned month's shortfall and
debt-to-income figure are reported, with which of the two stood in the way.
A purchase that is waiting holds the ones planned after it — a plan is a
sequence. One that never passes within the horizon is reported as not bought.

Until version 0.2 the engine bought automatically whenever cash allowed, with
per-year overrides. That answered "how many could I own?", which is not what
anyone plans a purchase around; scenarios saved by it still load, with an
empty plan.

## Properties you already own

A property bought before the plan is held from the first month. Nothing is paid
for it and the bank's test is not run, because it already happened.

- Its value starts at what it is worth **today** and grows with the same
  appreciation index as everything else, from month 0.
- Its rent starts at **today's** rent and is indexed the same way.
- Its loan is the amount **still owed**, repaid over the **months left** at its
  own rate (or the general rate if none is given), with the same annuity
  formula as a new mortgage. A paid-off property has no loan.
- Its loan payment counts in the affordability test for every later purchase,
  as a lender would see it.
- What was paid for it and when are recorded for the "gain since you bought"
  figure only; they do not change the projection.

## Income

Take-home pay and the monthly amount put aside both rise once a year by
`incomeGrowthBps`, on the plan's anniversary. Net rent is added to cash each
month when `reinvestNetRent` is on, which is how rent increases what can be
saved toward the next purchase.

## Net worth

```
net_worth = cash + Σ value(m) − Σ loan_balance
```

Reconciliation is asserted every year by
`equity_always_reconciles_to_value_minus_debt`.

## Known limitations — state these in the UI

- **Net worth dips in a purchase year.** Transfer tax and closing costs are sunk
  immediately while the asset is carried at cost. This is correct, not a bug,
  and it means net worth is *not* monotonic in savings rate — more savings can
  mean an earlier purchase and a lower figure at a given checkpoint.
- **No rate risk.** Every mortgage is fixed. Variable and mixed loans exist in
  the `mortgage` table but the engine ignores the distinction.
- **No sale, refinance or equity release.** Units are bought and held.
- **No vacancy clustering.** Occupancy is a smooth haircut, not a stochastic gap;
  real vacancies arrive in lumpy months and hurt more than the average suggests.
- **Appreciation is an assumption, not a forecast.** Nothing in this model
  predicts the housing market.
- **Nominal throughout.** Rents and values inflate; the savings contribution
  does not. If you compare against a real (inflation-adjusted) FIRE target, that
  mismatch matters.
