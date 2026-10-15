# 9. Common currencies in v1, with manually entered FX

**Status:** accepted · implements the currency decision

## Context
"Put the most common in v1." The original scaffold assumed EUR-only, and
`Money` divided by 100 to display.

## Decision
A registry of 24 common currencies (`currency.rs`, mirrored in the `currency`
table), each carrying its ISO-4217 **exponent**. Cross-currency conversion uses
`fx_rate` rows the user enters by hand.

## Consequences
- **Not every currency has two decimal places.** JPY and KRW have zero, KWD has
  three. Formatting a JPY balance by dividing by 100 is wrong by two orders of
  magnitude, so the exponent travels with the currency and `Money` stays a bare
  count of minor units. Asserted by
  `one_thousand_yen_is_one_thousand_minor_units_not_one_hundred_thousand`.
- **Rates are entered manually**, stored as integer parts-per-million. An
  offline-first app has no FX feed, and adding one would be the only outbound
  network call the app makes without being asked — which would contradict the
  whole design. Stale rates are the user's call, and `as_of` records when each
  was set.
- The transfer matcher refuses to pair transactions in different currencies: a
  cross-currency transfer needs a rate, not an amount tolerance. Asserted by
  `cross_currency_pairs_are_left_alone`.
- Crypto is deliberately **not** in the registry. It is not ISO-4217, its
  precision does not fit a 0–3 exponent, and it belongs in the snapshot model as
  a valued holding rather than as a transaction currency.
