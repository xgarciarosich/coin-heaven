import { ArrowDownLeft, ArrowUpRight, Check, TriangleAlert, X } from "lucide-react";
import { Panel, PanelHead } from "@/components/ui/panel";
import { Button } from "@/components/ui/controls";
// Exact cents throughout: this is the screen where the difference between two
// amounts decides whether a pair really cancels, and €498.50 rounded to €499
// hides exactly the evidence being judged.
import { fmtMoneyExact } from "@/lib/money";
import type { MatchSide, PendingSplit, PendingTransfer } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

/**
 * Proposed links, laid out so they can actually be judged.
 *
 * A one-line summary is not enough to tell a real internal transfer from a
 * coincidence: both dates, both accounts, both amounts and both descriptions
 * decide it, and the account names usually settle it on their own. Everything
 * the matcher used is shown, along with why it paired them.
 */
export function MatchReview({
  transfers,
  splits,
  onDecide,
}: {
  transfers: PendingTransfer[];
  splits: PendingSplit[];
  onDecide: (
    kind: "transfer" | "split",
    a: number,
    b: number,
    confirm: boolean,
  ) => void;
}) {
  const tr = useT();
  const m = tr.importer.match;
  const total = transfers.length + splits.length;
  if (total === 0) return null;

  return (
    <Panel>
      <PanelHead
        label={m.title(total)}
        aside={
          <span className="text-[11px] text-ink-3">
            {m.nothingMerged}
          </span>
        }
      />
      <div className="divide-y divide-rule">
        {transfers.map((t) => (
          <Card
            key={`t-${t.outId}-${t.inId}`}
            kind={m.transfer}
            meta={
              t.residualMinor !== 0
                ? m.difference(fmtMoneyExact(Math.abs(t.residualMinor)))
                : m.cancelExactly
            }
            warn={t.sameAccountName ? m.sameAccount : null}
            reason={t.reason}
            rows={[
              { tag: m.out, side: t.out, direction: "out" as const },
              { tag: m.in, side: t.incoming, direction: "in" as const },
            ]}
            confirmLabel={m.linkTransfer}
            rejectLabel={m.notTransfer}
            onConfirm={() => onDecide("transfer", t.outId, t.inId, true)}
            onReject={() => onDecide("transfer", t.outId, t.inId, false)}
          />
        ))}

        {splits.map((s) => (
          <Card
            key={`s-${s.parentId}-${s.reimbursementId}`}
            kind={m.reimbursement(
              m.fractions[s.fractionDenominator] ?? `1/${s.fractionDenominator}`,
            )}
            meta={
              s.residualMinor === 0
                ? m.exactShare
                : m.offShare(fmtMoneyExact(Math.abs(s.residualMinor)))
            }
            warn={null}
            reason={s.reason}
            footer={m.netsTo(fmtMoneyExact(s.parentNetAfter))}
            rows={[
              { tag: m.bill, side: s.parent, direction: "out" as const },
              { tag: m.refund, side: s.reimbursement, direction: "in" as const },
            ]}
            confirmLabel={m.linkRefund}
            rejectLabel={m.unrelated}
            onConfirm={() => onDecide("split", s.parentId, s.reimbursementId, true)}
            onReject={() => onDecide("split", s.parentId, s.reimbursementId, false)}
          />
        ))}
      </div>
    </Panel>
  );
}


function Card({
  kind,
  meta,
  warn,
  reason,
  footer,
  rows,
  confirmLabel,
  rejectLabel,
  onConfirm,
  onReject,
}: {
  kind: string;
  meta: string;
  warn: string | null;
  reason: string;
  footer?: string;
  rows: { tag: string; side: MatchSide; direction: "in" | "out" }[];
  confirmLabel: string;
  rejectLabel: string;
  onConfirm: () => void;
  onReject: () => void;
}) {
  return (
    <div className="px-5 py-4">
      <div className="flex items-baseline justify-between gap-4">
        <p className="text-[11px] tracking-wide text-brass uppercase">{kind}</p>
        <p className="text-[11px] text-ink-3">{meta}</p>
      </div>

      <div className="mt-3 overflow-hidden rounded-lg border border-rule">
        {rows.map((r, i) => (
          <div
            key={r.side.id}
            className={cn(
              "flex items-center gap-3 px-3.5 py-2.5",
              i > 0 && "border-t border-rule",
              i % 2 === 1 && "bg-surface-hi/50",
            )}
          >
            <span className="flex w-16 shrink-0 items-center gap-1.5 text-[10.5px] tracking-wide text-ink-3 uppercase">
              {r.direction === "out" ? (
                <ArrowUpRight className="size-3" strokeWidth={2} />
              ) : (
                <ArrowDownLeft className="size-3" strokeWidth={2} />
              )}
              {r.tag}
            </span>

            <span className="tnum w-24 shrink-0 text-[12px] text-ink-2">
              {r.side.bookedOn}
            </span>

            <span className="w-40 shrink-0 truncate text-[12px] text-ink-2">
              {r.side.accountName}
            </span>

            <span className="min-w-0 flex-1">
              <span className="block truncate text-[12.5px] text-ink">
                {r.side.description}
              </span>
              {r.side.counterparty && (
                <span className="block truncate text-[10.5px] text-ink-3">
                  {r.side.counterparty}
                </span>
              )}
            </span>

            <span
              className={cn(
                "tnum shrink-0 text-[13px]",
                r.side.amount < 0 ? "text-ink" : "text-positive",
              )}
            >
              {fmtMoneyExact(r.side.amount)}
            </span>

            <span className="tnum w-10 shrink-0 text-right text-[10.5px] text-ink-3">
              #{r.side.id}
            </span>
          </div>
        ))}
      </div>

      <p className="mt-2.5 text-[11.5px] leading-[1.6] text-ink-3">{reason}</p>
      {footer && <p className="mt-1 text-[11.5px] text-brass">{footer}</p>}
      {warn && (
        <p className="mt-2 flex items-start gap-1.5 text-[11.5px] leading-[1.5] text-negative">
          <TriangleAlert className="mt-px size-3.5 shrink-0" strokeWidth={2} />
          {warn}
        </p>
      )}

      <div className="mt-3 flex justify-end gap-2">
        <Button variant="ghost" onClick={onReject}>
          <X className="size-3" strokeWidth={2} />
          {rejectLabel}
        </Button>
        <Button variant="primary" onClick={onConfirm}>
          <Check className="size-3" strokeWidth={2} />
          {confirmLabel}
        </Button>
      </div>
    </div>
  );
}
