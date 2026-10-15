/**
 * What a region file may contain, and how it is read.
 *
 * A region file is written by a person, in the units a person uses: 19 for
 * 19%, 6000 for six thousand of the currency. This module checks it and
 * turns it into the units the app computes in (basis points and cents), so
 * nobody adding a country ever has to know those exist.
 *
 * It imports nothing, on purpose: the same code runs in the app and in
 * `pnpm check:regions`, which is plain Node.
 */

export interface TaxBand {
  /** In cents; `null` on the last band, which has no upper limit. */
  upTo: number | null;
  rateBps: number;
}

export interface Region {
  id: string;
  name: string;
  /** The country in English, for prompts. */
  country: string;
  locale: string;
  currency: string;
  currencySymbol: string;
  symbolFirst: boolean;
  /** What the tax on investment gains is called, where it was read, and
   *  what the rates leave out. */
  tax: { name: string; year: number | null; source: string | null; note: string | null } | null;
  /** Words on statement lines that identify no shop. */
  merchantNoise: string[];
  fire: {
    swrBps: number;
    pensionAge: number;
    taxBrackets: TaxBand[];
  };
  simulator: {
    transferTaxBps: number;
    financingBps: number;
    mortgageRateBps: number;
    mortgageTermMonths: number;
    otherClosingBps: number;
    dtiMaxBps: number;
    computableRentBps: number;
    rentalTaxBps: number;
    fixedCostsAnnual: number;
    mgmtFeeBps: number;
    maintenanceBps: number;
  };
}

/**
 * What a file falls back to for anything it leaves out. Deliberately
 * unremarkable round numbers: a contributor who knows their country's tax
 * bands but not its typical mortgage can still add it.
 */
const RETIREMENT = { pensionAge: 67, withdrawalRate: 4 };
const PROPERTY = {
  purchaseTax: 8,
  mortgageShare: 80,
  mortgageRate: 3.5,
  mortgageYears: 25,
  otherPurchaseCosts: 1.5,
  maxDebtToIncome: 35,
  rentCountedByBank: 70,
  rentalProfitTax: 20,
  fixedCostsPerYear: 1000,
  agencyFee: 8,
  maintenance: 5,
};

/**
 * What a file with no `merchantNoise` falls back to: the boilerplate words
 * of English-language statements. A country with its own bank wording lists
 * its own.
 */
const GENERIC_NOISE = [
  "payment", "purchase", "card", "transfer", "direct", "debit", "credit", "from", "with",
  "the", "and", "for", "ref", "reference", "date", "value", "account", "bank", "online",
  "contactless", "recurring", "invoice",
];

const pct = (v: number) => Math.round(v * 100);
const money = (v: number) => Math.round(v * 100);

type Outcome = { region: Region; problems: [] } | { region: null; problems: string[] };

/** Read one region file. `fileId` is its name without ".json". */
export function parseRegion(raw: unknown, fileId: string): Outcome {
  const problems: string[] = [];
  const obj = (v: unknown): Record<string, unknown> =>
    v !== null && typeof v === "object" && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
  const r = obj(raw);

  const text = (key: string): string => {
    const v = r[key];
    if (typeof v !== "string" || v.trim() === "") {
      problems.push(`"${key}" is missing: it should be text, like ${EXAMPLES[key] ?? '"…"'}`);
      return "";
    }
    return v.trim();
  };
  const num = (
    from: Record<string, unknown>,
    path: string,
    key: string,
    fallback: number,
    [lo, hi]: [number, number],
  ): number => {
    const v = from[key];
    if (v === undefined) return fallback;
    if (typeof v !== "number" || !Number.isFinite(v) || v < lo || v > hi) {
      problems.push(`"${path}.${key}" should be a number between ${lo} and ${hi}, not ${JSON.stringify(v)}`);
      return fallback;
    }
    return v;
  };

  const id = text("id");
  if (id && id !== fileId) problems.push(`"id" is "${id}" but the file is called ${fileId}.json: they must match`);
  const name = text("name");
  const country = text("country");
  const locale = text("locale");
  if (locale) {
    try {
      new Intl.NumberFormat(locale);
    } catch {
      problems.push(`"locale" is "${locale}", which is not a language tag such as "en-GB" or "de-DE"`);
    }
  }
  const currency = text("currency");
  if (currency && !/^[A-Z]{3}$/.test(currency))
    problems.push(`"currency" is "${currency}": it should be a three-letter code such as USD, GBP or MXN`);
  const currencySymbol = text("currencySymbol");
  if (r.symbolFirst !== undefined && typeof r.symbolFirst !== "boolean")
    problems.push(`"symbolFirst" should be true or false`);

  // --- tax on investment gains ------------------------------------------
  let tax: Region["tax"] = null;
  const taxBrackets: TaxBand[] = [];
  if (r.investmentTax !== undefined) {
    const t = obj(r.investmentTax);
    const bands = Array.isArray(t.bands) ? t.bands : null;
    if (!bands || bands.length === 0) {
      problems.push(`"investmentTax.bands" should be a list with at least one band, like [{ "rate": 28 }]`);
    } else {
      let floor = 0;
      bands.forEach((b, i) => {
        const band = obj(b);
        const where = `"investmentTax.bands[${i}]"`;
        const last = i === bands.length - 1;
        const rate = band.rate;
        if (typeof rate !== "number" || rate < 0 || rate > 100) {
          problems.push(`${where} needs a "rate" between 0 and 100 (a percentage), not ${JSON.stringify(rate)}`);
          return;
        }
        const upTo = band.upTo;
        if (last) {
          if (upTo !== undefined && upTo !== null)
            problems.push(`${where} is the last band, so it has no "upTo": it covers everything above`);
          taxBrackets.push({ upTo: null, rateBps: pct(rate) });
          return;
        }
        if (typeof upTo !== "number" || upTo <= floor) {
          problems.push(
            `${where} needs an "upTo" larger than ${floor} (the band before it): the amount of gains a year this rate applies up to`,
          );
          return;
        }
        floor = upTo;
        taxBrackets.push({ upTo: money(upTo), rateBps: pct(rate) });
      });
    }
    tax = {
      name: typeof t.name === "string" && t.name.trim() ? t.name.trim() : country,
      year: typeof t.year === "number" ? t.year : null,
      source: typeof t.source === "string" && t.source.trim() ? t.source.trim() : null,
      note: typeof t.note === "string" && t.note.trim() ? t.note.trim() : null,
    };
  }

  let merchantNoise = GENERIC_NOISE;
  if (r.merchantNoise !== undefined) {
    if (
      !Array.isArray(r.merchantNoise) ||
      r.merchantNoise.some((w) => typeof w !== "string" || w.trim() === "")
    ) {
      problems.push(`"merchantNoise" should be a list of words, like ["purchase", "pos debit"]`);
    } else {
      merchantNoise = (r.merchantNoise as string[]).map((w) => w.trim());
    }
  }

  const ret = obj(r.retirement);
  const prop = obj(r.property);
  for (const [section, known] of [
    ["retirement", RETIREMENT],
    ["property", PROPERTY],
  ] as const) {
    for (const key of Object.keys(obj(r[section])))
      if (!(key in known))
        problems.push(
          `"${section}.${key}" is not something Coin Heaven reads. The fields are: ${Object.keys(known).join(", ")}`,
        );
  }

  const region: Region = {
    id,
    name,
    country,
    locale,
    currency,
    currencySymbol,
    symbolFirst: r.symbolFirst === true,
    tax,
    merchantNoise,
    fire: {
      swrBps: pct(num(ret, "retirement", "withdrawalRate", RETIREMENT.withdrawalRate, [0.5, 15])),
      pensionAge: num(ret, "retirement", "pensionAge", RETIREMENT.pensionAge, [40, 90]),
      taxBrackets,
    },
    simulator: {
      transferTaxBps: pct(num(prop, "property", "purchaseTax", PROPERTY.purchaseTax, [0, 50])),
      financingBps: pct(num(prop, "property", "mortgageShare", PROPERTY.mortgageShare, [0, 100])),
      mortgageRateBps: pct(num(prop, "property", "mortgageRate", PROPERTY.mortgageRate, [0, 30])),
      mortgageTermMonths: Math.round(
        num(prop, "property", "mortgageYears", PROPERTY.mortgageYears, [1, 50]) * 12,
      ),
      otherClosingBps: pct(num(prop, "property", "otherPurchaseCosts", PROPERTY.otherPurchaseCosts, [0, 30])),
      dtiMaxBps: pct(num(prop, "property", "maxDebtToIncome", PROPERTY.maxDebtToIncome, [1, 100])),
      computableRentBps: pct(num(prop, "property", "rentCountedByBank", PROPERTY.rentCountedByBank, [0, 100])),
      rentalTaxBps: pct(num(prop, "property", "rentalProfitTax", PROPERTY.rentalProfitTax, [0, 100])),
      fixedCostsAnnual: money(
        num(prop, "property", "fixedCostsPerYear", PROPERTY.fixedCostsPerYear, [0, 1_000_000_000]),
      ),
      mgmtFeeBps: pct(num(prop, "property", "agencyFee", PROPERTY.agencyFee, [0, 100])),
      maintenanceBps: pct(num(prop, "property", "maintenance", PROPERTY.maintenance, [0, 100])),
    },
  };

  return problems.length > 0 ? { region: null, problems } : { region, problems: [] };
}

const EXAMPLES: Record<string, string> = {
  id: '"us"',
  name: '"United States"',
  country: '"United States"',
  locale: '"en-US"',
  currency: '"USD"',
  currencySymbol: '"$"',
};
