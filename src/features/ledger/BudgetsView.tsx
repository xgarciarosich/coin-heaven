import { useCallback, useEffect, useMemo, useState } from "react";
import { Plus, Trash2, Wand2, X } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import {
  Banner,
  Button,
  Empty,
  Segmented,
  SelectField,
  TextField,
} from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { fmtMoney, fmtMoneyExact, majorToMinor } from "@/lib/money";
import type { BudgetCell, BudgetPlan, Category } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import type { Dict } from "@/locales/en";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

type View = "year" | "month";
const VIEW_KEY = "coinheaven.budgets.view";

const parseAmount = (text: string) => {
  const v = Number(text.trim().replace(",", "."));
  return text.trim() !== "" && Number.isFinite(v) && v >= 0 ? majorToMinor(v) : null;
};

/**
 * Budgets for one year, by the year or by the month.
 *
 * Some people give a category a figure for the year and some give it one a
 * month, and neither should have to convert. It is one plan read two ways:
 * the year's figure spreads over its months, a month with its own figure
 * keeps it, and a year nobody wrote is what its months add up to. Switching
 * view never changes a number.
 *
 * A budget is only useful if it answers "am I on track?", which is a question
 * about pace rather than about a total: three quarters of a year's grocery
 * money spent is fine in October and alarming in April. Every card therefore
 * carries the elapsed share of the period beside the spent share, and says
 * what is left per remaining day.
 */
export function BudgetsView({
  year,
  categories,
  onChanged,
}: {
  year: number;
  categories: Category[];
  onChanged: () => void;
}) {
  const t = useT();
  const b = t.ledger.budgets;
  const [plans, setPlans] = useState<BudgetPlan[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState(false);
  const [view, setViewState] = useState<View>(() =>
    localStorage.getItem(VIEW_KEY) === "month" ? "month" : "year",
  );
  const now = new Date();
  // The month someone most likely means: this one, in the current year.
  const [month, setMonth] = useState(year === now.getFullYear() ? now.getMonth() : 0);

  const setView = (v: View) => {
    setViewState(v);
    localStorage.setItem(VIEW_KEY, v);
  };

  const load = useCallback(() => {
    api
      .budgetPlan(year)
      .then(setPlans)
      .catch((e: unknown) => setError(msg(e)));
  }, [year]);

  useEffect(load, [load]);

  const refresh = () => {
    setError(null);
    load();
    onChanged();
  };

  const seed = () => {
    setBusy(true);
    api
      .budgetSeedFromYear(year - 1, year)
      .then((seeds) => {
        setNote(seeds.length === 0 ? b.seededNone(year - 1) : b.seeded(seeds.length, year - 1));
        refresh();
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setBusy(false));
  };

  const cellOf = (p: BudgetPlan) => (view === "year" ? p.year : p.months[month]!);
  const monthName = t.common.monthsLong[month] ?? "";

  // What the spending side of the plan comes to in the period on screen.
  const totals = useMemo(() => {
    const spend = plans.filter((p) => p.kind !== "income");
    const cells = spend.map((p) => (view === "year" ? p.year : p.months[month]!));
    const planned = cells.reduce((a, c) => a + (c.amountMinor ?? 0), 0);
    const actual = cells
      .filter((c) => c.amountMinor !== null)
      .reduce((a, c) => a + c.actualMinor, 0);
    return { planned, actual };
  }, [plans, view, month]);

  return (
    <div className="space-y-5">
      {error && <Banner tone="bad">{error}</Banner>}
      {note && <Banner tone="good">{note}</Banner>}

      <Panel>
        <PanelHead
          label={b.title(year)}
          aside={
            <div className="flex items-center gap-2">
              <div className="w-52">
                <Segmented
                  options={[
                    ["year", b.viewYear],
                    ["month", b.viewMonth],
                  ]}
                  value={view}
                  onChange={setView}
                />
              </div>
              <Button variant="ghost" onClick={seed} disabled={busy}>
                <Wand2 className="size-3.5" strokeWidth={1.75} />
                {b.startFrom(year - 1)}
              </Button>
              <Button variant="primary" onClick={() => setAdding((v) => !v)}>
                {adding ? (
                  <X className="size-3.5" strokeWidth={2} />
                ) : (
                  <Plus className="size-3.5" strokeWidth={2} />
                )}
                {adding ? t.common.cancel : b.newBudget}
              </Button>
            </div>
          }
        />

        {view === "month" && (
          <div className="flex gap-1 overflow-x-auto border-b border-rule px-5 py-2.5">
            {t.common.months.map((m, i) => (
              <button
                key={m}
                type="button"
                onClick={() => setMonth(i)}
                className={cn(
                  "min-w-[52px] flex-1 rounded-md px-2 py-1.5 text-[12px] transition-colors",
                  month === i
                    ? "bg-brass-wash text-ink"
                    : "text-ink-3 hover:bg-surface-hi hover:text-ink",
                )}
              >
                {m}
              </button>
            ))}
          </div>
        )}

        {adding && (
          <BudgetForm
            year={year}
            month={month}
            view={view}
            categories={categories}
            plans={plans}
            onSaved={() => {
              setAdding(false);
              refresh();
            }}
            onError={setError}
          />
        )}

        {plans.length === 0 ? (
          <PanelBody>
            <Empty title={b.empty(year)} body={b.emptyBody} />
          </PanelBody>
        ) : (
          <>
            <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1 border-b border-rule px-5 py-3">
              <span className="text-[13px] text-ink">
                {view === "year" ? year : `${monthName} ${year}`}
              </span>
              <span className="tnum text-[12px] text-ink-3">
                {b.totals(fmtMoney(totals.planned), fmtMoney(totals.actual))}
              </span>
              <span
                className={cn(
                  "tnum text-[12px]",
                  totals.planned - totals.actual < 0 ? "text-negative" : "text-ink-2",
                )}
              >
                {totals.planned - totals.actual < 0
                  ? b.totalsOver(fmtMoney(totals.actual - totals.planned))
                  : b.totalsLeft(fmtMoney(totals.planned - totals.actual))}
              </span>
            </div>
            <PanelBody>
              <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
                {plans.map((p) => (
                  <Card
                    // A different period is a different figure being edited.
                    key={`${p.categoryId}-${cellOf(p).period}`}
                    plan={p}
                    cell={cellOf(p)}
                    year={year}
                    onPickMonth={(i) => {
                      setMonth(i);
                      setView("month");
                    }}
                    onRefresh={refresh}
                    onError={setError}
                  />
                ))}
              </div>
            </PanelBody>
            <p className="border-t border-rule px-5 py-3 text-[11px] leading-[1.6] text-ink-3">
              {b.togetherNote}
            </p>
          </>
        )}
      </Panel>
    </div>
  );
}

function Card({
  plan: p,
  cell: c,
  year,
  onPickMonth,
  onRefresh,
  onError,
}: {
  plan: BudgetPlan;
  cell: BudgetCell;
  year: number;
  onPickMonth: (month: number) => void;
  onRefresh: () => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const b = t.ledger.budgets;
  const isYear = c.period.length === 4;
  const monthIndex = isYear ? 0 : Number(c.period.slice(5)) - 1;
  const monthName = t.common.monthsLong[monthIndex] ?? c.period;
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");

  const amount = c.amountMinor;
  const used = amount !== null && amount > 0 ? c.actualMinor / amount : 0;
  const elapsed = c.elapsedBps / 10_000;
  const over = c.remainingMinor < 0;
  const income = p.kind === "income";
  // Spending faster than the calendar is the warning for an expense; for
  // income it is earning *slower* that deserves one, so the same comparison
  // reads in opposite directions and cannot be shared.
  const ahead = used > elapsed + 0.05;
  const behind = used < elapsed - 0.05;
  const tone = income
    ? over || ahead
      ? "good"
      : behind
        ? "warn"
        : "calm"
    : over
      ? "bad"
      : ahead
        ? "warn"
        : "calm";

  const done = () => {
    setEditing(false);
    onRefresh();
  };
  const fail = (e: unknown) => onError(msg(e));
  const save = (everyMonth = false) => {
    const minor = parseAmount(draft);
    if (minor === null) {
      onError(b.notANumber);
      return;
    }
    (everyMonth
      ? api.budgetSaveEveryMonth(p.categoryId, year, minor)
      : api.budgetSave(p.categoryId, c.period, minor)
    )
      .then(done)
      .catch(fail);
  };
  const startEditing = () => {
    setDraft(amount === null ? "" : String(amount / 100));
    setEditing(true);
  };

  const editor = (
    <div className="mt-3 flex flex-wrap items-end gap-2">
      <TextField
        label={b.amount}
        value={draft}
        autoFocus
        className="min-w-[96px] flex-1"
        onChange={(e) => setDraft(e.currentTarget.value)}
        onKeyDown={(e) => e.key === "Enter" && save()}
      />
      <Button variant="primary" onClick={() => save()}>
        {t.common.save}
      </Button>
      {!isYear && (
        <Button variant="secondary" onClick={() => save(true)}>
          {b.everyMonth}
        </Button>
      )}
      <Button variant="ghost" onClick={() => setEditing(false)}>
        <X className="size-3.5" strokeWidth={2} />
      </Button>
    </div>
  );

  const name = (
    <div className="flex items-center gap-1.5">
      {p.color && <span className="size-2 shrink-0 rounded-full" style={{ background: p.color }} />}
      <span className="truncate text-[13px] text-ink">{p.categoryName}</span>
    </div>
  );

  // A category budgeted for other months has nothing to be measured against
  // in this one; it says so rather than pretending the plan was zero.
  if (amount === null) {
    return (
      <div className="rounded-[10px] border border-dashed border-rule-strong px-4 py-3.5">
        {name}
        <p className="mt-1.5 text-[12px] text-ink-3">
          {b.noPlan(monthName)}
          {c.actualMinor > 0 && ` · ${b.noPlanSpent(fmtMoneyExact(c.actualMinor))}`}
        </p>
        {editing ? (
          editor
        ) : (
          <button
            type="button"
            onClick={startEditing}
            className="mt-2 text-[11px] text-brass underline-offset-2 hover:underline"
          >
            {b.setFigure}
          </button>
        )}
      </div>
    );
  }

  const notStarted = c.elapsedBps === 0 && c.daysLeft === c.daysTotal;

  return (
    <div
      className={cn(
        "rounded-[10px] border bg-surface px-4 py-3.5 transition-colors",
        tone === "bad" ? "border-negative/40" : "border-rule",
      )}
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          {name}
          <p className="mt-1.5">
            <span
              className={cn(
                "tnum text-[19px] leading-none",
                tone === "bad" && "text-negative",
                tone === "good" && "text-positive",
                tone !== "bad" && tone !== "good" && "text-ink",
              )}
            >
              {fmtMoney(Math.abs(c.remainingMinor))}
            </span>
            <span className="ml-1.5 text-[11px] text-ink-3">
              {b.headline(over, income)} {fmtMoney(amount)}
            </span>
          </p>
        </div>

        {isYear && (
          <button
            type="button"
            title={b.remove}
            aria-label={b.remove}
            onClick={() => api.budgetRemove(p.categoryId, c.period).then(onRefresh).catch(fail)}
            className="shrink-0 rounded p-1 text-ink-3 transition-colors hover:text-negative"
          >
            <Trash2 className="size-3.5" strokeWidth={1.75} />
          </button>
        )}
      </div>

      <Meter used={used} elapsed={elapsed} tone={tone} label={b.elapsed(Math.round(elapsed * 100))} />

      <div className="mt-2 flex items-baseline justify-between gap-2">
        <span className="text-[11px] text-ink-3">
          {b.soFar(
            isYear ? c.period : `${t.common.months[monthIndex]} ${year}`,
            fmtMoneyExact(c.actualMinor),
            c.txnCount,
          )}
        </span>
        <span className="text-right text-[11px] text-ink-3">
          {notStarted ? b.notStarted : pace(t, c, income)}
        </span>
      </div>

      {isYear && <MonthStrip plan={p} income={income} onPick={onPickMonth} />}

      {/* Where this figure comes from: the half of "they work together"
          that would otherwise be invisible. */}
      <p className="mt-2.5 text-[11px] leading-[1.5] text-ink-3">
        {isYear
          ? !c.own
            ? b.yearFromMonths(p.monthsSet)
            : p.monthsSet > 0
              ? b.yearWithOwn(p.monthsSet)
              : b.hintYear(fmtMoney(Math.round(amount / 12)))
          : c.own
            ? b.ownMonth
            : b.fromYear(fmtMoney(p.year.amountMinor ?? 0))}
        {isYear && c.own && p.monthsSumMinor > amount && (
          <span className="text-negative"> {b.monthsExceed(fmtMoney(p.monthsSumMinor))}</span>
        )}
      </p>

      {editing ? (
        editor
      ) : (
        <div className="mt-1.5 flex flex-wrap gap-x-4 gap-y-1">
          <button
            type="button"
            onClick={startEditing}
            className="text-[11px] text-ink-3 underline-offset-2 transition-colors hover:text-ink hover:underline"
          >
            {b.changeFigure}
          </button>
          {!isYear && c.own && (
            <button
              type="button"
              onClick={() => api.budgetRemove(p.categoryId, c.period).then(onRefresh).catch(fail)}
              className="text-[11px] text-ink-3 underline-offset-2 transition-colors hover:text-ink hover:underline"
            >
              {p.year.own ? b.followYear : b.clearMonth}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * The year as twelve small bars: what each month did against its own plan.
 *
 * It is the bridge between the two views — the year card shows which months
 * broke it, and a click on one goes there.
 */
function MonthStrip({
  plan,
  income,
  onPick,
}: {
  plan: BudgetPlan;
  income: boolean;
  onPick: (month: number) => void;
}) {
  const t = useT();
  const b = t.ledger.budgets;
  const peak = Math.max(1, ...plan.months.map((m) => Math.max(m.actualMinor, m.amountMinor ?? 0)));
  return (
    <div className="mt-3 flex h-9 items-end gap-[3px]">
      {plan.months.map((m, i) => {
        const planned = m.amountMinor;
        const overMonth = planned !== null && m.actualMinor > planned;
        const future = m.elapsedBps === 0;
        return (
          <button
            key={m.period}
            type="button"
            onClick={() => onPick(i)}
            title={
              planned === null
                ? b.monthBarNoPlan(t.common.months[i] ?? "", fmtMoney(m.actualMinor))
                : b.monthBar(t.common.months[i] ?? "", fmtMoney(m.actualMinor), fmtMoney(planned))
            }
            className="group relative h-full flex-1"
          >
            {/* The plan, as an outline to be filled. */}
            {planned !== null && (
              <span
                className={cn(
                  "absolute inset-x-0 bottom-0 rounded-[2px] border border-rule-strong/70",
                  m.own && "border-brass/70",
                )}
                style={{ height: `${Math.max((planned / peak) * 100, 6)}%` }}
              />
            )}
            {!future && (
              <span
                className={cn(
                  "absolute inset-x-0 bottom-0 rounded-[2px] transition-opacity group-hover:opacity-70",
                  planned === null
                    ? "bg-ink-3/40"
                    : income
                      ? overMonth
                        ? "bg-positive/70"
                        : "bg-ink-3/60"
                      : overMonth
                        ? "bg-negative/70"
                        : "bg-ink-3/60",
                )}
                style={{ height: `${(m.actualMinor / peak) * 100}%` }}
              />
            )}
          </button>
        );
      })}
    </div>
  );
}

/** What the remaining money buys per remaining day, in plain words. */
function pace(t: Dict, c: BudgetCell, income: boolean): string {
  const b = t.ledger.budgets;
  if (c.daysLeft <= 0) return b.closed;
  if (c.remainingMinor <= 0) {
    return income ? b.targetMet(c.daysLeft) : b.stillToCover(c.daysLeft);
  }
  const perDay = Math.round(c.remainingMinor / c.daysLeft);
  return income
    ? b.perDayEarn(fmtMoneyExact(perDay))
    : b.perDaySpend(fmtMoneyExact(perDay), c.daysLeft);
}

/**
 * Spent as a filled bar, with today's position marked on it.
 *
 * The marker is the whole point: a bar on its own says how much is gone, and
 * the only way to read that as good or bad is to know how much of the period
 * is gone too.
 */
function Meter({
  used,
  elapsed,
  tone,
  label,
}: {
  used: number;
  elapsed: number;
  tone: string;
  label: string;
}) {
  const pct = Math.min(used, 1) * 100;
  const mark = Math.min(Math.max(elapsed, 0), 1) * 100;
  return (
    <div className="relative mt-3 h-1.5 w-full rounded-full bg-surface-hi">
      <div
        className={cn(
          "h-full rounded-full transition-[width]",
          tone === "bad" && "bg-negative",
          tone === "warn" && "bg-brass",
          tone === "good" && "bg-positive",
          tone === "calm" && "bg-ink-3",
        )}
        style={{ width: `${pct}%` }}
      />
      {elapsed > 0 && elapsed < 1 && (
        <span
          title={label}
          className="absolute -top-1 h-3.5 w-px bg-ink"
          style={{ left: `${mark}%` }}
        />
      )}
    </div>
  );
}

type Scope = "year" | "every" | "one";

function BudgetForm({
  year,
  month,
  view,
  categories,
  plans,
  onSaved,
  onError,
}: {
  year: number;
  month: number;
  view: View;
  categories: Category[];
  plans: BudgetPlan[];
  onSaved: () => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const b = t.ledger.budgets;
  const [categoryId, setCategoryId] = useState<number | null>(null);
  // Whoever is looking at months is thinking in months.
  const [scope, setScope] = useState<Scope>(view === "month" ? "every" : "year");
  const [amount, setAmount] = useState("");

  useEffect(() => {
    if (categoryId === null && categories.length > 0) {
      // The first category without a budget is the likeliest next one.
      const free = categories.find((c) => !plans.some((p) => p.categoryId === c.id));
      setCategoryId((free ?? categories[0]!).id);
    }
  }, [categories, categoryId, plans]);

  const existing = plans.find((p) => p.categoryId === categoryId);
  const clash =
    existing !== undefined &&
    (scope === "one" ? existing.months[month]!.own : scope === "year" ? existing.year.own : true);
  const minor = parseAmount(amount);
  const period = `${year}-${String(month + 1).padStart(2, "0")}`;

  const hint = clash
    ? b.clash
    : minor === null
      ? undefined
      : scope === "year"
        ? b.hintYear(fmtMoney(Math.round(minor / 12)))
        : scope === "every"
          ? b.hintMonth(fmtMoney(minor * 12))
          : undefined;

  return (
    <div className="grid grid-cols-1 gap-3 border-b border-rule bg-brass-wash/30 px-5 py-4 sm:grid-cols-4">
      <SelectField
        label={b.category}
        value={categoryId ?? ""}
        onChange={(e) => setCategoryId(Number(e.currentTarget.value))}
      >
        {categories.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </SelectField>
      <SelectField
        label={b.plannedAs}
        value={scope}
        onChange={(e) => setScope(e.currentTarget.value as Scope)}
      >
        <option value="year">{b.perYear(year)}</option>
        <option value="every">{b.perMonth}</option>
        <option value="one">{b.oneMonth(`${t.common.monthsLong[month]} ${year}`)}</option>
      </SelectField>
      <TextField
        label={b.amount}
        value={amount}
        placeholder={scope === "year" ? "1200" : "100"}
        {...(hint ? { hint } : {})}
        onChange={(e) => setAmount(e.currentTarget.value)}
      />
      <div className="flex items-start pt-[22px]">
        <Button
          variant="primary"
          disabled={categoryId === null || amount.trim() === ""}
          onClick={() => {
            if (minor === null || categoryId === null) {
              onError(b.notANumber);
              return;
            }
            (scope === "every"
              ? api.budgetSaveEveryMonth(categoryId, year, minor)
              : api.budgetSave(categoryId, scope === "year" ? String(year) : period, minor)
            )
              .then(() => {
                setAmount("");
                onSaved();
              })
              .catch((e: unknown) => onError(msg(e)));
          }}
        >
          {b.save}
        </Button>
      </div>
    </div>
  );
}
