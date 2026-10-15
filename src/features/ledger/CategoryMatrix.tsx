import { useMemo, useState } from "react";
import { Panel, PanelHead } from "@/components/ui/panel";
import { fmtBps, fmtMoney } from "@/lib/money";
import type { Matrix, MatrixRow } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";


type Mode = "all" | "expense" | "income";

/**
 * Every category against every month of a year.
 *
 * This is the view a spreadsheet gets used for and an app usually refuses to
 * give: a single grid where a year of spending can be read across and a
 * category can be read down. The derived columns on the right are the three
 * questions people actually ask of such a grid — what did this cost me, what
 * share of everything is it, and what does it run at per month — so they are
 * columns rather than something to work out by hand.
 */
export function CategoryMatrix({ matrix }: { matrix: Matrix | null }) {
  const t = useT();
  const [mode, setMode] = useState<Mode>("all");

  const rows = useMemo(() => {
    const all = matrix?.rows ?? [];
    if (mode === "income") return all.filter((r) => r.totalMinor > 0);
    if (mode === "expense") return all.filter((r) => r.totalMinor < 0);
    return all;
  }, [matrix, mode]);

  // Shares are taken against the side a row sits on, so "28% of spending"
  // means that and not 28% of some income-and-spending mixture.
  const denominators = useMemo(() => {
    const income = rows.filter((r) => r.totalMinor > 0).reduce((n, r) => n + r.totalMinor, 0);
    const spend = rows.filter((r) => r.totalMinor < 0).reduce((n, r) => n + r.totalMinor, 0);
    return { income, spend: Math.abs(spend) };
  }, [rows]);

  const peak = useMemo(
    () => Math.max(1, ...rows.flatMap((r) => r.cells.map((c) => Math.abs(c)))),
    [rows],
  );

  const months = matrix?.activePeriods ?? 0;

  if (!matrix || matrix.rows.length === 0) {
    return (
      <Panel>
        <PanelHead label={t.ledger.matrix.title} />
        <div className="px-5 py-10 text-center text-[12px] text-ink-3">
          {t.ledger.matrix.empty}
        </div>
      </Panel>
    );
  }

  return (
    <Panel>
      <PanelHead
        label={t.ledger.matrix.title}
        aside={
          <div className="flex items-center gap-1">
            {(
              [
                ["all", t.ledger.matrix.modes.all],
                ["expense", t.ledger.matrix.modes.expense],
                ["income", t.ledger.matrix.modes.income],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                type="button"
                onClick={() => setMode(id)}
                className={cn(
                  "rounded px-2 py-1 text-[11px] transition-colors",
                  mode === id
                    ? "bg-brass-wash text-ink"
                    : "text-ink-3 hover:text-ink",
                )}
              >
                {label}
              </button>
            ))}
          </div>
        }
      />
      <div className="overflow-auto">
        <table className="w-full min-w-[980px] border-separate border-spacing-0">
          <thead>
            <tr>
              <Th className="sticky left-0 z-20 bg-surface text-left">
                {t.ledger.matrix.category}
              </Th>
              {t.common.months.map((m) => (
                <Th key={m} className="text-right">
                  {m}
                </Th>
              ))}
              <Th className="border-l border-rule text-right">{t.ledger.matrix.total}</Th>
              <Th className="text-right">{t.ledger.matrix.share}</Th>
              <Th className="text-right">{t.ledger.matrix.avg}</Th>
              <Th className="pr-5 text-right">{t.ledger.matrix.budget}</Th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <Line
                key={r.categoryId ?? "none"}
                row={r}
                peak={peak}
                months={months}
                denominator={r.totalMinor > 0 ? denominators.income : denominators.spend}
              />
            ))}
          </tbody>
          <tfoot>
            <Totals label={t.ledger.matrix.income} values={matrix.incomeByPeriod} total={matrix.incomeTotal} months={months} tone="positive" />
            <Totals label={t.ledger.matrix.spending} values={matrix.expenseByPeriod} total={matrix.expenseTotal} months={months} />
            <Totals label={t.ledger.matrix.balance} values={matrix.netByPeriod} total={matrix.netTotal} months={months} strong />
          </tfoot>
        </table>
      </div>
      <p className="border-t border-rule px-5 py-2.5 text-[11px] text-ink-3">
        {t.ledger.matrix.footnote(months)}
      </p>
    </Panel>
  );
}

function Line({
  row: r,
  peak,
  months,
  denominator,
}: {
  row: MatrixRow;
  peak: number;
  months: number;
  denominator: number;
}) {
  const t = useT();
  const share = denominator > 0 ? Math.round((Math.abs(r.totalMinor) / denominator) * 10_000) : 0;
  const avg = months > 0 ? Math.round(r.totalMinor / months) : 0;
  const overBudget =
    r.budgetMinor != null && r.budgetMinor > 0 && Math.abs(r.totalMinor) > r.budgetMinor;

  return (
    <tr className="group">
      <Td className="sticky left-0 z-10 bg-surface text-left group-hover:bg-surface-hi">
        <span className="flex items-center gap-1.5">
          {r.color && (
            <span className="size-2 shrink-0 rounded-full" style={{ background: r.color }} />
          )}
          <span className="truncate text-ink">
            {r.categoryId === null ? t.common.uncategorised : r.categoryName}
          </span>
        </span>
      </Td>
      {r.cells.map((c, i) => (
        <Td key={i} className="text-right">
          {c === 0 ? (
            <span className="text-ink-3">·</span>
          ) : (
            <span
              className={cn("tnum", c > 0 ? "text-positive" : "text-ink")}
              // A faint wash proportional to the cell turns the grid into a
              // heatmap you can scan without reading a single number.
              style={
                c < 0
                  ? {
                      background: `color-mix(in oklch, var(--ink-3) ${Math.round((Math.abs(c) / peak) * 22)}%, transparent)`,
                    }
                  : undefined
              }
            >
              {fmtMoney(c)}
            </span>
          )}
        </Td>
      ))}
      <Td className="border-l border-rule text-right">
        <span className={cn("tnum", r.totalMinor > 0 ? "text-positive" : "text-ink")}>
          {fmtMoney(r.totalMinor)}
        </span>
      </Td>
      <Td className="text-right text-ink-3">{fmtBps(share, 0)}</Td>
      <Td className="tnum text-right text-ink-2">{fmtMoney(avg)}</Td>
      <Td className="pr-5 text-right">
        {r.budgetMinor == null ? (
          <span className="text-ink-3">—</span>
        ) : (
          <span className={cn("tnum", overBudget ? "text-negative" : "text-ink-2")}>
            {fmtMoney(r.budgetMinor)}
          </span>
        )}
      </Td>
    </tr>
  );
}

function Totals({
  label,
  values,
  total,
  months,
  tone,
  strong,
}: {
  label: string;
  values: number[];
  total: number;
  months: number;
  tone?: "positive";
  strong?: boolean;
}) {
  const avg = months > 0 ? Math.round(total / months) : 0;
  return (
    <tr className={cn("border-t border-rule", strong && "bg-surface-hi/60")}>
      <Td
        className={cn(
          "sticky left-0 z-10 border-t border-rule bg-surface text-left",
          strong && "bg-surface-hi/60",
        )}
      >
        <span className={cn("eyebrow", strong && "text-ink")}>{label}</span>
      </Td>
      {values.map((v, i) => (
        <Td key={i} className="border-t border-rule text-right">
          <span
            className={cn(
              "tnum",
              tone === "positive" && "text-positive",
              strong && (v < 0 ? "text-negative" : "text-positive"),
            )}
          >
            {v === 0 ? "·" : fmtMoney(v)}
          </span>
        </Td>
      ))}
      <Td className="border-t border-l border-rule text-right">
        <span
          className={cn(
            "tnum",
            tone === "positive" && "text-positive",
            strong && (total < 0 ? "text-negative" : "text-positive"),
          )}
        >
          {fmtMoney(total)}
        </span>
      </Td>
      <Td className="border-t border-rule" />
      <Td className="tnum border-t border-rule text-right text-ink-2">{fmtMoney(avg)}</Td>
      <Td className="border-t border-rule pr-5" />
    </tr>
  );
}

function Th({ className, children }: { className?: string; children?: React.ReactNode }) {
  return (
    <th
      className={cn(
        "eyebrow sticky top-0 z-10 border-b border-rule bg-surface px-2 py-2.5 font-medium whitespace-nowrap",
        className,
      )}
    >
      {children}
    </th>
  );
}

function Td({ className, children }: { className?: string; children?: React.ReactNode }) {
  return (
    <td
      className={cn(
        "border-b border-rule/50 px-2 py-1.5 text-[11.5px] whitespace-nowrap",
        className,
      )}
    >
      {children}
    </td>
  );
}
