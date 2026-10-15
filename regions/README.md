# Regions: add your country

Everything Coin Heaven assumes about *where you live* is in this folder, one small
file per country. Nothing else in the code names a country, a currency, a tax
rate or a bank's wording — so adding yours is writing one file, not
programming.

Coin Heaven ships with `us.json` (United States) and `es.json` (Spain). On first
run it picks the one that matches the computer's language and region; the
choice is visible and changeable in **Settings → Region**.

## Five minutes, start to finish

1. Copy `us.json` to a file named after your country code:
   `mx.json`, `de.json`, `gb.json`.
2. Change what is in it. Percentages are written as percentages and money in
   whole units — `19` is 19%, `6000` is six thousand.
3. Run `pnpm check:regions`. It reads every file and says in plain words
   what is wrong with any it cannot use.
4. Run the app (`pnpm app:dev`). Your country is in **Settings → Region**
   and in the tax section of **Independence**; no code needs to know about it.
5. Open a pull request, so the next person from your country does not have
   to do this.

If you edit the file in VS Code or any editor that understands JSON Schema,
the `"$schema"` line at the top gives you autocompletion and a description
of every field as you type.

## The smallest file that works

```json
{
  "$schema": "./region.schema.json",
  "id": "xx",
  "name": "Your country, in its own language",
  "country": "Your country, in English",
  "locale": "en-GB",
  "currency": "GBP",
  "currencySymbol": "£",
  "symbolFirst": true
}
```

Everything else is optional and falls back to round, unremarkable numbers.

## Writing the tax on investment gains

`investmentTax.bands` is the tax someone pays on the profit when they sell
investments, from the lowest band up. The last band has no `upTo`: it covers
everything above the one before it.

| Your country has… | Write |
| --- | --- |
| One flat rate | `[{ "rate": 25 }]` |
| A tax-free allowance, then a flat rate | `[{ "upTo": 1000, "rate": 0 }, { "rate": 20 }]` |
| Rising bands | `[{ "upTo": 6000, "rate": 19 }, { "upTo": 50000, "rate": 21 }, { "rate": 23 }]` |
| No tax on gains | `[{ "rate": 0 }]` |

`name`, `year` and `source` are shown beside the rates in the app, so a
reader can tell how old they are and where to check them. Please fill them
in. `note` is for what the rates leave out ("state tax is not included"):
say it plainly, because the numbers will otherwise look more certain than
they are.

## Bank statement wording

`merchantNoise` is a list of words that appear on statement lines and
identify no shop: "purchase", "pos debit", "compra". Coin Heaven ignores them when
it works out which merchant a line is from, so two visits to the same shop
look alike and a category you set once applies to the next. Take a few
statement lines from your own bank and list the boilerplate words. Without
the list, a generic English one is used.

## Every field

| Field | Needed | What it does |
| --- | --- | --- |
| `id` | yes | The file's name without `.json`. |
| `name` | yes | The country in its own language. Shown in Settings. |
| `country` | yes | The country in English. Told to the AI assistant so it knows whose shops and rules to think of. |
| `locale` | yes | A language tag such as `en-US` or `de-DE`. Decides digit grouping, the decimal mark and how dates are written. |
| `currency` | yes | Three-letter code: `USD`, `GBP`, `MXN`. |
| `currencySymbol` | yes | The symbol shown next to amounts. |
| `symbolFirst` | no | `true` for `$1,234`; left out, it is `1.234 €`. |
| `investmentTax` | no | See above. Without it, the Independence page asks the person to type their own rates. |
| `retirement.pensionAge` | no | State pension age. Default 67. |
| `retirement.withdrawalRate` | no | Percent of the portfolio taken out each year. Default 4. |
| `property.*` | no | What a new buy-to-let plan starts from: `purchaseTax`, `mortgageShare`, `mortgageRate`, `mortgageYears`, `otherPurchaseCosts`, `maxDebtToIncome`, `rentCountedByBank`, `rentalProfitTax`, `fixedCostsPerYear`, `agencyFee`, `maintenance`. All percentages except `mortgageYears` and `fixedCostsPerYear`. Leave out whatever you do not know; generic values are used. |
| `merchantNoise` | no | See above. |

`region.schema.json` describes each of these in full.

All of these are **starting values**. Each person can change every one of
them inside the app, and what they change is saved with their own data — a
region file only decides what they see before they have touched anything.

## What a region does not do

- It does not translate the interface. Languages live in `src/locales/`.
- It does not convert money. Coin Heaven keeps one currency; changing region does
  not re-denominate amounts already recorded.
- It is not tax advice. Rates go out of date: say which year yours are for,
  and expect people to correct them in the app.
