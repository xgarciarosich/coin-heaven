# 1. Money is an integer count of minor units

**Status:** accepted · supersedes the spec's "double-precision floats"

## Context
The specification standardised monetary values on double-precision floats. This
app compounds arithmetic aggressively: a 120-month simulation, mortgage
amortisation schedules, and P&L sums over thousands of rows.

## Decision
`Money(i64)` in minor units (cents). Serialised as a bare integer, so no float
crosses IPC. Rates are basis points. `f64` appears only inside annuity and
compounding factors and converts back immediately with explicit
half-away-from-zero rounding.

## Consequences
- Exact addition; `0.1 + 0.2 == 0.3` holds by construction.
- Overflow guarded: `mul_bps` widens to `i128` internally.
- `split_evenly` conserves every cent, which split bills need.
- Cost: no fractional cents. Correct for accounting; if a future feature needs
  sub-cent unit prices (crypto), that is a separate quantity type, not this one.
