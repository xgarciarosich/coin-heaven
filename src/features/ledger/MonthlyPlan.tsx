import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronLeft, ChevronRight, GripVertical, Plus, Trash2, X } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { Amount } from "@/components/ui/amount";
import { Banner, Button, Empty } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { fmtMoney, fmtMoneyExact } from "@/lib/money";
import type { MonthPlan, PlanItem } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/* ------------------------------------------------------------- months -- */

const thisMonth = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
};

const addMonths = (period: string, by: number) => {
  const [y, m] = period.split("-").map(Number) as [number, number];
  const idx = y * 12 + (m - 1) + by;
  return `${Math.floor(idx / 12)}-${String((idx % 12) + 1).padStart(2, "0")}`;
};

/** Whole amounts without cents, amounts with cents with them. */
const money = (m: number) => (m % 100 === 0 ? fmtMoney(m) : fmtMoneyExact(m));

/**
 * A worksheet for one month: boxes of income and spending, in whatever order
 * makes sense to the person, with what is left at the bottom — the thing most
 * people keep in a spreadsheet — and every month they have kept to look back
 * on.
 *
 * It is separate from budgets on purpose. A budget is a target Coin Heaven measures
 * transactions against; this is the sum written before the month, with any
 * names and no categories, and nothing else needs to agree with it.
 */
export function MonthlyPlan() {
  const t = useT();
  const p = t.plan;
  const [plans, setPlans] = useState<MonthPlan[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [period, setPeriod] = useState(thisMonth());
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);

  const refresh = useCallback(
    () =>
      api
        .planList()
        .then((l) => {
          setPlans(l);
          setLoaded(true);
        })
        .catch((e: unknown) => setError(msg(e))),
    [],
  );
  useEffect(() => {
    void refresh();
  }, [refresh]);

  const label = (per: string) => {
    const [y, m] = per.split("-").map(Number) as [number, number];
    return `${t.common.monthsLong[m - 1] ?? per} ${y}`;
  };
  const short = (per: string) => {
    const [y, m] = per.split("-").map(Number) as [number, number];
    return `${t.common.months[m - 1] ?? per} ${y}`;
  };

  const current = plans.find((x) => x.period === period) ?? null;
  // What "start from" copies: the nearest plan before this month, or failing
  // that the nearest after it — someone filling in a past year starts from
  // the plan that came next.
  const source =
    [...plans].reverse().find((x) => x.period < period) ??
    plans.find((x) => x.period > period) ??
    null;

  const start = (from: MonthPlan | null) =>
    api
      .planSave(period, from?.items ?? [])
      .then(() => refresh())
      .catch((e: unknown) => setError(msg(e)));

  return (
    <div className="space-y-5">
      {error && <Banner tone="bad">{error}</Banner>}

      <Panel>
        <PanelHead
          label={label(period)}
          aside={
            <div className="flex items-center gap-1">
              <Button
                variant="ghost"
                aria-label={p.prev}
                title={p.prev}
                onClick={() => {
                  setConfirming(false);
                  setPeriod(addMonths(period, -1));
                }}
              >
                <ChevronLeft className="size-4" strokeWidth={1.75} />
              </Button>
              <Button
                variant="ghost"
                disabled={period === thisMonth()}
                onClick={() => {
                  setConfirming(false);
                  setPeriod(thisMonth());
                }}
              >
                {p.thisMonth}
              </Button>
              <Button
                variant="ghost"
                aria-label={p.next}
                title={p.next}
                onClick={() => {
                  setConfirming(false);
                  setPeriod(addMonths(period, 1));
                }}
              >
                <ChevronRight className="size-4" strokeWidth={1.75} />
              </Button>
            </div>
          }
        />

        {current ? (
          <Sheet
            // A different month is a different sheet: its typed values must
            // not survive the switch, and the one leaving is flushed first.
            key={period}
            period={period}
            initial={current.items}
            onSaved={() => void refresh()}
            onError={setError}
            footerAside={
              confirming ? (
                <div className="flex items-center gap-2">
                  <span className="text-[12px] text-ink-2">{p.deleteSure}</span>
                  <Button variant="ghost" onClick={() => setConfirming(false)}>
                    {t.common.cancel}
                  </Button>
                  <Button
                    variant="danger"
                    onClick={() =>
                      api
                        .planDelete(period)
                        .then(() => {
                          setConfirming(false);
                          return refresh();
                        })
                        .catch((e: unknown) => setError(msg(e)))
                    }
                  >
                    {p.deleteGo}
                  </Button>
                </div>
              ) : (
                <Button variant="ghost" onClick={() => setConfirming(true)}>
                  <Trash2 className="size-3" strokeWidth={1.75} />
                  {p.deleteMonth}
                </Button>
              )
            }
          />
        ) : (
          loaded && (
            <PanelBody>
              <Empty
                title={p.noPlan(label(period))}
                body={p.noPlanBody}
                action={
                  <div className="flex flex-wrap items-center justify-center gap-2">
                    {source && (
                      <Button variant="primary" onClick={() => void start(source)}>
                        {p.startFrom(label(source.period))}
                      </Button>
                    )}
                    <Button variant={source ? "secondary" : "primary"} onClick={() => void start(null)}>
                      {p.startEmpty}
                    </Button>
                  </div>
                }
              />
            </PanelBody>
          )
        )}
      </Panel>

      {plans.length > 0 && (
        <div className="space-y-2">
          <div className="flex items-baseline justify-between gap-4">
            <p className="eyebrow">{p.history}</p>
            <p className="text-[11px] text-ink-3">{p.historyNote}</p>
          </div>
          <div className="flex gap-2 overflow-x-auto pb-1">
            {[...plans].reverse().map((m) => {
              const on = m.period === period;
              return (
                <button
                  key={m.period}
                  type="button"
                  onClick={() => {
                    setConfirming(false);
                    setPeriod(m.period);
                  }}
                  className={cn(
                    "min-w-[148px] shrink-0 rounded-[10px] border px-4 py-3 text-left transition-colors",
                    on
                      ? "border-brass bg-surface shadow-card"
                      : "border-rule bg-surface hover:border-rule-strong",
                  )}
                >
                  <p className={cn("text-[11px]", on ? "text-brass" : "text-ink-3")}>
                    {short(m.period)}
                  </p>
                  <p
                    className={cn(
                      "tnum mt-1 text-[15px] leading-none",
                      m.netMinor < 0 ? "text-negative" : "text-ink",
                    )}
                  >
                    {fmtMoney(m.netMinor)}
                  </p>
                  <p className="tnum mt-1.5 text-[11px] text-ink-3">
                    {fmtMoney(m.incomeMinor)} − {fmtMoney(m.expenseMinor)}
                  </p>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

/* -------------------------------------------------------------- sheet -- */

interface Box {
  /** Stable across edits and reordering, so focus stays in the field typed in. */
  key: number;
  kind: "income" | "expense";
  name: string;
  amount: number;
  /** Just added by the person: put the cursor in its name. */
  fresh?: boolean;
}

let nextKey = 1;
type SaveState = "idle" | "waiting" | "saving" | "saved";

const move = <T,>(list: T[], from: number, to: number) => {
  const next = [...list];
  const [it] = next.splice(from, 1);
  next.splice(to, 0, it!);
  return next;
};

function Sheet({
  period,
  initial,
  onSaved,
  onError,
  footerAside,
}: {
  period: string;
  initial: PlanItem[];
  onSaved: () => void;
  onError: (m: string) => void;
  footerAside: React.ReactNode;
}) {
  const t = useT();
  const p = t.plan;
  const [boxes, setBoxes] = useState<Box[]>(() =>
    initial.map((i) => ({ key: nextKey++, kind: i.kind, name: i.name, amount: i.amountMinor })),
  );
  const [state, setState] = useState<SaveState>("idle");

  /* ---- saving ---------------------------------------------------------- */

  // The latest of everything the save needs, readable from a timer.
  const live = useRef({ boxes, onSaved, onError });
  live.current = { boxes, onSaved, onError };
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const running = useRef(false);
  const again = useRef(false);
  const mounted = useRef(true);

  const save = useCallback(async () => {
    if (running.current) {
      again.current = true;
      return;
    }
    running.current = true;
    if (mounted.current) setState("saving");
    try {
      const items: PlanItem[] = live.current.boxes
        // A box nobody named or priced is a mis-click, not a plan.
        .filter((b) => b.name.trim() !== "" || b.amount !== 0)
        .map((b) => ({ kind: b.kind, name: b.name.trim(), amountMinor: b.amount }));
      await api.planSave(period, items);
      if (mounted.current) setState("saved");
      live.current.onSaved();
    } catch (e) {
      live.current.onError(msg(e));
      if (mounted.current) setState("idle");
    } finally {
      running.current = false;
      if (again.current) {
        again.current = false;
        void save();
      }
    }
  }, [period]);

  /** Write what is on screen: at once, or once the typing has paused. */
  const queue = useCallback(
    (delay: number) => {
      if (timer.current) clearTimeout(timer.current);
      setState("waiting");
      timer.current = setTimeout(() => {
        timer.current = null;
        void save();
      }, delay);
    },
    [save],
  );

  // Leaving the sheet — another month, another tab — must not lose the last
  // thing typed.
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      if (timer.current) {
        clearTimeout(timer.current);
        timer.current = null;
        void save();
      }
    };
  }, [save]);

  const edit = (key: number, patch: Partial<Box>) => {
    setBoxes((bs) => bs.map((b) => (b.key === key ? { ...b, ...patch, fresh: false } : b)));
    queue(700);
  };
  const add = (kind: Box["kind"]) => {
    setBoxes((bs) => [...bs, { key: nextKey++, kind, name: "", amount: 0, fresh: true }]);
  };
  const remove = (key: number) => {
    setBoxes((bs) => bs.filter((b) => b.key !== key));
    queue(0);
  };

  /* ---- reordering ------------------------------------------------------ */

  // Pointer events rather than the browser's drag-and-drop: the latter is
  // intercepted by the window on some platforms, and cannot be driven from a
  // keyboard. The pointer is captured on the list, which stays put, because
  // a box that is moved in the DOM loses a capture held on itself.
  const listRef = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState<number | null>(null);
  const refocus = useRef<number | null>(null);

  const onDown = (e: React.PointerEvent, key: number) => {
    if (e.button !== 0) return;
    e.preventDefault();
    listRef.current?.setPointerCapture(e.pointerId);
    setDragging(key);
  };
  const onMove = (e: React.PointerEvent) => {
    if (dragging === null || !listRef.current) return;
    const els = [...listRef.current.querySelectorAll<HTMLElement>("[data-box]")];
    if (els.length === 0) return;
    // The box whose middle is nearest the pointer: gaps between boxes need no
    // special case.
    let target = 0;
    let best = Infinity;
    els.forEach((el, i) => {
      const r = el.getBoundingClientRect();
      const d = Math.abs(e.clientY - (r.top + r.height / 2));
      if (d < best) {
        best = d;
        target = i;
      }
    });
    const from = boxes.findIndex((b) => b.key === dragging);
    if (from !== -1 && target !== from) setBoxes((bs) => move(bs, from, target));
  };
  const onUp = (e: React.PointerEvent) => {
    if (dragging === null) return;
    listRef.current?.releasePointerCapture?.(e.pointerId);
    setDragging(null);
    queue(0);
  };

  const onHandleKey = (e: React.KeyboardEvent, key: number) => {
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    e.preventDefault();
    const from = boxes.findIndex((b) => b.key === key);
    const to = from + (e.key === "ArrowUp" ? -1 : 1);
    if (to < 0 || to >= boxes.length) return;
    refocus.current = key;
    setBoxes((bs) => move(bs, from, to));
    queue(400);
  };
  useEffect(() => {
    if (refocus.current === null) return;
    listRef.current
      ?.querySelector<HTMLElement>(`[data-handle="${refocus.current}"]`)
      ?.focus();
    refocus.current = null;
  }, [boxes]);

  /* ---- the figures ----------------------------------------------------- */

  const { running: balances, income, spend } = useMemo(() => {
    let sum = 0;
    let inc = 0;
    let out = 0;
    const r = boxes.map((b) => {
      if (b.kind === "income") {
        sum += b.amount;
        inc += b.amount;
      } else {
        sum -= b.amount;
        out += b.amount;
      }
      return sum;
    });
    return { running: r, income: inc, spend: out };
  }, [boxes]);
  const net = income - spend;

  const adder = (kind: Box["kind"], label: string) => (
    <button
      type="button"
      onClick={() => add(kind)}
      className={cn(
        "flex flex-1 items-center justify-center gap-1.5 rounded-[10px] border border-dashed px-4 py-3",
        "text-[12.5px] text-ink-3 transition-colors hover:text-ink",
        kind === "income"
          ? "border-positive/40 hover:border-positive hover:bg-positive/5"
          : "border-negative/40 hover:border-negative hover:bg-negative/5",
      )}
    >
      <Plus className="size-3.5" strokeWidth={2} />
      {label}
    </button>
  );

  return (
    <div>
      <div
        ref={listRef}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={onUp}
        className={cn("space-y-2 px-5 pt-4", dragging !== null && "cursor-grabbing select-none")}
      >
        {boxes.length === 0 ? (
          <p className="pb-2 text-[12.5px] text-ink-3">{p.noBoxes}</p>
        ) : (
          <div className="flex items-center gap-3 px-3 pb-0.5">
            <span className="w-4 shrink-0" />
            <span className="w-7 shrink-0" />
            <span className="flex-1" />
            <span className="eyebrow w-36 text-right">{p.amount}</span>
            <span className="eyebrow w-28 text-right">{p.runningHead}</span>
            <span className="w-6 shrink-0" />
          </div>
        )}

        {boxes.map((b, i) => {
          const income = b.kind === "income";
          const bal = balances[i] ?? 0;
          return (
            <div
              key={b.key}
              data-box
              className={cn(
                "group relative flex items-center gap-3 overflow-hidden rounded-[10px] border bg-surface py-1.5 pr-3 pl-3 transition-shadow",
                dragging === b.key
                  ? "border-brass shadow-card ring-1 ring-brass/40"
                  : "border-rule hover:border-rule-strong",
              )}
            >
              <span
                className={cn(
                  "absolute inset-y-0 left-0 w-[3px]",
                  income ? "bg-positive" : "bg-negative",
                )}
                aria-hidden
              />
              <button
                type="button"
                data-handle={b.key}
                aria-label={p.drag}
                title={p.drag}
                onPointerDown={(e) => onDown(e, b.key)}
                onKeyDown={(e) => onHandleKey(e, b.key)}
                className="w-4 shrink-0 cursor-grab touch-none rounded text-ink-3 outline-none hover:text-ink focus-visible:ring-1 focus-visible:ring-brass active:cursor-grabbing"
              >
                <GripVertical className="size-4" strokeWidth={1.75} />
              </button>
              <button
                type="button"
                title={income ? p.isIncome : p.isSpend}
                aria-label={income ? p.isIncome : p.isSpend}
                onClick={() => edit(b.key, { kind: income ? "expense" : "income" })}
                className={cn(
                  "w-7 shrink-0 rounded text-center text-[15px] leading-none font-medium transition-colors",
                  income
                    ? "text-positive hover:bg-positive/10"
                    : "text-negative hover:bg-negative/10",
                )}
              >
                {income ? "+" : "−"}
              </button>
              <input
                value={b.name}
                autoFocus={b.fresh}
                aria-label={income ? p.income : p.spend}
                placeholder={income ? p.incomePlaceholder : p.spendPlaceholder}
                onChange={(e) => edit(b.key, { name: e.currentTarget.value })}
                onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
                className="min-w-0 flex-1 rounded-md border border-transparent bg-transparent px-2 py-1.5 text-[13px] text-ink outline-none transition-colors placeholder:text-ink-3 hover:border-rule focus:border-brass focus:bg-surface"
              />
              <div className="w-36 shrink-0">
                <Amount
                  value={b.amount}
                  ariaLabel={p.amount}
                  onChange={(v) => edit(b.key, { amount: v })}
                />
              </div>
              <span
                className={cn(
                  "tnum w-28 shrink-0 text-right text-[12px]",
                  bal < 0 ? "text-negative" : "text-ink-3",
                )}
              >
                {money(bal)}
              </span>
              <button
                type="button"
                onClick={() => remove(b.key)}
                aria-label={p.remove}
                title={p.remove}
                className="w-6 shrink-0 rounded p-1 text-ink-3 opacity-0 transition-opacity hover:text-negative focus:opacity-100 group-hover:opacity-100"
              >
                <X className="size-3.5" strokeWidth={2} />
              </button>
            </div>
          );
        })}

        <div className="flex gap-2 pt-1 pb-4">
          {adder("income", p.addIncome)}
          {adder("expense", p.addSpend)}
        </div>
      </div>

      {boxes.length > 1 && (
        <p className="border-t border-rule px-5 py-2.5 text-[11px] leading-[1.6] text-ink-3">
          {p.runningNote}
        </p>
      )}

      <div className="flex flex-wrap items-end gap-x-8 gap-y-3 border-t border-rule bg-paper-sunk/60 px-5 py-4">
        <div>
          <p className="eyebrow">{p.totalIn}</p>
          <p className="tnum mt-1.5 text-[16px] leading-none text-ink">{money(income)}</p>
        </div>
        <div>
          <p className="eyebrow">{p.totalOut}</p>
          <p className="tnum mt-1.5 text-[16px] leading-none text-ink">{money(spend)}</p>
        </div>
        <div>
          <p className="eyebrow">{net < 0 ? p.short : p.net}</p>
          <p
            className={cn(
              "tnum mt-1.5 text-[26px] leading-none",
              net < 0 ? "text-negative" : net > 0 ? "text-positive" : "text-ink",
            )}
          >
            {money(Math.abs(net))}
          </p>
        </div>
        <p className="pb-0.5 text-[11.5px] leading-[1.5] text-ink-3">
          {net < 0 ? p.shortLine : p.leftLine}
          <br />
          {p.year(money(Math.abs(net) * 12))}
        </p>
        <span className="flex-1" />
        <div className="flex items-center gap-4">
          <span className="flex items-center gap-1.5 text-[11.5px] text-ink-3" aria-live="polite">
            {state === "saved" && <Check className="size-3.5 text-positive" strokeWidth={2} />}
            {state === "saving" || state === "waiting"
              ? p.saving
              : state === "saved"
                ? p.saved
                : p.autosave}
          </span>
          {footerAside}
        </div>
      </div>
    </div>
  );
}
