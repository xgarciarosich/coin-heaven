/**
 * Development fixtures.
 *
 * Running `pnpm dev` opens the frontend in a plain browser with no Tauri
 * backend behind it, which is by far the fastest way to iterate on the UI —
 * no Rust rebuild between changes. These fixtures stand in for the real IPC
 * responses in that situation only.
 *
 * Guarded by `import.meta.env.DEV`, so the whole module is dropped from
 * production bundles by dead-code elimination.
 */
import type {
  AppStatus,
  Balances,
  FireParams,
  SimParams,
  SimResult,
  UnlockStatus,
  YearRow,
} from "./types";

interface MockLine {
  bucket: string;
  label: string;
  grossValueMinor: number;
  debtMinor: number;
}
let sampleLoaded = false;
interface MockPlanItem { kind: "income" | "expense"; name: string; amountMinor: number; owner?: "shared" | "a" | "b" }
const mockSetups = new Map<string, Record<string, string>>();
const individual = { mode: "individual", personA: "", personB: "", split: "equal" };
const mockPlans = new Map<string, MockPlanItem[]>([
  ["2024-10", [
    { kind: "income", name: "Salary", amountMinor: 210_000 },
    { kind: "expense", name: "Rent", amountMinor: 80_000 },
    { kind: "expense", name: "Food", amountMinor: 30_000 },
    { kind: "expense", name: "Phone", amountMinor: 1_550 },
  ]],
  ["2026-09", [
    { kind: "income", name: "Salary", amountMinor: 240_000 },
    { kind: "income", name: "Side job", amountMinor: 15_000 },
    { kind: "expense", name: "Rent", amountMinor: 90_000 },
    { kind: "expense", name: "Food", amountMinor: 35_000 },
    { kind: "expense", name: "Savings transfer", amountMinor: 80_000 },
  ]],
]);

let aiSettings: Record<string, unknown> & { exaKeySet: boolean } = {
  enabled: true, requirePreview: false, allowRawDescriptions: false,
  searchUrl: "http://localhost:8888", searchEngine: "duckduckgo", exaKeySet: false,
};
const savedSnapshots = new Map<number, { id: number; takenOn: string; lines: MockLine[] } & Record<string, unknown>>();
const deletedSnapshots = new Set<number>();

const matrix2026 = () => {
    const cats = [
      { categoryId: 1, categoryName: "Salary", kind: "income", color: "#22c55e", base: 240000 },
      { categoryId: 4, categoryName: "Housing", kind: "expense", color: "#f97316", base: -98000, budgetMinor: 1200000 },
      { categoryId: 5, categoryName: "Food & groceries", kind: "expense", color: "#eab308", base: -41000, budgetMinor: 600000 },
      { categoryId: 7, categoryName: "Leisure & travel", kind: "expense", color: "#a855f7", base: -22000, budgetMinor: 200000 },
      { categoryId: null, categoryName: "Uncategorised", kind: "expense", color: null, base: -5400 },
    ];
    const rows = cats.map((c, ci) => {
      const cells = Array.from({ length: 12 }, (_, m) =>
        m > 8 ? 0 : Math.round(c.base * (1 + Math.sin(m + ci) * 0.28)),
      );
      return {
        categoryId: c.categoryId, categoryName: c.categoryName, kind: c.kind, color: c.color,
        cells, counts: cells.map((v) => (v === 0 ? 0 : 3 + ci)),
        totalMinor: cells.reduce((a, b) => a + b, 0),
        budgetMinor: (c as { budgetMinor?: number }).budgetMinor ?? null,
      };
    });
    const per = (pick: (v: number) => boolean) =>
      Array.from({ length: 12 }, (_, m) =>
        rows.reduce((a, r) => a + (pick(r.cells[m]!) ? r.cells[m]! : 0), 0),
      );
    const income = per((v) => v > 0);
    const expense = per((v) => v < 0);
    const net = income.map((v, i) => v + expense[i]!);
    const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0);
    return {
      periods: Array.from({ length: 12 }, (_, m) => `2026-${String(m + 1).padStart(2, "0")}`),
      rows,
      incomeByPeriod: income, expenseByPeriod: expense, netByPeriod: net,
      incomeTotal: sum(income), expenseTotal: sum(expense), netTotal: sum(net),
      activePeriods: 9,
    };
  };

const mockFlat = (name: string, buyMonth: number, price: number, rent: number) => ({
  name, buyMonth, price, renovation: 1_500_000, monthlyRent: rent, transferTaxBps: 1_000,
  financingBps: null, mortgageRateBps: null, mortgageTermMonths: null, fixedCostsAnnual: null,
});

/** Asset classes, kept so a reorder in dev mode survives a refresh. */
const mockClasses = [
    { key: "cash", name: "Cash", kind: "liquid", color: "#94a3b8", sortOrder: 10, countsFire: true, lineCount: 4 },
    { key: "equities", name: "Equities", kind: "liquid", color: "#22c55e", sortOrder: 20, countsFire: true, lineCount: 4 },
    { key: "crypto", name: "Crypto", kind: "liquid", color: "#a855f7", sortOrder: 30, countsFire: true, lineCount: 2 },
    { key: "pension", name: "Pension", kind: "liquid", color: "#3b82f6", sortOrder: 40, countsFire: true, lineCount: 4 },
    { key: "real_estate", name: "Real estate", kind: "property", color: "#d97706", sortOrder: 50, countsFire: false, lineCount: 4 },
    { key: "business", name: "Business stake", kind: "other_asset", color: "#0ea5e9", sortOrder: 60, countsFire: false, lineCount: 2 },
    { key: "other_debt", name: "Other debt", kind: "liability", color: "#f87171", sortOrder: 80, countsFire: false, lineCount: 1 },
];

export const mockParams: SimParams = {
  horizonYears: 10,
  startMonth: null,
  startCash: 6_000_000,
  monthlySavings: 150_000,
  netMonthlySalary: 320_000,
  incomeGrowthBps: 200,
  existingMonthlyDebt: 0,
  cashBuffer: 600_000,
  reinvestNetRent: true,
  financingBps: 8_000,
  mortgageRateBps: 280,
  mortgageTermMonths: 300,
  otherClosingBps: 150,
  dtiMaxBps: 3_500,
  computableRentBps: 7_000,
  occupancyBps: 9_000,
  rentCpiBps: 200,
  appreciationBps: 200,
  mgmtFeeBps: 800,
  maintenanceBps: 500,
  fixedCostsAnnual: 90_000,
  rentalTaxBps: 1_900,
  properties: [
    mockFlat("Flat downtown", 6, 12_000_000, 70_000),
    mockFlat("Studio near the campus", 54, 14_000_000, 82_000),
    mockFlat("Penthouse by the beach", 70, 26_000_000, 120_000),
  ],
};

/**
 * A rough reimplementation of the Rust engine, good enough to shape a layout
 * against. Deliberately NOT the real model — the real one is in Rust and is
 * the only source of truth for any number a user sees.
 */
function fakeRun(p: SimParams): SimResult {
  const months = p.horizonYears * 12;
  const order = p.properties.map((_, i) => i).filter((i) => !p.properties[i]!.owned).sort(
    (a, b) => p.properties[a]!.buyMonth - p.properties[b]!.buyMonth,
  );
  const terms = (i: number) => {
    const f = p.properties[i]!;
    if (f.owned) {
      const loan = f.owned.loanRemaining;
      const r = (f.mortgageRateBps ?? p.mortgageRateBps) / 10_000 / 12;
      const n = f.mortgageTermMonths ?? p.mortgageTermMonths;
      const pay = loan === 0 ? 0 : r === 0 ? loan / n : (loan * r) / (1 - Math.pow(1 + r, -n));
      return { loan, r, pay, need: 0 };
    }
    const ltv = f.financingBps ?? p.financingBps;
    const loan = (f.price * ltv) / 10_000;
    const r = (f.mortgageRateBps ?? p.mortgageRateBps) / 10_000 / 12;
    const n = f.mortgageTermMonths ?? p.mortgageTermMonths;
    const pay = r === 0 ? loan / n : (loan * r) / (1 - Math.pow(1 + r, -n));
    const need =
      f.price - loan + (f.price * (f.transferTaxBps + p.otherClosingBps)) / 10_000 + f.renovation;
    return { loan, r, pay, need };
  };
  const outcomes = p.properties.map((f, index) => {
    const t = terms(index);
    return {
      index, plannedMonth: f.buyMonth, boughtMonth: null as number | null,
      blockedBy: null as "cash" | "bank" | null,
      cashNeeded: Math.round(t.need), cashShort: 0, loan: Math.round(t.loan),
      monthlyPayment: Math.round(t.pay), dtiBpsPlanned: 0, netRentFirst: 0,
      valueEnd: 0, debtEnd: 0, netRentEnd: 0,
    };
  });
  const held: { i: number; debt: number; value: number; rent: number }[] = [];
  let cash = p.startCash;
  let next = 0;
  const years: YearRow[] = [];
  const monthly: number[] = [];
  let [ySave, yRent, yBuys] = [0, 0, 0];
  p.properties.forEach((f, i) => {
    if (!f.owned) return;
    held.push({ i, debt: f.owned.loanRemaining, value: f.price, rent: f.monthlyRent });
    outcomes[i]!.boughtMonth = 0;
  });

  const netOf = (h: (typeof held)[number]) => {
    const got = (h.rent * p.occupancyBps) / 10_000;
    const t = terms(h.i);
    return got - (got * (p.mgmtFeeBps + p.maintenanceBps)) / 10_000 - p.fixedCostsAnnual / 12 - t.pay;
  };

  for (let m = 1; m <= months; m++) {
    const factor = Math.pow(1 + p.incomeGrowthBps / 10_000, Math.floor((m - 1) / 12));
    const salary = p.netMonthlySalary * factor;
    const saving = p.monthlySavings * factor;
    cash += saving;
    ySave += saving;
    let [gross, net] = [0, 0];
    for (const h of held) {
      const t = terms(h.i);
      h.debt = Math.max(0, h.debt - (t.pay - h.debt * t.r));
      h.rent *= Math.pow(1 + p.rentCpiBps / 10_000, 1 / 12);
      h.value *= Math.pow(1 + p.appreciationBps / 10_000, 1 / 12);
      gross += h.rent;
      net += netOf(h);
    }
    if (p.reinvestNetRent) cash += net;
    yRent += net;
    const service = () => held.reduce((a, h) => a + terms(h.i).pay, 0);
    while (next < order.length && p.properties[order[next]!]!.buyMonth <= m) {
      const i = order[next]!;
      const f = p.properties[i]!;
      const t = terms(i);
      const income = salary + ((gross + f.monthlyRent) * p.computableRentBps) / 10_000;
      const dtiAfter = Math.round(((service() + t.pay) / Math.max(income, 1)) * 10_000);
      const short = Math.max(0, t.need - (cash - p.cashBuffer));
      const o = outcomes[i]!;
      if (m === f.buyMonth) {
        o.dtiBpsPlanned = dtiAfter;
        o.cashShort = Math.round(short);
        o.blockedBy = short > 0 ? "cash" : dtiAfter > p.dtiMaxBps ? "bank" : null;
      }
      if (short > 0 || dtiAfter > p.dtiMaxBps) break;
      cash -= t.need;
      const h = { i, debt: t.loan, value: f.price + f.renovation, rent: f.monthlyRent };
      held.push(h);
      o.boughtMonth = m;
      o.netRentFirst = Math.round(netOf(h));
      yBuys += 1;
      next += 1;
    }
    const value = held.reduce((a, h) => a + h.value, 0);
    const debt = held.reduce((a, h) => a + h.debt, 0);
    monthly.push(Math.round(cash + value - debt));
    if (m % 12 === 0) {
      const income = salary + (gross * p.computableRentBps) / 10_000;
      const dtiBps = Math.round((service() / Math.max(income, 1)) * 10_000);
      years.push({
        year: m / 12,
        savingsContributed: Math.round(ySave),
        netRentReceived: Math.round(yRent),
        purchases: yBuys,
        properties: held.length,
        cash: Math.round(cash),
        grossRentMonthly: Math.round(gross),
        netRentMonthly: Math.round(held.reduce((a, h) => a + netOf(h), 0)),
        portfolioValue: Math.round(value),
        debt: Math.round(debt),
        equity: Math.round(value - debt),
        propertyEquity: p.properties.map((_, i) => {
          const h = held.find((x) => x.i === i);
          return h ? Math.round(h.value - h.debt) : 0;
        }),
        netWorth: Math.round(cash + value - debt),
        salaryMonthly: Math.round(salary),
        dtiBps,
        dtiOk: dtiBps <= p.dtiMaxBps,
      });
      [ySave, yRent, yBuys] = [0, 0, 0];
    }
  }
  for (const h of held) {
    const o = outcomes[h.i]!;
    o.valueEnd = Math.round(h.value);
    o.debtEnd = Math.round(h.debt);
    o.netRentEnd = Math.round(netOf(h));
  }

  const last = years[years.length - 1];
  return {
    years,
    monthlyNetWorth: monthly,
    properties: outcomes,
    finalNetWorth: last?.netWorth ?? 0,
    finalProperties: held.length,
    finalNetRentMonthly: last?.netRentMonthly ?? 0,
    finalSavingCapacity: Math.round(
      p.monthlySavings * Math.pow(1 + p.incomeGrowthBps / 10_000, p.horizonYears - 1) +
        (last?.netRentMonthly ?? 0),
    ),
  };
}

const status: AppStatus = {
  schemaVersion: 4,
  unlocked: true,
  dbPath: "~/Library/Application Support/com.coinheaven.app/atlas.db",
  version: "1.0.0",
};

const unlock: UnlockStatus = {
  state: "opened",
  hasPassphrase: false,
  phrasePending: false,
  phraseConfirmed: true,
  phraseAcknowledged: true,
};

const balances: Balances = {
  liquid: 18_450_000,
  realEstateEquity: 24_200_000,
  otherAssets: 0,
  otherDebt: 0,
};

const fireParams: FireParams = {
  annualSpendTarget: 3_600_000,
  swrBps: 400,
  annualNetRentalIncome: 1_180_000,
};

export const mockResponses: Record<string, (args?: never) => unknown> = {
  app_status: () => status,
  unlock_status: () => unlock,
  sim_defaults: () => mockParams,
  sim_run: () => null, // replaced below; needs the caller's params
  fire_defaults: () => fireParams,
  fire_latest_balances: () => balances,
  sim_list_scenarios: () => [],
  sim_save_scenario: () => 1,
  matching_pending_transfers: () => [
    {
      outId: 41, inId: 42, residualMinor: -150, dayGap: 1, sameAccountName: false,
      reason:
        "Opposite signs a day later, differing by a small amount — consistent with a transfer fee.",
      out: { id: 41, bookedOn: "2026-03-01", description: "TRASPASO A AHORRO",
             counterparty: "ES91 **** 1332", accountName: "Santander corriente",
             amount: -50000, currency: "EUR" },
      incoming: { id: 42, bookedOn: "2026-03-02", description: "TRASPASO DESDE CORRIENTE",
             counterparty: null, accountName: "Ahorro", amount: 49850, currency: "EUR" },
    },
  ],
  matching_pending_splits: () => [
    {
      parentId: 88, reimbursementId: 91, fractionDenominator: 4, residualMinor: 0,
      dayGap: 1, parentNetAfter: -9000, existingLinks: 0,
      reason: "The refund is exactly a quarter of the bill, arriving a day later.",
      parent: { id: 88, bookedOn: "2026-04-10", description: "TRATTORIA DA LUIGI",
             counterparty: null, accountName: "Santander corriente",
             amount: -12000, currency: "EUR" },
      reimbursement: { id: 91, bookedOn: "2026-04-11", description: "BIZUM RECIBIDO",
             counterparty: "ANA LOPEZ", accountName: "Santander corriente",
             amount: 3000, currency: "EUR" },
    },
  ],
  recovery_take_phrase: () =>
    "ridge canvas lumber orbit palace tenant fossil marble ginger walnut copper anchor",
  category_list: () => [
    { id: 1, parentId: null, name: "Salary", kind: "income", color: "#22c55e", sortOrder: 10, txnCount: 12 },
    { id: 4, parentId: null, name: "Housing", kind: "expense", color: "#f97316", sortOrder: 40, txnCount: 24 },
    { id: 5, parentId: null, name: "Food & groceries", kind: "expense", color: "#eab308", sortOrder: 50, txnCount: 96 },
    { id: 7, parentId: null, name: "Leisure & travel", kind: "expense", color: "#a855f7", sortOrder: 70, txnCount: 31 },
  ],
  event_list: () => [
    { id: 1, name: "Venice Trip 2026", defaultCategoryId: 7, defaultCategoryName: "Leisure & travel",
      startsOn: "2026-05-01", endsOn: "2026-05-08", budgetMinor: 150000, isClosed: false,
      txnCount: 14, netMinor: -128400 },
  ],
  pl_summary: () => ({
    lines: [
      { categoryId: 4, categoryName: "Housing", kind: "expense", color: "#f97316", netMinor: -90000, txnCount: 2 },
      { categoryId: 5, categoryName: "Food & groceries", kind: "expense", color: "#eab308", netMinor: -41230, txnCount: 38 },
      { categoryId: 7, categoryName: "Leisure & travel", kind: "expense", color: "#a855f7", netMinor: -22800, txnCount: 9 },
      { categoryId: 1, categoryName: "Salary", kind: "income", color: "#22c55e", netMinor: 240000, txnCount: 1 },
    ],
    incomeMinor: 240000, expenseMinor: -154030, netMinor: 85970, uncategorisedCount: 6,
  }),
  txn_delete: () => 1,
  txn_list: () => {
    const shops = [
      { d: "COMPRA TARJETA 5487 MERCADONA VALENCIA", c: 5, n: "Food & groceries", a: -4520 },
      { d: "RECIBO IBERDROLA CLIENTES SAU", c: 4, n: "Housing", a: -7180 },
      { d: "RESTAURANTE EL POBLET", c: 7, n: "Leisure & travel", a: -6200 },
      { d: "NOMINA ACME IBERIA SL", c: 1, n: "Salary", a: 240000 },
      { d: "COMPRA CARREFOUR EXPRESS 77", c: null, n: null, a: -2890 },
    ];
    const rows = [];
    let id = 100;
    for (let m = 9; m >= 1; m--) {
      for (let i = 0; i < shops.length; i++) {
        const s = shops[i]!;
        const day = String(4 + i * 5).padStart(2, "0");
        rows.push({
          id: id++,
          bookedOn: `2026-${String(m).padStart(2, "0")}-${day}`,
          description: s.d,
          counterparty: null,
          amountMinor: Math.round(s.a * (1 + ((m + i) % 5) * 0.06)),
          netMinor: Math.round(s.a * (1 + ((m + i) % 5) * 0.06)),
          currency: "EUR",
          accountId: 1,
          accountName: "Santander corriente",
          notes: null,
          merchant:
            s.d
              .split(" ")
              .filter((w) => w.length > 3 && !/^\d+$/.test(w))
              .sort((a, b) => b.length - a.length)[0]
              ?.toLowerCase() ?? null,
          categoryId: s.c,
          categoryName: s.n,
          projectId: null,
          projectName: null,
          isNeutralTransfer: false,
          needsReview: s.c === null,
          parentTxnId: null,
          reimbursementCount: 0,
        });
      }
    }
    rows.push({
      id: 1, bookedOn: "2026-05-02", description: "TRATTORIA DA LUIGI", counterparty: null,
      amountMinor: -12000, netMinor: -9000, currency: "EUR", accountId: 1, accountName: "Santander corriente",
      notes: null, merchant: "trattoria", categoryId: 7, categoryName: "Leisure & travel", projectId: 1, projectName: "Venice Trip 2026",
      isNeutralTransfer: false, needsReview: false, parentTxnId: null, reimbursementCount: 1,
    });
    return { total: rows.length, rows };
  },
  snapshot_list: () => {
    const at = (
      takenOn: string,
      label: string,
      cash: number,
      eq: number,
      pen: number,
      flat: number,
      loan: number,
      biz: number,
      debt: number,
    ) => {
      const lines = [
        { id: null, bucket: "cash", label: "Santander", grossValueMinor: cash, debtMinor: 0, currency: "EUR" },
        { id: null, bucket: "equities", label: "Vanguard", grossValueMinor: eq, debtMinor: 0, currency: "EUR" },
        { id: null, bucket: "pension", label: "Workplace", grossValueMinor: pen, debtMinor: 0, currency: "EUR" },
        { id: null, bucket: "real_estate", label: "Flat downtown", grossValueMinor: flat, debtMinor: loan, currency: "EUR" },
        { id: null, bucket: "business", label: "Consultancy", grossValueMinor: biz, debtMinor: 0, currency: "EUR" },
        { id: null, bucket: "other_debt", label: "Car loan", grossValueMinor: 0, debtMinor: debt, currency: "EUR" },
      ];
      if (takenOn === "2026-06-30") {
        lines.push(
          { id: 91, bucket: "crypto", label: "Bitcoin", grossValueMinor: 186_421, debtMinor: 0, currency: "EUR", source: "holding" } as never,
        );
      }
      const liquid = cash + eq + pen;
      const equity = flat - loan;
      return {
        id: Number(takenOn.replaceAll("-", "")),
        takenOn,
        label,
        isScheduled: true,
        grossAssetsMinor: cash + eq + pen + flat + biz,
        totalDebtMinor: loan + debt,
        netWorthMinor: liquid + equity + biz - debt,
        liquidMinor: liquid,
        reEquityMinor: equity,
        lines,
      };
    };
    return [
      // Whatever was saved in this browser session, so that making and
      // correcting a date can be tried without a backend.
      ...[...savedSnapshots.values()],
      at("2024-12-31", "H2 2024", 3_000_000, 4_200_000, 2_100_000, 21_000_000, 15_600_000, 900_000, 700_000),
      at("2025-06-30", "H1 2025", 3_400_000, 5_600_000, 2_400_000, 21_600_000, 15_100_000, 1_200_000, 550_000),
      at("2025-12-31", "H2 2025", 3_900_000, 7_400_000, 2_800_000, 22_400_000, 14_600_000, 1_600_000, 400_000),
      at("2026-06-30", "H1 2026", 4_300_000, 9_900_000, 3_250_000, 23_200_000, 14_100_000, 2_000_000, 250_000),
    ]
      .filter((x) => !deletedSnapshots.has(x.id) && (savedSnapshots.get(x.id) ?? x) === x)
      .sort((a, b) => a.takenOn.localeCompare(b.takenOn));
  },
  snapshot_save: ((args: { id: number | null; input: { takenOn: string; lines: MockLine[] } }) => {
    const id = args.id ?? Number(args.input.takenOn.replaceAll("-", ""));
    const lines = args.input.lines;
    const of = (b: string[]) => lines.filter((l) => b.includes(l.bucket));
    const gross = (ls: MockLine[]) => ls.reduce((a, l) => a + l.grossValueMinor, 0);
    const debt = (ls: MockLine[]) => ls.reduce((a, l) => a + l.debtMinor, 0);
    const owed = of(["other_debt"]);
    const assets = lines.filter((l) => l.bucket !== "other_debt");
    savedSnapshots.set(id, {
      id,
      takenOn: args.input.takenOn,
      label: null,
      isScheduled: false,
      grossAssetsMinor: gross(assets),
      totalDebtMinor: debt(assets) + gross(owed) + debt(owed),
      netWorthMinor: gross(assets) - debt(assets) - gross(owed) - debt(owed),
      liquidMinor: gross(of(["cash", "equities", "pension"])),
      reEquityMinor: gross(of(["real_estate"])) - debt(of(["real_estate"])),
      lines,
    });
    return id;
  }) as never,
  snapshot_delete: ((args: { id: number }) => {
    savedSnapshots.delete(args.id);
    deletedSnapshots.add(args.id);
  }) as never,
  snapshot_prompt: () => ({ due: false, suggestedDate: "2026-06-30", label: "H1 2026" }),
  fire_view: () => ({
    hasSnapshot: true,
    params: { annualSpendTarget: 3600000, swrBps: 400, annualNetRentalIncome: 1180000 },
    balances: { liquid: 18450000, realEstateEquity: 8550000, otherAssets: 0, otherDebt: 0 },
    result: {
      liquidTarget: 60500000, liquidAssets: 18450000, liquidShortfall: 42050000,
      liquidProgressBps: 3049, liquidCovered: false, totalNetWorth: 27000000,
      rentCoverageBps: 3277, coveredByRentAlone: false, residualAnnualSpend: 2420000,
    },
  }),
  ai_settings_get: () => aiSettings,
  ai_settings_save: ((args: { settings: Record<string, unknown> }) => {
    const { exaKey, ...rest } = args.settings;
    aiSettings = {
      ...aiSettings,
      ...rest,
      exaKeySet: exaKey === undefined || exaKey === null ? aiSettings.exaKeySet : exaKey !== "",
    };
  }) as never,
  ai_provider_list: () => [
    { id: 1, provider: "ollama", displayName: "Ollama (local)", model: "qwen3.5-9B", baseUrl: null,
      isEnabled: true, maxOutputTokens: 2000, temperature: 0, systemPrompt: null,
      hasKey: false, isLocal: true, requiresKey: false, defaultBaseUrl: "http://localhost:11434" },
  ],
  ai_chat_preview: (args?: Record<string, unknown>) => {
    const messages = (args?.messages ?? []) as { role: string; content: string }[];
    const payload =
      "FINANCIAL CONTEXT (aggregates from the person's own records):\n" +
      JSON.stringify({ today: "2026-10-05", currency: "EUR",
        cash_flow: { months_covered: 12, income_total: 36240, spending_total: -24910 },
        net_worth: { latest_by_asset_class: [{ class: "Cash", kind: "liquid", net: 43000 }] } }, null, 2) +
      "\n\nCONVERSATION:\n" + messages.map((m) => `${m.role === "user" ? "Person" : "Coin Heaven"}: ${m.content}`).join("\n");
    return { provider: "ollama", model: "qwen3.5-9B", purpose: "nl_query", payload,
      payloadSha256: "c0ffee", bytes: payload.length, rowCount: messages.length,
      redactions: { ibans: 0, cards: 0, emails: 0, phones: 0, accountNumbers: 0 } };
  },
  ai_chat_send: () =>
    "Your savings rate over the last 12 months is about **31%**.\n\nYou took in €36,240 and spent €24,910, so you kept €11,330.\n\n- Housing is your largest cost at roughly 38% of spending.\n- Food comes second.\n\nIf you kept this up, the FIRE page puts you about 10 years and 7 months from your target.",
  ai_autocat_preview: () => ({
    provider: "ollama", model: "qwen3.5-9B", purpose: "categorize",
    payload: JSON.stringify({ country: "Spain", currency: "EUR",
      categories: [{ name: "Leisure & travel", kind: "expense" }],
      merchants_to_file: [{ id: 0, merchant: "restaurante", example: "restaurante el poblet", direction: "money out", typical_amount: 55, times: 12 }] }, null, 2),
    payloadSha256: "beef", bytes: 420, rowCount: 1,
    redactions: { ibans: 0, cards: 0, emails: 0, phones: 0, accountNumbers: 0 },
  }),
  ai_autocat_send: () => [
    { key: "restaurante", categoryId: 7, categoryName: "Leisure & travel", confidenceBps: 8500 },
  ],
  settings_get: () => ({
    primaryCurrency: "EUR",
    locale: "es-ES",
    language: "",
    region: "es",
    country: "Spain",
    modules: {
      import: true, ledger: true, wealth: true, fire: true,
      simulator: true, assistant: true,
    },
  }),
  settings_save: () => undefined,
  category_usage: () => ({ transactions: 38, events: 1, rules: 0 }),
  rule_list: () => [
    { id: 1, name: "Groceries", priority: 50, matchKind: "contains", pattern: "mercadona",
      accountId: null, minAmountMinor: null, maxAmountMinor: null, setCategoryId: 5,
      setProjectId: null, setNeutralTransfer: false, isEnabled: true },
    { id: 2, name: "Internal moves", priority: 10, matchKind: "starts_with", pattern: "traspaso",
      accountId: null, minAmountMinor: null, maxAmountMinor: null, setCategoryId: null,
      setProjectId: null, setNeutralTransfer: true, isEnabled: true },
  ],
  txn_years: () => [2026, 2025],
  cashflow_summary: () => {
    const m = matrix2026();
    return {
      incomeMinor: m.incomeTotal,
      expenseMinor: m.expenseTotal,
      netMinor: m.netTotal,
      savingsRateBps: Math.round((m.netTotal / m.incomeTotal) * 10000),
      transactionCount: 412,
    };
  },
  cashflow_monthly: () => {
    const m = matrix2026();
    return m.periods.slice(0, 9).map((period, i) => ({
      period,
      incomeMinor: m.incomeByPeriod[i]!,
      expenseMinor: m.expenseByPeriod[i]!,
      netMinor: m.netByPeriod[i]!,
    }));
  },
  cashflow_yearly: () => [
    { period: "2025", incomeMinor: 2760000, expenseMinor: -2010000, netMinor: 750000 },
    { period: "2026", incomeMinor: 2880000, expenseMinor: -1840000, netMinor: 1040000 },
  ],
  pl_matrix: matrix2026,
  budget_plan: () => {
    // Today, for the fixtures, is 22 September 2026.
    const cell = (period: string, amount: number | null, own: boolean, actual: number, n: number) => {
      const month = period.length === 7 ? Number(period.slice(5)) : null;
      const days = month === null ? 365 : new Date(2026, month, 0).getDate();
      const elapsed =
        month === null ? 265 : month < 9 ? days : month === 9 ? 22 : 0;
      return {
        period, amountMinor: amount, own, actualMinor: actual,
        remainingMinor: (amount ?? 0) - actual, txnCount: n,
        elapsedBps: Math.round((elapsed / days) * 10000), daysTotal: days, daysLeft: days - elapsed,
      };
    };
    const make = (
      categoryId: number, categoryName: string, kind: string, color: string,
      annual: number | null, own: Record<number, number>, spend: (m: number) => number,
    ) => {
      const written = Object.values(own).reduce((a, b) => a + b, 0);
      const free = 12 - Object.keys(own).length;
      const share = annual !== null && free > 0 ? Math.max(0, Math.round((annual - written) / free)) : null;
      const months = Array.from({ length: 12 }, (_, i) => {
        const actual = i < 9 ? spend(i + 1) : 0;
        return cell(
          `2026-${String(i + 1).padStart(2, "0")}`,
          own[i + 1] ?? share, own[i + 1] !== undefined, actual, actual ? 6 : 0,
        );
      });
      const total = months.reduce((a, m) => a + m.actualMinor, 0);
      return {
        categoryId, categoryName, kind, color,
        monthsSet: Object.keys(own).length,
        monthsSumMinor: months.reduce((a, m) => a + (m.amountMinor ?? 0), 0),
        year: cell("2026", annual ?? written, annual !== null, total, 54),
        months,
      };
    };
    const every = (v: number) => Object.fromEntries(Array.from({ length: 12 }, (_, i) => [i + 1, v]));
    return [
      make(1, "Salary", "income", "#22c55e", 4000000, {}, () => 240000),
      make(4, "Housing", "expense", "#f97316", 1200000, {}, () => 98000),
      make(5, "Food & groceries", "expense", "#eab308", null, every(50000), (m) => 48000 + ((m * 37) % 9) * 2600),
      make(7, "Leisure & travel", "expense", "#a855f7", 360000, { 8: 120000 }, (m) => (m === 8 ? 131000 : 14000 + m * 900)),
    ];
  },
  plan_list: () =>
    [...mockPlans.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([period, items]) => {
        const incomeMinor = items.filter((i) => i.kind === "income").reduce((a, i) => a + i.amountMinor, 0);
        const expenseMinor = items.filter((i) => i.kind === "expense").reduce((a, i) => a + i.amountMinor, 0);
        return {
          period,
          setup: mockSetups.get(period) ?? individual,
          items: items.map((i) => ({ owner: "shared", ...i })),
          incomeMinor,
          expenseMinor,
          netMinor: incomeMinor - expenseMinor,
        };
      }),
  plan_save: ((args: { period: string; items: MockPlanItem[]; setup: Record<string, string> | null }) => {
    mockPlans.set(args.period, args.items);
    if (args.setup) mockSetups.set(args.period, args.setup);
  }) as never,
  plan_delete: ((args: { period: string }) => {
    mockPlans.delete(args.period);
  }) as never,
  region_apply: () => undefined,
  import_impact: () => ({ filename: "movimientos-2026-01.csv", total: 61, touched: 14, linkedFromElsewhere: 1 }),
  market_preview: () => [
    { holdingId: 3, name: "SAP", symbol: "SAP", assetClass: "equities", buysMissing: 26, from: "2024-03-04", source: "Yahoo Finance" },
    { holdingId: 4, name: "Bitcoin", symbol: "BTC", assetClass: "crypto", buysMissing: 11, from: "2025-01-06", source: "Yahoo Finance" },
  ],
  market_fetch: () => [
    { holdingId: 3, name: "SAP", symbolUsed: "SAP.DE", currency: "EUR", stored: 26, leftOut: 0, from: "2024-02-20", to: "2026-10-13", problem: null },
    { holdingId: 4, name: "Bitcoin", symbolUsed: null, currency: null, stored: 0, leftOut: 0, from: null, to: null, problem: "other_currency" },
  ],
  market_clear: () => 26,
  recurring_list: () => ({
    monthlyMinor: 5430,
    yearlyMinor: 65160,
    items: [
      { name: "FIBRELINE INTERNET", categoryName: "Subscriptions", every: "monthly", typicalMinor: 3990, varies: false, count: 14, firstOn: "2025-08-08", lastOn: "2026-09-08", nextOn: "2026-10-08", yearlyMinor: 48545, active: true },
      { name: "FITCLUB GYM", categoryName: "Health", every: "monthly", typicalMinor: 3400, varies: false, count: 14, firstOn: "2025-08-15", lastOn: "2026-09-15", nextOn: "2026-10-15", yearlyMinor: 41366, active: true },
      { name: "ELECTRA ENERGY", categoryName: "Housing", every: "monthly", typicalMinor: 8900, varies: true, count: 14, firstOn: "2025-08-05", lastOn: "2026-09-05", nextOn: "2026-10-05", yearlyMinor: 108283, active: true },
      { name: "SAFEHOME INSURANCE", categoryName: null, every: "yearly", typicalMinor: 18000, varies: false, count: 2, firstOn: "2025-03-15", lastOn: "2026-03-15", nextOn: "2027-03-15", yearlyMinor: 18000, active: true },
      { name: "OLDGYM CLUB", categoryName: "Health", every: "monthly", typicalMinor: 3000, varies: false, count: 3, firstOn: "2026-01-20", lastOn: "2026-03-20", nextOn: "2026-04-19", yearlyMinor: 36500, active: false },
    ],
  }),
  data_export: () => ({
    folder: "~/Documents/Coin Heaven export 2026-10-13",
    files: [
      { name: "transactions.csv", rows: 240 }, { name: "categories.csv", rows: 13 },
      { name: "net_worth.csv", rows: 14 }, { name: "holdings.csv", rows: 2 },
      { name: "holding_movements.csv", rows: 17 }, { name: "holding_prices.csv", rows: 28 },
      { name: "budgets.csv", rows: 3 }, { name: "monthly_plans.csv", rows: 18 },
    ],
  }),
  sample_status: () => ({ loaded: sampleLoaded, canLoad: !sampleLoaded }),
  sample_load: (() => { sampleLoaded = true; }) as never,
  sample_remove: (() => { sampleLoaded = false; }) as never,
  backup_status: () => ({ enabled: true, directory: null, minIntervalSeconds: 900, retainCopies: 10, dirty: false, lastBackupAt: null, lastOutcome: null }),
  backup_set_directory: () => null,
  backup_now: () => "noDirectory",
  update_check: () => ({ current: "1.0.0", latest: "v1.1.0", url: "https://github.com/example/coin-heaven/releases/tag/v1.1.0", newer: true }),
  budget_save: () => undefined,
  budget_save_every_month: () => undefined,
  budget_remove: () => undefined,
  budget_seed_from_year: () => [{ categoryId: 5, amountMinor: 620000 }],
  autocat_suggest: () => ({
    uncategorisedTotal: 61,
    covered: 47,
    groups: [
      { key: "mercadona", txnIds: [11, 12, 13, 14], txnCount: 26, totalMinor: -108450,
        firstOn: "2026-01-04", lastOn: "2026-09-18",
        samples: [
          { id: 11, bookedOn: "2026-09-18", description: "COMPRA TARJETA 5487 MERCADONA VALENCIA", amountMinor: -4520 },
          { id: 12, bookedOn: "2026-09-11", description: "COMPRA TARJETA 5487 MERCADONA VALENCIA", amountMinor: -3810 },
          { id: 13, bookedOn: "2026-09-02", description: "MERCADONA 0091 VALENCIA", amountMinor: -5230 },
        ],
        suggestedCategoryId: 5, suggestedCategoryName: "Food & groceries",
        source: "rule", ruleName: "Groceries", confidenceBps: 10000, evidenceCount: 26 },
      { key: "iberdrola", txnIds: [21, 22], txnCount: 9, totalMinor: -61920,
        firstOn: "2026-01-15", lastOn: "2026-09-15",
        samples: [
          { id: 21, bookedOn: "2026-09-15", description: "RECIBO IBERDROLA CLIENTES SAU", amountMinor: -7180 },
          { id: 22, bookedOn: "2026-08-15", description: "RECIBO IBERDROLA CLIENTES SAU", amountMinor: -6640 },
        ],
        suggestedCategoryId: 4, suggestedCategoryName: "Housing",
        source: "history", ruleName: null, confidenceBps: 8600, evidenceCount: 7 },
      { key: "restaurante", txnIds: [31, 32], txnCount: 12, totalMinor: -38400,
        firstOn: "2026-02-03", lastOn: "2026-09-09",
        samples: [
          { id: 31, bookedOn: "2026-09-09", description: "RESTAURANTE EL POBLET", amountMinor: -4200 },
          { id: 32, bookedOn: "2026-07-21", description: "RESTAURANTE CASA CARMELA", amountMinor: -6800 },
        ],
        suggestedCategoryId: null, suggestedCategoryName: null,
        source: "none", ruleName: null, confidenceBps: 0, evidenceCount: 0 },
    ],
  }),
  autocat_apply: () => ({ categorised: 35, rulesCreated: 1 }),
  asset_class_list: () => [...mockClasses].sort((a, b) => a.sortOrder - b.sortOrder),
  asset_class_save: () => "business",
  asset_class_delete: () => undefined,
  wealth_line_rename: () => 3,
  asset_class_reorder: (a: unknown) => {
    const { orderedKeys } = a as { orderedKeys: string[] };
    orderedKeys.forEach((k, i) => {
      const c = mockClasses.find((x) => x.key === k);
      if (c) c.sortOrder = (i + 1) * 10;
    });
  },
  wealth_item_order: () => ({}),
  wealth_item_reorder: () => undefined,
  holding_reorder: () => undefined,
  fire_params_save: () => undefined,
  fire_ledger_suggestion: () => ({
    from: "2025-09-01", to: "2026-08-31", monthsWithData: 12,
    annualSpending: 2_184_000, annualIncome: 3_356_000, annualSaving: 1_172_000,
    savingsRateBps: 3492,
  }),
  fire_plan: () => {
    const params = { annualSpendTarget: 3_600_000, swrBps: 400, annualNetRentalIncome: 1_180_000 };
    const planParams = {
      annualSavings: 2_400_000, realReturnBps: 500,
      currentAge: 38, targetAge: 45, horizonYears: 40,
      withdrawalTax: true, gainShareBps: 5000, pensionAge: 67,
      annualPension: 1_200_000, bandBps: 200,
      taxBrackets: [
        { upTo: 600_000, rateBps: 1_900 }, { upTo: 5_000_000, rateBps: 2_100 },
        { upTo: 20_000_000, rateBps: 2_300 }, { upTo: 30_000_000, rateBps: 2_700 },
        { upTo: null, rateBps: 3_000 },
      ],
    };
    const balances = {
      liquid: 18_450_000, realEstateEquity: 8_550_000, otherAssets: 0, otherDebt: 0,
    };
    const residual = params.annualSpendTarget - params.annualNetRentalIncome;
    const target = Math.round((residual * 10_000) / params.swrBps);
    const grow = (v: number, r = 1.05) => Math.round(v * r) + planParams.annualSavings;
    const path = [];
    let [liquid, lo, hi] = [balances.liquid, balances.liquid, balances.liquid];
    for (let year = 0; year <= planParams.horizonYears; year++) {
      const t = Math.round(target * (1.06 - Math.min(year, 29) * 0.004));
      path.push({ year, age: planParams.currentAge + year, liquid, liquidLow: lo, liquidHigh: hi, target: t });
      liquid = grow(liquid);
      lo = grow(lo, 1.03);
      hi = grow(hi, 1.07);
    }
    const yearsToTarget = path.findIndex((p) => p.liquid >= target);
    return {
      params, planParams, balances, hasSnapshot: true, planIsNew: false, paramsIsNew: false,
      result: {
        liquidTarget: target, liquidAssets: balances.liquid,
        liquidShortfall: Math.max(0, target - balances.liquid),
        liquidProgressBps: Math.round((balances.liquid / target) * 10_000),
        liquidCovered: balances.liquid >= target,
        totalNetWorth: balances.liquid + balances.realEstateEquity,
        rentCoverageBps: Math.round(
          (params.annualNetRentalIncome / params.annualSpendTarget) * 10_000,
        ),
        coveredByRentAlone: false, residualAnnualSpend: residual,
      },
      plan: {
        path,
        yearsToTarget: yearsToTarget < 0 ? null : yearsToTarget - 0.4,
        yearsLow: 12.9, yearsHigh: 9.1,
        targetNow: Math.round(target * 1.06),
        grossWithdrawal: Math.round(residual * 1.06),
        withdrawalTax: Math.round(residual * 0.06),
        milestones: [
          { shareBps: 2500, years: 0, age: 38 },
          { shareBps: 5000, years: 4.2, age: 43 },
          { shareBps: 7500, years: 7.6, age: 46 },
          { shareBps: 10000, years: 10.6, age: 49 },
        ],
        levers: [
          { kind: "save", monthly: 10000, monthsEarlier: 4 },
          { kind: "spend", monthly: 10000, monthsEarlier: 9 },
          { kind: "save", monthly: 30000, monthsEarlier: 11 },
          { kind: "spend", monthly: 30000, monthsEarlier: 25 },
        ],
        ageAtTarget: yearsToTarget < 0 ? null : planParams.currentAge + yearsToTarget,
        coastNumber: Math.round(target / 1.05 ** 22),
        coasting: false,
        goal: {
          age: 45, yearsAvailable: 7, onTrack: false, spareYears: null,
          liquidAtGoal: 45_500_000, targetAtGoal: 62_300_000,
          extraSavingMonthly: 143_000, spendCutMonthly: 61_000,
        },
        sensitivity: [300, 350, 400, 450, 500].map((swrBps) => ({
          swrBps,
          target: Math.round((residual * 10_000) / swrBps),
          years: swrBps === 300 ? 11.8 : swrBps === 350 ? 9.9 : swrBps === 400 ? 8.3 : swrBps === 450 ? 7.0 : 5.9,
        })),
      },
    };
  },
  portfolio_view: () => ({
    asOf: "2026-09-27", checkpoint: "2026-06-30",
    value: 3_214_550, invested: 2_635_000, gain: 579_550, xirrBps: 1184, unpriced: [],
    holdings: [
      { id: 1, name: "Bitcoin", symbol: "BTC", assetClass: "crypto", unitsE8: 2_000_000,
        unitsComplete: true, unitsEstimated: true, priceMicro: 93_210_550_000, priceDate: "2026-09-27",
        priceSource: "assistant", priceUrl: "https://www.coindesk.com/price/bitcoin",
        value: 186_421, invested: 185_000, gain: 1_421, xirrBps: 210, txnCount: 37,
        schedules: [{ id: 1, holdingId: 1, amountMinor: 5_000, everyDays: 15, dayOfMonth: null,
          startDate: "2025-01-01", endDate: null }] },
      { id: 2, name: "Vanguard FTSE All-World", symbol: "IE00BK5BQT80", assetClass: "equities",
        unitsE8: 22_000_000_000, unitsComplete: true, unitsEstimated: false, priceMicro: 137_720_000, priceDate: "2026-06-30",
        priceSource: "manual", priceUrl: null, value: 3_028_129, invested: 2_450_000,
        gain: 578_129, xirrBps: 1290, txnCount: 24, schedules: [] },
    ],
  }),
  portfolio_gain_share: () => 1803,
  holding_txns: () => [
    { id: 3, onDate: "2026-06-30", kind: "units", amountMinor: 0, unitsE8: 2_000_000, scheduled: false, skipped: false },
    { id: 2, onDate: "2026-06-16", kind: "buy", amountMinor: 5_000, unitsE8: null, scheduled: true, skipped: false },
    { id: 1, onDate: "2026-06-01", kind: "buy", amountMinor: 5_000, unitsE8: null, scheduled: true, skipped: true },
  ],
  holding_save: () => 3, holding_delete: () => undefined, holding_txn_save: () => 4,
  holding_txn_delete: () => undefined, schedule_save: () => 2, schedule_delete: () => undefined,
  prices_save: () => 2,
  portfolio_history_view: () => {
    // Twenty-one months of regular buying with a market that wobbles.
    const out = [];
    let invested = 0;
    for (let m = 0; m <= 20; m++) {
      invested += m === 0 ? 600_000 : 101_750;
      const drift = 1 + m * 0.011 + Math.sin(m / 2.2) * 0.045;
      const d = new Date(Date.UTC(2025, m + 1, 0));
      out.push({ date: d.toISOString().slice(0, 10), invested, value: Math.round(invested * drift) });
    }
    out.push({ date: "2026-09-27", invested: 2_635_000, value: 3_214_550 });
    return out;
  },
  ai_price_preview: () => ({
    provider: "anthropic", model: "claude-opus-5", purpose: "price_check",
    payload: "Today is 2026-09-27. Find the current price in EUR ...\n- id 1: Bitcoin (BTC)\n- id 2: Vanguard FTSE All-World (IE00BK5BQT80)",
    payloadSha256: "ab12", bytes: 310, rowCount: 2,
    redactions: { ibans: 0, cards: 0, emails: 0, phones: 0, accountNumbers: 0 },
  }),
  ai_price_send: () => [
    { holdingId: 1, name: "Bitcoin", priceMicro: 93_210_550_000, priceDate: "2026-10-05", historical: false,
      sourceUrl: "https://www.coindesk.com/price/bitcoin", sourceSeen: true, evidence: "BTC is trading at 93,210.55 EUR", note: null },
    { holdingId: 2, name: "Vanguard FTSE All-World", priceMicro: 137_720_000, priceDate: "2026-10-02", historical: false,
      sourceUrl: "https://made-up.example/vwce", sourceSeen: false, note: "Closing price of the previous trading day" },
    ...["2025-01-31", "2025-02-28", "2025-03-31", "2025-04-30"].map((d, i) => ({
      holdingId: 1, name: "Bitcoin", priceMicro: (88_000 + i * 2_400) * 1_000_000, priceDate: d,
      historical: true, sourceUrl: "https://www.coindesk.com/price/bitcoin", sourceSeen: true, note: null,
    })),
  ],
  ai_egress_log: () => [],
  passphrase_strength: () => ({
    score: 3,
    acceptable: true,
    message: "Good.",
  }),
  setup_database: () => "opened",
  account_list: () => [
    { id: 1, name: "Santander corriente", institution: "Santander", kind: "checking", currency: "EUR", txnCount: 412 },
    { id: 2, name: "Ahorro", institution: null, kind: "savings", currency: "EUR", txnCount: 38 },
  ],
  mapping_list: () => [
    { id: 1, name: "Santander CSV", delimiter: ";", encoding: "windows-1252", hasHeader: true,
      skipRows: 0, dateCol: 0, dateFormat: "%d/%m/%Y", descriptionCol: 1, amountCol: 2,
      counterpartyCol: null, decimalSeparator: ",",
      thousandsSeparator: ".", invertSign: false, currency: "EUR" },
  ],
  date_formats: () => [
    { pattern: "%Y-%m-%d", example: "2026-03-04", label: "Year-month-day", group: "ISO", hasTime: false },
    { pattern: "%d/%m/%Y", example: "04/03/2026", label: "Day/month/year", group: "Europe", hasTime: false },
    { pattern: "%m/%d/%Y", example: "03/04/2026", label: "Month/day/year", group: "Americas", hasTime: false },
    { pattern: "%Y-%m-%d %H:%M:%S", example: "2026-03-04 14:30:00", label: "Year-month-day + time", group: "With a time", hasTime: true },
  ],
  import_inspect: () => ({
    detection: {
      mapping: {
        name: "Detected · Fecha/Concepto/Importe", delimiter: ";", encoding: "windows-1252",
        hasHeader: true, skipRows: 2, dateCol: 0, dateFormat: "%d/%m/%Y", descriptionCol: 1,
        amountCol: 2, counterpartyCol: null,
        decimalSeparator: ",", thousandsSeparator: ".", invertSign: false, currency: "EUR",
      },
      confidence: 92,
      notes: [
        "Header found on line 3.",
        "Skipped 2 line(s) of preamble above the data.",
        "Dates read as %d/%m/%Y.",
        "Single signed amount column.",
        "Ignored a running-balance column.",
      ],
      warnings: [],
      headerRow: 2,
      firstDataRow: 3,
    },
    headers: ["Fecha", "Concepto", "Importe", "Saldo"],
    sampleLines: [],
    totalRows: 118,
    needsConfirmation: false,
    source: { delimited: { delimiter: ";", encoding: "windows-1252" } },
    rejected: [{ line: 61, reason: "date 'TOTAL' does not match %d/%m/%Y", raw: "" }],
    rows: [
      { line: 4, bookedOn: "2026-01-05", amountMinor: -4520, description: "MERCADONA VALENCIA", counterparty: null },
      { line: 5, bookedOn: "2026-01-07", amountMinor: 240000, description: "NOMINA ENERO", counterparty: null },
      { line: 6, bookedOn: "2026-01-09", amountMinor: -8000, description: "IBERDROLA LUZ", counterparty: null },
      { line: 7, bookedOn: "2026-01-12", amountMinor: -3800, description: "TRATTORIA DA LUIGI", counterparty: null },
    ],
  }),
  import_batches: () => [
    { id: 3, accountName: "Santander corriente", sourceFilename: "movimientos-2026-01.csv",
      importedAt: "2026-02-01T09:12:00Z", insertedCount: 118, duplicateCount: 24, rejectedCount: 2 },
    { id: 2, accountName: "Ahorro", sourceFilename: "ahorro-2025.csv",
      importedAt: "2026-01-04T18:40:00Z", insertedCount: 38, duplicateCount: 0, rejectedCount: 0 },
  ],
};

export function mockInvoke<T>(cmd: string, args?: Record<string, unknown>): T {
  if (cmd === "sim_run") {
    return fakeRun((args?.params as SimParams) ?? mockParams) as T;
  }
  const fn = mockResponses[cmd];
  if (!fn) throw new Error(`no dev fixture for "${cmd}"`);
  return (fn as (a?: unknown) => unknown)(args) as T;
}
