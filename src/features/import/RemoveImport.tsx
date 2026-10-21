import { Button } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import type { ImportImpact } from "@/lib/types";
import { useT } from "@/lib/i18n";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * What removing an import would delete, said before anything is, with the two
 * ways to do it: only what has not been touched, or everything.
 *
 * Shared by the import list and the transaction manager, so the warning reads
 * the same wherever the removal starts.
 */
export function RemoveImport({
  batchId,
  impact,
  onCancel,
  onDone,
  onError,
}: {
  batchId: number;
  impact: ImportImpact;
  onCancel: () => void;
  /** Called with how many transactions were removed. */
  onDone: (removed: number) => void;
  onError: (message: string) => void;
}) {
  const t = useT();
  const r = t.importer.remove;
  const go = (everything: boolean) =>
    api
      .importUndo(batchId, everything)
      .then(onDone)
      .catch((e: unknown) => onError(msg(e)));

  return (
    <div
      role="alertdialog"
      aria-label={r.title(impact.filename ?? "—")}
      className="space-y-3 rounded-[10px] border border-negative/50 bg-negative/[0.05] px-5 py-4"
    >
      <p className="text-[13px] font-medium text-ink">{r.title(impact.filename ?? "—")}</p>
      <div className="space-y-1.5 text-[12.5px] leading-[1.65] text-ink-2">
        <p>{r.body(impact.total)}</p>
        {impact.touched > 0 && <p className="text-ink">{r.touched(impact.touched)}</p>}
        {impact.linkedFromElsewhere > 0 && <p>{r.linked(impact.linkedFromElsewhere)}</p>}
        <p className="font-medium text-negative">{r.permanent}</p>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="ghost" onClick={onCancel}>
          {t.common.cancel}
        </Button>
        {impact.touched > 0 && impact.total > impact.touched && (
          <Button variant="secondary" onClick={() => void go(false)}>
            {r.onlyUntouched(impact.total - impact.touched)}
          </Button>
        )}
        <Button variant="danger" onClick={() => void go(true)}>
          {r.all(impact.total)}
        </Button>
      </div>
    </div>
  );
}
