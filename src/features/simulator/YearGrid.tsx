import { Home } from "lucide-react";
import { fmtBps, fmtCompact, fmtMoney } from "@/lib/money";
import type { YearRow } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/**
 * The year-by-year table.
 *
 * It is a read-out now, not an editor. When the simulator bought on its own,
 * this was where a year was told to behave differently; with the purchases
 * planned one by one above, there is nothing left for a year to override.
 */
export function YearGrid({
  rows,
  start,
  dtiMaxBps,
}: {
  rows: YearRow[];
  /** "YYYY-MM": the calendar month the plan starts in. */
  start: string;
  dtiMaxBps: number;
}) {
  const t = useT();
  const g = t.simulator.grid;
  const [y0, m0] = start.split("-").map(Number) as [number, number];
  // The calendar year a plan year ends in.
  const endsIn = (year: number) => Math.floor((y0 * 12 + (m0 - 1) + year * 12 - 1) / 12);

  return (
    <div className="overflow-x-auto">
      <table className="w-full">
        <thead>
          <tr className="border-b border-rule">
            {g.columns.map((h, i) => (
              <th
                key={h}
                className={cn(
                  "eyebrow py-2.5 font-medium whitespace-nowrap",
                  i === 0 ? "pl-5 text-left" : "pr-4 text-right",
                  i === g.columns.length - 1 && "pr-5",
                )}
              >
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((y) => (
            <tr
              key={y.year}
              className={cn(
                "border-b border-rule/60 transition-colors last:border-b-0 hover:bg-surface-hi",
                y.purchases > 0 && "bg-brass-wash/30",
              )}
            >
              <td className="tnum py-2 pl-5 text-[12px] text-ink-3">
                {y.year}
                <span className="ml-1.5 text-[10.5px]">· {endsIn(y.year)}</span>
              </td>
              <td className="tnum py-2 pr-4 text-right text-[12.5px] text-ink">
                {fmtCompact(y.savingsContributed)}
              </td>
              <td
                className={cn(
                  "tnum py-2 pr-4 text-right text-[12.5px]",
                  y.netRentReceived < 0 ? "text-negative" : "text-ink",
                )}
              >
                {y.properties === 0 && y.netRentReceived === 0 ? (
                  <span className="text-ink-3">·</span>
                ) : (
                  fmtCompact(y.netRentReceived)
                )}
              </td>
              <td className="py-2 pr-4 text-right text-[12.5px]">
                {y.purchases > 0 ? (
                  <span className="inline-flex items-center gap-1 text-brass">
                    <Home className="size-3" strokeWidth={1.75} />
                    {y.purchases > 1 && <span className="tnum">×{y.purchases}</span>}
                  </span>
                ) : (
                  <span className="text-ink-3">·</span>
                )}
              </td>
              <td className="tnum py-2 pr-4 text-right text-[12.5px] text-ink">{y.properties}</td>
              <td className="tnum py-2 pr-4 text-right text-[12.5px] text-ink">
                {fmtCompact(y.cash)}
              </td>
              <td
                className={cn(
                  "tnum py-2 pr-4 text-right text-[12.5px]",
                  y.netRentMonthly < 0 ? "text-negative" : "text-ink",
                )}
              >
                {fmtMoney(y.netRentMonthly)}
              </td>
              <td className="tnum py-2 pr-4 text-right text-[12.5px] text-ink">
                {fmtCompact(y.debt)}
              </td>
              <td className="tnum py-2 pr-4 text-right text-[12.5px] font-medium text-ink">
                {fmtCompact(y.netWorth)}
              </td>
              <td
                className={cn(
                  "tnum py-2 pr-5 text-right text-[12.5px]",
                  y.dtiBps > dtiMaxBps ? "text-negative" : "text-ink-3",
                )}
              >
                {fmtBps(y.dtiBps, 0)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
