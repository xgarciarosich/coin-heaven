import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import {
  Area,
  AreaChart,
  CartesianGrid,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { Check, Sparkles } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import {
  Banner,
  Checkbox,
  Disclosure,
  PageHead,
  Segmented,
  SelectField,
  TextField,
} from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import {
  fmtAxis,
  fmtBps,
  fmtCompact,
  fmtDecimal,
  fmtMajor,
  fmtMoney,
  majorToMinor,
  minorToMajor,
  sym,
} from "@/lib/money";
import type { FireParams, FirePlanView, LedgerSuggestion, PlanParams } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { REGIONS, region } from "@/lib/region";
import type { Dict } from "@/locales/en";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
const euros = fmtMajor;

/** Real returns, above inflation: the three moods the markets come in. */
const MOODS = [300, 500, 700] as const;

/**
 * Module 3 — financial independence.
 *
 * The question is not "how much do I have", which the net-worth module already
 * answers. It is "when", and "what would change that". So the page opens with
 * the answer as a sentence, shows how far along the road is, and then puts
 * the three things a person actually controls — what they spend, what they
 * put aside, and how hopeful they are about markets — where they can be moved
 * and the answer watched as it moves.
 *
 * Everything else a careful reader might want (tax, pension, rent, withdrawal
 * rates) is one click below, each section saying what it is currently set to
 * so it can stay closed.
 *
 * Everything is in today's money at a real rate of return. That is not a
 * simplification — a 4% safe withdrawal rate is already inflation-adjusted, so
 * pairing it with a nominal return would quietly shorten every answer here by
 * years.
 */
export function FireView() {
  const t = useT();
  const f = t.fire;
  const [view, setView] = useState<FirePlanView | null>(null);
  const [params, setParams] = useState<FireParams | null>(null);
  const [plan, setPlan] = useState<PlanParams | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [ledger, setLedger] = useState<LedgerSuggestion | null>(null);
  const [gainShare, setGainShare] = useState<number | null>(null);
  // Nothing is written until the person has moved something themselves.
  const touched = useRef(false);

  useEffect(() => {
    api
      .fireLedgerSuggestion()
      .then(setLedger)
      .catch(() => setLedger(null));
    api
      .portfolioGainShare()
      .then(setGainShare)
      .catch(() => setGainShare(null));
  }, []);

  const load = useCallback(() => {
    api
      .firePlan(params, plan)
      .then((v) => {
        setView(v);
        // Nothing saved yet starts from the country the person chose — its
        // withdrawal rate, its pension age, its tax bands — not from the ones
        // the app happens to ship with.
        const r = region().fire;
        setParams((cur) =>
          cur ?? (v.paramsIsNew ? { ...v.params, swrBps: r.swrBps } : v.params),
        );
        setPlan((cur) => {
          if (cur) return cur;
          const base = v.planIsNew && r.taxBrackets.length > 0
            ? { ...v.planParams, taxBrackets: r.taxBrackets }
            : v.planParams;
          // An empty age used to be shown as if it were the country's, while
          // the calculation had no age to build a pension bridge from.
          return { ...base, pensionAge: base.pensionAge ?? r.pensionAge };
        });
        setError(null);
      })
      .catch((e: unknown) => setError(msg(e)));
  }, [params, plan]);

  useEffect(() => {
    const t = setTimeout(load, 140);
    return () => clearTimeout(t);
  }, [load]);

  // Saved on its own, a moment after the last change: a plan someone has to
  // remember to save is a plan that is lost the first time they forget.
  useEffect(() => {
    if (!touched.current || !params || !plan) return;
    const timer = setTimeout(() => {
      api
        .fireParamsSave(params, plan)
        .then(() => setSaved(true))
        .catch((e: unknown) => setError(msg(e)));
    }, 900);
    return () => clearTimeout(timer);
  }, [params, plan]);

  const editParams = (patch: Partial<FireParams>) => {
    touched.current = true;
    setSaved(false);
    setParams((cur) => (cur ? { ...cur, ...patch } : cur));
  };
  const editPlan = (patch: Partial<PlanParams>) => {
    touched.current = true;
    setSaved(false);
    setPlan((cur) => (cur ? { ...cur, ...patch } : cur));
  };

  const r = view?.result;
  const p = view?.plan;

  // Plotted to a few years past the target, not to the end of the horizon.
  // Forty years of compounding runs to several times the target, and on that
  // scale the target line — the only thing the chart exists to show a crossing
  // of — is pressed flat against the axis.
  const chart = useMemo(() => {
    const path = p?.path ?? [];
    const cut =
      p?.yearsToTarget != null
        ? Math.min(path.length, Math.ceil(p.yearsToTarget) + 6)
        : path.length;
    return path.slice(0, cut).map((pt) => ({
      label: pt.age != null ? `${pt.age}` : `+${pt.year}`,
      year: pt.year,
      portfolio: minorToMajor(pt.liquid),
      // A range area: Recharts draws between the two values of the pair.
      band: [minorToMajor(pt.liquidLow), minorToMajor(pt.liquidHigh)],
      target: minorToMajor(pt.target),
    }));
  }, [p]);

  const progress =
    r && p && p.targetNow > 0 ? Math.min(1, Math.max(0, r.liquidAssets / p.targetNow)) : 0;

  return (
    <div className="min-w-0">
      <PageHead
        title={headline(f, p?.yearsToTarget ?? null, r?.liquidCovered ?? false)}
        lede={
          r && p
            ? [
                p.ageAtTarget !== null && p.yearsToTarget ? f.atAge(p.ageAtTarget) : null,
                p.yearsLow !== null && p.yearsHigh !== null && p.yearsToTarget
                  ? f.range(fmtDecimal(p.yearsHigh), fmtDecimal(p.yearsLow))
                  : null,
                p.yearsToTarget === null ? f.nothingWithin(plan?.horizonYears ?? 40) : null,
                f.todaysMoney,
              ]
                .filter(Boolean)
                .join(" ")
            : f.ledeEmpty
        }
        actions={
          saved && (
            <span className="flex items-center gap-1.5 pb-1.5 text-[11.5px] text-ink-3">
              <Check className="size-3.5 text-positive" strokeWidth={2} />
              {f.saved}
            </span>
          )
        }
      />

      <div className="space-y-5 px-10 pb-12">
        {error && <Banner tone="bad">{error}</Banner>}
        {view && !view.hasSnapshot && <Banner>{f.noSnapshot}</Banner>}
        <p className="text-[11.5px] leading-[1.6] text-ink-3">{f.notAdvice}</p>

        {r && p && params && plan && (
          <>
            {/* ------------------------------------------- how far along -- */}
            <Panel>
              <PanelBody className="space-y-4 py-5">
                <div className="flex items-end justify-between gap-6">
                  <div>
                    <p className="eyebrow">{f.have}</p>
                    <p className="tnum mt-2 text-[26px] leading-none text-ink">
                      {fmtMoney(r.liquidAssets)}
                    </p>
                  </div>
                  <p className="tnum pb-0.5 text-[13px] text-brass">
                    {fmtDecimal(progress * 100)}%
                  </p>
                  <div className="text-right">
                    <p className="eyebrow">{f.need}</p>
                    <p className="tnum mt-2 text-[26px] leading-none text-ink">
                      {fmtMoney(p.targetNow)}
                    </p>
                  </div>
                </div>

                <div>
                  <div className="relative h-2.5 rounded-full bg-surface-hi">
                    <div
                      className="h-full rounded-full bg-brass transition-[width] duration-300"
                      style={{ width: `${progress * 100}%` }}
                    />
                    {[25, 50, 75].map((x) => (
                      <span
                        key={x}
                        className="absolute top-0 h-full w-px bg-paper"
                        style={{ left: `${x}%` }}
                      />
                    ))}
                  </div>
                  <div className="mt-2 grid grid-cols-4">
                    {p.milestones.map((m) => {
                      const done = m.years === 0;
                      return (
                        <div key={m.shareBps} className="text-right">
                          <p className={cn("text-[11px]", done ? "text-brass" : "text-ink-3")}>
                            {f.milestoneLabel(m.shareBps / 100)}
                          </p>
                          <p className={cn("tnum text-[11.5px]", done ? "text-ink-3" : "text-ink-2")}>
                            {m.years === null
                              ? "—"
                              : done
                                ? f.reached
                                : f.milestoneAt(fmtDecimal(m.years), m.age)}
                          </p>
                        </div>
                      );
                    })}
                  </div>
                </div>

                <p className="text-[12px] leading-[1.6] text-ink-3">
                  {r.coveredByRentAlone
                    ? f.rentCoversAll
                    : p.withdrawalTax > 0
                      ? f.needNoteTax(
                          fmtMoney(p.grossWithdrawal),
                          fmtMoney(p.withdrawalTax),
                          fmtBps(params.swrBps, 2),
                        )
                      : f.needNote(fmtMoney(r.residualAnnualSpend), fmtBps(params.swrBps, 2))}
                </p>
              </PanelBody>
            </Panel>

            {/* ------------------------------------- the plan, the path -- */}
            <div className="grid grid-cols-1 gap-5 xl:grid-cols-[340px_minmax(0,1fr)]">
              <Panel>
                <PanelHead label={f.yourPlan} />
                <PanelBody className="space-y-6">
                  <Dial
                    label={f.spendMonthly}
                    unit={f.perMonth}
                    value={Math.round(minorToMajor(params.annualSpendTarget) / 12)}
                    min={500}
                    max={8000}
                    step={50}
                    onChange={(v) => editParams({ annualSpendTarget: majorToMinor(v * 12) })}
                  >
                    {ledger && ledger.monthsWithData > 0 && (
                      <UseMine
                        label={f.fromLedger(fmtMoney(Math.round(ledger.annualSpending / 12)))}
                        note={f.ledgerWindow(ledger.monthsWithData)}
                        onClick={() => editParams({ annualSpendTarget: ledger.annualSpending })}
                      />
                    )}
                  </Dial>

                  <Dial
                    label={f.saveMonthly}
                    unit={f.perMonth}
                    value={Math.round(minorToMajor(plan.annualSavings) / 12)}
                    min={0}
                    max={6000}
                    step={50}
                    onChange={(v) => editPlan({ annualSavings: majorToMinor(v * 12) })}
                  >
                    {ledger && ledger.monthsWithData > 0 && ledger.annualSaving > 0 && (
                      <UseMine
                        label={f.fromLedger(fmtMoney(Math.round(ledger.annualSaving / 12)))}
                        note={
                          ledger.savingsRateBps !== null
                            ? f.savingsRate(fmtBps(ledger.savingsRateBps, 0))
                            : ""
                        }
                        onClick={() => editPlan({ annualSavings: ledger.annualSaving })}
                      />
                    )}
                  </Dial>

                  <div className="space-y-2">
                    <div className="flex items-baseline justify-between">
                      <span className="text-[12.5px] text-ink-2">{f.markets}</span>
                      <span className="flex items-baseline gap-1 text-[12px] text-ink-3">
                        <input
                          type="number"
                          step="0.5"
                          aria-label={f.markets}
                          value={plan.realReturnBps / 100}
                          onChange={(e) =>
                            editPlan({
                              realReturnBps: Math.round(Number(e.currentTarget.value) * 100),
                            })
                          }
                          className="tnum w-12 border-b border-transparent bg-transparent text-right text-ink outline-none hover:border-rule focus:border-brass"
                        />
                        {f.aboveInflation}
                      </span>
                    </div>
                    <Segmented
                      options={[
                        [MOODS[0], f.moods.cautious],
                        [MOODS[1], f.moods.balanced],
                        [MOODS[2], f.moods.hopeful],
                      ]}
                      value={MOODS.find((m) => m === plan.realReturnBps) ?? null}
                      onChange={(v) => editPlan({ realReturnBps: v })}
                    />
                    <p className="text-[11px] leading-[1.5] text-ink-3">{f.marketsHint}</p>
                  </div>

                  <div className="flex items-center justify-between gap-3 border-t border-rule pt-4">
                    <span className="text-[12.5px] text-ink-2">{f.ageNow}</span>
                    <input
                      type="number"
                      min={16}
                      max={90}
                      placeholder="—"
                      value={plan.currentAge ?? ""}
                      onChange={(e) =>
                        editPlan({
                          currentAge: e.currentTarget.value ? Number(e.currentTarget.value) : null,
                        })
                      }
                      className="tnum w-16 rounded-md border border-rule bg-surface px-2 py-1 text-right text-[13px] text-ink outline-none focus:border-brass"
                    />
                  </div>
                  {plan.currentAge === null && (
                    <p className="-mt-3 text-[11px] text-ink-3">{f.addAge}</p>
                  )}

                  <div className="-mt-2 flex items-center justify-between gap-3">
                    <span className="text-[12.5px] text-ink-2">{f.goalAge}</span>
                    <input
                      type="number"
                      min={18}
                      max={100}
                      placeholder="—"
                      value={plan.targetAge ?? ""}
                      onChange={(e) =>
                        editPlan({
                          targetAge: e.currentTarget.value ? Number(e.currentTarget.value) : null,
                        })
                      }
                      className="tnum w-16 rounded-md border border-rule bg-surface px-2 py-1 text-right text-[13px] text-ink outline-none focus:border-brass"
                    />
                  </div>
                  {p.goal && (
                    <GoalVerdict
                      goal={p.goal}
                      ageAtTarget={p.ageAtTarget}
                      onSave={(m) => editPlan({ annualSavings: plan.annualSavings + m * 12 })}
                      onSpend={(m) =>
                        editParams({
                          annualSpendTarget: Math.max(0, params.annualSpendTarget - m * 12),
                        })
                      }
                      onAge={(a) => editPlan({ targetAge: a })}
                    />
                  )}
                </PanelBody>
              </Panel>

              <Panel className="flex flex-col">
                <PanelHead
                  label={f.path}
                  aside={
                    <div className="flex gap-4">
                      <Key swatch="var(--brass)" label={f.portfolio} />
                      <Key swatch="var(--ink-3)" label={f.target} line />
                    </div>
                  }
                />
                {/* The plan beside it grows when it has a verdict to show; the
                    chart takes whatever height that leaves rather than
                    sitting at the top of an empty panel. */}
                <PanelBody className="flex flex-1 flex-col pt-5 pr-3 pb-2 pl-1">
                  <div className="relative min-h-[370px] w-full flex-1">
                    <div className="absolute inset-0">
                    <ResponsiveContainer width="100%" height="100%">
                      <AreaChart data={chart} margin={{ top: 4, right: 8, bottom: 0, left: 4 }}>
                        <defs>
                          <linearGradient id="fire-fill" x1="0" y1="0" x2="0" y2="1">
                            <stop offset="0%" stopColor="var(--brass)" stopOpacity={0.32} />
                            <stop offset="100%" stopColor="var(--brass)" stopOpacity={0.04} />
                          </linearGradient>
                        </defs>
                        <CartesianGrid vertical={false} stroke="var(--rule)" />
                        <XAxis
                          dataKey="label"
                          axisLine={false}
                          tickLine={false}
                          tickMargin={10}
                          interval="preserveStartEnd"
                          minTickGap={28}
                        />
                        <YAxis
                          axisLine={false}
                          tickLine={false}
                          width={50}
                          tickMargin={6}
                          tickFormatter={fmtAxis}
                        />
                        <Tooltip
                          contentStyle={{
                            background: "var(--surface)",
                            border: "1px solid var(--rule)",
                            borderRadius: 8,
                            fontSize: 12,
                          }}
                          labelFormatter={(l) =>
                            plan.currentAge != null ? f.age(String(l)) : f.year(String(l))
                          }
                          // The band is a pair of values, not one: formatting
                          // it as a single number is how "€NaN" got here.
                          formatter={(v) =>
                            Array.isArray(v)
                              ? `${euros(Number(v[0]))} – ${euros(Number(v[1]))}`
                              : euros(Number(v))
                          }
                        />
                        {p.yearsToTarget !== null && p.yearsToTarget > 0 && (
                          <ReferenceLine
                            x={chart[Math.round(p.yearsToTarget)]?.label ?? ""}
                            stroke="var(--positive)"
                            strokeDasharray="3 3"
                            label={{
                              value: f.targetMarker,
                              position: "top",
                              fill: "var(--positive)",
                              fontSize: 11,
                            }}
                          />
                        )}
                        <Area
                          type="monotone"
                          dataKey="band"
                          name={f.band}
                          stroke="none"
                          fill="var(--brass)"
                          fillOpacity={0.1}
                          activeDot={false}
                        />
                        <Area
                          type="monotone"
                          dataKey="portfolio"
                          name={f.portfolio}
                          stroke="var(--brass)"
                          strokeWidth={1.75}
                          fill="url(#fire-fill)"
                        />
                        <Area
                          type="monotone"
                          dataKey="target"
                          name={f.target}
                          stroke="var(--ink-3)"
                          strokeWidth={1.25}
                          strokeDasharray="4 3"
                          fill="none"
                        />
                      </AreaChart>
                    </ResponsiveContainer>
                    </div>
                  </div>
                  <p className="px-9 pt-2 pb-1 text-[11px] leading-[1.6] text-ink-3">
                    {plan.pensionAge && plan.annualPension > 0 ? f.pathNotePension : f.pathNote}
                  </p>
                </PanelBody>
              </Panel>
            </div>

            {/* ----------------------------------- what would move the date -- */}
            <div>
              <p className="eyebrow mb-2.5 flex items-center gap-1.5">
                <Sparkles className="size-3 text-brass" strokeWidth={1.75} />
                {f.levers}
              </p>
              <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
                {p.levers.map((l) => (
                  <button
                    key={`${l.kind}-${l.monthly}`}
                    type="button"
                    title={f.tryIt}
                    disabled={!l.monthsEarlier || l.monthsEarlier <= 0}
                    // Trying one is the point: the card applies itself.
                    onClick={() =>
                      l.kind === "save"
                        ? editPlan({ annualSavings: plan.annualSavings + l.monthly * 12 })
                        : editParams({
                            annualSpendTarget: Math.max(0, params.annualSpendTarget - l.monthly * 12),
                          })
                    }
                    className="rounded-[10px] border border-rule bg-surface px-4 py-3.5 text-left shadow-card transition-colors hover:border-brass disabled:pointer-events-none disabled:opacity-60"
                  >
                    <p className="text-[12px] leading-[1.45] text-ink-2">
                      {l.kind === "save"
                        ? f.leverSave(fmtMoney(l.monthly))
                        : f.leverSpend(fmtMoney(l.monthly))}
                    </p>
                    <p className="tnum mt-2 text-[17px] leading-none text-positive">
                      {l.monthsEarlier === null
                        ? "—"
                        : l.monthsEarlier <= 0
                          ? f.noChange
                          : f.monthsEarlier(l.monthsEarlier)}
                    </p>
                  </button>
                ))}
              </div>
              <p className="mt-2 text-[11px] text-ink-3">{f.leversNote}</p>
            </div>

            {/* ------------------------------------------------ the detail -- */}
            <div className="space-y-3">
              <Disclosure
                title={f.otherIncome}
                summary={
                  [
                    params.annualNetRentalIncome > 0
                      ? f.sumRent(fmtCompact(params.annualNetRentalIncome))
                      : null,
                    plan.pensionAge && plan.annualPension > 0
                      ? f.sumPension(fmtCompact(plan.annualPension), plan.pensionAge)
                      : null,
                  ]
                    .filter(Boolean)
                    .join(" · ") || f.sumNone
                }
              >
                <PanelBody className="space-y-4">
                  <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
                    <TextField
                      label={f.rent}
                      type="number"
                      value={minorToMajor(params.annualNetRentalIncome)}
                      onChange={(e) =>
                        editParams({
                          annualNetRentalIncome: majorToMinor(Number(e.currentTarget.value)),
                        })
                      }
                      hint={f.rentHint}
                    />
                    <TextField
                      label={f.pension}
                      type="number"
                      value={minorToMajor(plan.annualPension)}
                      onChange={(e) =>
                        editPlan({ annualPension: majorToMinor(Number(e.currentTarget.value)) })
                      }
                      hint={f.pensionHint}
                    />
                    <TextField
                      label={f.pensionAge}
                      type="number"
                      value={plan.pensionAge ?? ""}
                      placeholder={String(region().fire.pensionAge)}
                      onChange={(e) =>
                        editPlan({
                          // Emptied, it goes back to the country's age: a
                          // pension with no age has no bridge to compute.
                          pensionAge: e.currentTarget.value
                            ? Number(e.currentTarget.value)
                            : region().fire.pensionAge,
                        })
                      }
                    />
                  </div>
                  {params.annualNetRentalIncome > 0 && (
                    <div className="space-y-2">
                      <div className="flex h-2 overflow-hidden rounded-full bg-surface-hi">
                        <div
                          className="bg-brass"
                          style={{ width: `${Math.min(100, r.rentCoverageBps / 100)}%` }}
                        />
                      </div>
                      <p className="text-[12px] leading-[1.6] text-ink-2">
                        {f.rentBody(fmtBps(r.rentCoverageBps, 0), fmtMoney(r.residualAnnualSpend))}
                      </p>
                    </div>
                  )}
                  <p className="text-[11.5px] text-ink-3">{f.totalWealth(fmtMoney(r.totalNetWorth))}</p>
                </PanelBody>
              </Disclosure>

              <Disclosure
                title={f.tax}
                summary={
                  plan.withdrawalTax
                    ? f.sumTax(fmtMoney(p.withdrawalTax))
                    : f.sumTaxOff
                }
              >
                <PanelBody className="space-y-3">
                  <Checkbox
                    label={f.taxToggle}
                    checked={plan.withdrawalTax}
                    onChange={(e) => editPlan({ withdrawalTax: e.currentTarget.checked })}
                  />
                  <p className="text-[11.5px] leading-[1.6] text-ink-3">{f.taxHint}</p>
                  <div className="flex flex-wrap items-end gap-4">
                    <TextField
                      label={f.gainShare}
                      type="number"
                      className="w-52"
                      disabled={!plan.withdrawalTax}
                      value={plan.gainShareBps / 100}
                      onChange={(e) =>
                        editPlan({ gainShareBps: Math.round(Number(e.currentTarget.value) * 100) })
                      }
                      hint={f.gainShareHint}
                    />
                    {gainShare !== null && plan.withdrawalTax && (
                      <div className="pb-5">
                        <UseMine
                          label={f.fromPortfolio(fmtBps(gainShare, 0))}
                          note=""
                          onClick={() => editPlan({ gainShareBps: gainShare })}
                        />
                      </div>
                    )}
                  </div>
                  {plan.withdrawalTax && (
                    <Brackets
                      brackets={plan.taxBrackets}
                      onChange={(taxBrackets) => editPlan({ taxBrackets })}
                    />
                  )}
                </PanelBody>
              </Disclosure>

              <Disclosure
                title={f.careful}
                summary={f.sumCareful(fmtBps(params.swrBps, 2), fmtBps(plan.bandBps, 1))}
              >
                <PanelBody className="grid grid-cols-1 gap-8 lg:grid-cols-2">
                  <div className="space-y-2.5">
                    <TextField
                      label={f.swr}
                      type="number"
                      step="0.25"
                      className="w-40"
                      value={params.swrBps / 100}
                      onChange={(e) =>
                        editParams({ swrBps: Math.round(Number(e.currentTarget.value) * 100) })
                      }
                    />
                    <p className="text-[12px] leading-[1.6] text-ink-2">
                      {f.swrExplain(
                        fmtBps(params.swrBps, 2),
                        fmtMoney(p.grossWithdrawal),
                        fmtMoney(p.targetNow),
                      )}
                    </p>
                    <div className="divide-y divide-rule rounded-md border border-rule">
                      {p.sensitivity.map((s) => {
                        const current = s.swrBps === params.swrBps;
                        return (
                          <button
                            key={s.swrBps}
                            type="button"
                            onClick={() => editParams({ swrBps: s.swrBps })}
                            className={cn(
                              "flex w-full items-center gap-4 px-4 py-2 text-left transition-colors hover:bg-surface-hi",
                              current && "bg-brass-wash/40",
                            )}
                          >
                            <span
                              className={cn(
                                "tnum w-12 text-[12px]",
                                current ? "text-ink" : "text-ink-3",
                              )}
                            >
                              {fmtBps(s.swrBps, 2)}
                            </span>
                            <span className="tnum flex-1 text-[12.5px] text-ink">
                              {f.needs(fmtMoney(s.target))}
                            </span>
                            <span className="tnum text-[12px] text-ink-2">
                              {s.years === null
                                ? f.notOnPath
                                : s.years === 0
                                  ? f.already
                                  : f.yr(fmtDecimal(s.years))}
                            </span>
                          </button>
                        );
                      })}
                    </div>
                  </div>

                  <div className="space-y-2.5">
                    <TextField
                      label={f.bandLabel}
                      type="number"
                      step="0.5"
                      className="w-40"
                      value={plan.bandBps / 100}
                      onChange={(e) =>
                        editPlan({ bandBps: Math.round(Number(e.currentTarget.value) * 100) })
                      }
                    />
                    <p className="text-[12px] leading-[1.6] text-ink-2">
                      {f.bandExplain(
                        fmtBps(plan.realReturnBps - plan.bandBps, 1),
                        fmtBps(plan.realReturnBps + plan.bandBps, 1),
                        p.yearsHigh === null ? "—" : fmtDecimal(p.yearsHigh),
                        p.yearsLow === null ? "—" : fmtDecimal(p.yearsLow),
                      )}
                    </p>
                  </div>
                </PanelBody>
              </Disclosure>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function headline(f: Dict["fire"], years: number | null, covered: boolean): string {
  if (covered) return f.headlineThere;
  if (years === null) return f.notOnPath;
  if (years < 1 / 12) return f.headlineWithin;
  let y = Math.floor(years);
  let m = Math.round((years - y) * 12);
  if (m === 12) {
    y += 1;
    m = 0;
  }
  return f.headlineAway(y, m);
}

/**
 * One of the numbers a person controls: a slider to feel the effect, and the
 * figure itself typeable for when they know exactly what it is.
 */
function Dial({
  label,
  unit,
  value,
  min,
  max,
  step,
  onChange,
  children,
}: {
  label: string;
  unit: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (v: number) => void;
  children?: React.ReactNode;
}) {
  const id = useId();
  // Someone who spends more than the slider allows still gets a slider.
  const top = Math.max(max, Math.ceil((value * 1.25) / step) * step);
  const pct = top === min ? 0 : ((Math.min(Math.max(value, min), top) - min) / (top - min)) * 100;
  return (
    <div>
      <div className="flex items-baseline justify-between gap-3">
        <label htmlFor={id} className="text-[12.5px] text-ink-2">
          {label}
        </label>
        <span className="flex items-baseline gap-1.5">
          <span className="text-[13px] text-ink-3">{sym()}</span>
          <input
            type="number"
            min={0}
            step={step}
            value={value}
            aria-label={label}
            onChange={(e) => onChange(Math.max(0, Number(e.currentTarget.value) || 0))}
            className="tnum w-[84px] border-b border-transparent bg-transparent text-right text-[20px] leading-none text-ink outline-none transition-colors hover:border-rule focus:border-brass"
          />
          <span className="text-[11px] text-ink-3">{unit}</span>
        </span>
      </div>
      <input
        id={id}
        type="range"
        className="mt-2.5"
        style={{ "--pct": `${pct}%` } as React.CSSProperties}
        min={min}
        max={top}
        step={step}
        value={Math.min(Math.max(value, min), top)}
        onChange={(e) => onChange(Number(e.currentTarget.value))}
      />
      {children}
    </div>
  );
}

function Key({ swatch, label, line }: { swatch: string; label: string; line?: boolean }) {
  return (
    <span className="flex items-center gap-1.5 text-[11px] text-ink-3">
      <span
        className={cn("block rounded-full", line ? "h-[2px] w-3" : "size-2")}
        style={{ background: swatch }}
      />
      {label}
    </span>
  );
}

/** "Use what my own records say", offered under a field rather than imposed. */
function UseMine({
  label,
  note,
  onClick,
}: {
  label: string;
  note: string;
  onClick: () => void;
}) {
  return (
    <div className="mt-1.5 space-y-0.5">
      <button
        type="button"
        onClick={onClick}
        className="text-left text-[11.5px] text-brass underline-offset-2 hover:underline"
      >
        {label}
      </button>
      {note && <p className="text-[10.5px] leading-[1.4] text-ink-3">{note}</p>}
    </div>
  );
}

/**
 * The answer to "could I stop at this age?".
 *
 * The date itself comes out of the numbers, so the age someone wishes for
 * cannot move it. What it can do is be checked — and when the answer is no,
 * turned into the changes that would make it yes, each one a button so that
 * trying it is one click.
 */
function GoalVerdict({
  goal,
  ageAtTarget,
  onSave,
  onSpend,
  onAge,
}: {
  goal: NonNullable<FirePlanView["plan"]["goal"]>;
  ageAtTarget: number | null;
  onSave: (monthly: number) => void;
  onSpend: (monthly: number) => void;
  onAge: (age: number) => void;
}) {
  const f = useT().fire;
  if (goal.onTrack) {
    return (
      <div className="rounded-md border border-positive/35 bg-positive/[0.07] px-3.5 py-3">
        <p className="text-[12.5px] font-medium text-positive">{f.goalYes(goal.age)}</p>
        <p className="mt-1 text-[12px] leading-[1.55] text-ink-2">
          {goal.spareYears !== null && goal.spareYears >= 0.5
            ? f.goalSpare(fmtDecimal(goal.spareYears))
            : f.goalJust}
        </p>
      </div>
    );
  }
  const option = "block w-full rounded px-2 py-1.5 text-left text-[12px] text-ink transition-colors hover:bg-surface";
  return (
    <div className="rounded-md border border-brass/40 bg-brass-wash/40 px-3.5 py-3">
      <p className="text-[12.5px] font-medium text-ink">{f.goalNo(goal.age)}</p>
      <p className="mt-1 text-[12px] leading-[1.55] text-ink-2">
        {f.goalGap(goal.age, fmtMoney(goal.liquidAtGoal), fmtMoney(goal.targetAtGoal))}
      </p>
      <p className="eyebrow mt-3 mb-1">{f.goalFix}</p>
      <div className="-mx-2">
        {goal.extraSavingMonthly !== null && (
          <button type="button" className={option} onClick={() => onSave(goal.extraSavingMonthly ?? 0)}>
            → {f.goalSave(fmtMoney(goal.extraSavingMonthly))}
          </button>
        )}
        {goal.spendCutMonthly !== null && (
          <button type="button" className={option} onClick={() => onSpend(goal.spendCutMonthly ?? 0)}>
            → {f.goalSpend(fmtMoney(goal.spendCutMonthly))}
          </button>
        )}
        {ageAtTarget !== null && (
          <button type="button" className={option} onClick={() => onAge(ageAtTarget)}>
            → {f.goalLater(ageAtTarget)}
          </button>
        )}
        {goal.extraSavingMonthly === null && goal.spendCutMonthly === null && ageAtTarget === null && (
          <p className="px-2 py-1.5 text-[12px] text-ink-2">{f.goalNone}</p>
        )}
      </div>
    </div>
  );
}

/**
 * The tax table: a country's, or the person's own.
 *
 * The rates come from the region files, one per country, and picking a
 * country copies its rates into the plan. From then on they belong to the
 * plan rather than to the code: tax law changes more often than this app
 * will, and a rate that has moved is corrected here with nothing to update.
 * Which country is "selected" is not stored anywhere — it is whichever one
 * the plan's rates still match, so an edited table reads as the person's own.
 */
function Brackets({
  brackets,
  onChange,
}: {
  brackets: FirePlanView["planParams"]["taxBrackets"];
  onChange: (b: FirePlanView["planParams"]["taxBrackets"]) => void;
}) {
  const f = useT().fire;
  const withTax = REGIONS.filter((r) => r.fire.taxBrackets.length > 0);
  const same = (a: typeof brackets, b: typeof brackets) =>
    a.length === b.length && a.every((x, i) => x.upTo === b[i]!.upTo && x.rateBps === b[i]!.rateBps);
  // The person's own region first, so two countries with the same rates
  // resolve to the one they live in.
  const match =
    [region(), ...withTax].find((r) => r.fire.taxBrackets.length > 0 && same(r.fire.taxBrackets, brackets)) ??
    null;
  const set = (i: number, patch: Partial<(typeof brackets)[number]>) =>
    onChange(brackets.map((b, j) => (j === i ? { ...b, ...patch } : b)));
  const cell =
    "tnum w-28 rounded-md border border-rule bg-surface px-2 py-1 text-right text-[12px] text-ink outline-none focus:border-brass";
  return (
    <div className="space-y-2 border-t border-rule pt-3">
      <p className="eyebrow">{f.brackets}</p>
      <p className="text-[11.5px] leading-[1.6] text-ink-3">{f.bracketsHint}</p>
      <div className="flex flex-wrap items-end gap-x-4 gap-y-2 pb-1">
        <SelectField
          label={f.taxCountry}
          className="w-80"
          value={match?.id ?? ""}
          onChange={(e) => {
            const picked = withTax.find((r) => r.id === e.currentTarget.value);
            if (picked) onChange(picked.fire.taxBrackets);
          }}
        >
          {withTax.map((r) => (
            <option key={r.id} value={r.id}>
              {f.taxOption(r.name, r.tax?.name ?? r.country)}
            </option>
          ))}
          {match === null && <option value="">{f.taxOwn}</option>}
        </SelectField>
        <p className="min-w-0 flex-1 pb-1.5 text-[11.5px] leading-[1.5] text-ink-3">
          {match?.tax
            ? `${f.taxSource(match.tax.name, match.tax.year, match.tax.source)}${
                match.tax.note ? ` ${match.tax.note}` : ""
              }`
            : match === null
              ? f.taxOwnNote
              : ""}
        </p>
      </div>
      <div className="space-y-1.5">
        {brackets.map((b, i) => {
          const last = i === brackets.length - 1;
          return (
            <div key={i} className="flex items-center gap-2 text-[12px] text-ink-2">
              <span className="w-24">{brackets.length === 1 ? f.bracketAll : last ? f.bracketAbove : f.bracketUpTo}</span>
              {last ? (
                <span className="w-28 text-right text-ink-3">—</span>
              ) : (
                <input
                  type="number"
                  className={cell}
                  value={minorToMajor(b.upTo ?? 0)}
                  onChange={(e) => set(i, { upTo: majorToMinor(Number(e.currentTarget.value)) })}
                />
              )}
              <span className="w-6 text-center text-ink-3">→</span>
              <input
                type="number"
                step="0.5"
                className={cn(cell, "w-20")}
                value={b.rateBps / 100}
                onChange={(e) => set(i, { rateBps: Math.round(Number(e.currentTarget.value) * 100) })}
              />
              <span className="text-ink-3">%</span>
              {brackets.length > 1 && (
                <button
                  type="button"
                  aria-label={f.bracketRemove}
                  title={f.bracketRemove}
                  className="rounded p-1 text-ink-3 hover:text-negative"
                  onClick={() => {
                    const next = brackets.filter((_, j) => j !== i);
                    // Whatever is now last has no ceiling.
                    onChange(next.map((x, j) => (j === next.length - 1 ? { ...x, upTo: null } : x)));
                  }}
                >
                  ×
                </button>
              )}
            </div>
          );
        })}
      </div>
      <button
        type="button"
        className="text-[11.5px] text-brass underline-offset-2 hover:underline"
        onClick={() => {
          const top = brackets.at(-1);
          const floor = brackets.at(-2)?.upTo ?? 0;
          onChange([
            ...brackets.slice(0, -1),
            { upTo: floor + 10_000_000, rateBps: top?.rateBps ?? 0 },
            { upTo: null, rateBps: top?.rateBps ?? 0 },
          ]);
        }}
      >
        {f.bracketAdd}
      </button>
      <p className="pt-1 text-[11.5px] leading-[1.6] text-ink-3">{f.taxAddCountry}</p>
    </div>
  );
}
