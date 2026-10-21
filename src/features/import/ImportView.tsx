import { useCallback, useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  Check,
  ChevronDown,
  ChevronRight,
  FileSpreadsheet,
  Sparkles,
  Undo2,
  X,
} from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { MatchReview } from "./MatchReview";
import {
  Banner,
  Button,
  Checkbox,
  Empty,
  PageHead,
  SelectField,
  TextField,
} from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { RemoveImport } from "./RemoveImport";
import { fmtMoney } from "@/lib/money";
import type {
  Account,
  BankMapping,
  Batch,
  DateFormat,
  EgressPreview,
  ImportImpact,
  ImportSummary,
  Inspection,
  PendingSplit,
  PendingTransfer,
  Preview,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { region } from "@/lib/region";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Getting statements in.
 *
 * `embedded` drops the page header and outer padding so this can sit as a tab
 * inside the cash-flow module, which is where it belongs: importing is not a
 * destination, it is the first step of looking at your money.
 */
export function ImportView({
  embedded = false,
  onImported,
}: {
  embedded?: boolean;
  onImported?: () => void;
} = {}) {
  const t = useT();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [batches, setBatches] = useState<Batch[]>([]);
  const [transfers, setTransfers] = useState<PendingTransfer[]>([]);
  const [splits, setSplits] = useState<PendingSplit[]>([]);
  const [aiEnabled, setAiEnabled] = useState(false);
  const [dateFormats, setDateFormats] = useState<DateFormat[]>([]);

  const [accountId, setAccountId] = useState<number | null>(null);
  const [path, setPath] = useState<string | null>(null);
  const [inspection, setInspection] = useState<Inspection | null>(null);
  const [mapping, setMapping] = useState<BankMapping | null>(null);
  const [manual, setManual] = useState<Preview | null>(null);
  const [showColumns, setShowColumns] = useState(false);
  const [summary, setSummary] = useState<ImportSummary | null>(null);
  const [aiPending, setAiPending] = useState<EgressPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [removing, setRemoving] = useState<{ id: number; impact: ImportImpact } | null>(null);

  const refresh = useCallback(() => {
    Promise.all([
      api.accountList(),
      api.importBatches(),
      api.matchingPendingTransfers(),
      api.matchingPendingSplits(),
      api.aiSettingsGet(),
      api.dateFormats(),
    ])
      .then(([a, b, t, s, ai, df]) => {
        setAccounts(a);
        setBatches(b);
        setTransfers(t);
        setSplits(s);
        setAiEnabled(ai.enabled);
        setDateFormats(df);
        setAccountId((cur) => cur ?? a[0]?.id ?? null);
      })
      .catch((e: unknown) => setError(msg(e)));
  }, []);

  useEffect(refresh, [refresh]);

  // Re-parse when the user overrides a column by hand — and when the
  // account changes, because the account's currency decides how many
  // decimals an amount has.
  const accountCurrency = accounts.find((a) => a.id === accountId)?.currency;
  useEffect(() => {
    if (!path || !mapping || (!showColumns && !accountCurrency)) return;
    let live = true;
    const timer = setTimeout(() => {
      api
        .importPreview(path, accountCurrency ? { ...mapping, currency: accountCurrency } : mapping)
        .then((p) => live && (setManual(p), setError(null)))
        .catch((e: unknown) => live && setError(msg(e)));
    }, 150);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [path, mapping, showColumns, accountCurrency]);

  const choose = async () => {
    const chosen = await open({
      multiple: false,
      filters: [
        { name: t.importer.fileFilter, extensions: ["csv", "xlsx", "xlsm", "xls", "ods", "txt", "tsv", "pdf"] },
      ],
    });
    if (typeof chosen !== "string") return;

    setPath(chosen);
    setSummary(null);
    setManual(null);
    setShowColumns(false);
    setBusy(t.importer.reading);
    setError(null);
    api
      .importInspect(chosen)
      .then((i) => {
        setInspection(i);
        setMapping(i.detection.mapping);
        setShowColumns(i.needsConfirmation);
      })
      .catch((e: unknown) => {
        setError(msg(e));
        setInspection(null);
      })
      .finally(() => setBusy(null));
  };

  const runImport = () => {
    if (!accountId || !path || !mapping) return;
    setBusy(t.importer.importing);
    setError(null);
    api
      .importRun(accountId, null, path, mapping)
      .then((s) => {
        setSummary(s);
        setInspection(null);
        setMapping(null);
        setManual(null);
        setPath(null);
        refresh();
        // The ledger behind this tab is now out of date by exactly the rows
        // that just arrived.
        onImported?.();
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setBusy(null));
  };

  const askAssistant = () => {
    if (!path || !inspection) return;
    setBusy(t.importer.preparing);
    api
      .aiMappingPreview(
        path,
        inspection.detection.headerRow,
        inspection.detection.firstDataRow,
      )
      .then(setAiPending)
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setBusy(null));
  };

  const sendToAssistant = () => {
    if (!aiPending || !mapping || !inspection) return;
    setBusy(t.importer.asking);
    api
      .aiMappingSend(aiPending, mapping, inspection.headers.length || 12)
      .then((m) => {
        setMapping(m);
        setShowColumns(true);
        setAiPending(null);
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setBusy(null));
  };

  const shown = manual ?? inspection;
  const rowCount = shown?.totalRows ?? 0;
  const set = <K extends keyof BankMapping>(k: K, v: BankMapping[K]) =>
    setMapping((m) => (m ? { ...m, [k]: v } : m));

  return (
    <div className="min-w-0">
      {embedded ? (
        <div className="flex items-center justify-between gap-4 pb-4">
          <p className="max-w-xl text-[12px] leading-[1.6] text-ink-3">{t.importer.lede}</p>
          <Button variant="primary" onClick={choose} disabled={!!busy}>
            <FileSpreadsheet className="size-3.5" strokeWidth={1.75} />
            {t.importer.chooseFile}
          </Button>
        </div>
      ) : (
        <PageHead
          title={t.importer.title}
          lede={t.importer.lede}
          actions={
            <Button variant="primary" onClick={choose} disabled={!!busy}>
              <FileSpreadsheet className="size-3.5" strokeWidth={1.75} />
              {t.importer.chooseFile}
            </Button>
          }
        />
      )}

      <div className={cn("space-y-5", !embedded && "px-10 pb-12")}>
        {error && <Banner tone="bad">{error}</Banner>}
        {note && <Banner tone="good">{note}</Banner>}
        {busy && <Banner>{busy}</Banner>}

        {summary && (
          <Banner tone="good">
            {t.importer.imported(summary.inserted, summary.duplicates, summary.rejected)}
          </Banner>
        )}

        {accounts.length === 0 ? (
          <Empty
            title={t.importer.noAccounts}
            body={t.importer.noAccountsBody}
            action={<NewAccount onCreated={refresh} onError={setError} />}
          />
        ) : (
          !inspection && (
            <Empty
              title={path ? t.importer.couldNotRead : t.importer.chooseStatement}
              body={t.importer.chooseBody}
              action={
                <Button variant="primary" onClick={choose}>
                  <FileSpreadsheet className="size-3.5" strokeWidth={1.75} />
                  {t.importer.chooseFile}
                </Button>
              }
            />
          )
        )}

        {inspection && mapping && (
          <>
            <Panel
              className={cn(
                inspection.needsConfirmation ? "border-brass/50" : "border-positive/40",
              )}
            >
              <PanelHead
                label={
                  inspection.needsConfirmation ? t.importer.checkFirst : t.importer.detected
                }
                aside={
                  <span className="text-[11px] text-ink-3">
                    {t.importer.confidence(inspection.detection.confidence)}
                  </span>
                }
              />
              <PanelBody className="space-y-3">
                <p className="text-[13px] text-ink">
                  <strong className="tnum">{t.importer.readable(rowCount)}</strong>
                  {shown && shown.rejected.length > 0 && (
                    <span className="text-ink-3">
                      {" "}
                      · {t.importer.unreadable(shown.rejected.length)}
                    </span>
                  )}
                  {" · "}
                  <span className="text-ink-3">
                    {path?.split("/").pop()}
                  </span>
                </p>

                <ul className="space-y-1">
                  {inspection.detection.notes.map((n) => (
                    <li key={n} className="flex gap-2 text-[12px] text-ink-2">
                      <Check className="mt-0.5 size-3 shrink-0 text-positive" strokeWidth={2.5} />
                      {n}
                    </li>
                  ))}
                  {inspection.detection.warnings.map((w) => (
                    <li key={w} className="flex gap-2 text-[12px] text-brass">
                      <span className="mt-0.5 size-3 shrink-0 text-center">!</span>
                      {w}
                    </li>
                  ))}
                </ul>

                <div className="flex flex-wrap items-center gap-2 border-t border-rule pt-3">
                  <SelectField
                    value={accountId ?? ""}
                    className="w-56"
                    onChange={(e) => setAccountId(Number(e.currentTarget.value))}
                  >
                    {accounts.map((a) => (
                      <option key={a.id} value={a.id}>
                        {t.importer.into(a.name, a.currency)}
                      </option>
                    ))}
                  </SelectField>
                  <Button
                    variant="primary"
                    disabled={!!busy || rowCount === 0}
                    onClick={runImport}
                  >
                    {t.importer.importRows(rowCount)}
                  </Button>
                  <Button variant="ghost" onClick={() => setShowColumns((v) => !v)}>
                    {showColumns ? (
                      <ChevronDown className="size-3" strokeWidth={2} />
                    ) : (
                      <ChevronRight className="size-3" strokeWidth={2} />
                    )}
                    {t.importer.adjustColumns}
                  </Button>
                  {aiEnabled && inspection.needsConfirmation && (
                    <Button onClick={askAssistant} disabled={!!busy}>
                      <Sparkles className="size-3" strokeWidth={1.75} />
                      {t.importer.askAssistant}
                    </Button>
                  )}
                  <NewAccount onCreated={refresh} onError={setError} compact />
                </div>
              </PanelBody>
            </Panel>

            {aiPending && (
              <Panel className="border-brass/50">
                <PanelHead
                  label={t.importer.aboutToSend(aiPending.bytes)}
                  aside={
                    <span className="font-mono text-[10.5px] text-ink-3">
                      sha256 {aiPending.payloadSha256.slice(0, 16)}…
                    </span>
                  }
                />
                <PanelBody className="space-y-3">
                  <p className="text-[12px] text-ink-2">
                    {t.importer.sendExplain(aiPending.rowCount, aiPending.provider)}
                  </p>
                  <pre
                    data-selectable
                    className="max-h-56 overflow-auto rounded-md border border-rule bg-paper-sunk p-3 font-mono text-[11px] leading-[1.5] text-ink-2"
                  >
                    {aiPending.payload}
                  </pre>
                  <div className="flex justify-end gap-2">
                    <Button
                      variant="ghost"
                      onClick={() => {
                        api.aiDecline(aiPending).catch(() => undefined);
                        setAiPending(null);
                      }}
                    >
                      {t.importer.dontSend}
                    </Button>
                    <Button variant="primary" onClick={sendToAssistant} disabled={!!busy}>
                      {t.importer.send}
                    </Button>
                  </div>
                </PanelBody>
              </Panel>
            )}

            {showColumns && (
              <Panel>
                <PanelHead
                  label={t.importer.columns}
                  aside={
                    <Button
                      variant="ghost"
                      onClick={() => {
                        setMapping(inspection.detection.mapping);
                        setManual(null);
                      }}
                    >
                      {t.importer.resetDetected}
                    </Button>
                  }
                />
                <PanelBody className="space-y-4">
                  {inspection.headers.length > 0 && (
                    <div className="flex flex-wrap gap-1.5">
                      {inspection.headers.map((h, i) => (
                        <span
                          key={`${i}-${h}`}
                          className="rounded border border-rule bg-surface-hi px-1.5 py-0.5 font-mono text-[10.5px] text-ink-2"
                        >
                          <span className="text-brass">{i}</span> {h || "—"}
                        </span>
                      ))}
                    </div>
                  )}
                  <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
                    <Num label={t.importer.dateCol} v={mapping.dateCol} on={(v) => set("dateCol", v ?? 0)} />
                    <SelectField
                      label={t.importer.dateFormat}
                      value={mapping.dateFormat}
                      className="sm:col-span-2"
                      onChange={(e) => set("dateFormat", e.currentTarget.value)}
                    >
                      {[...new Set(dateFormats.map((f) => f.group))].map((group) => (
                        <optgroup key={group} label={t.importer.dateGroups[group] ?? group}>
                          {dateFormats
                            .filter((f) => f.group === group)
                            .map((f) => (
                              <option key={f.pattern} value={f.pattern}>
                                {f.example} — {t.importer.dateLabels[f.pattern] ?? f.label}
                              </option>
                            ))}
                        </optgroup>
                      ))}
                    </SelectField>
                    <Num
                      label={t.importer.descriptionCol}
                      v={mapping.descriptionCol}
                      on={(v) => set("descriptionCol", v ?? 0)}
                    />
                    <Num
                      label={t.importer.amountCol}
                      v={mapping.amountCol}
                      nullable
                      on={(v) => set("amountCol", v)}
                    />
                    <TextField
                      label={t.importer.decimalSep}
                      value={mapping.decimalSeparator}
                      maxLength={1}
                      onChange={(e) => set("decimalSeparator", e.currentTarget.value || ",")}
                    />
                    <Num
                      label={t.importer.skipRows}
                      v={mapping.skipRows}
                      on={(v) => set("skipRows", v ?? 0)}
                    />
                  </div>
                  <div className="flex gap-5 border-t border-rule pt-3.5">
                    <Checkbox
                      label={t.importer.hasHeader}
                      checked={mapping.hasHeader}
                      onChange={(e) => set("hasHeader", e.currentTarget.checked)}
                    />
                    <Checkbox
                      label={t.importer.invertSign}
                      checked={mapping.invertSign}
                      onChange={(e) => set("invertSign", e.currentTarget.checked)}
                    />
                  </div>
                </PanelBody>
              </Panel>
            )}

            {shown && (
              <Panel>
                <PanelHead
                  label={t.importer.preview}
                  aside={
                    <span className="text-[11px] text-ink-3">
                      {t.importer.firstOf(Math.min(shown.rows.length, 50), rowCount)}
                    </span>
                  }
                />
                {shown.rows.length === 0 ? (
                  <PanelBody>
                    <p className="text-[12px] leading-[1.6] text-ink-3">
                      {t.importer.nothingParsed(aiEnabled)}
                    </p>
                  </PanelBody>
                ) : (
                  <div className="max-h-[320px] overflow-auto">
                    <table className="w-full">
                      <thead className="sticky top-0 bg-surface">
                        <tr className="border-b border-rule">
                          <th className="eyebrow py-2.5 pl-5 text-left font-medium">
                            {t.importer.date}
                          </th>
                          <th className="eyebrow py-2.5 text-left font-medium">
                            {t.importer.description}
                          </th>
                          <th className="eyebrow py-2.5 pr-5 text-right font-medium">
                            {t.importer.amount}
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        {shown.rows.map((r) => (
                          <tr key={r.line} className="border-b border-rule/60">
                            <td className="tnum py-1.5 pl-5 text-[12px] whitespace-nowrap text-ink-3">
                              {r.bookedOn}
                            </td>
                            <td className="max-w-0 truncate py-1.5 pr-4 text-[12.5px] text-ink">
                              {r.description}
                            </td>
                            <td
                              className={cn(
                                "tnum py-1.5 pr-5 text-right text-[12.5px]",
                                r.amountMinor < 0 ? "text-ink" : "text-positive",
                              )}
                            >
                              {fmtMoney(r.amountMinor)}
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
                {shown.rejected.length > 0 && (
                  <div className="border-t border-rule px-5 py-3">
                    <p className="eyebrow mb-2">{t.importer.rejectedRows}</p>
                    <ul className="space-y-1">
                      {shown.rejected.slice(0, 5).map((r) => (
                        <li key={r.line} className="text-[11.5px] text-ink-3">
                          <span className="tnum text-negative">{t.importer.line(r.line)}</span> —{" "}
                          {r.reason}
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </Panel>
            )}
          </>
        )}

        <MatchReview
          transfers={transfers}
          splits={splits}
          onDecide={(kind, a, b, confirm) =>
            api
              .matchingDecide(kind, a, b, confirm)
              .then(refresh)
              .catch((e: unknown) => setError(msg(e)))
          }
        />

        {removing && (
          <RemoveImport
            batchId={removing.id}
            impact={removing.impact}
            onCancel={() => setRemoving(null)}
            onDone={(n) => {
              setRemoving(null);
              setSummary(null);
              setNote(t.importer.remove.done(n));
              refresh();
              // The ledger behind this tab has just lost those rows.
              onImported?.();
            }}
            onError={setError}
          />
        )}

        {batches.length > 0 && (
          <Panel>
            <PanelHead label={t.importer.recent} />
            <div className="divide-y divide-rule">
              {batches.map((b) => (
                <div key={b.id} className="flex items-center gap-4 px-5 py-2.5 text-[12.5px]">
                  <span className="min-w-0 flex-1 truncate text-ink">
                    {b.sourceFilename ?? "—"}
                  </span>
                  <span className="text-ink-3">{b.accountName}</span>
                  <span className="tnum text-ink-2">
                    +{b.insertedCount}
                    {b.duplicateCount > 0 && (
                      <span className="text-ink-3"> {t.importer.dup(b.duplicateCount)}</span>
                    )}
                  </span>
                  <Button
                    variant="ghost"
                    onClick={() =>
                      api
                        .importImpact(b.id)
                        .then((impact) => setRemoving({ id: b.id, impact }))
                        .catch((e: unknown) => setError(msg(e)))
                    }
                  >
                    <Undo2 className="size-3" strokeWidth={1.75} />
                    {t.importer.undo}
                  </Button>
                </div>
              ))}
            </div>
          </Panel>
        )}
      </div>
    </div>
  );
}

/* ----------------------------------------------------------------- bits -- */

function Num({
  label,
  v,
  on,
  nullable,
}: {
  label: string;
  v: number | null;
  on: (v: number | null) => void;
  nullable?: boolean;
}) {
  return (
    <TextField
      label={label}
      type="number"
      min={0}
      value={v ?? ""}
      placeholder={nullable ? "—" : "0"}
      onChange={(e) => {
        const raw = e.currentTarget.value;
        on(raw === "" ? (nullable ? null : 0) : Number(raw));
      }}
    />
  );
}

function NewAccount({
  onCreated,
  onError,
  compact,
}: {
  onCreated: () => void;
  onError: (m: string) => void;
  compact?: boolean;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [kind, setKind] = useState("checking");
  const [currency, setCurrency] = useState(region().currency);

  if (!open) {
    return (
      <Button variant={compact ? "ghost" : "primary"} onClick={() => setOpen(true)}>
        {t.importer.newAccount}
      </Button>
    );
  }
  return (
    <div className="flex items-end gap-2">
      <TextField
        label={t.importer.name}
        value={name}
        autoFocus
        className="w-40"
        onChange={(e) => setName(e.currentTarget.value)}
      />
      <SelectField
        label={t.importer.kind}
        value={kind}
        className="w-32"
        onChange={(e) => setKind(e.currentTarget.value)}
      >
        {["checking", "savings", "credit_card", "cash", "brokerage", "other"].map((k) => (
          <option key={k} value={k}>
            {t.importer.accountKinds[k] ?? k}
          </option>
        ))}
      </SelectField>
      <TextField
        label={t.importer.currency}
        value={currency}
        maxLength={3}
        className="w-20"
        onChange={(e) => setCurrency(e.currentTarget.value.toUpperCase())}
      />
      <Button
        variant="primary"
        onClick={() =>
          api
            .accountCreate(name, kind, currency, null)
            .then(() => {
              setOpen(false);
              setName("");
              onCreated();
            })
            .catch((e: unknown) => onError(msg(e)))
        }
      >
        {t.common.create}
      </Button>
      <Button variant="ghost" onClick={() => setOpen(false)}>
        <X className="size-3.5" strokeWidth={2} />
      </Button>
    </div>
  );
}
