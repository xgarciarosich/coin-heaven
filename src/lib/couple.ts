import type { PlanItem, PlanSetup } from "@/lib/types";

export interface PersonShare {
  /** Brought in by this person alone. */
  income: number;
  /** Paid by this person alone. */
  own: number;
  /** This person's part of the shared spending. */
  sharedSpend: number;
  /** This person's part of the shared income, if any. */
  sharedIncome: number;
  /** What is left for this person at the end of the month. */
  left: number;
}

export interface CoupleSplit {
  /** Person A's share of what is shared, in basis points (5000 = half). */
  shareABps: number;
  sharedSpend: number;
  sharedIncome: number;
  a: PersonShare;
  b: PersonShare;
}

/**
 * How a month's plan divides between two people.
 *
 * Each box is one person's or shared. Shared boxes are divided half each, in
 * proportion to what each brings in alone (if neither brings in anything, half
 * each), or in a proportion the couple sets themselves. The two parts of a shared amount always add up to it exactly,
 * so the two "left" figures add up to the household's.
 */
export function splitPlan(items: readonly PlanItem[], setup: PlanSetup): CoupleSplit {
  const sum = (owner: PlanItem["owner"], kind: PlanItem["kind"]) =>
    items
      .filter((i) => (i.owner ?? "shared") === owner && i.kind === kind)
      .reduce((a, i) => a + i.amountMinor, 0);
  const incomeA = sum("a", "income");
  const incomeB = sum("b", "income");
  const sharedIncome = sum("shared", "income");
  const sharedSpend = sum("shared", "expense");
  const shareABps =
    setup.split === "custom"
      ? Math.min(10_000, Math.max(0, Math.round(setup.customABps)))
      : setup.split === "income" && incomeA + incomeB > 0
        ? Math.round((incomeA * 10_000) / (incomeA + incomeB))
        : 5_000;
  const part = (total: number) => {
    const a = Math.round((total * shareABps) / 10_000);
    return [a, total - a] as const;
  };
  const [spendA, spendB] = part(sharedSpend);
  const [inA, inB] = part(sharedIncome);
  const person = (income: number, own: number, s: number, si: number): PersonShare => ({
    income,
    own,
    sharedSpend: s,
    sharedIncome: si,
    left: income + si - own - s,
  });
  return {
    shareABps,
    sharedSpend,
    sharedIncome,
    a: person(incomeA, sum("a", "expense"), spendA, inA),
    b: person(incomeB, sum("b", "expense"), spendB, inB),
  };
}
