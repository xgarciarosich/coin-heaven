const s = (n: number) => (n === 1 ? "" : "s");

export const fire = {
  notAdvice: "Not financial advice. This is arithmetic on the numbers and assumptions you enter — returns, inflation, tax and spending will differ — not a forecast, and not a recommendation to buy, sell or retire. Consider speaking to a qualified adviser before making decisions.",
  ledeEmpty: "Say what you spend and what you put aside, and this works out when work becomes optional.",
  todaysMoney: "Everything here is in today's money.",
  saved: "Saved",
  noSnapshot: "Nothing is recorded in Net worth yet, so this starts from zero. Add what you have there and every figure here fills in.",

  headlineThere: "You are there",
  headlineWithin: "Within the month",
  headlineAway: (y: number, m: number) =>
    `Work becomes optional in ${[
      y > 0 ? `${y} year${s(y)}` : null,
      m > 0 ? `${m} month${s(m)}` : null,
    ]
      .filter(Boolean)
      .join(" and ")}`,
  notOnPath: "Not on this path yet",
  atAge: (a: number) => `You would be ${a}.`,
  range: (lo: string, hi: string) =>
    `Between ${lo} and ${hi} years, depending on how markets go.`,
  nothingWithin: (h: number) =>
    `Nothing within ${h} years on these numbers — put more aside, spend less, or both.`,

  have: "What you have invested",
  need: "What you need",
  rentCoversAll: "Rent already covers everything you spend — the amount needed is zero by arithmetic, not by luck.",
  needNote: (spend: string, swr: string) =>
    `Enough to take out ${spend} a year, at ${swr} of the pot each year, so that it lasts.`,
  needNoteTax: (gross: string, tax: string, swr: string) =>
    `Enough to take out ${gross} a year — ${tax} of it goes in tax — at ${swr} of the pot each year, so that it lasts.`,
  milestoneLabel: (pct: number) => (pct >= 100 ? "There" : pct === 50 ? "Halfway" : `${pct}%`),
  milestoneAt: (y: string, age: number | null) =>
    age !== null ? `in ${y} yrs · age ${age}` : `in ${y} yrs`,
  reached: "reached",

  yourPlan: "Your plan",
  spendMonthly: "What you spend",
  saveMonthly: "What you put aside",
  perMonth: "a month",
  fromLedger: (v: string) => `Use my real figure: ${v} a month`,
  ledgerWindow: (months: number) =>
    months >= 12
      ? "Your last 12 full months of transactions."
      : `From ${months} month${s(months)} of transactions, so far.`,
  savingsRate: (v: string) => `That is ${v} of what you earn.`,
  markets: "How markets treat you",
  aboveInflation: "% above inflation",
  moods: { cautious: "Cautious", balanced: "Balanced", hopeful: "Hopeful" },
  marketsHint: "What your investments earn each year once prices have gone up. Balanced is roughly what shares have done over the long run.",
  ageNow: "Your age today",
  addAge: "Add it and the dates turn into ages.",

  path: "The road there",
  portfolio: "Your investments",
  target: "What you need",
  targetMarker: "there",
  band: "Range",
  age: (a: string) => `Age ${a}`,
  year: (y: string) => `Year ${y}`,
  pathNote: "The shaded band is the cautious-to-hopeful range. The dashed line stays flat because everything is in today's money — rising prices are already taken out.",
  pathNotePension: "The dashed line falls as your pension gets closer: every year you wait is one year fewer your investments have to carry alone. The shaded band is the cautious-to-hopeful range.",

  levers: "What would bring it forward",
  leverSave: (v: string) => `Put aside ${v} more a month`,
  leverSpend: (v: string) => `Spend ${v} less a month`,
  monthsEarlier: (n: number) => `${n} month${s(n)} sooner`,
  noChange: "no change",
  tryIt: "Click to try it",
  leversNote: "Click one to try it. Spending less counts twice: you need a smaller pot, and the money not spent is invested.",

  otherIncome: "Rent and pension",
  sumRent: (v: string) => `${v} of rent a year`,
  sumPension: (v: string, age: number) => `${v} of pension from ${age}`,
  sumNone: "none set",
  rent: "Rent you keep, a year",
  rentHint: "After costs, the mortgage and tax.",
  pension: "Pension, a year",
  pensionHint: "In today's money. Your investments only carry the full bill until it starts.",
  pensionAge: "Pension starts at age",
  rentBody: (share: string, residual: string) =>
    `Rent pays for ${share} of what you spend, leaving ${residual} a year for your investments to cover. That is why the value of the property itself is not counted above — its contribution is already here, and counting it twice would bring the date forward by years that do not exist.`,
  totalWealth: (v: string) => `Everything you own, property included, comes to ${v}.`,

  tax: "Tax when you take money out",
  sumTax: (v: string) => `allowing ${v} a year`,
  sumTaxOff: "not counted",
  taxToggle: "Allow for tax on what I take out",
  taxHint: "Only the profit part of each withdrawal is taxed, at the rates below. Leave this on: without it the amount needed is too low.",
  gainShare: "Share of each withdrawal that is profit %",
  gainShareHint: "About half for a fund held for many years.",
  fromPortfolio: (v: string) => `Use my portfolio: ${v} of it is profit`,

  careful: "How long the money has to last",
  sumCareful: (swr: string, band: string) => `taking out ${swr} a year · range ±${band}`,
  swr: "Taken out each year %",
  swrExplain: (swr: string, withdrawal: string, target: string) =>
    `Once you stop, you live on a slice of your investments each year. At ${swr}, taking ${withdrawal} a year needs ${target}. A smaller slice makes the money last longer but needs more of it; 4% is the usual rule of thumb for about thirty years. Click a row to try another.`,
  needs: (v: string) => `needs ${v}`,
  bandLabel: "Good and bad markets ± %",
  bandExplain: (lo: string, hi: string, early: string, late: string) =>
    `Nobody knows what markets will do, so the chart shades a range around your guess: from ${lo} a year if they go badly to ${hi} if they go well. That is what turns one date into "between ${early} and ${late} years".`,
  goalAge: "I would like to stop at",
  goalYes: (age: number) => `Yes — stopping at ${age} works.`,
  goalSpare: (y: string) => `You get there ${y} years before that, on these numbers.`,
  goalJust: "With almost nothing to spare, on these numbers.",
  goalNo: (age: number) => `Not by ${age}, on these numbers.`,
  goalGap: (age: number, have: string, need: string) =>
    `At ${age} you would have ${have}, and you would need ${need}.`,
  goalFix: "Any one of these gets you there",
  goalSave: (v: string) => `Put aside ${v} more a month`,
  goalSpend: (v: string) => `Spend ${v} less a month`,
  goalLater: (age: number) => `Or stop at ${age} instead`,
  goalNone: "No realistic change in saving or spending reaches it. A later age is the lever.",
  brackets: "Tax rates on investment gains",
  bracketsHint: "Pick a country to use its rates, then correct anything that differs for you or has changed since — what you type here is yours and is kept with your plan.",
  taxCountry: "Whose tax rules",
  taxOwn: "My own rates",
  taxOption: (name: string, tax: string) => `${name} — ${tax}`,
  taxSource: (tax: string, year: number | null, source: string | null) =>
    `${tax}${year === null ? "" : `, ${year}`}${source === null ? "" : `. Check them at ${source}`}.`,
  taxOwnNote: "These are rates you typed. They match none of the countries Coin Heaven knows.",
  taxAddCountry: "Country not listed? Type its rates below. To add it for everyone, it is one small file: see regions/README.md in the source code.",
  bracketUpTo: "Up to",
  bracketAbove: "Above that",
  bracketAll: "All gains",
  bracketAdd: "Add a band",
  bracketRemove: "Remove this band",
  already: "already there",
  yr: (y: string) => `${y} yrs`,
};
