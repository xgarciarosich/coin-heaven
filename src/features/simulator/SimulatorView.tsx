import { useEffect, useRef, useState } from "react";
import { AlertTriangle, Check, ChevronDown, Clock, Home, Plus, RotateCcw, Trash2 } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { Button, Segmented } from "@/components/ui/controls";
import { FieldGroup, Slider } from "@/components/ui/field";
import { api } from "@/lib/ipc";
import {
  fmtBps,
  fmtCompact,
  fmtDecimal,
  fmtMoney,
  majorToMinor,
  minorToMajor,
  sym,
} from "@/lib/money";
import type { PlannedProperty, PropertyOutcome, SimParams, YearRow } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { region } from "@/lib/region";
import { useSimulation } from "./useSimulation";
import { SimChart } from "./SimChart";
import { YearGrid } from "./YearGrid";

/** The plan the screen works on is saved under this name and reopened. */
const CURRENT = "current";

/** How kind the property market is: appreciation, rent rises, occupancy. */
const MOODS = {
  cautious: { appreciationBps: 0, rentCpiBps: 100, occupancyBps: 8_500 },
  base: { appreciationBps: 200, rentCpiBps: 200, occupancyBps: 9_000 },
  hopeful: { appreciationBps: 350, rentCpiBps: 300, occupancyBps: 9_500 },
} as const;
type Mood = keyof typeof MOODS;

const thisMonth = () => new Date().toISOString().slice(0, 7);

/** Plan month 1 is `start`; month n is n − 1 calendar months after it. */
function calendar(start: string, planMonth: number) {
  const [y, m] = start.split("-").map(Number) as [number, number];
  const at = y * 12 + (m - 1) + (planMonth - 1);
  return { year: Math.floor(at / 12), month: (at % 12) + 1 };
}
function planMonth(start: string, year: number, month: number) {
  const [y, m] = start.split("-").map(Number) as [number, number];
  return Math.max(1, year * 12 + (month - 1) - (y * 12 + (m - 1)) + 1);
}

/**
 * Module 4 — a buy-to-let plan.
 *
 * The person decides what to buy and when; the screen tells them whether it
 * works. So the centre of the page is the list of purchases, each with the
 * things that belong to that flat alone — its price, its renovation, its
 * rent, the purchase tax of the region it is in, and its own mortgage terms
 * if it has them — and each answering at once: can it be bought when
 * planned, with how much cash, and what does it leave each month.
 *
 * Around the list sit the two sets of numbers that are not about any one
 * property: the person's own (pay and how it grows, what they put aside, the
 * cash they start with) and the general ones every purchase shares unless it
 * says otherwise.
 */
export function SimulatorView({ initial }: { initial: SimParams }) {
  const t = useT();
  const m = t.simulator;
  const [p, setP] = useState<SimParams>(() => ({
    ...initial,
    startMonth: initial.startMonth ?? thisMonth(),
  }));
  const { result, errors, pending } = useSimulation(p);
  const start = p.startMonth ?? thisMonth();

  // Reopen the plan that was being worked on. Until that has been tried,
  // nothing is saved — or the defaults would overwrite it.
  const loaded = useRef(false);
  useEffect(() => {
    api
      .simListScenarios()
      .then((all) => all.find((s) => s.name === CURRENT))
      .then((found) => (found ? api.simLoadScenario(found.id) : null))
      .then((saved) => {
        if (saved) setP({ ...saved, startMonth: saved.startMonth ?? thisMonth() });
      })
      .catch(() => undefined)
      .finally(() => {
        loaded.current = true;
      });
  }, []);

  // And keep it: a plan typed in over twenty minutes should not need a save
  // button to survive closing the window.
  useEffect(() => {
    if (!loaded.current || errors.length > 0) return;
    const timer = setTimeout(() => {
      api.simSaveScenario(CURRENT, p).catch(() => undefined);
    }, 1000);
    return () => clearTimeout(timer);
  }, [p, errors.length]);

  const set =
    <K extends keyof SimParams>(k: K) =>
    (v: SimParams[K]) =>
      setP((prev) => ({ ...prev, [k]: v }));
  const setMoney = (k: keyof SimParams) => (major: number) =>
    setP((prev) => ({ ...prev, [k]: majorToMinor(major) }));

  const setProperty = (i: number, patch: Partial<PlannedProperty>) =>
    setP((prev) => ({
      ...prev,
      properties: prev.properties.map((f, j) => (j === i ? { ...f, ...patch } : f)),
    }));
  const addProperty = () =>
    setP((prev) => {
      // A new one starts as a copy of the last, a year later: the next flat
      // is usually like the previous one, and never in the same month.
      const last = prev.properties.filter((x) => !x.owned).at(-1);
      const next: PlannedProperty = last
        ? { ...last, name: "", buyMonth: last.buyMonth + 12, owned: null }
        : {
            name: "",
            buyMonth: 6,
            price: 12_000_000,
            renovation: 1_000_000,
            monthlyRent: 70_000,
            transferTaxBps: region().simulator.transferTaxBps,
            financingBps: null,
            mortgageRateBps: null,
            mortgageTermMonths: null,
            fixedCostsAnnual: null,
            owned: null,
          };
      return { ...prev, properties: [...prev.properties, next] };
    });
  // One already owned goes first: it is there before anything is bought.
  const addOwned = () =>
    setP((prev) => {
      const owned: PlannedProperty = {
        name: "",
        buyMonth: 1,
        price: 20_000_000,
        renovation: 0,
        monthlyRent: 90_000,
        transferTaxBps: 0,
        financingBps: null,
        mortgageRateBps: null,
        mortgageTermMonths: null,
        fixedCostsAnnual: null,
        owned: { boughtOn: null, paid: null, loanRemaining: 0 },
      };
      const firstPlanned = prev.properties.findIndex((x) => !x.owned);
      const at = firstPlanned === -1 ? prev.properties.length : firstPlanned;
      return {
        ...prev,
        properties: [...prev.properties.slice(0, at), owned, ...prev.properties.slice(at)],
      };
    });

  const mood =
    (Object.keys(MOODS) as Mood[]).find((k) =>
      (Object.keys(MOODS[k]) as (keyof (typeof MOODS)[Mood])[]).every((f) => p[f] === MOODS[k][f]),
    ) ?? null;

  const last = result?.years.at(-1);
  // What is already owned is not a purchase: the story counts only the plan.
  const ownedCount = p.properties.filter((f) => f.owned).length;
  const planned = p.properties.length - ownedCount;

  return (
    <div className="flex h-full">
      {/* ------------------------------------------------------- results -- */}
      <div className="min-w-0 flex-1 overflow-y-auto">
        <header data-tauri-drag-region className="px-10 pt-9 pb-8">
          <p className="eyebrow">
            {m.eyebrow(p.horizonYears)}
            {pending && <span className="ml-2 text-ink-3">{m.recalculating}</span>}
          </p>

          <div className="mt-2.5 flex items-end gap-4">
            <h1 className="display tnum text-[42px] leading-[0.95] text-ink">
              {result ? fmtMoney(result.finalNetWorth) : "—"}
            </h1>
            <span className="mb-1.5 text-[12px] text-ink-3">{m.netWorthIn(p.horizonYears)}</span>
          </div>

          <p className="mt-3 max-w-2xl text-[13px] leading-[1.65] text-ink-2">
            {result
              ? planned === 0 && ownedCount === 0
                ? m.storyEmpty
                : planned === 0
                  ? m.storyOwnedOnly(
                      fmtMoney(result.finalNetRentMonthly),
                      fmtMoney(result.finalSavingCapacity),
                    )
                  : m.story(
                    result.finalProperties - ownedCount,
                    planned,
                    fmtMoney(result.finalNetRentMonthly),
                    fmtMoney(result.finalSavingCapacity),
                  )
              : m.adjust}
          </p>
        </header>

        <div className="space-y-5 px-10 pb-12">
          {errors.length > 0 && (
            <div className="rounded-[10px] border border-negative/35 bg-negative/[0.06] px-5 py-4">
              <p className="text-[12px] font-medium text-negative">{m.cannotSimulate}</p>
              <ul className="mt-1.5 space-y-0.5">
                {errors.map((e) => (
                  <li key={e.field} className="text-[12px] text-ink-2">
                    <span className="text-[11px] text-ink-3">
                      {(m.fields as Record<string, string>)[e.field] ?? e.field}
                    </span>{" "}
                    — {e.message}
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* ------------------------------------------------ the plan -- */}
          <Panel>
            <PanelHead
              label={m.yourPurchases}
              aside={
                <div className="flex items-center gap-2">
                  <Button onClick={addOwned} title={m.addOwnedHint}>
                    <Home className="size-3.5" strokeWidth={1.75} />
                    {m.addOwned}
                  </Button>
                  <Button variant="primary" onClick={addProperty}>
                    <Plus className="size-3.5" strokeWidth={2} />
                    {m.addProperty}
                  </Button>
                </div>
              }
            />
            {p.properties.length === 0 ? (
              <PanelBody>
                <p className="text-[12.5px] leading-[1.65] text-ink-2">{m.noPurchases}</p>
              </PanelBody>
            ) : (
              <div className="divide-y divide-rule">
                {p.properties.map((f, i) => (
                  <PropertyCard
                    key={i}
                    number={i + 1}
                    property={f}
                    outcome={result?.properties[i] ?? null}
                    params={p}
                    start={start}
                    onChange={(patch) => setProperty(i, patch)}
                    onRemove={() =>
                      setP((prev) => ({
                        ...prev,
                        properties: prev.properties.filter((_, j) => j !== i),
                      }))
                    }
                  />
                ))}
              </div>
            )}
          </Panel>

          <StatStrip last={last} params={p} capacity={result?.finalSavingCapacity} />

          <SimChart result={result} params={p} start={start} />

          <Panel>
            <PanelHead label={m.yearByYear} />
            <YearGrid rows={result?.years ?? []} start={start} dtiMaxBps={p.dtiMaxBps} />
          </Panel>

          <p className="max-w-2xl text-[11.5px] leading-[1.7] text-ink-3">{m.disclaimer}</p>
        </div>
      </div>

      {/* ------------------------------------------------------ controls -- */}
      <aside className="w-[300px] shrink-0 overflow-y-auto border-l border-rule bg-paper-sunk">
        <div data-tauri-drag-region className="flex h-9 items-center justify-end px-4">
          <button
            type="button"
            onClick={() => setP({ ...initial, startMonth: thisMonth() })}
            className="flex items-center gap-1.5 rounded px-1.5 py-1 text-[11px] text-ink-3 transition-colors hover:text-ink"
          >
            <RotateCcw className="size-3" strokeWidth={1.75} />
            {m.reset}
          </button>
        </div>

        <FieldGroup title={m.groups.you}>
          <Slider
            label={m.fields.startCash}
            value={minorToMajor(p.startCash)}
            min={0}
            max={300_000}
            step={1000}
            onChange={setMoney("startCash")}
            format={(v) => fmtCompact(majorToMinor(v))}
          />
          <Slider
            label={m.fields.monthlySavings}
            value={minorToMajor(p.monthlySavings)}
            min={0}
            max={6000}
            step={50}
            onChange={setMoney("monthlySavings")}
            format={(v) => fmtMoney(majorToMinor(v))}
            note={m.notes.savings}
          />
          <Slider
            label={m.fields.netMonthlySalary}
            value={minorToMajor(p.netMonthlySalary)}
            min={800}
            max={12_000}
            step={50}
            onChange={setMoney("netMonthlySalary")}
            format={(v) => fmtMoney(majorToMinor(v))}
            note={m.notes.salary}
          />
          <Slider
            label={m.fields.incomeGrowthBps}
            value={p.incomeGrowthBps}
            min={0}
            max={1000}
            step={25}
            onChange={set("incomeGrowthBps")}
            format={(v) => fmtBps(v, 1)}
            note={m.notes.growth}
          />
          <Slider
            label={m.fields.horizonYears}
            value={p.horizonYears}
            min={3}
            max={30}
            step={1}
            onChange={set("horizonYears")}
            format={(v) => m.termYears(v)}
          />
        </FieldGroup>

        <FieldGroup title={m.groups.general} hint={m.notes.general}>
          <Slider
            label={m.fields.financingBps}
            value={p.financingBps}
            min={0}
            max={10_000}
            step={250}
            onChange={set("financingBps")}
            format={(v) => fmtBps(v, 0)}
            note={m.notes.financing}
          />
          <Slider
            label={m.fields.mortgageRateBps}
            value={p.mortgageRateBps}
            min={0}
            max={800}
            step={5}
            onChange={set("mortgageRateBps")}
            format={(v) => fmtBps(v, 2)}
          />
          <Slider
            label={m.fields.mortgageTermMonths}
            value={p.mortgageTermMonths}
            min={60}
            max={480}
            step={12}
            onChange={set("mortgageTermMonths")}
            format={(v) => m.termYears(v / 12)}
          />
          <Slider
            label={m.fields.dtiMaxBps}
            value={p.dtiMaxBps}
            min={1000}
            max={6000}
            step={100}
            onChange={set("dtiMaxBps")}
            format={(v) => fmtBps(v, 0)}
            note={m.notes.dti}
          />
          <Slider
            label={m.fields.otherClosingBps}
            value={p.otherClosingBps}
            min={0}
            max={500}
            step={10}
            onChange={set("otherClosingBps")}
            format={(v) => fmtBps(v, 2)}
            note={m.notes.closing}
          />
          <Slider
            label={m.fields.cashBuffer}
            value={minorToMajor(p.cashBuffer)}
            min={0}
            max={50_000}
            step={500}
            onChange={setMoney("cashBuffer")}
            format={(v) => fmtCompact(majorToMinor(v))}
            note={m.notes.buffer}
          />
        </FieldGroup>

        <FieldGroup title={m.groups.market} hint={m.notes.market}>
          <Segmented
            options={[
              ["cautious", m.moods.cautious],
              ["base", m.moods.base],
              ["hopeful", m.moods.hopeful],
            ]}
            value={mood}
            onChange={(k: Mood) => setP((prev) => ({ ...prev, ...MOODS[k] }))}
          />
          <Slider
            label={m.fields.appreciationBps}
            value={p.appreciationBps}
            min={-300}
            max={700}
            step={10}
            onChange={set("appreciationBps")}
            format={(v) => fmtBps(v, 1)}
          />
          <Slider
            label={m.fields.rentCpiBps}
            value={p.rentCpiBps}
            min={0}
            max={600}
            step={10}
            onChange={set("rentCpiBps")}
            format={(v) => fmtBps(v, 1)}
          />
          <Slider
            label={m.fields.occupancyBps}
            value={p.occupancyBps}
            min={5000}
            max={10_000}
            step={100}
            onChange={set("occupancyBps")}
            format={(v) => m.monthsLet(fmtDecimal((v / 10_000) * 12))}
          />
        </FieldGroup>

        <FieldGroup title={m.groups.running} hint={m.notes.running} collapsible>
          <Slider
            label={m.fields.mgmtFeeBps}
            value={p.mgmtFeeBps}
            min={0}
            max={2000}
            step={50}
            onChange={set("mgmtFeeBps")}
            format={(v) => fmtBps(v, 1)}
          />
          <Slider
            label={m.fields.maintenanceBps}
            value={p.maintenanceBps}
            min={0}
            max={2000}
            step={50}
            onChange={set("maintenanceBps")}
            format={(v) => fmtBps(v, 1)}
          />
          <Slider
            label={m.fields.fixedCostsAnnual}
            value={minorToMajor(p.fixedCostsAnnual)}
            min={0}
            max={5000}
            step={50}
            onChange={setMoney("fixedCostsAnnual")}
            format={(v) => fmtMoney(majorToMinor(v))}
            note={m.notes.fixed}
          />
          <Slider
            label={m.fields.rentalTaxBps}
            value={p.rentalTaxBps}
            min={0}
            max={4700}
            step={50}
            onChange={set("rentalTaxBps")}
            format={(v) => fmtBps(v, 1)}
            note={m.notes.rentalTax}
          />
          <Slider
            label={m.fields.computableRentBps}
            value={p.computableRentBps}
            min={0}
            max={10_000}
            step={500}
            onChange={set("computableRentBps")}
            format={(v) => fmtBps(v, 0)}
            note={m.notes.computable}
          />
        </FieldGroup>
      </aside>
    </div>
  );
}

/* ------------------------------------------------------------ property -- */

/**
 * One planned purchase: what the person decided, and what the numbers say
 * about it.
 */
function PropertyCard({
  number,
  property: f,
  outcome: o,
  params,
  start,
  onChange,
  onRemove,
}: {
  number: number;
  property: PlannedProperty;
  outcome: PropertyOutcome | null;
  params: SimParams;
  start: string;
  onChange: (patch: Partial<PlannedProperty>) => void;
  onRemove: () => void;
}) {
  const t = useT();
  const m = t.simulator;
  const own =
    f.financingBps !== null ||
    f.mortgageRateBps !== null ||
    f.mortgageTermMonths !== null ||
    f.fixedCostsAnnual !== null;
  const [terms, setTerms] = useState(own);
  const when = calendar(start, f.buyMonth);
  const firstYear = Number(start.slice(0, 4));
  const date = (planM: number) => {
    const c = calendar(start, planM);
    return `${t.common.monthsLong[c.month - 1]} ${c.year}`;
  };
  const yieldBps = f.price > 0 ? Math.round((f.monthlyRent * 12 * 10_000) / f.price) : 0;
  const delayed = o?.boughtMonth != null && o.boughtMonth > o.plannedMonth;
  const owned = f.owned ?? null;
  const setOwned = (patch: Partial<NonNullable<PlannedProperty["owned"]>>) =>
    owned && onChange({ owned: { ...owned, ...patch } });

  const select =
    "appearance-none rounded-md border border-rule bg-surface px-2 py-1.5 text-[12.5px] text-ink outline-none focus:border-brass";

  return (
    <div className="space-y-3 px-5 py-4">
      <div className="flex flex-wrap items-center gap-3">
        <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-brass-wash text-[11px] text-brass">
          {number}
        </span>
        <input
          value={f.name}
          placeholder={m.propertyName(number)}
          onChange={(e) => onChange({ name: e.currentTarget.value })}
          className="min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-2 py-1 text-[14px] text-ink outline-none transition-colors placeholder:text-ink-3 hover:border-rule focus:border-brass focus:bg-surface"
        />
        {owned ? (
          <>
            <span className="rounded bg-positive/10 px-1.5 py-0.5 text-[11px] text-positive">
              {m.ownedBadge}
            </span>
            <span className="text-[11.5px] text-ink-3">{m.ownedSince}</span>
            <input
              type="month"
              aria-label={m.ownedSince}
              max={start}
              value={owned.boughtOn ?? ""}
              onChange={(e) => setOwned({ boughtOn: e.currentTarget.value || null })}
              className={select}
            />
          </>
        ) : (
          <>
        <span className="text-[11.5px] text-ink-3">{m.buyIn}</span>
          <select
            aria-label={m.buyMonth}
            className={select}
            value={when.month}
            onChange={(e) =>
              onChange({ buyMonth: planMonth(start, when.year, Number(e.currentTarget.value)) })
            }
          >
            {t.common.monthsLong.map((name, i) => (
              <option key={name} value={i + 1}>
                {name}
              </option>
            ))}
          </select>
          <select
            aria-label={m.buyYear}
            className={select}
            value={when.year}
            onChange={(e) =>
              onChange({ buyMonth: planMonth(start, Number(e.currentTarget.value), when.month) })
            }
          >
            {Array.from({ length: params.horizonYears + 1 }, (_, i) => firstYear + i).map((y) => (
              <option key={y} value={y}>
                {y}
              </option>
            ))}
          </select>
          </>
        )}
        <button
          type="button"
          onClick={onRemove}
          aria-label={t.common.remove}
          title={t.common.remove}
          className="rounded p-1.5 text-ink-3 transition-colors hover:text-negative"
        >
          <Trash2 className="size-3.5" strokeWidth={1.75} />
        </button>
      </div>

      {owned ? (
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <Num
            label={m.fields.worthToday}
            suffix={sym()}
            value={minorToMajor(f.price)}
            onChange={(v) => onChange({ price: majorToMinor(v ?? 0) })}
          />
          <Num
            label={m.fields.paid}
            suffix={sym()}
            optional
            value={owned.paid === null ? null : minorToMajor(owned.paid)}
            onChange={(v) => setOwned({ paid: v === null ? null : majorToMinor(v) })}
          />
          <Num
            label={m.fields.monthlyRent}
            suffix={sym()}
            value={minorToMajor(f.monthlyRent)}
            onChange={(v) => onChange({ monthlyRent: majorToMinor(v ?? 0) })}
            hint={f.monthlyRent > 0 ? m.yieldOf(fmtBps(yieldBps, 1)) : m.notes.ownedNoRent}
          />
          <Num
            label={m.fields.loanRemaining}
            suffix={sym()}
            value={minorToMajor(owned.loanRemaining)}
            onChange={(v) => setOwned({ loanRemaining: majorToMinor(v ?? 0) })}
            hint={m.notes.loanRemaining}
          />
        </div>
      ) : (
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Num
          label={m.fields.propertyPrice}
          suffix={sym()}
          value={minorToMajor(f.price)}
          onChange={(v) => onChange({ price: majorToMinor(v ?? 0) })}
        />
        <Num
          label={m.fields.renovation}
          suffix={sym()}
          value={minorToMajor(f.renovation)}
          onChange={(v) => onChange({ renovation: majorToMinor(v ?? 0) })}
        />
        <Num
          label={m.fields.monthlyRent}
          suffix={sym()}
          value={minorToMajor(f.monthlyRent)}
          onChange={(v) => onChange({ monthlyRent: majorToMinor(v ?? 0) })}
          hint={m.yieldOf(fmtBps(yieldBps, 1))}
        />
        <Num
          label={m.fields.transferTaxBps}
          suffix="%"
          step={0.5}
          value={f.transferTaxBps / 100}
          onChange={(v) => onChange({ transferTaxBps: Math.round((v ?? 0) * 100) })}
          hint={m.notes.transferTax}
        />
      </div>
      )}

      <button
        type="button"
        onClick={() => setTerms((v) => !v)}
        className="flex items-center gap-1 text-[11.5px] text-ink-3 transition-colors hover:text-ink"
      >
        <ChevronDown
          className={cn("size-3 transition-transform", !terms && "-rotate-90")}
          strokeWidth={2}
        />
        {own ? m.ownTerms : m.generalTerms}
      </button>
      {terms && (
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          {!owned && (
          <Num
            label={m.fields.financingBps}
            suffix="%"
            optional
            value={f.financingBps === null ? null : f.financingBps / 100}
            placeholder={String(params.financingBps / 100)}
            onChange={(v) => onChange({ financingBps: v === null ? null : Math.round(v * 100) })}
          />
          )}
          <Num
            label={m.fields.mortgageRateBps}
            suffix="%"
            step={0.05}
            optional
            value={f.mortgageRateBps === null ? null : f.mortgageRateBps / 100}
            placeholder={String(params.mortgageRateBps / 100)}
            onChange={(v) => onChange({ mortgageRateBps: v === null ? null : Math.round(v * 100) })}
          />
          <Num
            label={owned ? m.fields.yearsLeft : m.fields.mortgageYears}
            optional
            value={f.mortgageTermMonths === null ? null : f.mortgageTermMonths / 12}
            placeholder={String(params.mortgageTermMonths / 12)}
            onChange={(v) =>
              onChange({ mortgageTermMonths: v === null ? null : Math.round(v * 12) })
            }
          />
          <Num
            label={m.fields.fixedCostsAnnual}
            suffix={sym()}
            optional
            value={f.fixedCostsAnnual === null ? null : minorToMajor(f.fixedCostsAnnual)}
            placeholder={String(minorToMajor(params.fixedCostsAnnual))}
            onChange={(v) => onChange({ fixedCostsAnnual: v === null ? null : majorToMinor(v) })}
          />
          <p className="col-span-full -mt-1 text-[11px] text-ink-3">{m.notes.blank}</p>
        </div>
      )}

      {o && owned && (
        <div className="rounded-md border border-positive/30 bg-positive/[0.06] px-3.5 py-3">
          <p className="flex items-center gap-1.5 text-[12.5px] font-medium text-positive">
            <Check className="size-3.5" strokeWidth={2} />
            {m.verdictOwned(fmtMoney(f.price - owned.loanRemaining))}
          </p>
          <div className="mt-2.5 grid grid-cols-2 gap-x-6 gap-y-1.5 lg:grid-cols-4">
            <Fact label={m.factOwedNow} value={fmtMoney(owned.loanRemaining)} />
            <Fact label={m.factPayment} value={m.perMonth(fmtMoney(o.monthlyPayment))} />
            <Fact
              label={m.factKeep}
              value={m.perMonth(fmtMoney(o.netRentFirst))}
              tone={o.netRentFirst < 0 ? "negative" : undefined}
            />
            <Fact
              label={m.factGain}
              value={owned.paid === null ? "—" : fmtMoney(f.price - owned.paid)}
              tone={owned.paid !== null && f.price < owned.paid ? "negative" : undefined}
            />
          </div>
        </div>
      )}

      {o && !owned && (
        <div
          className={cn(
            "rounded-md border px-3.5 py-3",
            o.boughtMonth === null
              ? "border-negative/35 bg-negative/[0.06]"
              : delayed
                ? "border-brass/40 bg-brass-wash/40"
                : "border-positive/30 bg-positive/[0.06]",
          )}
        >
          <p
            className={cn(
              "flex items-center gap-1.5 text-[12.5px] font-medium",
              o.boughtMonth === null ? "text-negative" : delayed ? "text-ink" : "text-positive",
            )}
          >
            {o.boughtMonth === null ? (
              <AlertTriangle className="size-3.5" strokeWidth={2} />
            ) : delayed ? (
              <Clock className="size-3.5" strokeWidth={2} />
            ) : (
              <Check className="size-3.5" strokeWidth={2} />
            )}
            {o.boughtMonth === null
              ? m.verdictNever
              : delayed
                ? m.verdictLate(date(o.boughtMonth), o.boughtMonth - o.plannedMonth)
                : m.verdictOk(date(o.plannedMonth))}
          </p>
          {o.blockedBy && (
            <p className="mt-1 text-[12px] leading-[1.55] text-ink-2">
              {o.blockedBy === "cash"
                ? m.whyCash(date(o.plannedMonth), fmtMoney(o.cashShort))
                : m.whyBank(fmtBps(o.dtiBpsPlanned, 0), fmtBps(params.dtiMaxBps, 0))}
            </p>
          )}
          <div className="mt-2.5 grid grid-cols-2 gap-x-6 gap-y-1.5 lg:grid-cols-4">
            <Fact label={m.factCash} value={fmtMoney(o.cashNeeded)} />
            <Fact label={m.factLoan} value={fmtMoney(o.loan)} />
            <Fact label={m.factPayment} value={m.perMonth(fmtMoney(o.monthlyPayment))} />
            <Fact
              label={m.factKeep}
              value={o.boughtMonth === null ? "—" : m.perMonth(fmtMoney(o.netRentFirst))}
              tone={o.boughtMonth !== null && o.netRentFirst < 0 ? "negative" : undefined}
            />
          </div>
        </div>
      )}
    </div>
  );
}

function Fact({
  label,
  value,
  tone,
}: {
  label: string;
  value: string;
  tone?: "negative" | undefined;
}) {
  return (
    <div>
      <p className="text-[10.5px] text-ink-3">{label}</p>
      <p className={cn("tnum text-[12.5px]", tone === "negative" ? "text-negative" : "text-ink")}>
        {value}
      </p>
    </div>
  );
}

/**
 * A labelled number, with its unit inside the box. An `optional` one hands
 * back `null` when emptied, which the plan reads as "use the general value".
 */
function Num({
  label,
  suffix,
  step,
  hint,
  placeholder,
  value,
  optional,
  onChange,
}: {
  label: string;
  suffix?: string;
  step?: number;
  hint?: string;
  placeholder?: string;
  value: number | null;
  optional?: boolean;
  onChange: (v: number | null) => void;
}) {
  return (
    <label className="block space-y-1.5">
      <span className="eyebrow block">{label}</span>
      <span className="flex items-center rounded-md border border-rule bg-surface pr-2.5 transition-colors focus-within:border-brass">
        <input
          type="number"
          step={step}
          min={0}
          placeholder={placeholder}
          value={value ?? ""}
          onChange={(e) => {
            const raw = e.currentTarget.value;
            onChange(raw === "" ? (optional ? null : 0) : Number(raw));
          }}
          className="tnum w-full min-w-0 bg-transparent px-2.5 py-1.5 text-[12.5px] text-ink outline-none placeholder:text-ink-3"
        />
        {suffix && <span className="text-[11.5px] text-ink-3">{suffix}</span>}
      </span>
      {hint && <span className="block text-[11px] leading-snug text-ink-3">{hint}</span>}
    </label>
  );
}

/* ----------------------------------------------------------------- parts -- */

function StatStrip({
  last,
  params,
  capacity,
}: {
  last: YearRow | undefined;
  params: SimParams;
  capacity: number | undefined;
}) {
  const m = useT().simulator;
  // `exactOptionalPropertyTypes` is on, so optional fields must admit undefined.
  const cells: {
    label: string;
    value: string;
    note?: string | undefined;
    tone?: string | undefined;
    icon?: boolean;
  }[] = [
    {
      label: m.propertyEquity,
      value: last ? fmtCompact(last.equity) : "—",
      note: last ? m.valueNote(fmtCompact(last.portfolioValue), fmtCompact(last.debt)) : undefined,
      icon: true,
    },
    {
      label: m.cashOnHand,
      value: last ? fmtCompact(last.cash) : "—",
      note: m.bufferHeld(fmtCompact(params.cashBuffer)),
    },
    {
      label: m.savingCapacity,
      value: capacity === undefined ? "—" : m.perMonth(fmtMoney(capacity)),
      note: last ? m.capacityNote(fmtMoney(last.netRentMonthly)) : undefined,
    },
    {
      label: m.borrowing,
      value: last ? fmtBps(last.dtiBps, 0) : "—",
      note: m.limit(fmtBps(params.dtiMaxBps, 0)),
      tone: last && !last.dtiOk ? "text-negative" : undefined,
    },
  ];

  return (
    <Panel className="grid grid-cols-2 divide-rule sm:grid-cols-4 sm:divide-x">
      {cells.map((c) => (
        <div key={c.label} className="px-5 py-4">
          <p className="eyebrow flex items-center gap-1.5">
            {c.icon && <Home className="size-3" strokeWidth={1.75} />}
            {c.label}
          </p>
          <p className={cn("tnum mt-2 text-[21px] leading-none text-ink", c.tone)}>{c.value}</p>
          {c.note && <p className="mt-1.5 text-[11px] leading-[1.45] text-ink-3">{c.note}</p>}
        </div>
      ))}
    </Panel>
  );
}

