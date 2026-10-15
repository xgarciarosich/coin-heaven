import { useMemo } from "react";
import {
  Bar,
  CartesianGrid,
  Cell,
  ComposedChart,
  Line,
  Pie,
  PieChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { fmtAxis, fmtBps, fmtMajor, fmtMoney, minorToMajor } from "@/lib/money";
import type { CashflowSummary, Matrix, PeriodTotals, PlSummary } from "@/lib/types";
import { CategoryMatrix } from "./CategoryMatrix";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";


export function CashFlowAnalytics({
  summary,
  monthly,
  yearly,
  pl,
  matrix,
}: {
  summary: CashflowSummary | null;
  monthly: PeriodTotals[];
  yearly: PeriodTotals[];
  pl: PlSummary | null;
  matrix: Matrix | null;
}) {
  const t = useT();
  const monthlyData = useMemo(
    () =>
      monthly.map((m) => ({
        label: t.common.months[Number(m.period.slice(5, 7)) - 1] ?? m.period,
        income: minorToMajor(m.incomeMinor),
        // Charted as a positive height; the axis label says "spent".
        expense: Math.abs(minorToMajor(m.expenseMinor)),
        net: minorToMajor(m.netMinor),
      })),
    [monthly, t],
  );

  const spend = useMemo(
    () =>
      (pl?.lines ?? [])
        .filter((l) => l.netMinor < 0)
        .map((l) => ({
          name: l.categoryId === null ? t.common.uncategorised : l.categoryName,
          value: Math.abs(minorToMajor(l.netMinor)),
          color: l.color ?? "var(--brass)",
        }))
        .sort((a, b) => b.value - a.value),
    [pl, t],
  );

  const yearlyData = useMemo(
    () =>
      yearly.map((y) => ({
        label: y.period,
        income: minorToMajor(y.incomeMinor),
        expense: Math.abs(minorToMajor(y.expenseMinor)),
        net: minorToMajor(y.netMinor),
      })),
    [yearly],
  );

  const tip = {
    contentStyle: {
      background: "var(--surface)",
      border: "1px solid var(--rule)",
      borderRadius: 8,
      fontSize: 12,
    },
    formatter: (v: unknown) => fmtMajor(Number(v)),
  };

  return (
    <div className="space-y-5">
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        <Kpi
          label={t.ledger.analytics.moneyIn}
          value={summary ? fmtMoney(summary.incomeMinor) : "—"}
          tone="positive"
        />
        <Kpi
          label={t.ledger.analytics.moneyOut}
          value={summary ? fmtMoney(summary.expenseMinor) : "—"}
        />
        <Kpi
          label={t.ledger.analytics.kept}
          value={summary ? fmtMoney(summary.netMinor) : "—"}
          sub={
            summary?.savingsRateBps != null
              ? t.ledger.analytics.keptShare(fmtBps(summary.savingsRateBps, 0))
              : t.ledger.analytics.noIncome
          }
          tone={summary && summary.netMinor < 0 ? "negative" : "positive"}
        />
      </div>

      <CategoryMatrix matrix={matrix} />

      <Panel>
        <PanelHead
          label={t.ledger.analytics.monthByMonth}
          aside={
            <div className="flex gap-4">
              <Key swatch="var(--positive)" label={t.ledger.analytics.in} />
              <Key swatch="var(--ink-3)" label={t.ledger.analytics.out} />
              <Key swatch="var(--brass)" label={t.ledger.analytics.kept} line />
            </div>
          }
        />
        <PanelBody className="pt-5 pr-3 pb-2 pl-1">
          <div className="h-[260px] w-full">
            <ResponsiveContainer width="100%" height="100%">
              <ComposedChart data={monthlyData} margin={{ top: 4, right: 8, bottom: 0, left: 4 }}>
                <CartesianGrid vertical={false} stroke="var(--rule)" />
                <XAxis dataKey="label" axisLine={false} tickLine={false} tickMargin={10} />
                <YAxis
                  axisLine={false}
                  tickLine={false}
                  width={46}
                  tickMargin={6}
                  tickFormatter={fmtAxis}
                />
                <Tooltip cursor={{ fill: "var(--surface-hi)" }} {...tip} />
                <Bar dataKey="income" name={t.ledger.analytics.in} fill="var(--positive)" radius={[3, 3, 0, 0]} />
                <Bar dataKey="expense" name={t.ledger.analytics.out} fill="var(--ink-3)" radius={[3, 3, 0, 0]} />
                <Line
                  type="monotone"
                  dataKey="net"
                  name={t.ledger.analytics.kept}
                  stroke="var(--brass)"
                  strokeWidth={1.75}
                  dot={false}
                />
              </ComposedChart>
            </ResponsiveContainer>
          </div>
        </PanelBody>
      </Panel>

      <div className="grid grid-cols-1 gap-5 lg:grid-cols-2">
        <Panel>
          <PanelHead label={t.ledger.analytics.whereItWent} />
          <PanelBody>
            {spend.length === 0 ? (
              <p className="text-[12px] text-ink-3">{t.ledger.analytics.nothingSpent}</p>
            ) : (
              <div className="flex items-center gap-4">
                <div className="h-[190px] w-[190px] shrink-0">
                  <ResponsiveContainer width="100%" height="100%">
                    <PieChart>
                      <Pie
                        data={spend}
                        dataKey="value"
                        nameKey="name"
                        innerRadius={52}
                        outerRadius={82}
                        paddingAngle={1.5}
                        stroke="var(--surface)"
                        strokeWidth={2}
                      >
                        {spend.map((s) => (
                          <Cell key={s.name} fill={s.color} />
                        ))}
                      </Pie>
                      <Tooltip {...tip} />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
                <ul className="min-w-0 flex-1 space-y-1.5">
                  {spend.slice(0, 7).map((s) => (
                    <li key={s.name} className="flex items-center gap-2 text-[12px]">
                      <span
                        className="size-2 shrink-0 rounded-full"
                        style={{ background: s.color }}
                      />
                      <span className="min-w-0 flex-1 truncate text-ink-2">{s.name}</span>
                      <span className="tnum text-ink">
                        {fmtMajor(s.value)}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
            )}
          </PanelBody>
        </Panel>

        <Panel>
          <PanelHead
            label={t.ledger.analytics.yearOnYear}
            aside={
              <span className="text-[11px] text-ink-3">
                {yearlyData.length < 2 ? t.ledger.analytics.oneYear : t.ledger.analytics.allData}
              </span>
            }
          />
          <PanelBody className="pt-5 pr-3 pb-2 pl-1">
            <div className="h-[190px] w-full">
              <ResponsiveContainer width="100%" height="100%">
                <ComposedChart data={yearlyData} margin={{ top: 4, right: 8, bottom: 0, left: 4 }}>
                  <CartesianGrid vertical={false} stroke="var(--rule)" />
                  <XAxis dataKey="label" axisLine={false} tickLine={false} tickMargin={10} />
                  <YAxis
                    axisLine={false}
                    tickLine={false}
                    width={46}
                    tickMargin={6}
                    tickFormatter={fmtAxis}
                  />
                  <Tooltip cursor={{ fill: "var(--surface-hi)" }} {...tip} />
                  <Bar dataKey="income" name={t.ledger.analytics.in} fill="var(--positive)" radius={[3, 3, 0, 0]} />
                  <Bar dataKey="expense" name={t.ledger.analytics.out} fill="var(--ink-3)" radius={[3, 3, 0, 0]} />
                  <Line
                    type="monotone"
                    dataKey="net"
                    name={t.ledger.analytics.kept}
                    stroke="var(--brass)"
                    strokeWidth={1.75}
                    dot={{ r: 2.5, strokeWidth: 0, fill: "var(--brass)" }}
                  />
                </ComposedChart>
              </ResponsiveContainer>
            </div>
          </PanelBody>
        </Panel>
      </div>

      {pl && pl.uncategorisedCount > 0 && (
        <p className="text-[11.5px] text-ink-3">
          {t.ledger.analytics.uncategorised(pl.uncategorisedCount)}
        </p>
      )}
    </div>
  );
}

function Kpi({
  label,
  value,
  sub,
  tone,
}: {
  label: string;
  value: string;
  sub?: string;
  tone?: "positive" | "negative";
}) {
  return (
    <div className="rounded-[10px] border border-rule bg-surface px-5 py-4">
      <p className="eyebrow">{label}</p>
      <p
        className={cn(
          "tnum mt-2 text-[22px] leading-none text-ink",
          tone === "positive" && "text-positive",
          tone === "negative" && "text-negative",
        )}
      >
        {value}
      </p>
      {sub && <p className="mt-1.5 text-[11px] text-ink-3">{sub}</p>}
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
