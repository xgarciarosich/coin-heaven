# 13. Asset classes belong to the user; independence gets its own module

**Status:** accepted · supersedes the fixed bucket list in ADR 2

## Context

Three things were wrong with the shape of the app rather than with any of its
arithmetic.

**Importing was a destination.** It sat in the sidebar as a peer of Cash flow,
one click away from the only screens that show what importing did. Nobody
opens an importer to look at an importer.

**The eight asset buckets were a guess at one person's balance sheet.** Cash,
equities, crypto, pension, real estate, other asset, mortgage debt, other debt.
Someone with a business stake, a classic car, a stock-option grant or a foreign
pension had nowhere to put it but "Other asset", which records the number and
loses the meaning.

**FIRE was a panel.** It answered "how much do I need" in a box below the
net-worth chart, and never answered the question people actually have, which is
"when".

## Decision

**Import is the first tab of Cash flow.** The `import` module toggle now
governs that tab rather than a sidebar entry, and the Cash flow entry stays
visible while *either* it or `ledger` is on — turning the importer off should
not take the ledger with it. A finished import refreshes the ledger behind it.

**Asset classes are rows the user owns.** A class has a name they choose and a
`kind` they pick from a fixed four: liquid, property, other asset, liability.
The name is free because it is a label; the kind is not, because net worth, the
liquid FIRE target and property equity are all *defined* in terms of it. A user
can call a class anything. They cannot invent a fifth way for it to behave.

The key is a slug stored on the snapshot line, so renaming is a display change
and never rewrites history. Deleting a class in use requires saying where its
lines go, the same contract the category manager keeps.

This also settled a real inconsistency. `v_net_worth` counted cash, equities
and crypto as liquid; `fire_latest_balances` counted those *plus* pensions. Two
definitions of one word in one app is a discrepancy waiting to be reported. Both
now read the class kind, and anyone who does not consider a pension spendable
before 67 has somewhere to say so.

**Independence is its own module**, built around a date rather than a number:

* the target, what you have, and **when** — years, and the age you would be;
* the path, plotted to a few years past the crossing rather than to the end of
  the horizon, because forty years of compounding runs to several times the
  target and flattens the only line the chart exists to show a crossing of;
* the **coasting point** — the capital that reaches the target on its own with
  nothing further saved. Usually passed long before the target, and passing it
  changes what the job is for;
* a sensitivity table over the withdrawal rate, which is the one assumption on
  the page that cannot be checked against anything;
* what rent is doing, and why property equity is therefore not on the assets
  side.

Everything is in today's money at a **real** rate of return. That is a
contract, not a simplification: a 4% safe withdrawal rate is already
inflation-adjusted, so pairing it with a nominal return would quietly shorten
every answer by years. The target line is flat for the same reason — a rising
one would be inflation counted twice.

## Consequences

* One new table (`asset_class`), `wealth_line` rebuilt with a foreign key to
  it, and `v_net_worth` redefined in terms of `kind`.
* `liquidMinor` now includes pensions where it did not before, so an existing
  snapshot's "liquid" figure changes. It moves toward agreeing with the FIRE
  panel, which is the point.
* Contributions are modelled at year end, the conservative reading.
* `years_to_target` interpolates linearly inside the crossing year, so it is
  approximate to within a couple of months.
* The `fire` toggle defaults on; assumptions persist in `app_setting`.
