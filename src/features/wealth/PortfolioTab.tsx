import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { MarketLookup } from "./MarketLookup";
import {
  Area,
  CartesianGrid,
  ComposedChart,
  Line,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { AlertTriangle, Check, ChevronDown, ExternalLink, Plus, Search, Trash2, X } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { Banner, Button, Empty, SelectField, TextField } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { useT } from "@/lib/i18n";
import { fmtAxis, fmtBps, fmtMoney, fmtMoneyExact, majorToMinor, minorToMajor, sym } from "@/lib/money";
import { fmtDate, region } from "@/lib/region";
import type {
  AssetClass,
  EgressPreview,
  HistoryPoint,
  HoldingTxn,
  HoldingView,
  PortfolioView,
  PriceProposal,
  Schedule,
} from "@/lib/types";
import { cn } from "@/lib/utils";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Accepts "0,015" and "0.015" alike: this is a Spanish app. */
const parseDecimal = (s: string) => Number(s.trim().replace(/\./g, (m, i, all) =>
  // A dot followed later by a comma is a thousands separator.
  all.indexOf(",") > i ? "" : m,
).replace(",", "."));

export const fmtUnits = (e8: number) =>
  new Intl.NumberFormat(region().locale, { maximumFractionDigits: 8 }).format(e8 / 1e8);

/** A price per unit: cents for normal prices, more digits for tiny ones. */
const fmtPrice = (micro: number) => {
  const eur = micro / 1e6;
  const digits = eur >= 1 ? 2 : eur >= 0.01 ? 4 : 6;
  const body = new Intl.NumberFormat(region().locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(eur);
  return region().symbolFirst ? `${sym()}${body}` : `${body} ${sym()}`;
};

const today = () => new Date().toISOString().slice(0, 10);

/**
 * The investment portfolio: what is held, what went in, and what it is worth.
 *
 * A holding's value is always units × a dated price, and what went in is
 * always the sum of its buys — including the ones a recurring buy wrote on
 * its own. Nothing is a stored total, so nothing can drift from its history.
 *
 * The screen is about now. Prices are looked up as of today, and the chart
 * is where the past lives: what had gone in and what it was worth, month by
 * month, so growth is something seen rather than a figure to take on trust.
 */
export function PortfolioTab({
  classes,
  onChanged,
}: {
  classes: AssetClass[];
  onChanged: () => void;
}) {
  const t = useT();
  const p = t.portfolio;
  const [view, setView] = useState<PortfolioView | null>(null);
  const [history, setHistory] = useState<HistoryPoint[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [preview, setPreview] = useState<EgressPreview | null>(null);
  const [proposals, setProposals] = useState<PriceProposal[] | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    Promise.all([api.portfolioView(null), api.portfolioHistory()])
      .then(([v, h]) => {
        setView(v);
        setHistory(h);
      })
      .catch((e: unknown) => setError(msg(e)));
  }, []);

  useEffect(load, [load]);

  const refresh = () => {
    load();
    onChanged();
  };

  const priceDate = today();

  const startCheck = () => {
    setError(null);
    setNote(null);
    setProposals(null);
    api
      .aiPricePreview()
      .then(setPreview)
      .catch((e: unknown) => setError(msg(e)));
  };

  const send = () => {
    if (!preview) return;
    setBusy(true);
    api
      .aiPriceSend(preview)
      .then((ps) => {
        setProposals(ps);
        setPreview(null);
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setBusy(false));
  };

  return (
    <div className="space-y-5">
      {error && <Banner tone="bad">{error}</Banner>}
      {note && <Banner tone="good">{note}</Banner>}

      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-[12px] text-ink-3">
          {view && lastPriced(view) ? p.pricedOn(fmtDate(lastPriced(view)!)) : p.noPricesYet}
        </p>
        <div className="flex gap-2">
          <Button onClick={startCheck} disabled={!view || view.holdings.length === 0}>
            <Search className="size-3.5" strokeWidth={1.75} />
            {p.checkPrices}
          </Button>
          <Button variant="primary" onClick={() => setAdding((v) => !v)}>
            {adding ? <X className="size-3.5" strokeWidth={2} /> : <Plus className="size-3.5" strokeWidth={2} />}
            {adding ? t.common.cancel : p.newHolding}
          </Button>
        </div>
      </div>

      {view && (
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <Stat label={p.value} value={fmtMoney(view.value)} />
          <Stat label={p.invested} value={fmtMoney(view.invested)} />
          <Stat
            label={p.growth}
            value={`${view.gain >= 0 ? "+" : ""}${fmtMoney(view.gain)}`}
            tone={view.gain >= 0 ? "positive" : "negative"}
          />
          <Stat
            label={p.annualReturn}
            value={view.xirrBps === null ? "—" : fmtBps(view.xirrBps, 1)}
            note={view.xirrBps === null ? p.returnNone : p.returnNote}
            tone={view.xirrBps !== null && view.xirrBps < 0 ? "negative" : undefined}
          />
        </div>
      )}

      <MarketLookup onChanged={refresh} />

      {history.length > 1 && <Evolution history={history} />}

      {view && view.unpriced.length > 0 && <Banner>{p.unpriced(view.unpriced.join(", "))}</Banner>}
      {view && view.holdings.some((h) => h.unitsComplete && h.unitsEstimated) && (
        <Banner>
          {p.estimated(
            view.holdings
              .filter((h) => h.unitsComplete && h.unitsEstimated)
              .map((h) => h.name)
              .join(", "),
          )}
        </Banner>
      )}
      {view && view.holdings.some((h) => !h.unitsComplete) && (
        <Banner>
          {p.incomplete(
            view.holdings
              .filter((h) => !h.unitsComplete)
              .map((h) => h.name)
              .join(", "),
          )}
        </Banner>
      )}

      {preview && (
        <Panel className="border-brass/50">
          <PanelHead label={p.aiTitle} />
          <PanelBody className="space-y-3">
            <p className="text-[12px] text-ink-2">{p.aiExplain(preview.provider, preview.rowCount)}</p>
            <pre
              data-selectable
              className="max-h-48 overflow-auto rounded-md border border-rule bg-paper-sunk p-3 font-mono text-[11px] leading-[1.5] text-ink-2"
            >
              {preview.payload}
            </pre>
            <div className="flex justify-end gap-2">
              <Button
                variant="ghost"
                onClick={() => {
                  api.aiDecline(preview).catch(() => undefined);
                  setPreview(null);
                }}
              >
                {t.assistant.dontSend}
              </Button>
              <Button variant="primary" onClick={send} disabled={busy}>
                <Search className="size-3.5" strokeWidth={1.75} />
                {busy ? p.aiSending : p.aiSend}
              </Button>
            </div>
          </PanelBody>
        </Panel>
      )}

      {proposals && (
        <Review
          date={priceDate}
          proposals={proposals}
          onCancel={() => setProposals(null)}
          onSaved={(n) => {
            setProposals(null);
            setNote(n);
            refresh();
          }}
          onError={setError}
        />
      )}

      {adding && (
        <HoldingForm
          classes={classes}
          onSaved={() => {
            setAdding(false);
            refresh();
          }}
          onError={setError}
        />
      )}

      {view && view.holdings.length === 0 && !adding ? (
        <Empty title={p.empty} body={p.emptyBody} />
      ) : (
        view && (
          <Panel>
            <div className="overflow-auto">
              <table className="w-full">
                <thead>
                  <tr className="border-b border-rule">
                    {p.columns.map((h, i) => (
                      <th
                        key={h || i}
                        className={cn(
                          "eyebrow px-3 py-2.5 font-medium whitespace-nowrap",
                          i === 0 ? "pl-5 text-left" : "text-right",
                          i === p.columns.length - 1 && "pr-5",
                        )}
                      >
                        {h}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {view.holdings.map((h) => (
                    <Fragment key={h.id}>
                      <HoldingRow h={h} open={open === h.id} onToggle={() => setOpen(open === h.id ? null : h.id)} />
                      {open === h.id && (
                        <tr className="border-b border-rule bg-brass-wash/25">
                          <td colSpan={8} className="px-5 py-4">
                            <HoldingDetail
                              holding={h}
                              defaultDate={priceDate}
                              onChanged={refresh}
                              onError={setError}
                              onNote={setNote}
                            />
                          </td>
                        </tr>
                      )}
                    </Fragment>
                  ))}
                </tbody>
              </table>
            </div>
          </Panel>
        )
      )}
    </div>
  );
}

/** The most recent date any holding was priced on. */
function lastPriced(view: PortfolioView): string | null {
  const dates = view.holdings.map((h) => h.priceDate).filter((d): d is string => d !== null);
  return dates.length === 0 ? null : dates.reduce((a, b) => (a > b ? a : b));
}

/**
 * Value against money put in, through time.
 *
 * Two lines rather than one because the value alone cannot tell growth from
 * saving: a portfolio that doubled because its owner doubled what was in it
 * has not grown. The gap between the lines is the growth, and it is shaded
 * so that it is the thing the eye lands on.
 */
function Evolution({ history }: { history: HistoryPoint[] }) {
  const t = useT();
  const p = t.portfolio;
  const data = useMemo(
    () =>
      history.map((h) => ({
        date: h.date,
        invested: minorToMajor(h.invested),
        value: minorToMajor(h.value),
      })),
    [history],
  );
  const last = history.at(-1);
  const gain = last ? last.value - last.invested : 0;
  const tick = (iso: string) => `${t.common.months[Number(iso.slice(5, 7)) - 1]} ${iso.slice(2, 4)}`;
  const up = gain >= 0;

  return (
    <Panel>
      <PanelHead
        label={p.evolution}
        aside={
          <div className="flex items-center gap-4">
            <Key swatch={up ? "var(--positive)" : "var(--negative)"} label={p.value} />
            <Key swatch="var(--ink-3)" label={p.invested} dashed />
          </div>
        }
      />
      <PanelBody className="pt-5 pr-3 pb-2 pl-1">
        <div className="h-[250px] w-full">
          <ResponsiveContainer width="100%" height="100%">
            <ComposedChart data={data} margin={{ top: 4, right: 8, bottom: 0, left: 4 }}>
              <defs>
                <linearGradient id="pf-value" x1="0" y1="0" x2="0" y2="1">
                  <stop offset="0%" stopColor={up ? "var(--positive)" : "var(--negative)"} stopOpacity={0.24} />
                  <stop offset="100%" stopColor={up ? "var(--positive)" : "var(--negative)"} stopOpacity={0.02} />
                </linearGradient>
              </defs>
              <CartesianGrid vertical={false} stroke="var(--rule)" />
              <XAxis
                dataKey="date"
                axisLine={false}
                tickLine={false}
                tickMargin={10}
                minTickGap={36}
                tickFormatter={tick}
              />
              <YAxis axisLine={false} tickLine={false} width={46} tickMargin={6} tickFormatter={fmtAxis} />
              <Tooltip
                cursor={{ stroke: "var(--rule-strong)", strokeWidth: 1 }}
                content={<EvolutionTip />}
              />
              <Area
                type="monotone"
                dataKey="value"
                stroke={up ? "var(--positive)" : "var(--negative)"}
                strokeWidth={1.75}
                fill="url(#pf-value)"
              />
              <Line
                type="stepAfter"
                dataKey="invested"
                stroke="var(--ink-3)"
                strokeWidth={1.25}
                strokeDasharray="4 3"
                dot={false}
                activeDot={false}
              />
            </ComposedChart>
          </ResponsiveContainer>
        </div>
        <p className="px-9 pt-1 pb-1 text-[11px] leading-[1.6] text-ink-3">{p.evolutionNote}</p>
      </PanelBody>
    </Panel>
  );
}

function EvolutionTip({
  active,
  payload,
}: {
  active?: boolean;
  payload?: { payload?: { date: string; invested: number; value: number } }[];
}) {
  const p = useT().portfolio;
  const row = payload?.[0]?.payload;
  if (!active || !row) return null;
  const gain = Math.round((row.value - row.invested) * 100);
  return (
    <div className="rounded-lg border border-rule bg-surface px-3 py-2.5 shadow-card">
      <p className="eyebrow mb-2">{row.date}</p>
      <dl className="space-y-1 text-[11.5px]">
        {(
          [
            [p.value, fmtMoney(Math.round(row.value * 100)), "text-ink"],
            [p.invested, fmtMoney(Math.round(row.invested * 100)), "text-ink-2"],
            [p.growth, `${gain >= 0 ? "+" : ""}${fmtMoney(gain)}`, gain >= 0 ? "text-positive" : "text-negative"],
          ] as const
        ).map(([label, value, tone]) => (
          <div key={label} className="flex items-center gap-6">
            <dt className="flex-1 text-ink-3">{label}</dt>
            <dd className={cn("tnum font-medium", tone)}>{value}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}

function Key({ swatch, label, dashed }: { swatch: string; label: string; dashed?: boolean }) {
  return (
    <span className="flex items-center gap-1.5 text-[11px] text-ink-3">
      <span
        className={cn("block", dashed ? "h-0 w-3 border-t border-dashed" : "size-2 rounded-full")}
        style={dashed ? { borderColor: swatch } : { background: swatch }}
      />
      {label}
    </span>
  );
}

function Stat({
  label,
  value,
  note,
  tone,
}: {
  label: string;
  value: string;
  note?: string;
  tone?: "positive" | "negative" | undefined;
}) {
  return (
    <div className="rounded-[10px] border border-rule bg-surface px-5 py-4">
      <p className="eyebrow">{label}</p>
      <p
        className={cn(
          "tnum mt-2 text-[21px] leading-none text-ink",
          tone === "positive" && "text-positive",
          tone === "negative" && "text-negative",
        )}
      >
        {value}
      </p>
      {note && <p className="mt-1.5 text-[11px] leading-[1.45] text-ink-3">{note}</p>}
    </div>
  );
}

function HoldingRow({ h, open, onToggle }: { h: HoldingView; open: boolean; onToggle: () => void }) {
  const p = useT().portfolio;
  return (
    <tr
      className={cn("cursor-pointer border-b border-rule/60 transition-colors hover:bg-surface-hi", open && "bg-surface-hi")}
      onClick={onToggle}
    >
      <td className="py-2 pr-3 pl-5">
        <div className="flex items-center gap-2">
          <ChevronDown
            className={cn("size-3.5 shrink-0 text-ink-3 transition-transform", !open && "-rotate-90")}
            strokeWidth={1.75}
          />
          <div className="min-w-0">
            <p className="truncate text-[12.5px] text-ink">{h.name}</p>
            <p className="text-[10.5px] text-ink-3">
              {[h.symbol, h.schedules.length > 0 ? p.recurring.toLowerCase() : null].filter(Boolean).join(" · ")}
            </p>
          </div>
        </div>
      </td>
      <td className="tnum px-3 py-2 text-right text-[12px] text-ink-2">
        {!h.unitsComplete ? (
          <span className="mr-1 text-brass">{p.unitsFloor}</span>
        ) : (
          h.unitsEstimated && (
            <span className="mr-0.5 text-brass" title={p.estimatedHint}>
              ≈
            </span>
          )
        )}
        {fmtUnits(h.unitsE8)}
      </td>
      <td className="px-3 py-2 text-right">
        {h.priceMicro === null ? (
          <span className="text-[12px] text-ink-3">—</span>
        ) : (
          <>
            <span className="tnum block text-[12px] text-ink">{fmtPrice(h.priceMicro)}</span>
            <span className="block text-[10.5px] text-ink-3">
              {p.priceFrom(fmtDate(h.priceDate ?? ""))}
              {h.priceSource === "assistant" && ` · ${p.byAssistant}`}
            </span>
          </>
        )}
      </td>
      <td className="tnum px-3 py-2 text-right text-[12.5px] text-ink">{h.value === null ? "—" : fmtMoney(h.value)}</td>
      <td className="tnum px-3 py-2 text-right text-[12px] text-ink-2">{fmtMoney(h.invested)}</td>
      <td
        className={cn(
          "tnum px-3 py-2 text-right text-[12px]",
          h.gain === null ? "text-ink-3" : h.gain >= 0 ? "text-positive" : "text-negative",
        )}
      >
        {h.gain === null ? "—" : `${h.gain >= 0 ? "+" : ""}${fmtMoney(h.gain)}`}
      </td>
      <td className="tnum px-3 py-2 text-right text-[12px] text-ink-2">
        {h.xirrBps === null ? "—" : fmtBps(h.xirrBps, 1)}
      </td>
      <td className="pr-5" />
    </tr>
  );
}

/* --------------------------------------------------------------- detail -- */

type DetailTab = "movements" | "recurring" | "price";

function HoldingDetail({
  holding: h,
  defaultDate,
  onChanged,
  onError,
  onNote,
}: {
  holding: HoldingView;
  defaultDate: string;
  onChanged: () => void;
  onError: (m: string) => void;
  onNote: (m: string) => void;
}) {
  const p = useT().portfolio;
  const [tab, setTab] = useState<DetailTab>("movements");

  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex gap-1">
          {(["movements", "recurring", "price"] as const).map((id) => (
            <button
              key={id}
              type="button"
              onClick={() => setTab(id)}
              className={cn(
                "rounded px-2.5 py-1 text-[11.5px] transition-colors",
                tab === id ? "bg-surface text-ink shadow-[0_1px_2px_oklch(0_0_0/0.06)]" : "text-ink-3 hover:text-ink",
              )}
            >
              {p[id]}
            </button>
          ))}
        </div>
        <Button
          variant="ghost"
          onClick={() =>
            api
              .holdingDelete(h.id)
              .then(onChanged)
              .catch((e: unknown) => onError(msg(e)))
          }
        >
          <Trash2 className="size-3" strokeWidth={1.75} />
          {p.delete}
        </Button>
      </div>

      {tab === "movements" && <Movements holding={h} onChanged={onChanged} onError={onError} />}
      {tab === "recurring" && <Recurring holding={h} onChanged={onChanged} onError={onError} />}
      {tab === "price" && (
        <ManualPrice holding={h} defaultDate={defaultDate} onChanged={onChanged} onError={onError} onNote={onNote} />
      )}
    </div>
  );
}

function Movements({
  holding: h,
  onChanged,
  onError,
}: {
  holding: HoldingView;
  onChanged: () => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const p = t.portfolio;
  const [rows, setRows] = useState<HoldingTxn[]>([]);
  const [kind, setKind] = useState<HoldingTxn["kind"]>("buy");
  const [on, setOn] = useState(today());
  const [amount, setAmount] = useState("");
  const [units, setUnits] = useState("");

  const load = useCallback(() => {
    api
      .holdingTxns(h.id)
      .then(setRows)
      .catch((e: unknown) => onError(msg(e)));
  }, [h.id, onError]);
  useEffect(load, [load]);

  const done = () => {
    setAmount("");
    setUnits("");
    load();
    onChanged();
  };

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-end gap-2">
        <SelectField
          label={t.importer.kind}
          className="w-36"
          value={kind}
          onChange={(e) => setKind(e.currentTarget.value as HoldingTxn["kind"])}
        >
          {(["buy", "sell", "units"] as const).map((k) => (
            <option key={k} value={k}>
              {p.kinds[k]}
            </option>
          ))}
        </SelectField>
        <TextField label={p.date} type="date" className="w-40" value={on} onChange={(e) => setOn(e.currentTarget.value)} />
        {kind !== "units" && (
          <TextField label={p.amount} className="w-32" value={amount} onChange={(e) => setAmount(e.currentTarget.value)} />
        )}
        <TextField
          label={kind === "units" ? p.unitsHeld : p.unitsOptional}
          className="w-40"
          value={units}
          placeholder="0,015"
          onChange={(e) => setUnits(e.currentTarget.value)}
        />
        <Button
          variant="primary"
          disabled={kind === "units" ? units.trim() === "" : amount.trim() === ""}
          onClick={() => {
            const u = units.trim() === "" ? null : Math.round(parseDecimal(units) * 1e8);
            const a = kind === "units" ? 0 : majorToMinor(parseDecimal(amount));
            if ((u !== null && !Number.isFinite(u)) || !Number.isFinite(a)) return;
            api
              .holdingTxnSave(null, h.id, on, kind, a, u)
              .then(done)
              .catch((e: unknown) => onError(msg(e)));
          }}
        >
          {p.add}
        </Button>
      </div>
      {kind === "units" && <p className="text-[11px] text-ink-3">{p.unitsHint}</p>}

      {rows.length === 0 ? (
        <p className="text-[12px] text-ink-3">{p.noMovements}</p>
      ) : (
        <div className="max-h-64 divide-y divide-rule/60 overflow-auto rounded-md border border-rule bg-surface">
          {rows.map((r) => (
            <div key={r.id} className={cn("flex items-center gap-3 px-3 py-1.5 text-[12px]", r.skipped && "opacity-45")}>
              <span className="tnum w-24 text-ink-3">{fmtDate(r.onDate)}</span>
              <span className="w-24 text-ink-2">{p.kinds[r.kind]}</span>
              <span className="tnum w-28 text-right text-ink">
                {r.kind === "units" ? "" : fmtMoneyExact(r.amountMinor)}
              </span>
              <span className="tnum flex-1 text-right text-ink-2">{r.unitsE8 === null ? "—" : fmtUnits(r.unitsE8)}</span>
              <span className="w-20 text-right text-[10.5px] text-ink-3">
                {r.skipped ? p.skipped : r.scheduled ? p.scheduled : ""}
              </span>
              {!r.skipped && (
                <button
                  type="button"
                  title={r.scheduled ? p.skip : t.common.delete}
                  className="rounded p-1 text-ink-3 hover:text-ink"
                  onClick={() =>
                    api
                      .holdingTxnDelete(r.id)
                      .then(done)
                      .catch((e: unknown) => onError(msg(e)))
                  }
                >
                  <X className="size-3" strokeWidth={2} />
                </button>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function Recurring({
  holding: h,
  onChanged,
  onError,
}: {
  holding: HoldingView;
  onChanged: () => void;
  onError: (m: string) => void;
}) {
  const p = useT().portfolio;
  const [amount, setAmount] = useState("");
  const [mode, setMode] = useState<"days" | "month">("days");
  const [every, setEvery] = useState("15");
  const [dom, setDom] = useState("1");
  const [start, setStart] = useState(today());
  const [end, setEnd] = useState("");

  return (
    <div className="space-y-3">
      {h.schedules.map((s) => (
        <div key={s.id} className="flex items-center justify-between rounded-md border border-rule bg-surface px-3 py-2">
          <span className="text-[12.5px] text-ink">
            {p.scheduleText(fmtMoneyExact(s.amountMinor), s.everyDays, s.dayOfMonth, s.startDate, s.endDate)}
          </span>
          <Button
            variant="ghost"
            title={p.stopNote}
            onClick={() =>
              s.id !== null &&
              api
                .scheduleDelete(s.id)
                .then(onChanged)
                .catch((e: unknown) => onError(msg(e)))
            }
          >
            {p.stop}
          </Button>
        </div>
      ))}

      <div className="flex flex-wrap items-end gap-2">
        <TextField label={p.amount} className="w-28" value={amount} onChange={(e) => setAmount(e.currentTarget.value)} />
        <SelectField
          label={p.repeat}
          className="w-44"
          value={mode}
          onChange={(e) => setMode(e.currentTarget.value as "days" | "month")}
        >
          <option value="days">{p.byDays}</option>
          <option value="month">{p.byMonth}</option>
        </SelectField>
        {mode === "days" ? (
          <TextField label={`${p.every} (${p.days})`} type="number" className="w-24" value={every} onChange={(e) => setEvery(e.currentTarget.value)} />
        ) : (
          <TextField label={p.monthlyOn} type="number" className="w-28" value={dom} onChange={(e) => setDom(e.currentTarget.value)} />
        )}
        <TextField label={p.start} type="date" className="w-40" value={start} onChange={(e) => setStart(e.currentTarget.value)} />
        <TextField label={p.end} type="date" className="w-40" value={end} onChange={(e) => setEnd(e.currentTarget.value)} />
        <Button
          variant="primary"
          disabled={amount.trim() === ""}
          onClick={() => {
            const schedule: Schedule = {
              id: null,
              holdingId: h.id,
              amountMinor: majorToMinor(parseDecimal(amount)),
              everyDays: mode === "days" ? Number(every) : null,
              dayOfMonth: mode === "month" ? Number(dom) : null,
              startDate: start,
              endDate: end || null,
            };
            api
              .scheduleSave(schedule)
              .then(() => {
                setAmount("");
                onChanged();
              })
              .catch((e: unknown) => onError(msg(e)));
          }}
        >
          {p.addSchedule}
        </Button>
      </div>
      <p className="text-[11px] text-ink-3">{p.scheduleHint}</p>
    </div>
  );
}

function ManualPrice({
  holding: h,
  defaultDate,
  onChanged,
  onError,
  onNote,
}: {
  holding: HoldingView;
  defaultDate: string;
  onChanged: () => void;
  onError: (m: string) => void;
  onNote: (m: string) => void;
}) {
  const p = useT().portfolio;
  const [on, setOn] = useState(defaultDate);
  const [price, setPrice] = useState("");
  return (
    <div className="flex flex-wrap items-end gap-2">
      <TextField label={p.date} type="date" className="w-40" value={on} onChange={(e) => setOn(e.currentTarget.value)} />
      <TextField label={p.pricePerUnit} className="w-40" value={price} onChange={(e) => setPrice(e.currentTarget.value)} />
      <Button
        variant="primary"
        disabled={price.trim() === ""}
        onClick={() => {
          const v = parseDecimal(price);
          if (!Number.isFinite(v)) return;
          api
            .pricesSave([{ holdingId: h.id, onDate: on, priceMicro: Math.round(v * 1e6), source: "manual", sourceUrl: null }])
            .then((n) => {
              setPrice("");
              onNote(p.pricesSaved(n));
              onChanged();
            })
            .catch((e: unknown) => onError(msg(e)));
        }}
      >
        {p.savePrice}
      </Button>
    </div>
  );
}

function HoldingForm({
  classes,
  onSaved,
  onError,
}: {
  classes: AssetClass[];
  onSaved: () => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const p = t.portfolio;
  const assets = classes.filter((c) => c.kind !== "liability");
  const [name, setName] = useState("");
  const [symbol, setSymbol] = useState("");
  const [cls, setCls] = useState(assets.find((c) => c.key === "equities")?.key ?? assets[0]?.key ?? "");
  return (
    <Panel>
      <PanelBody className="flex flex-wrap items-end gap-3">
        <TextField label={p.name} className="w-56" value={name} autoFocus placeholder="Bitcoin" onChange={(e) => setName(e.currentTarget.value)} />
        <TextField
          label={p.symbol}
          className="w-44"
          value={symbol}
          placeholder="BTC"
          hint={p.symbolHint}
          onChange={(e) => setSymbol(e.currentTarget.value)}
        />
        <SelectField label={p.assetClass} className="w-44" value={cls} onChange={(e) => setCls(e.currentTarget.value)}>
          {assets.map((c) => (
            <option key={c.key} value={c.key}>
              {c.name}
            </option>
          ))}
        </SelectField>
        <Button
          variant="primary"
          disabled={name.trim() === "" || cls === ""}
          onClick={() =>
            api
              .holdingSave(null, name, symbol || null, cls)
              .then(onSaved)
              .catch((e: unknown) => onError(msg(e)))
          }
        >
          {t.common.create}
        </Button>
      </PanelBody>
    </Panel>
  );
}

/* --------------------------------------------------------------- review -- */

type ReviewRow = PriceProposal & { use: boolean; text: string };

/**
 * Prices the assistant found, shown with where each came from, for a person
 * to accept. Nothing is saved from here until they say so, and a price whose
 * source the search never returned starts unticked.
 *
 * Two kinds arrive together. Today's prices say what things are worth now.
 * Past prices — month-ends, asked for only where buys were recorded by their
 * cost alone — are what turn those buys into units and give the history
 * chart real ground to stand on; they are listed under the holding they
 * belong to, folded away, because there can be many and they are rarely
 * worth reading one by one.
 */
function Review({
  date,
  proposals,
  onCancel,
  onSaved,
  onError,
}: {
  date: string;
  proposals: PriceProposal[];
  onCancel: () => void;
  onSaved: (note: string) => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const p = t.portfolio;
  const [rows, setRows] = useState<ReviewRow[]>(
    proposals.map((pr) => ({
      ...pr,
      use: pr.priceMicro !== null && pr.sourceSeen,
      text: pr.priceMicro === null ? "" : String(pr.priceMicro / 1e6).replace(".", ","),
    })),
  );
  const [open, setOpen] = useState<number | null>(null);
  const set = (i: number, patch: Partial<ReviewRow>) =>
    setRows((rs) => rs.map((x, j) => (j === i ? { ...x, ...patch } : x)));
  const chosen = rows.filter((r) => r.use && r.text.trim() !== "");
  const indexed = rows.map((r, i) => ({ r, i }));
  const current = indexed.filter(({ r }) => !r.historical);
  const past = indexed.filter(({ r }) => r.historical && r.priceDate !== null);
  const holdings = [...new Set(past.map(({ r }) => r.holdingId))];

  const line = ({ r, i }: { r: ReviewRow; i: number }, label: string) => (
    <div key={i} className="flex flex-wrap items-center gap-3 px-3 py-2">
      <input
        type="checkbox"
        className="size-3.5 accent-[var(--brass)]"
        checked={r.use}
        disabled={r.text.trim() === ""}
        onChange={(e) => set(i, { use: e.currentTarget.checked })}
      />
      <span className="w-44 truncate text-[12.5px] text-ink">{label}</span>
      <input
        value={r.text}
        placeholder={p.notFound}
        onChange={(e) => set(i, { text: e.currentTarget.value })}
        className="tnum w-32 rounded border border-rule bg-surface px-2 py-1 text-right text-[12px] text-ink outline-none focus:border-brass"
      />
      {!r.historical && (
        <span className="tnum text-[11px] text-ink-3">{r.priceDate ? fmtDate(r.priceDate) : ""}</span>
      )}
      {r.sourceUrl && /^https?:\/\//i.test(r.sourceUrl) && (
        <a
          // security-allow: dynamic-href -- only http(s) addresses reach this line
          href={r.sourceUrl}
          target="_blank"
          rel="noreferrer"
          className="flex items-center gap-1 text-[11px] text-brass hover:underline"
        >
          <ExternalLink className="size-3" strokeWidth={1.75} />
          {hostOf(r.sourceUrl)}
        </a>
      )}
      {r.sourceUrl && !r.sourceSeen && (
        <span className="flex items-center gap-1 text-[11px] text-negative">
          <AlertTriangle className="size-3" strokeWidth={2} />
          {p.sourceUnchecked}
        </span>
      )}
      {r.evidence && (
        <span className="w-full pl-7 text-[11px] italic text-ink-3">“{r.evidence}”</span>
      )}
      {r.note && <span className="w-full pl-7 text-[11px] text-ink-3">{r.note}</span>}
    </div>
  );

  return (
    <Panel className="border-brass/50">
      <PanelHead label={p.aiReview} />
      <PanelBody className="space-y-3">
        <p className="text-[11.5px] leading-[1.6] text-ink-3">{p.aiReviewNote}</p>
        <div className="divide-y divide-rule rounded-md border border-rule bg-surface">
          {current.map((c) => line(c, c.r.name))}
        </div>

        {holdings.length > 0 && (
          <div className="space-y-2">
            <p className="text-[11.5px] leading-[1.6] text-ink-3">{p.historyNote}</p>
            {holdings.map((id) => {
              const mine = past
                .filter(({ r }) => r.holdingId === id)
                .sort((a, b) => (a.r.priceDate ?? "").localeCompare(b.r.priceDate ?? ""));
              const on = mine.filter(({ r }) => r.use && r.text.trim() !== "").length;
              const doubtful = mine.filter(({ r }) => r.sourceUrl && !r.sourceSeen).length;
              return (
                <div key={id} className="rounded-md border border-rule bg-surface">
                  <button
                    type="button"
                    onClick={() => setOpen(open === id ? null : id)}
                    className="flex w-full items-center gap-3 px-3 py-2 text-left"
                  >
                    <ChevronDown
                      className={cn("size-3.5 text-ink-3 transition-transform", open !== id && "-rotate-90")}
                      strokeWidth={1.75}
                    />
                    <span className="flex-1 text-[12.5px] text-ink">{mine[0]?.r.name}</span>
                    {doubtful > 0 && (
                      <span className="flex items-center gap-1 text-[11px] text-negative">
                        <AlertTriangle className="size-3" strokeWidth={2} />
                        {p.historyDoubtful(doubtful)}
                      </span>
                    )}
                    <span className="tnum text-[11.5px] text-ink-3">
                      {p.historyCount(on, mine.length)}
                    </span>
                  </button>
                  {open === id && (
                    <div className="divide-y divide-rule/60 border-t border-rule">
                      {mine.map((m) => line(m, fmtDate(m.r.priceDate ?? "")))}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}

        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onCancel}>
            {t.common.cancel}
          </Button>
          <Button
            variant="primary"
            disabled={chosen.length === 0}
            onClick={() =>
              api
                .pricesSave(
                  chosen.map((r) => ({
                    holdingId: r.holdingId,
                    // A current price is stored on the day it was looked up,
                    // even when the quote is the last close before it: it is
                    // what the holding is taken to be worth today. A past
                    // price is stored on the date it is the price of.
                    onDate: r.historical && r.priceDate ? r.priceDate : date,
                    priceMicro: Math.round(parseDecimal(r.text) * 1e6),
                    source: "assistant" as const,
                    sourceUrl: r.sourceUrl,
                  })),
                )
                .then((n) => onSaved(p.pricesSaved(n)))
                .catch((e: unknown) => onError(msg(e)))
            }
          >
            <Check className="size-3.5" strokeWidth={2} />
            {p.savePrices(chosen.length)}
          </Button>
        </div>
      </PanelBody>
    </Panel>
  );
}

function hostOf(url: string) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

export { fmtPrice, parseDecimal };
