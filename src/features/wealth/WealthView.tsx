import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Area,
  AreaChart,
  CartesianGrid,
  Cell,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { Check, ChevronDown, ChevronRight, Plus, Settings2, Trash2, X } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import {
  Banner,
  Button,
  Checkbox,
  Empty,
  PageHead,
  Segmented,
  SelectField,
  TextField,
} from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { move } from "@/lib/useReorder";
import { Grip, Reorderable } from "@/components/ui/reorderable";
import { fmtAxis, fmtCompact, fmtDecimal, fmtMajor, fmtMoney, minorToMajor } from "@/lib/money";
import type { AssetClass, AssetKind, Snapshot } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { region } from "@/lib/region";
import { Amount } from "@/components/ui/amount";
import { PortfolioTab } from "./PortfolioTab";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
const today = () => new Date().toISOString().slice(0, 10);

const FALLBACK_COLOR = "var(--ink-3)";
/** For telling the items inside one class apart, where the class has one colour. */
const ITEM_COLORS = ["#6f8fbf", "#c98a4b", "#5fa38a", "#b0739f", "#9a9a4e", "#7d7fc4", "#c4716b", "#4f9aa8"];

type SubTab = "balance" | "portfolio";

const isManual = (l: { source?: string | null }) => !l.source || l.source === "manual";

/** A complete date between 1900 and today, as a date field reports it. */
function isWholeDate(d: string) {
  return /^\d{4}-\d{2}-\d{2}$/.test(d) && d >= "1900-01-01" && d <= today();
}

export function WealthView() {
  const t = useT();
  const [sub, setSub] = useState<SubTab>("balance");
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [classes, setClasses] = useState<AssetClass[]>([]);
  const [itemOrder, setItemOrder] = useState<Record<string, string[]>>({});
  const [prompt, setPrompt] = useState<{ due: boolean; suggestedDate: string; label: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  // The date whose balance is on screen; the latest one when nothing is picked.
  const [selected, setSelected] = useState<string | null>(null);
  // A date made a moment ago: it opens unfolded and says where it came from.
  const [fresh, setFresh] = useState<{ date: string; from: string | null } | null>(null);
  const [chart, setChart] = useState<"total" | "items">("total");
  const [managingClasses, setManagingClasses] = useState(false);

  const refresh = useCallback(
    () =>
      Promise.all([
        api.snapshotList(),
        api.snapshotPrompt(),
        api.assetClassList(),
        api.wealthItemOrder(),
      ])
        .then(([s, p, k, o]) => {
          setSnapshots(s);
          setPrompt(p);
          setClasses(k);
          setItemOrder(o);
          setLoaded(true);
        })
        .catch((e: unknown) => setError(msg(e))),
    [],
  );

  useEffect(() => {
    const t = setTimeout(refresh, 120);
    return () => clearTimeout(t);
  }, [refresh]);

  // One stack band per class the user actually holds something in. A property
  // contributes its equity — value less the loan against it — so the stack
  // sums to net worth rather than to gross assets, and a liability pulls the
  // stack down rather than sitting invisibly beside it.
  const bands = useMemo(
    () =>
      classes.filter((k) =>
        snapshots.some((s) => s.lines.some((l) => l.bucket === k.key)),
      ),
    [classes, snapshots],
  );

  // A balance recorded as a single total has no lines to break down. Showing
  // an empty chart would read as a bug; showing the one number there is does
  // not.
  const unbroken = bands.length === 0 && snapshots.length > 0;

  const series = useMemo(
    () =>
      snapshots.map((s) => {
        const row: Record<string, number | string> = {
          date: s.takenOn.slice(0, 7),
          net: minorToMajor(s.netWorthMinor),
        };
        for (const k of bands) {
          const signed = s.lines
            .filter((l) => l.bucket === k.key)
            .reduce((a, l) => a + worth(k.kind, l.grossValueMinor, l.debtMinor), 0);
          row[k.key] = minorToMajor(signed);
        }
        return row;
      }),
    [snapshots, bands],
  );

  const latest = snapshots.at(-1);
  const prior = snapshots.at(-2);
  const delta = latest && prior ? latest.netWorthMinor - prior.netWorthMinor : null;

  // Every item that has ever been written down, on any date. An item is its
  // class and its name; once it exists it is offered on every date, so a name
  // is typed once and not again for each date it applies to.
  const known = useMemo(() => {
    const seen = new Map<string, { bucket: string; label: string }>();
    for (const s of snapshots)
      for (const l of s.lines)
        if (isManual(l)) seen.set(`${l.bucket}\u0000${l.label}`, { bucket: l.bucket, label: l.label });
    return [...seen.values()];
  }, [snapshots]);

  /**
   * Open a date to write in. There is nothing to "save" afterwards: a new
   * date is recorded the moment it is asked for, as a copy of the one before
   * it, and every correction after that is kept as it is made.
   */
  const startNew = (date: string) => {
    if (snapshots.some((s) => s.takenOn === date)) {
      setSelected(date);
      return;
    }
    const from = [...snapshots].reverse().find((s) => s.takenOn < date) ?? null;
    const lines = (from?.lines ?? []).filter(isManual).map((l) => ({ ...l, id: null }));
    api
      .snapshotSave(null, { takenOn: date, label: null, lines })
      .then(() => refresh())
      .then(() => {
        setFresh({ date, from: from?.takenOn ?? null });
        setSelected(date);
      })
      .catch((e: unknown) => setError(msg(e)));
  };

  // What is on screen: the chosen date, else the latest one.
  const shown = snapshots.find((s) => s.takenOn === selected) ?? latest ?? null;
  const before = shown ? (snapshots[snapshots.indexOf(shown) - 1] ?? null) : null;

  return (
    <div className="min-w-0">
      <PageHead
        title={latest ? fmtMoney(latest.netWorthMinor) : t.wealth.title}
        {...(latest ? {} : { lede: t.wealth.lede })}
        actions={
          sub === "balance" && (
            <>
              {/* The classes are chosen while writing items down, so they are
                  managed from here rather than from a tab of their own. */}
              <Button
                variant={managingClasses ? "primary" : "secondary"}
                onClick={() => setManagingClasses((v) => !v)}
              >
                <Settings2 className="size-3.5" strokeWidth={1.75} />
                {t.wealth.tabs.classes}
              </Button>
              {snapshots.length > 0 && (
                <Button variant="primary" onClick={() => startNew(today())}>
                  <Plus className="size-3.5" strokeWidth={2} />
                  {t.wealth.newUpdate}
                </Button>
              )}
            </>
          )
        }
      />

      <div className="space-y-5 px-10 pb-12">
        {error && <Banner tone="bad">{error}</Banner>}

        <div className="flex items-center gap-1 border-b border-rule">
          {(
            [
              ["balance", t.wealth.tabs.balance],
              ["portfolio", t.portfolio.tabs.portfolio],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              onClick={() => setSub(id)}
              className={cn(
                "-mb-px border-b-2 px-3 py-2 text-[12.5px] transition-colors",
                sub === id
                  ? "border-brass text-ink"
                  : "border-transparent text-ink-3 hover:text-ink",
              )}
            >
              {label}
            </button>
          ))}
        </div>

        {sub === "balance" && managingClasses && (
          <div className="space-y-3 rounded-[10px] border border-brass/50 bg-surface p-4">
            <div className="flex items-center justify-between">
              <p className="eyebrow">{t.wealth.tabs.classes}</p>
              <Button
                variant="ghost"
                onClick={() => setManagingClasses(false)}
                aria-label={t.common.close}
              >
                <X className="size-3.5" strokeWidth={2} />
              </Button>
            </div>
            <ClassManager classes={classes} onChanged={refresh} onError={setError} />
          </div>
        )}

        {sub === "portfolio" ? (
          <PortfolioTab classes={classes} onChanged={refresh} />
        ) : snapshots.length === 0 ? (
          loaded && (
            <Empty
              title={t.wealth.noSnapshots}
              body={t.wealth.noSnapshotsBody}
              action={
                <Button variant="primary" onClick={() => startNew(today())}>
                  {t.wealth.recordFirst}
                </Button>
              }
            />
          )
        ) : (
          <>
            {prompt?.due && (
              <Banner>
                {t.wealth.due(longDate(latest?.takenOn ?? "", t.common.months))}{" "}
                <button
                  type="button"
                  className="text-brass underline underline-offset-2"
                  onClick={() => startNew(prompt.suggestedDate)}
                >
                  {t.wealth.recordFor(longDate(prompt.suggestedDate, t.common.months))}
                </button>
                .
              </Banner>
            )}

            <Panel>
              <PanelHead
                label={t.wealth.overTime}
                aside={
                  <div className="flex items-center gap-4">
                    {delta !== null && (
                      <span
                        className={cn(
                          "tnum text-[12px]",
                          delta >= 0 ? "text-positive" : "text-negative",
                        )}
                      >
                        {delta >= 0 ? "+" : ""}
                        {t.wealth.since(
                          fmtMoney(delta),
                          longDate(prior?.takenOn ?? "", t.common.months),
                        )}
                      </span>
                    )}
                    {snapshots.length > 1 && (
                      <div className="w-56">
                        <Segmented
                          options={[
                            ["total", t.wealth.chartTotal],
                            ["items", t.wealth.chartItems],
                          ]}
                          value={chart}
                          onChange={setChart}
                        />
                      </div>
                    )}
                  </div>
                }
              />
              {chart === "items" && snapshots.length > 1 ? (
                <ItemEvolution snapshots={snapshots} classes={classes} />
              ) : (
                <PanelBody className="pt-5 pr-3 pb-2 pl-1">
                  <div className="h-[240px] w-full">
                    <ResponsiveContainer width="100%" height="100%">
                      <AreaChart data={series} margin={{ top: 4, right: 8, bottom: 0, left: 4 }}>
                        <defs>
                          <linearGradient id="w-net" x1="0" y1="0" x2="0" y2="1">
                            <stop offset="0%" stopColor="var(--brass)" stopOpacity={0.3} />
                            <stop offset="100%" stopColor="var(--brass)" stopOpacity={0.05} />
                          </linearGradient>
                          {bands.map((b) => (
                            <linearGradient key={b.key} id={`w-${b.key}`} x1="0" y1="0" x2="0" y2="1">
                              <stop offset="0%" stopColor={b.color ?? FALLBACK_COLOR} stopOpacity={0.3} />
                              <stop offset="100%" stopColor={b.color ?? FALLBACK_COLOR} stopOpacity={0.05} />
                            </linearGradient>
                          ))}
                        </defs>
                        <CartesianGrid vertical={false} stroke="var(--rule)" />
                        <XAxis dataKey="date" axisLine={false} tickLine={false} tickMargin={10} />
                        <YAxis
                          axisLine={false}
                          tickLine={false}
                          width={46}
                          tickMargin={6}
                          tickFormatter={fmtAxis}
                        />
                        <Tooltip contentStyle={TOOLTIP} formatter={(v) => fmtMajor(Number(v))} />
                        {unbroken && (
                          <Area
                            type="monotone"
                            dataKey="net"
                            name={t.wealth.netWorth}
                            stroke="var(--brass)"
                            strokeWidth={1.5}
                            fill="url(#w-net)"
                          />
                        )}
                        {bands.map((b) => (
                          <Area
                            key={b.key}
                            type="monotone"
                            dataKey={b.key}
                            name={b.name}
                            stackId="nw"
                            stroke={b.color ?? FALLBACK_COLOR}
                            strokeWidth={1.25}
                            fill={`url(#w-${b.key})`}
                          />
                        ))}
                      </AreaChart>
                    </ResponsiveContainer>
                  </div>
                  <div className="mt-2 flex flex-wrap gap-4 pl-10">
                    {bands.map((b) => (
                      <span key={b.key} className="flex items-center gap-1.5 text-[11px] text-ink-3">
                        <span
                          className="size-2 rounded-full"
                          style={{ background: b.color ?? FALLBACK_COLOR }}
                        />
                        {b.name}
                      </span>
                    ))}
                  </div>
                </PanelBody>
              )}
            </Panel>

            <Timeline
              snapshots={snapshots}
              current={shown?.takenOn ?? null}
              onPick={setSelected}
              onNew={() => startNew(today())}
            />

            {shown && (
              <BalanceSheet
                // A different date is a different form: its typed values must
                // not survive the switch.
                key={shown.id}
                snapshot={shown}
                before={before}
                known={known}
                taken={snapshots.filter((s) => s.id !== shown.id).map((s) => s.takenOn)}
                fresh={fresh?.date === shown.takenOn ? fresh : null}
                classes={classes}
                itemOrder={itemOrder}
                onReorderClasses={(keys) =>
                  api
                    .assetClassReorder(keys)
                    .then(refresh)
                    .catch((e: unknown) => setError(msg(e)))
                }
                onSaved={(date) => {
                  // The sheet follows its date if the date itself was changed.
                  if (date !== shown.takenOn) setSelected(date);
                  setFresh((f) => (f && f.date === shown.takenOn ? { ...f, date } : f));
                  void refresh();
                }}
                onDeleted={() => {
                  setSelected(null);
                  void refresh();
                }}
                onError={setError}
              />
            )}

            {shown && <Composition snapshot={shown} classes={classes} />}
          </>
        )}
      </div>
    </div>
  );
}

const TOOLTIP = {
  background: "var(--surface)",
  border: "1px solid var(--rule)",
  borderRadius: 8,
  fontSize: 12,
};

/** "2026-06-30" as "30 Jun 2026": a date someone reads, not one they parse. */
function longDate(iso: string, months: readonly string[]) {
  const [y, m, d] = iso.split("-");
  if (!y || !m || !d) return iso;
  return `${Number(d)} ${months[Number(m) - 1] ?? m} ${y}`;
}

/** What a line adds to net worth: a liability takes away whatever it holds. */
const worth = (kind: string | undefined, gross: number, debt: number) =>
  kind === "liability" ? -(gross + debt) : gross - debt;

/* ------------------------------------------------------ item by item -- */

interface Track {
  label: string;
  /** One per recorded date; null where it was not held. */
  values: (number | null)[];
  first: number;
  last: number | null;
  change: number;
}

const track = (label: string, values: (number | null)[]): Track => {
  const held = values.filter((v): v is number => v !== null);
  const first = held[0] ?? 0;
  const last = values.at(-1) ?? null;
  return { label, values, first, last, change: (last ?? 0) - first };
};

/**
 * How each kind of thing moved through the recorded dates.
 *
 * The stacked chart shows what the total is made of; it cannot show whether
 * property or investments is the thing that moved, because every band rides
 * on the ones beneath it. Here each class gets its own line on its own scale,
 * with where it started, where it is, and the difference — and opens to show
 * the items inside it the same way, so the question "which one?" is one click
 * further rather than a long flat list to read.
 */
function ItemEvolution({ snapshots, classes }: { snapshots: Snapshot[]; classes: AssetClass[] }) {
  const t = useT();
  const w = t.wealth;
  const [open, setOpen] = useState<Set<string>>(new Set());

  const groups = useMemo(() => {
    return classes
      .map((cls) => {
        const items = new Map<string, (number | null)[]>();
        const total: (number | null)[] = snapshots.map(() => null);
        snapshots.forEach((s, i) => {
          for (const l of s.lines) {
            if (l.bucket !== cls.key) continue;
            const v = worth(cls.kind, l.grossValueMinor, l.debtMinor);
            const values = items.get(l.label) ?? snapshots.map(() => null);
            values[i] = (values[i] ?? 0) + v;
            items.set(l.label, values);
            total[i] = (total[i] ?? 0) + v;
          }
        });
        return {
          cls,
          total: track(cls.name, total),
          items: [...items.entries()]
            .map(([label, values]) => track(label, values))
            .sort((a, b) => Math.abs(b.last ?? 0) - Math.abs(a.last ?? 0)),
        };
      })
      .filter((g) => g.items.length > 0)
      .sort((a, b) => Math.abs(b.total.last ?? 0) - Math.abs(a.total.last ?? 0));
  }, [snapshots, classes]);

  const dates = useMemo(() => snapshots.map((s) => s.takenOn), [snapshots]);
  const months = t.common.months;
  const allOpen = groups.length > 0 && groups.every((g) => open.has(g.cls.key));

  const toggle = (key: string) =>
    setOpen((o) => {
      const next = new Set(o);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  return (
    <div>
      <div className="flex items-center gap-4 border-b border-rule px-5 py-2">
        <span className="eyebrow w-[200px] shrink-0">{w.item}</span>
        <span className="eyebrow min-w-0 flex-1 truncate text-center">
          {longDate(dates[0] ?? "", months)} → {longDate(dates.at(-1) ?? "", months)}
        </span>
        <span className="eyebrow w-24 text-right">{w.firstValue}</span>
        <span className="eyebrow w-24 text-right">{w.lastValue}</span>
        <span className="eyebrow w-32 text-right">{w.change}</span>
      </div>
      <div className="divide-y divide-rule/60">
        {groups.map((g) => {
          const color = g.cls.color ?? FALLBACK_COLOR;
          const isOpen = open.has(g.cls.key);
          return (
            <div key={g.cls.key}>
              <button
                type="button"
                aria-expanded={isOpen}
                onClick={() => toggle(g.cls.key)}
                className="flex w-full items-center gap-4 px-5 py-2.5 text-left transition-colors hover:bg-surface-hi/60"
              >
                <span className="flex w-[200px] shrink-0 items-center gap-2">
                  {isOpen ? (
                    <ChevronDown className="size-3.5 shrink-0 text-ink-3" strokeWidth={2} />
                  ) : (
                    <ChevronRight className="size-3.5 shrink-0 text-ink-3" strokeWidth={2} />
                  )}
                  <span className="size-2.5 shrink-0 rounded-full" style={{ background: color }} />
                  <span className="min-w-0">
                    <span className="block truncate text-[13px] font-medium text-ink">{g.cls.name}</span>
                    <span className="block text-[10.5px] text-ink-3">{w.items(g.items.length)}</span>
                  </span>
                </span>
                <Trend values={g.total.values} dates={dates} color={color} height={44} />
                <Figures tr={g.total} strong />
              </button>

              {isOpen && (
                <div className="border-t border-rule/60 bg-paper-sunk/40 pb-2">
                  <ClassChart items={g.items} dates={dates} />
                  {g.items.map((it, i) => (
                    <div key={it.label} className="flex items-center gap-4 px-5 py-1.5">
                      <span className="flex w-[200px] shrink-0 items-center gap-2 pl-[22px]">
                        <span
                          className="size-2 shrink-0 rounded-[2px]"
                          style={{ background: ITEM_COLORS[i % ITEM_COLORS.length] }}
                        />
                        <span className="truncate text-[12.5px] text-ink-2">{it.label}</span>
                      </span>
                      <Trend
                        values={it.values}
                        dates={dates}
                        color={ITEM_COLORS[i % ITEM_COLORS.length]!}
                        height={30}
                      />
                      <Figures tr={it} />
                    </div>
                  ))}
                </div>
              )}
            </div>
          );
        })}
      </div>
      <div className="flex items-start gap-4 border-t border-rule px-5 py-2.5">
        <p className="flex-1 text-[11px] leading-[1.6] text-ink-3">{w.itemsNote}</p>
        <button
          type="button"
          onClick={() => setOpen(allOpen ? new Set() : new Set(groups.map((g) => g.cls.key)))}
          className="shrink-0 text-[11px] text-ink-3 underline-offset-2 hover:text-ink hover:underline"
        >
          {allOpen ? w.hideAll : w.showAll}
        </button>
      </div>
    </div>
  );
}

/** Started at, now, and the move between them. */
function Figures({ tr, strong }: { tr: Track; strong?: boolean }) {
  const t = useT();
  const pct = tr.first !== 0 ? (tr.change / Math.abs(tr.first)) * 100 : null;
  const up = tr.change >= 0;
  return (
    <>
      <span className="tnum w-24 text-right text-[12px] text-ink-3">{fmtMoney(tr.first)}</span>
      <span className={cn("tnum w-24 text-right text-[12.5px] text-ink", strong && "font-medium")}>
        {tr.last === null ? <span className="font-normal text-ink-3">{t.wealth.gone}</span> : fmtMoney(tr.last)}
      </span>
      <span
        className={cn(
          "tnum w-32 text-right text-[12px]",
          tr.change === 0 ? "text-ink-3" : up ? "text-positive" : "text-negative",
        )}
      >
        {tr.change === 0
          ? "—"
          : `${up ? "+" : "−"}${fmtCompact(Math.abs(tr.change))}${
              pct === null ? "" : ` · ${up ? "+" : "−"}${fmtDecimal(Math.abs(pct))}%`
            }`}
      </span>
    </>
  );
}

/**
 * A line through the dates something has a value on, scaled to itself and
 * spaced by the calendar: two updates a week apart sit close together, and a
 * year's gap looks like a year.
 */
function Trend({
  values,
  dates,
  color,
  height,
}: {
  values: (number | null)[];
  dates: string[];
  color: string;
  height: number;
}) {
  const t = useT();
  const [W, pad] = [300, 5];
  const held = values.filter((v): v is number => v !== null);
  const lo = Math.min(...held);
  const hi = Math.max(...held);
  const span = hi - lo || 1;
  const times = dates.map((d) => Date.parse(d));
  const t0 = times[0] ?? 0;
  const tSpan = (times.at(-1) ?? 0) - t0 || 1;
  const x = (i: number) => pad + (((times[i] ?? t0) - t0) / tSpan) * (W - pad * 2);
  const y = (v: number) =>
    hi === lo ? height / 2 : height - pad - ((v - lo) / span) * (height - pad * 2);
  const pts = values
    .map((v, i) => (v === null ? null : { x: x(i), y: y(v), v, date: dates[i] ?? "" }))
    .filter((p): p is { x: number; y: number; v: number; date: string } => p !== null);
  const line = pts.map((p, i) => `${i === 0 ? "M" : "L"}${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(" ");
  const first = pts[0];
  const last = pts.at(-1);
  const area =
    first && last && pts.length > 1
      ? `${line} L${last.x.toFixed(1)},${height} L${first.x.toFixed(1)},${height} Z`
      : null;
  return (
    <div className="relative min-w-0 flex-1" style={{ height }}>
      <svg
        viewBox={`0 0 ${W} ${height}`}
        preserveAspectRatio="none"
        className="absolute inset-0 size-full"
        aria-hidden
      >
        {lo < 0 && hi > 0 && (
          <line
            x1={pad}
            x2={W - pad}
            y1={y(0)}
            y2={y(0)}
            stroke="var(--rule-strong)"
            strokeDasharray="2 3"
            vectorEffect="non-scaling-stroke"
          />
        )}
        {area && <path d={area} fill={color} opacity={0.1} />}
        {pts.length > 1 && (
          <path
            d={line}
            fill="none"
            stroke={color}
            strokeWidth={1.5}
            strokeLinejoin="round"
            vectorEffect="non-scaling-stroke"
          />
        )}
      </svg>
      {/* The dots are elements rather than part of the drawing, so they stay
          round however wide the row is, and each can say its date and value. */}
      {pts.map((p, i) => (
        <span
          key={p.date}
          title={`${longDate(p.date, t.common.months)} · ${fmtMoney(p.v)}`}
          className={cn(
            "absolute -translate-x-1/2 -translate-y-1/2 rounded-full",
            i === pts.length - 1 ? "size-[7px]" : "size-[5px] opacity-60",
          )}
          style={{
            left: `${(p.x / W) * 100}%`,
            top: `${(p.y / height) * 100}%`,
            background: color,
          }}
        />
      ))}
    </div>
  );
}

/** The items of one class stacked on a shared scale, with dates and amounts. */
function ClassChart({ items, dates }: { items: Track[]; dates: string[] }) {
  const data = dates.map((d, i) => {
    const row: Record<string, number | string> = { date: d.slice(0, 7) };
    items.forEach((it, j) => {
      row[`i${j}`] = minorToMajor(it.values[i] ?? 0);
    });
    return row;
  });
  return (
    <div className="h-[190px] w-full pt-4 pr-5 pb-1 pl-2">
      <ResponsiveContainer width="100%" height="100%">
        <AreaChart data={data} margin={{ top: 4, right: 0, bottom: 0, left: 4 }}>
          <CartesianGrid vertical={false} stroke="var(--rule)" />
          <XAxis dataKey="date" axisLine={false} tickLine={false} tickMargin={8} />
          <YAxis axisLine={false} tickLine={false} width={46} tickMargin={6} tickFormatter={fmtAxis} />
          <Tooltip contentStyle={TOOLTIP} formatter={(v) => fmtMajor(Number(v))} />
          {items.map((it, j) => (
            <Area
              key={it.label}
              type="monotone"
              dataKey={`i${j}`}
              name={it.label}
              stackId="c"
              stroke={ITEM_COLORS[j % ITEM_COLORS.length]!}
              strokeWidth={1.25}
              fill={ITEM_COLORS[j % ITEM_COLORS.length]!}
              fillOpacity={0.16}
            />
          ))}
        </AreaChart>
      </ResponsiveContainer>
    </div>
  );
}

/* ----------------------------------------------------------- timeline -- */

/**
 * Every recorded date, newest first, each with what the total was and how it
 * had moved. It is the history and the navigation at once: the thing a list
 * of "snapshots" with edit buttons used to make someone work out.
 */
function Timeline({
  snapshots,
  current,
  onPick,
  onNew,
}: {
  snapshots: Snapshot[];
  current: string | null;
  onPick: (date: string) => void;
  onNew: () => void;
}) {
  const t = useT();
  return (
    <div className="flex gap-2 overflow-x-auto pb-1">
      <button
        type="button"
        onClick={onNew}
        className="flex min-w-[132px] shrink-0 flex-col items-start justify-center gap-1 rounded-[10px] border border-dashed border-rule-strong px-4 py-3 text-left text-ink-3 transition-colors hover:border-brass hover:text-ink"
      >
        <Plus className="size-4" strokeWidth={1.75} />
        <span className="text-[12px]">{t.wealth.newUpdate}</span>
      </button>
      {[...snapshots].reverse().map((s, i, all) => {
        const older = all[i + 1];
        const change = older ? s.netWorthMinor - older.netWorthMinor : null;
        const pct =
          older && older.netWorthMinor !== 0 && change !== null
            ? (change / Math.abs(older.netWorthMinor)) * 100
            : null;
        const on = current === s.takenOn;
        return (
          <button
            key={s.id}
            type="button"
            onClick={() => onPick(s.takenOn)}
            className={cn(
              "min-w-[150px] shrink-0 rounded-[10px] border px-4 py-3 text-left transition-colors",
              on
                ? "border-brass bg-surface shadow-card"
                : "border-rule bg-surface hover:border-rule-strong",
            )}
          >
            <p className={cn("text-[11px]", on ? "text-brass" : "text-ink-3")}>
              {longDate(s.takenOn, t.common.months)}
            </p>
            <p className="tnum mt-1 text-[15px] leading-none text-ink">{fmtMoney(s.netWorthMinor)}</p>
            <p
              className={cn(
                "tnum mt-1.5 text-[11px]",
                change === null ? "text-ink-3" : change >= 0 ? "text-positive" : "text-negative",
              )}
            >
              {change === null
                ? t.wealth.firstOne
                : `${change >= 0 ? "+" : "−"}${fmtCompact(Math.abs(change))}${
                    pct === null ? "" : ` · ${pct >= 0 ? "+" : ""}${fmtDecimal(pct)}%`
                  }`}
            </p>
          </button>
        );
      })}
    </div>
  );
}

/* ------------------------------------------------------ balance sheet -- */

interface Row {
  /** Stable across edits, so React keeps focus in the field being typed in. */
  key: number;
  bucket: string;
  label: string;
  gross: number;
  debt: number;
  /** The name it has in the history, when it has one: a change is a rename. */
  orig: string | null;
}

let nextKey = 1;

type SaveState = "idle" | "waiting" | "saving" | "saved";

/**
 * One date's balance, laid out the way someone would write it on paper: what
 * they own, then what they owe, each kind of thing as a line with what it
 * was last time, what it is now and the move — and under each kind, when it
 * is opened, the items that make it up.
 *
 * Nothing here is saved by pressing anything. A balance is a list of figures
 * someone corrects one at a time, usually while looking something up in
 * another window; a Save button at the bottom of that is a thing to forget.
 * Each correction is written a moment after it is made.
 */
function BalanceSheet({
  snapshot,
  before,
  known,
  taken,
  fresh,
  classes,
  itemOrder,
  onReorderClasses,
  onSaved,
  onDeleted,
  onError,
}: {
  snapshot: Snapshot;
  before: Snapshot | null;
  known: { bucket: string; label: string }[];
  /** The dates other balances already sit on. */
  taken: string[];
  fresh: { from: string | null } | null;
  classes: AssetClass[];
  /** Item names in the order the person arranged them, per class. */
  itemOrder: Record<string, string[]>;
  onReorderClasses: (orderedKeys: string[]) => void;
  onSaved: (date: string) => void;
  onDeleted: () => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const w = t.wealth;

  const [rows, setRows] = useState<Row[]>(() => {
    const mine = snapshot.lines.filter(isManual).map((l) => ({
      key: nextKey++,
      bucket: l.bucket,
      label: l.label,
      gross: l.grossValueMinor,
      debt: l.debtMinor,
      orig: l.label as string | null,
    }));
    // Items recorded on other dates are offered here too, empty, so a name
    // written once never has to be written again. They are only kept on
    // this date if they are given an amount.
    const others = known
      .filter((k) => !mine.some((r) => r.bucket === k.bucket && r.label === k.label))
      .map((k) => ({ key: nextKey++, ...k, gross: 0, debt: 0, orig: k.label as string | null }));
    // In the order the person dragged them into; anything new after that,
    // alphabetically.
    const place = (r: { bucket: string; label: string }) => {
      const i = (itemOrder[r.bucket] ?? []).indexOf(r.label);
      return i === -1 ? Number.MAX_SAFE_INTEGER : i;
    };
    return [...mine, ...others].sort(
      (a, b) => place(a) - place(b) || a.label.localeCompare(b.label),
    );
  });
  // The classes in the order shown, moved live while a heading is dragged.
  const [classOrder, setClassOrder] = useState(() => classes.map((c) => c.key));
  useEffect(() => setClassOrder(classes.map((c) => c.key)), [classes]);
  const classOrderNow = useRef(classOrder);
  classOrderNow.current = classOrder;
  const [takenOn, setTakenOn] = useState(snapshot.takenOn);
  // What is in the date box while it is being typed. A date field reports a
  // value on every keystroke ("0002-03-14" on the way to "2026-03-14"), and
  // saving each one moved the balance to a nonsense date and reset the box.
  const [dateDraft, setDateDraft] = useState(snapshot.takenOn);
  const [confirming, setConfirming] = useState(false);
  const [state, setState] = useState<SaveState>("idle");
  // A date just made is there to be filled in; one being looked back at is
  // read as its headings first.
  const [open, setOpen] = useState<Set<string>>(
    () => new Set(fresh ? classes.map((c) => c.key) : []),
  );

  // Portfolio lines are computed, not typed: the backend rewrites them from
  // the holdings every time this date is saved.
  const auto = snapshot.lines
    .filter((l) => !isManual(l))
    .map((l) => ({ bucket: l.bucket, label: l.label, gross: l.grossValueMinor }));

  /* ---- saving ---------------------------------------------------------- */

  // The latest of everything the save needs, readable from a timer.
  const live = useRef({ rows, takenOn, classes, taken, onSaved, onError, dateTaken: w.dateTaken });
  live.current = { rows, takenOn, classes, taken, onSaved, onError, dateTaken: w.dateTaken };
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const running = useRef(false);
  const again = useRef(false);
  const mounted = useRef(true);

  const save = useCallback(async () => {
    if (running.current) {
      again.current = true;
      return;
    }
    const { rows, takenOn, classes, taken, onSaved, onError, dateTaken } = live.current;
    if (taken.includes(takenOn)) {
      onError(dateTaken);
      if (mounted.current) setState("idle");
      return;
    }
    running.current = true;
    if (mounted.current) setState("saving");
    try {
      // A name changed here is changed on every date: the flat is one flat.
      // Unless another item of the same kind already has that name — then
      // the two would silently become one, on every date, for good.
      const renames = rows.filter(
        (r) =>
          r.orig !== null &&
          r.label.trim() !== "" &&
          r.label.trim() !== r.orig &&
          !rows.some(
            (o) =>
              o.key !== r.key &&
              o.bucket === r.bucket &&
              (o.label.trim() === r.label.trim() || o.orig === r.label.trim()),
          ),
      );
      for (const r of renames) await api.wealthLineRename(r.bucket, r.orig ?? "", r.label.trim());

      const kept = rows
        // A row with no amount is not held on this date: an item offered from
        // another date and left empty, or a mis-click.
        .filter((r) => r.gross !== 0 || r.debt !== 0);
      const lines = kept.map((r) => {
        const cls = classes.find((c) => c.key === r.bucket);
        const liability = cls?.kind === "liability";
        return {
          id: null,
          bucket: r.bucket,
          label: r.label.trim() || (cls?.name ?? r.bucket),
          // What is owed lives in one field, so it cannot be counted twice.
          grossValueMinor: liability ? r.gross + r.debt : r.gross,
          debtMinor: liability ? 0 : r.debt,
          currency: region().currency,
        };
      });
      await api.snapshotSave(snapshot.id, { takenOn, label: null, lines });

      if (mounted.current) {
        // What was just written is now each row's name in the history.
        const done = new Set([...renames, ...kept.filter((r) => r.orig === null)].map((r) => r.key));
        setRows((rs) =>
          rs.map((r) =>
            done.has(r.key) && r.label.trim() !== "" ? { ...r, orig: r.label.trim() } : r,
          ),
        );
        setState("saved");
      }
      onSaved(takenOn);
    } catch (e) {
      onError(msg(e));
      if (mounted.current) setState("idle");
    } finally {
      running.current = false;
      if (again.current) {
        again.current = false;
        void save();
      }
    }
  }, [snapshot.id]);

  /** Write what is on screen: at once, or once the typing has paused. */
  const queue = (delay: number) => {
    if (timer.current) clearTimeout(timer.current);
    setState("waiting");
    timer.current = setTimeout(() => {
      timer.current = null;
      void save();
    }, delay);
  };

  // Leaving the sheet — another date, another tab — must not lose the last
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

  const edit = (key: number, patch: Partial<Row>, delay: number | null = 700) => {
    setRows((rs) => rs.map((r) => (r.key === key ? { ...r, ...patch } : r)));
    if (delay !== null) queue(delay);
    else setState("waiting");
  };
  const add = (bucket: string) => {
    setRows((rs) => [...rs, { key: nextKey++, bucket, label: "", gross: 0, debt: 0, orig: null }]);
    setOpen((o) => new Set(o).add(bucket));
  };
  const remove = (key: number) => {
    setRows((rs) => rs.filter((r) => r.key !== key));
    queue(0);
  };

  /** Move one of `subset` (a part of `all`) and keep the rest where it is. */
  const moveWithin = <T,>(all: T[], subset: T[], from: number, to: number) => {
    const moved = move(subset, from, to);
    let i = 0;
    return all.map((x) => (subset.includes(x) ? moved[i++]! : x));
  };
  const moveClass = (keys: string[], from: number, to: number) =>
    setClassOrder((order) => moveWithin(order, keys, from, to));
  const moveItem = (bucket: string, from: number, to: number) =>
    setRows((rs) => moveWithin(rs, rs.filter((r) => r.bucket === bucket), from, to));
  const saveItemOrder = (bucket: string) => {
    const labels = live.current.rows
      .filter((r) => r.bucket === bucket)
      .map((r) => r.label.trim())
      .filter(Boolean);
    api.wealthItemReorder(bucket, labels).catch((e: unknown) => onError(msg(e)));
  };

  const prev = (bucket: string, label: string) =>
    before?.lines.find((l) => l.bucket === bucket && l.label === label) ?? null;

  const months = t.common.months;

  /* ---- the figures ----------------------------------------------------- */

  const ordered = classOrder
    .map((k) => classes.find((c) => c.key === k))
    .filter((c): c is AssetClass => c !== undefined);
  const sections = ordered.map((cls) => {
    const mine = rows.filter((r) => r.bucket === cls.key);
    const theirs = auto.filter((l) => l.bucket === cls.key);
    const now =
      mine.reduce((a, r) => a + worth(cls.kind, r.gross, r.debt), 0) +
      theirs.reduce((a, l) => a + worth(cls.kind, l.gross, 0), 0);
    const then = before
      ? before.lines
          .filter((l) => l.bucket === cls.key)
          .reduce((a, l) => a + worth(cls.kind, l.grossValueMinor, l.debtMinor), 0)
      : null;
    const held = mine.filter((r) => r.gross !== 0 || r.debt !== 0).length + theirs.length;
    return { cls, mine, theirs, now, then, held };
  });
  const owned = sections.filter((s) => s.cls.kind !== "liability");
  const owed = sections.filter((s) => s.cls.kind === "liability");
  const sum = (list: typeof sections, pick: (s: (typeof sections)[number]) => number | null) =>
    list.reduce((a, s) => a + (pick(s) ?? 0), 0);
  const ownNow = sum(owned, (s) => s.now);
  const oweNow = -sum(owed, (s) => s.now);
  const total = ownNow - oweNow;
  const change = before ? total - before.netWorthMinor : null;

  const allKeys = sections.filter((s) => s.mine.length + s.theirs.length > 0).map((s) => s.cls.key);
  const allOpen = allKeys.length > 0 && allKeys.every((k) => open.has(k));

  const group = (title: string, totalLabel: string, list: typeof sections, inverse: boolean) => {
    if (list.length === 0) return null;
    const now = inverse ? -sum(list, (s) => s.now) : sum(list, (s) => s.now);
    const then = before ? (inverse ? -sum(list, (s) => s.then) : sum(list, (s) => s.then)) : null;
    return (
      <div className="border-b border-rule last:border-b-0">
        <p className="eyebrow bg-paper-sunk/50 px-5 py-2">{title}</p>
        <Reorderable
          keys={list.map((x) => x.cls.key)}
          onMove={(from, to) => moveClass(list.map((x) => x.cls.key), from, to)}
          onDrop={() => onReorderClasses(classOrderNow.current)}
        >
          {(key, _i, item, grip, dragging) => {
          const { cls, mine, theirs, now: clsNow, then: clsThen, held } = list.find(
            (x) => x.cls.key === key,
          )!;
          const empty = mine.length === 0 && theirs.length === 0;
          const isOpen = open.has(cls.key);
          const liability = cls.kind === "liability";
          return (
            <div
              key={cls.key}
              {...item}
              className={cn("group/class border-t border-rule/60", dragging && "bg-brass-wash/30")}
            >
              <Line
                strong={held > 0}
                label={
                  <div className="flex items-center gap-2 py-1.5">
                    <Grip handle={grip} label={w.dragClass} className="-ml-4 group-hover/class:opacity-100" />
                    <button
                      type="button"
                      disabled={empty}
                      aria-expanded={isOpen}
                      onClick={() =>
                        setOpen((o) => {
                          const next = new Set(o);
                          if (next.has(cls.key)) next.delete(cls.key);
                          else next.add(cls.key);
                          return next;
                        })
                      }
                      className="flex min-w-0 items-center gap-2 text-left disabled:cursor-default"
                    >
                      {isOpen && !empty ? (
                        <ChevronDown className="size-3.5 shrink-0 text-ink-3" strokeWidth={2} />
                      ) : (
                        <ChevronRight
                          className={cn("size-3.5 shrink-0", empty ? "text-rule-strong" : "text-ink-3")}
                          strokeWidth={2}
                        />
                      )}
                      <span
                        className="size-2.5 shrink-0 rounded-full"
                        style={{ background: cls.color ?? FALLBACK_COLOR }}
                      />
                      <span
                        className={cn(
                          "truncate text-[13px]",
                          held > 0 ? "font-medium text-ink" : "text-ink-3",
                        )}
                      >
                        {cls.name}
                      </span>
                      {held > 0 && (
                        <span className="shrink-0 text-[11px] text-ink-3">{w.items(held)}</span>
                      )}
                    </button>
                    <button
                      type="button"
                      onClick={() => add(cls.key)}
                      className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-[11px] text-ink-3 transition-colors hover:bg-surface-hi hover:text-ink"
                    >
                      <Plus className="size-3" strokeWidth={2} />
                      {w.addTo}
                    </button>
                  </div>
                }
                was={clsThen === null || (clsThen === 0 && held === 0) ? null : liability ? -clsThen : clsThen}
                now={liability ? -clsNow : clsNow}
                hasBefore={before !== null && (held > 0 || (clsThen ?? 0) !== 0)}
                inverse={liability}
                quietNew
                amount={
                  held > 0 ? (
                    <span
                      className={cn(
                        "tnum block px-2.5 py-1.5 text-right text-[13px] font-medium",
                        !liability && clsNow < 0 ? "text-negative" : "text-ink",
                      )}
                    >
                      {fmtMoney(liability ? -clsNow : clsNow)}
                    </span>
                  ) : null
                }
              />

              {isOpen && !empty && (
                <div className="pb-2">
                  {theirs.map((l) => {
                    const was = prev(l.bucket, l.label);
                    return (
                      <Line
                        key={`auto-${l.label}`}
                        indent
                        label={
                          <span className="flex items-center gap-2 px-2 py-1.5">
                            <span className="truncate text-[12.5px] text-ink-2">{l.label}</span>
                            <span className="shrink-0 rounded bg-brass-wash px-1.5 py-0.5 text-[10px] text-brass">
                              {w.fromPortfolio}
                            </span>
                          </span>
                        }
                        was={was ? was.grossValueMinor : null}
                        now={l.gross}
                        hasBefore={before !== null}
                        amount={
                          <span className="tnum block px-2.5 py-1.5 text-right text-[12.5px] text-ink-2">
                            {fmtMoney(l.gross)}
                          </span>
                        }
                      />
                    );
                  })}

                  <Reorderable
                    keys={mine.map((r) => r.key)}
                    onMove={(from, to) => moveItem(cls.key, from, to)}
                    onDrop={() => saveItemOrder(cls.key)}
                  >
                  {(rowKey, _j, rowItem, rowGrip, rowDragging) => {
                    const r = mine.find((x) => x.key === rowKey)!;
                    const was = prev(r.bucket, r.orig ?? r.label);
                    const owedNow = r.gross + r.debt;
                    return (
                      <div
                        key={r.key}
                        {...rowItem}
                        className={cn(rowDragging && "rounded-md bg-brass-wash/40")}
                      >
                        <Line
                          indent
                          label={
                            <div className="flex items-center gap-1">
                            <Grip handle={rowGrip} label={w.dragItem} className="-ml-5" />
                            <input
                              value={r.label}
                              placeholder={cls.kind === "property" ? w.propertyPlaceholder : w.labelPlaceholder}
                              // A row just added has no name yet: start there.
                              autoFocus={r.label === "" && r.gross === 0}
                              // A name is written once it is finished, not
                              // letter by letter: it renames the item on
                              // every date.
                              onChange={(e) => edit(r.key, { label: e.currentTarget.value }, null)}
                              onBlur={() => queue(0)}
                              onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
                              className="w-full rounded-md border border-transparent bg-transparent px-2 py-1.5 text-[12.5px] text-ink outline-none transition-colors placeholder:text-ink-3 hover:border-rule focus:border-brass focus:bg-surface"
                            />
                            </div>
                          }
                          was={was ? (liability ? was.grossValueMinor + was.debtMinor : was.grossValueMinor) : null}
                          now={liability ? owedNow : r.gross}
                          hasBefore={before !== null}
                          inverse={liability}
                          amount={
                            <Amount
                              value={liability ? owedNow : r.gross}
                              onChange={(v) => edit(r.key, liability ? { gross: v, debt: 0 } : { gross: v })}
                            />
                          }
                          onRemove={() => remove(r.key)}
                          removeLabel={t.common.remove}
                        />
                        {cls.kind === "property" && (
                          <Line
                            indent
                            label={
                              <span className="block pl-5 text-[11.5px] text-ink-3">↳ {w.loan}</span>
                            }
                            was={was ? was.debtMinor : null}
                            now={r.debt}
                            hasBefore={before !== null}
                            inverse
                            amount={<Amount value={r.debt} onChange={(v) => edit(r.key, { debt: v })} />}
                          />
                        )}
                      </div>
                    );
                  }}
                  </Reorderable>
                </div>
              )}
            </div>
          );
          }}
        </Reorderable>
        <div className="border-t border-rule bg-paper-sunk/30">
          <Line
            strong
            label={<span className="block py-2 text-[12.5px] font-medium text-ink">{totalLabel}</span>}
            was={then}
            now={now}
            hasBefore={before !== null}
            inverse={inverse}
            amount={
              <span className="tnum block px-2.5 py-2 text-right text-[13px] font-medium text-ink">
                {fmtMoney(now)}
              </span>
            }
          />
        </div>
      </div>
    );
  };

  return (
    <Panel>
      <PanelHead
        label={w.balanceOn(longDate(takenOn, months))}
        aside={
          <div className="flex items-center gap-3">
            <span className="text-[11px] text-ink-3">{w.date}</span>
            <input
              type="date"
              value={dateDraft}
              min="1900-01-01"
              max={today()}
              onChange={(e) => {
                const d = e.currentTarget.value;
                setDateDraft(d);
                // Saved as soon as it is a whole, plausible date; the steps on
                // the way there are left alone.
                if (isWholeDate(d) && d !== takenOn) {
                  setTakenOn(d);
                  queue(600);
                }
              }}
              onBlur={() => {
                if (!isWholeDate(dateDraft)) setDateDraft(takenOn);
              }}
              className="w-[9.5rem] shrink-0 rounded-md border border-rule bg-surface px-2 py-1 text-[12px] text-ink outline-none focus:border-brass"
            />
          </div>
        }
      />

      {fresh && (
        <p className="border-b border-rule bg-brass-wash/30 px-5 py-2.5 text-[11.5px] leading-[1.6] text-ink-2">
          {fresh.from ? w.newFromLast(longDate(fresh.from, months)) : w.newFirst}
        </p>
      )}

      <div className="flex items-center gap-3 border-b border-rule px-5 py-2">
        <span className="flex-1">
          {allKeys.length > 0 && (
            <button
              type="button"
              onClick={() => setOpen(allOpen ? new Set() : new Set(allKeys))}
              className="eyebrow underline-offset-2 hover:text-ink hover:underline"
            >
              {allOpen ? w.hideAll : w.showAll}
            </button>
          )}
        </span>
        <span className="eyebrow w-28 text-right">
          {before ? longDate(before.takenOn, months) : ""}
        </span>
        <span className="eyebrow w-36 pr-2.5 text-right">{longDate(takenOn, months)}</span>
        <span className="eyebrow w-24 text-right">{before ? w.change : ""}</span>
        <span className="w-6" />
      </div>

      {group(w.own, w.totalOwn, owned, false)}
      {group(w.owe, w.totalOwe, owed, true)}

      <div className="flex flex-wrap items-end gap-x-6 gap-y-3 border-t border-rule bg-paper-sunk/60 px-5 py-4">
        <div>
          <p className="eyebrow">{w.netWorth}</p>
          <p className="tnum mt-1.5 text-[24px] leading-none text-ink">{fmtMoney(total)}</p>
        </div>
        <p className="tnum text-[12px] leading-none text-ink-3">
          {w.sumLine(fmtMoney(ownNow), fmtMoney(oweNow))}
        </p>
        {change !== null && before && (
          <p className={cn("tnum text-[12px] leading-none", change >= 0 ? "text-positive" : "text-negative")}>
            {change >= 0 ? "+" : ""}
            {w.since(fmtMoney(change), longDate(before.takenOn, months))}
          </p>
        )}
        <span className="flex-1" />
        {confirming ? (
          <div className="flex items-center gap-2">
            <span className="text-[12px] text-ink-2">{w.deleteSure}</span>
            <Button variant="ghost" onClick={() => setConfirming(false)}>
              {t.common.cancel}
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                // Nothing typed a moment ago should be written to a date
                // that is being removed.
                if (timer.current) clearTimeout(timer.current);
                timer.current = null;
                api
                  .snapshotDelete(snapshot.id)
                  .then(onDeleted)
                  .catch((e: unknown) => onError(msg(e)));
              }}
            >
              {t.common.delete}
            </Button>
          </div>
        ) : (
          <div className="flex items-center gap-4">
            <span className="flex items-center gap-1.5 text-[11.5px] text-ink-3" aria-live="polite">
              {state === "saved" && <Check className="size-3.5 text-positive" strokeWidth={2} />}
              {state === "saving" || state === "waiting"
                ? w.saving
                : state === "saved"
                  ? w.savedBalance
                  : w.autosave}
            </span>
            <Button variant="ghost" onClick={() => setConfirming(true)}>
              <Trash2 className="size-3" strokeWidth={1.75} />
              {w.deleteDate}
            </Button>
          </div>
        )}
      </div>
    </Panel>
  );
}

/* -------------------------------------------------------- composition -- */

/**
 * What the balance is made of, as shares of what is owned.
 *
 * Shares are of gross assets: a flat counts at its value and its loan sits on
 * the other side of the sheet, so a mortgaged home does not vanish from the
 * picture. Debts are said in words underneath rather than drawn as negative
 * slices, which a pie cannot show.
 */
function Composition({ snapshot, classes }: { snapshot: Snapshot; classes: AssetClass[] }) {
  const t = useT();
  const w = t.wealth;
  const [by, setBy] = useState<"item" | "class">("item");

  const slices = useMemo(() => {
    const owned = classes.filter((k) => k.kind !== "liability");
    const lines = snapshot.lines.filter(
      (l) => l.grossValueMinor > 0 && owned.some((k) => k.key === l.bucket),
    );
    const list =
      by === "class"
        ? owned.map((k) => ({
            key: k.key,
            name: k.name,
            value: lines.filter((l) => l.bucket === k.key).reduce((a, l) => a + l.grossValueMinor, 0),
            color: k.color ?? FALLBACK_COLOR,
          }))
        : lines.map((l, i) => ({
            key: `${l.bucket}:${l.label}:${i}`,
            name: l.label,
            value: l.grossValueMinor,
            color: ITEM_COLORS[i % ITEM_COLORS.length]!,
          }));
    return list.filter((x) => x.value > 0).sort((a, b) => b.value - a.value);
  }, [snapshot, classes, by]);

  const total = slices.reduce((a, x) => a + x.value, 0);
  if (total <= 0) return null;
  const share = (v: number) => `${((v / total) * 100).toFixed(v / total < 0.1 ? 1 : 0)} %`;

  return (
    <Panel>
      <PanelHead
        label={w.shareTitle}
        aside={
          <div className="w-48">
            <Segmented
              options={[
                ["item", w.shareByItem],
                ["class", w.shareByClass],
              ]}
              value={by}
              onChange={setBy}
            />
          </div>
        }
      />
      <PanelBody className="flex flex-wrap items-center gap-8">
        <div className="relative size-[200px] shrink-0">
          <ResponsiveContainer width="100%" height="100%">
            <PieChart>
              <Pie
                data={slices}
                dataKey="value"
                nameKey="name"
                innerRadius={62}
                outerRadius={96}
                paddingAngle={slices.length > 1 ? 1.5 : 0}
                stroke="var(--surface)"
                isAnimationActive={false}
              >
                {slices.map((x) => (
                  <Cell key={x.key} fill={x.color} />
                ))}
              </Pie>
              <Tooltip
                contentStyle={TOOLTIP}
                formatter={(v) => [`${fmtMoney(Number(v))} · ${share(Number(v))}`, ""]}
              />
            </PieChart>
          </ResponsiveContainer>
          <div className="pointer-events-none absolute inset-0 flex flex-col items-center justify-center">
            <span className="eyebrow">{w.shareOwned}</span>
            <span className="tnum mt-1 text-[15px] text-ink">{fmtCompact(total)}</span>
          </div>
        </div>
        <ul className="min-w-[220px] flex-1 space-y-1.5">
          {slices.map((x) => (
            <li key={x.key} className="flex items-center gap-2.5 text-[12.5px]">
              <span className="size-2.5 shrink-0 rounded-full" style={{ background: x.color }} />
              <span className="min-w-0 flex-1 truncate text-ink-2">{x.name}</span>
              <span className="tnum w-14 text-right text-ink">{share(x.value)}</span>
              <span className="tnum w-28 text-right text-ink-3">{fmtMoney(x.value)}</span>
            </li>
          ))}
          {snapshot.totalDebtMinor > 0 && (
            <li className="pt-2 text-[11.5px] leading-[1.6] text-ink-3">
              {w.shareDebtNote(fmtMoney(snapshot.totalDebtMinor))}
            </li>
          )}
        </ul>
      </PanelBody>
    </Panel>
  );
}

/** One row of the sheet: what it is, what it was, what it is now, the move. */
function Line({
  label,
  was,
  now,
  hasBefore,
  inverse,
  indent,
  strong,
  quietNew,
  amount,
  onRemove,
  removeLabel,
}: {
  label: React.ReactNode;
  was: number | null;
  now: number;
  hasBefore: boolean;
  /** Owing more is bad news: colour the move the other way round. */
  inverse?: boolean;
  /** An item under its heading. */
  indent?: boolean;
  strong?: boolean;
  /** A heading does not announce itself as "new"; its items do. */
  quietNew?: boolean;
  amount: React.ReactNode;
  onRemove?: () => void;
  removeLabel?: string;
}) {
  const t = useT();
  const move = was === null ? null : now - was;
  const good = move === null ? null : inverse ? move <= 0 : move >= 0;
  return (
    <div className={cn("group flex items-center gap-3 px-5", indent ? "py-0.5 pl-[50px]" : "py-0.5")}>
      <div className="min-w-0 flex-1">{label}</div>
      <span className={cn("tnum w-28 text-right text-ink-3", strong ? "text-[12.5px]" : "text-[12px]")}>
        {was === null ? (hasBefore && !quietNew ? t.wealth.isNew : "") : fmtMoney(was)}
      </span>
      <div className="w-36">{amount}</div>
      <span
        className={cn(
          "tnum w-24 text-right",
          strong ? "text-[12px]" : "text-[11.5px]",
          move === null || move === 0 ? "text-ink-3" : good ? "text-positive" : "text-negative",
        )}
      >
        {move === null || !hasBefore ? "" : move === 0 ? "—" : `${move > 0 ? "+" : "−"}${fmtCompact(Math.abs(move))}`}
      </span>
      <span className="w-6">
        {onRemove && (
          <button
            type="button"
            onClick={onRemove}
            aria-label={removeLabel}
            title={removeLabel}
            className="rounded p-1 text-ink-3 opacity-0 transition-opacity hover:text-negative focus:opacity-100 group-hover:opacity-100"
          >
            <X className="size-3" strokeWidth={2} />
          </button>
        )}
      </span>
    </div>
  );
}

/* ------------------------------------------------------ asset classes -- */

/**
 * The classes a snapshot line can be filed under.
 *
 * A name is free text; the *kind* is not, because net worth, the liquid FIRE
 * target and property equity are all defined in terms of it. So the editor
 * asks for a kind in plain language and says what each one does, rather than
 * letting someone invent a class whose meaning the arithmetic cannot know.
 */
function ClassManager({
  classes,
  onChanged,
  onError,
}: {
  classes: AssetClass[];
  onChanged: () => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const [adding, setAdding] = useState(false);
  const [removing, setRemoving] = useState<AssetClass | null>(null);

  return (
    <div className="space-y-5">
      <Panel>
        <PanelHead
          label={t.wealth.classes}
          aside={
            <Button variant="primary" onClick={() => setAdding((v) => !v)}>
              {adding ? (
                <X className="size-3.5" strokeWidth={2} />
              ) : (
                <Plus className="size-3.5" strokeWidth={2} />
              )}
              {adding ? t.common.cancel : t.wealth.newClass}
            </Button>
          }
        />

        {adding && (
          <ClassForm
            onSaved={() => {
              setAdding(false);
              onChanged();
            }}
            onError={onError}
          />
        )}

        <div className="divide-y divide-rule">
          {classes.map((k) => (
            <ClassRow
              key={k.key}
              cls={k}
              onChanged={onChanged}
              onError={onError}
              onRemove={() => setRemoving(k)}
            />
          ))}
        </div>

        <p className="border-t border-rule px-5 py-3 text-[11px] leading-[1.6] text-ink-3">
          {t.wealth.classesNote}
        </p>
      </Panel>

      {removing && (
        <RemoveClass
          cls={removing}
          classes={classes}
          onClose={() => setRemoving(null)}
          onDone={() => {
            setRemoving(null);
            onChanged();
          }}
          onError={onError}
        />
      )}
    </div>
  );
}

function ClassRow({
  cls,
  onChanged,
  onError,
  onRemove,
}: {
  cls: AssetClass;
  onChanged: () => void;
  onError: (m: string) => void;
  onRemove: () => void;
}) {
  const t = useT();
  const [editing, setEditing] = useState(false);

  if (editing) {
    return (
      <ClassForm
        cls={cls}
        onSaved={() => {
          setEditing(false);
          onChanged();
        }}
        onCancel={() => setEditing(false)}
        onError={onError}
      />
    );
  }

  return (
    <div className="flex items-center gap-4 px-5 py-2.5">
      <span
        className="size-2.5 shrink-0 rounded-full"
        style={{ background: cls.color ?? FALLBACK_COLOR }}
      />
      <span className="w-44 shrink-0 truncate text-[12.5px] text-ink">{cls.name}</span>
      <span className="w-24 shrink-0 rounded bg-brass-wash px-1.5 py-0.5 text-center text-[10px] text-brass">
        {t.wealth.kinds[cls.kind] ?? cls.kind}
      </span>
      <span className="min-w-0 flex-1 truncate text-[11px] text-ink-3">
        {t.wealth.kindNotes[cls.kind]}
      </span>
      {cls.kind !== "liability" && (
        <span
          className={cn(
            "shrink-0 rounded px-1.5 py-0.5 text-[10px]",
            cls.countsFire ? "bg-positive/10 text-positive" : "bg-surface-hi text-ink-3",
          )}
        >
          {cls.countsFire ? t.wealth.inFire : t.wealth.notInFire}
        </span>
      )}
      <span className="tnum w-14 shrink-0 text-right text-[11px] text-ink-3">
        {t.common.lines(cls.lineCount)}
      </span>
      <Button variant="ghost" onClick={() => setEditing(true)}>
        {t.common.edit}
      </Button>
      <Button variant="ghost" onClick={onRemove} aria-label={t.wealth.removeLabel(cls.name)}>
        <Trash2 className="size-3" strokeWidth={1.75} />
      </Button>
    </div>
  );
}

function ClassForm({
  cls,
  onSaved,
  onCancel,
  onError,
}: {
  cls?: AssetClass;
  onSaved: () => void;
  onCancel?: () => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const [name, setName] = useState(cls?.name ?? "");
  const [kind, setKind] = useState<AssetKind>(cls?.kind ?? "liquid");
  const [color, setColor] = useState(cls?.color ?? "#94a3b8");
  const [countsFire, setCountsFire] = useState(cls?.countsFire ?? true);

  return (
    <div className="space-y-3 border-b border-rule bg-brass-wash/30 px-5 py-4">
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-4">
        <TextField
          label={t.wealth.name}
          value={name}
          autoFocus
          placeholder={t.wealth.namePlaceholder}
          className="sm:col-span-2"
          onChange={(e) => setName(e.currentTarget.value)}
        />
        <SelectField
          label={t.wealth.behavesAs}
          value={kind}
          onChange={(e) => {
            const k = e.currentTarget.value as AssetKind;
            setKind(k);
            // A sensible starting answer for the kind; the box stays theirs.
            if (!cls) setCountsFire(k === "liquid");
          }}
        >
          {(Object.keys(t.wealth.kinds) as AssetKind[]).map((k) => (
            <option key={k} value={k}>
              {t.wealth.kinds[k]}
            </option>
          ))}
        </SelectField>
        <div className="space-y-1.5">
          <span className="eyebrow block">{t.wealth.colour}</span>
          <input
            type="color"
            value={color}
            onChange={(e) => setColor(e.currentTarget.value)}
            className="h-[31px] w-full cursor-pointer rounded-md border border-rule bg-surface px-1"
          />
        </div>
      </div>

      {kind !== "liability" && (
        <div className="space-y-1">
          <Checkbox
            label={t.wealth.countsFire}
            checked={countsFire}
            onChange={(e) => setCountsFire(e.currentTarget.checked)}
          />
          <p className="pl-[22px] text-[11px] leading-[1.5] text-ink-3">{t.wealth.countsFireHint}</p>
        </div>
      )}

      <div className="flex items-center justify-between gap-4">
        <p className="text-[11px] leading-[1.6] text-ink-3">{t.wealth.kindNotes[kind]}</p>
        <div className="flex shrink-0 gap-2">
          {onCancel && (
            <Button variant="ghost" onClick={onCancel}>
              {t.common.cancel}
            </Button>
          )}
          <Button
            variant="primary"
            disabled={name.trim() === ""}
            onClick={() =>
              api
                .assetClassSave(cls?.key ?? null, name, kind, color, countsFire)
                .then(onSaved)
                .catch((e: unknown) => onError(msg(e)))
            }
          >
            {cls ? t.common.save : t.common.create}
          </Button>
        </div>
      </div>
    </div>
  );
}

/**
 * Deleting a class in use has to say where its lines go.
 *
 * Silently dropping the class off historic lines would change what past
 * snapshots mean without saying so, which is the same contract the category
 * manager keeps.
 */
function RemoveClass({
  cls,
  classes,
  onClose,
  onDone,
  onError,
}: {
  cls: AssetClass;
  classes: AssetClass[];
  onClose: () => void;
  onDone: () => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const others = classes.filter((c) => c.key !== cls.key);
  const [target, setTarget] = useState<string>(others[0]?.key ?? "");

  return (
    <Panel>
      <PanelHead
        label={t.wealth.removeTitle(cls.name)}
        aside={
          <Button variant="ghost" onClick={onClose}>
            <X className="size-3.5" strokeWidth={2} />
          </Button>
        }
      />
      <PanelBody className="space-y-3">
        {cls.lineCount === 0 ? (
          <p className="text-[12px] text-ink-2">{t.wealth.unused}</p>
        ) : (
          <>
            <p className="text-[12px] leading-[1.6] text-ink-2">
              {t.wealth.inUse(cls.lineCount)}
            </p>
            <SelectField
              label={t.wealth.moveTo}
              className="w-60"
              value={target}
              onChange={(e) => setTarget(e.currentTarget.value)}
            >
              {others.map((c) => (
                <option key={c.key} value={c.key}>
                  {c.name} · {t.wealth.kinds[c.kind] ?? c.kind}
                </option>
              ))}
            </SelectField>
          </>
        )}
        <div className="flex justify-end gap-2 border-t border-rule pt-3">
          <Button variant="ghost" onClick={onClose}>
            {t.common.cancel}
          </Button>
          <Button
            variant="danger"
            disabled={cls.lineCount > 0 && target === ""}
            onClick={() =>
              api
                .assetClassDelete(cls.key, cls.lineCount > 0 ? target : null)
                .then(onDone)
                .catch((e: unknown) => onError(msg(e)))
            }
          >
            {t.common.remove}
          </Button>
        </div>
      </PanelBody>
    </Panel>
  );
}
