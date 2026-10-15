import { useMemo } from "react";
import {
  Area,
  Bar,
  BarChart,
  CartesianGrid,
  Cell,
  ComposedChart,
  ReferenceLine,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { fmtAxis, fmtMajor, fmtMoney, minorToMajor } from "@/lib/money";
import type { SimParams, SimResult } from "@/lib/types";
import { useT } from "@/lib/i18n";

/** One colour per property, in plan order; cash is always the same grey. */
const COLORS: string[] = ["#b8864b", "#5f86b8", "#5fa38a", "#b0739f", "#9a9a4e", "#7d7fc4", "#c4716b", "#4f9aa8"];
const CASH = "var(--ink-3)";
const colorOf = (i: number): string => COLORS[i % COLORS.length] ?? CASH;

/** Plan month 1 is `start`; month n is n − 1 calendar months after it. */
function calendar(start: string, planMonth: number) {
  const [y, m] = start.split("-").map(Number) as [number, number];
  const at = y * 12 + (m - 1) + (planMonth - 1);
  return { year: Math.floor(at / 12), month: (at % 12) + 1 };
}

const MARGIN = { top: 22, right: 12, bottom: 0, left: 4 };
const AXIS_W = 46;

/**
 * How the plan builds wealth, drawn so it can be read without a legend.
 *
 * Above: everything owned, stacked — cash at the bottom, then the share of
 * each property, each in its own colour and numbered. Where one begins is the
 * day it was bought, marked with its number. The top edge is net worth.
 * Below, on the same years: the rent kept each month. It used to share one
 * chart with a second axis, and a line on a different scale crossing a
 * stack of areas reads as a relationship between them that is not there.
 */
export function SimChart({
  result,
  params,
  start,
}: {
  result: SimResult | null;
  params: SimParams;
  start: string;
}) {
  const t = useT();
  const m = t.simulator;
  const years = result?.years ?? [];
  const horizon = params.horizonYears;
  const name = (i: number) => params.properties[i]?.name?.trim() || m.propertyN(i + 1);
  const calYear = (x: number) => calendar(start, Math.max(1, x * 12)).year;
  const when = (month: number) => {
    const c = calendar(start, month);
    return `${t.common.months[c.month - 1] ?? c.month} ${c.year}`;
  };

  const data = useMemo(() => {
    const row = (x: number, cash: number, eq: number[], rent: number) => {
      const r: Record<string, number> = { x, cash: minorToMajor(cash), rent: minorToMajor(rent) };
      params.properties.forEach((_, i) => {
        r[`p${i}`] = minorToMajor(eq[i] ?? 0);
      });
      return r;
    };
    // Year 0 is today: the cash in hand and nothing bought.
    const today = row(0, params.startCash, [], 0);
    return [today, ...years.map((y) => row(y.year, y.cash, y.propertyEquity ?? [], y.netRentMonthly))];
  }, [years, params.properties, params.startCash]);

  const last = years.at(-1);
  const ticks = Array.from({ length: horizon + 1 }, (_, i) => i).filter(
    (i) => horizon <= 12 || i % 2 === 0 || i === horizon,
  );
  const bought = (result?.properties ?? []).filter((o) => o.boughtMonth !== null);

  const common = {
    type: "number" as const,
    dataKey: "x",
    domain: [0, horizon] as [number, number],
    ticks,
    axisLine: false,
    tickLine: false,
    tickMargin: 10,
    tickFormatter: (x: number) => (x === 0 ? m.today : String(calYear(x))),
  };

  return (
    <Panel>
      <PanelHead
        label={m.accumulation}
        aside={
          last && (
            <span className="tnum text-[12px] text-ink-2">
              {m.totalAt(calYear(last.year))} <b className="font-medium text-ink">{fmtMoney(last.netWorth)}</b>
            </span>
          )
        }
      />
      <p className="px-5 pt-3 text-[11.5px] leading-[1.6] text-ink-3">{m.buildsLede}</p>

      <PanelBody className="pt-3 pr-3 pb-1 pl-1">
        <div className="h-[290px] w-full">
          <ResponsiveContainer width="100%" height="100%">
            <ComposedChart data={data} margin={MARGIN} syncId="sim">
              <CartesianGrid vertical={false} stroke="var(--rule)" strokeOpacity={0.85} />
              <XAxis {...common} />
              <YAxis axisLine={false} tickLine={false} width={AXIS_W} tickMargin={6} tickFormatter={fmtAxis} />
              <Tooltip cursor={{ stroke: "var(--rule-strong)", strokeWidth: 1 }} content={<Tip start={start} names={params.properties.map((_, i) => name(i))} />} />
              <Area
                type="monotone"
                dataKey="cash"
                stackId="nw"
                stroke={CASH}
                strokeWidth={1.25}
                fill={CASH}
                fillOpacity={0.16}
                isAnimationActive={false}
              />
              {params.properties.map((_, i) => (
                <Area
                  key={i}
                  type="monotone"
                  dataKey={`p${i}`}
                  stackId="nw"
                  stroke={colorOf(i)}
                  strokeWidth={1.5}
                  fill={colorOf(i)}
                  fillOpacity={0.3}
                  isAnimationActive={false}
                />
              ))}
              {bought.map((o) => (
                <ReferenceLine
                  key={o.index}
                  x={(o.boughtMonth ?? 0) / 12}
                  stroke={colorOf(o.index)}
                  strokeDasharray="3 3"
                  strokeOpacity={0.9}
                  label={{
                    value: String(o.index + 1),
                    position: "top",
                    fill: colorOf(o.index),
                    fontSize: 12,
                    fontWeight: 600,
                  }}
                />
              ))}
            </ComposedChart>
          </ResponsiveContainer>
        </div>

        {/* The key doubles as the explanation: what each colour is, when it
            began, and what it is worth at the end. */}
        <ul className="mt-2 flex flex-wrap gap-x-5 gap-y-2 pl-10">
          <li className="flex items-center gap-1.5 text-[11.5px] text-ink-2">
            <span className="size-2.5 rounded-[3px]" style={{ background: CASH, opacity: 0.55 }} />
            {m.cash}
            {last && <span className="tnum text-ink-3">{fmtMoney(last.cash)}</span>}
          </li>
          {params.properties.map((_, i) => {
            const o = result?.properties[i];
            const owned = o?.boughtMonth !== null && o?.boughtMonth !== undefined;
            return (
              <li key={i} className="flex items-center gap-1.5 text-[11.5px] text-ink-2">
                <span
                  className="flex size-4 items-center justify-center rounded-full text-[10px] font-semibold text-white"
                  style={{ background: colorOf(i), opacity: owned ? 1 : 0.4 }}
                >
                  {i + 1}
                </span>
                {name(i)}
                <span className="text-ink-3">
                  {owned ? m.boughtIn(when(o!.boughtMonth!)) : m.notBought}
                </span>
                {owned && last && (
                  <span className="tnum text-ink-3">· {fmtMoney(last.propertyEquity?.[i] ?? 0)}</span>
                )}
              </li>
            );
          })}
        </ul>
      </PanelBody>

      <div className="border-t border-rule">
        <div className="flex items-baseline justify-between gap-4 px-5 pt-3">
          <p className="eyebrow">{m.rentTitle}</p>
        </div>
        <p className="px-5 pt-1 text-[11.5px] leading-[1.6] text-ink-3">{m.rentLede}</p>
        <div className="h-[130px] w-full pt-2 pr-3 pb-2 pl-1">
          <ResponsiveContainer width="100%" height="100%">
            <BarChart data={data.slice(1)} margin={{ ...MARGIN, top: 8 }} syncId="sim">
              <CartesianGrid vertical={false} stroke="var(--rule)" strokeOpacity={0.85} />
              <XAxis {...common} />
              <YAxis axisLine={false} tickLine={false} width={AXIS_W} tickMargin={6} tickFormatter={fmtAxis} />
              <Tooltip cursor={{ stroke: "var(--rule-strong)", strokeWidth: 1 }} content={() => null} />
              <ReferenceLine y={0} stroke="var(--rule-strong)" />
              <Bar dataKey="rent" barSize={16} radius={[2, 2, 0, 0]} isAnimationActive={false}>
                {data.slice(1).map((d) => (
                  <Cell key={d.x} fill={(d.rent ?? 0) < 0 ? "var(--negative)" : "var(--positive)"} fillOpacity={0.75} />
                ))}
              </Bar>
            </BarChart>
          </ResponsiveContainer>
        </div>
      </div>
    </Panel>
  );
}

interface TipItem {
  dataKey?: string | number;
  value?: number;
  payload?: Record<string, number>;
}

function Tip({
  active,
  payload,
  start,
  names,
}: {
  active?: boolean;
  payload?: TipItem[];
  start: string;
  names: string[];
}) {
  const m = useT().simulator;
  if (!active || !payload?.length) return null;
  const row = payload[0]?.payload;
  if (!row) return null;
  const x = row.x ?? 0;
  const cash = row.cash ?? 0;
  const parts = names.map((n, i) => ({ n, v: row[`p${i}`] ?? 0, i }));
  const total = cash + parts.reduce((a, p) => a + p.v, 0);
  return (
    <div className="min-w-[190px] rounded-lg border border-rule bg-surface px-3 py-2.5 shadow-card">
      <p className="eyebrow mb-2">
        {x === 0 ? m.today : m.tipYear(x, calendar(start, x * 12).year)}
      </p>
      <dl className="space-y-1">
        <Row swatch={CASH} label={m.cash} value={fmtMajor(cash)} />
        {parts
          .filter((p) => p.v !== 0)
          .map((p) => (
            <Row key={p.i} swatch={colorOf(p.i)} label={p.n} value={fmtMajor(p.v)} />
          ))}
        <div className="mt-1.5 border-t border-rule pt-1.5">
          <Row label={m.tipTotal} value={fmtMajor(total)} strong />
        </div>
        {x > 0 && (
          <Row swatch="var(--positive)" label={m.netRent} value={m.perMonth(fmtMajor(row.rent ?? 0))} />
        )}
      </dl>
    </div>
  );
}

function Row({
  swatch,
  label,
  value,
  strong,
}: {
  swatch?: string;
  label: string;
  value: string;
  strong?: boolean;
}) {
  return (
    <div className="flex items-center gap-3 text-[11.5px]">
      {swatch ? (
        <span className="size-1.5 shrink-0 rounded-full" style={{ background: swatch }} />
      ) : (
        <span className="size-1.5 shrink-0" />
      )}
      <dt className="flex-1 text-ink-2">{label}</dt>
      <dd className={strong ? "tnum font-semibold text-ink" : "tnum font-medium text-ink"}>{value}</dd>
    </div>
  );
}
