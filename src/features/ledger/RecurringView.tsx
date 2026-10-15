import { useEffect, useState } from "react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { Banner, Empty } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { fmtMoney, fmtMoneyExact } from "@/lib/money";
import { fmtDate } from "@/lib/region";
import type { Recurring, RecurringReport } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * The payments that come round on their own, found in the ledger.
 *
 * Nobody declares these, and the ones nobody remembers are the ones worth
 * seeing: it answers "what am I paying for every month that I have stopped
 * noticing". Payments that have not been charged for longer than their own
 * rhythm are listed apart, because "I cancelled it" and "it is still
 * running" must not be added together.
 */
export function RecurringView() {
  const t = useT();
  const r = t.ledger.recurring;
  const [report, setReport] = useState<RecurringReport | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .recurringList()
      .then(setReport)
      .catch((e: unknown) => setError(msg(e)));
  }, []);

  if (error) return <Banner tone="bad">{error}</Banner>;
  if (!report) return null;

  const active = report.items.filter((i) => i.active);
  const stopped = report.items.filter((i) => !i.active);

  if (report.items.length === 0) {
    return (
      <Panel>
        <PanelBody>
          <Empty title={r.emptyTitle} body={r.emptyBody} />
        </PanelBody>
      </Panel>
    );
  }

  return (
    <div className="space-y-5">
      <Panel>
        <PanelHead label={t.ledger.views.recurring} />
        <PanelBody className="space-y-2">
          {active.length > 0 && (
            <p className="text-[15px] leading-[1.5] text-ink">
              {r.summary(fmtMoney(report.monthlyMinor), fmtMoney(report.yearlyMinor))}
            </p>
          )}
          <p className="text-[12px] leading-[1.6] text-ink-3">{r.lede}</p>
        </PanelBody>
        {active.length > 0 && <Table rows={active} />}
      </Panel>

      {stopped.length > 0 && (
        <Panel>
          <PanelHead label={r.stopped} />
          <p className="px-5 pt-3 text-[11.5px] leading-[1.6] text-ink-3">{r.stoppedNote}</p>
          <Table rows={stopped} muted />
        </Panel>
      )}
    </div>
  );
}

function Table({ rows, muted }: { rows: Recurring[]; muted?: boolean }) {
  const r = useT().ledger.recurring;
  return (
    <div className="overflow-x-auto">
      <table className="w-full">
        <thead>
          <tr className="border-b border-rule">
            <th className="eyebrow py-2.5 pl-5 text-left font-medium">{r.name}</th>
            <th className="eyebrow py-2.5 pr-4 text-left font-medium">{r.every}</th>
            <th className="eyebrow py-2.5 pr-4 text-right font-medium">{r.usual}</th>
            <th className="eyebrow py-2.5 pr-4 text-right font-medium">
              {muted ? "" : r.next}
            </th>
            <th className="eyebrow py-2.5 pr-5 text-right font-medium">{r.perYear}</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((i) => (
            <tr
              key={`${i.name}-${i.typicalMinor}-${i.every}`}
              className={cn(
                "border-b border-rule/60 last:border-b-0 hover:bg-surface-hi",
                muted && "text-ink-3",
              )}
            >
              <td className="py-2.5 pl-5">
                <p className={cn("text-[12.5px]", muted ? "text-ink-3" : "text-ink")}>{i.name}</p>
                <p className="mt-0.5 text-[11px] text-ink-3">
                  {[i.categoryName, r.seen(i.count), i.varies ? r.varies : null]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              </td>
              <td className="py-2.5 pr-4 text-[12px] text-ink-2">{r.rhythm[i.every] ?? i.every}</td>
              <td className="tnum py-2.5 pr-4 text-right text-[12.5px] text-ink">
                {i.varies ? "≈ " : ""}
                {fmtMoneyExact(i.typicalMinor)}
              </td>
              <td className="tnum py-2.5 pr-4 text-right text-[12px] text-ink-3">
                {muted ? r.lastPaid(fmtDate(i.lastOn)) : fmtDate(i.nextOn)}
              </td>
              <td className="tnum py-2.5 pr-5 text-right text-[12.5px] font-medium text-ink">
                {fmtMoney(i.yearlyMinor)}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
