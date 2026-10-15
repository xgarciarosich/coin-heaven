import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Check } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { Banner, Button, Checkbox } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { region } from "@/lib/region";
import type { AppSettings, BackupStatus, ExportOutcome, SampleStatus, UpdateInfo } from "@/lib/types";
import { useLang, useT } from "@/lib/i18n";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Whether this region writes "1.234,56" — then a spreadsheet there wants
 *  semicolons between columns and a comma inside a number. */
function commaDecimal(locale: string) {
  return (
    new Intl.NumberFormat(locale).formatToParts(1.5).find((p) => p.type === "decimal")?.value === ","
  );
}

/**
 * Getting data out, trying the app with made-up data, and hearing about a new
 * version: the three things that are about the app rather than about any one
 * screen of it.
 */
export function DataPanels({
  settings,
  onSave,
  onChanged,
}: {
  settings: AppSettings;
  onSave: (next: AppSettings) => void;
  onChanged: () => void;
}) {
  const t = useT();
  const d = t.settings.data;
  const lang = useLang();
  const [error, setError] = useState<string | null>(null);

  /* ---- export ---- */
  const [exporting, setExporting] = useState(false);
  const [outcome, setOutcome] = useState<ExportOutcome | null>(null);
  const exportAll = async () => {
    setError(null);
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder !== "string") return;
    setExporting(true);
    const comma = commaDecimal(region().locale);
    api
      .dataExport(folder, comma ? ";" : ",", comma ? "," : ".")
      .then(setOutcome)
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setExporting(false));
  };

  /* ---- sample data ---- */
  const [sample, setSample] = useState<SampleStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const refreshSample = () =>
    api
      .sampleStatus()
      .then(setSample)
      .catch((e: unknown) => setError(msg(e)));
  useEffect(() => {
    void refreshSample();
  }, []);
  const toggleSample = () => {
    if (!sample) return;
    setBusy(true);
    setError(null);
    (sample.loaded
      ? api.sampleRemove()
      : api.sampleLoad(lang, settings.primaryCurrency || region().currency)
    )
      .then(() => {
        onChanged();
        return refreshSample();
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setBusy(false));
  };

  /* ---- backups ---- */
  const [backup, setBackup] = useState<BackupStatus | null>(null);
  const [backupMsg, setBackupMsg] = useState<string | null>(null);
  const [backingUp, setBackingUp] = useState(false);
  const refreshBackup = () =>
    api
      .backupStatus()
      .then(setBackup)
      .catch((e: unknown) => setError(msg(e)));
  useEffect(() => {
    void refreshBackup();
  }, []);
  const chooseBackupFolder = async () => {
    setError(null);
    const folder = await open({ directory: true, multiple: false });
    if (typeof folder !== "string") return;
    api
      .backupSetDirectory(folder)
      .then(refreshBackup)
      .catch((e: unknown) => setError(msg(e)));
  };
  const backUpNow = () => {
    setBackingUp(true);
    setError(null);
    setBackupMsg(null);
    api
      .backupNow()
      .then((o) => {
        setBackupMsg(
          typeof o === "object" ? d.backupDone(o.written.path) : o === "noDirectory" ? d.backupNoFolder : d.backupNothing,
        );
        return refreshBackup();
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setBackingUp(false));
  };

  /* ---- updates ---- */
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [checking, setChecking] = useState(false);
  const [copied, setCopied] = useState(false);
  const check = () => {
    setChecking(true);
    setError(null);
    api
      .updateCheck()
      .then((u) => {
        setInfo(u);
        try {
          localStorage.setItem("coinheaven.update.checked", String(Date.now()));
        } catch {
          /* a check that is not remembered is just checked again */
        }
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setChecking(false));
  };

  return (
    <>
      {error && <Banner tone="bad">{error}</Banner>}

      <Panel>
        <PanelHead label={d.exportTitle} />
        <PanelBody className="space-y-3">
          <p className="text-[12px] leading-[1.65] text-ink-2">{d.exportBody}</p>
          <Button variant="primary" onClick={() => void exportAll()} disabled={exporting}>
            {exporting ? d.exportBusy : d.exportGo}
          </Button>
          {outcome && (
            <div className="space-y-1.5 border-t border-rule pt-3">
              <p className="flex items-center gap-1.5 text-[12px] text-positive">
                <Check className="size-3.5" strokeWidth={2} />
                {d.exportDone(outcome.files.length, outcome.folder)}
              </p>
              <ul className="space-y-0.5 pl-5 text-[11.5px] text-ink-3">
                {outcome.files.map((f) => (
                  <li key={f.name}>{d.exportRows(f.name, f.rows)}</li>
                ))}
              </ul>
            </div>
          )}
        </PanelBody>
      </Panel>

      <Panel>
        <PanelHead label={d.backupTitle} />
        <PanelBody className="space-y-3">
          <p className="text-[12px] leading-[1.65] text-ink-2">{d.backupBody}</p>
          {backup && (
            <p className="text-[12px] text-ink-3">
              {backup.directory ? d.backupFolder(backup.directory) : d.backupNoFolder}
              {backup.lastBackupAt && ` · ${d.backupLast(backup.lastBackupAt)}`}
            </p>
          )}
          <div className="flex flex-wrap gap-2">
            <Button variant="primary" onClick={() => void chooseBackupFolder()}>
              {backup?.directory ? d.backupChange : d.backupChoose}
            </Button>
            <Button onClick={backUpNow} disabled={backingUp || !backup?.directory}>
              {backingUp ? d.backupBusy : d.backupNow}
            </Button>
          </div>
          {backup?.lastOutcome === "failed" && (
            <Banner tone="bad">{d.backupFailed}</Banner>
          )}
          {backupMsg && <p className="text-[12px] text-positive">{backupMsg}</p>}
        </PanelBody>
      </Panel>

      {sample && (sample.canLoad || sample.loaded) && (
        <Panel>
          <PanelHead label={d.sampleTitle} />
          <PanelBody className="space-y-3">
            <p className="text-[12px] leading-[1.65] text-ink-2">
              {sample.loaded ? d.sampleLoaded : d.sampleBody}
            </p>
            <Button
              variant={sample.loaded ? "secondary" : "primary"}
              onClick={toggleSample}
              disabled={busy}
            >
              {sample.loaded ? d.sampleRemove : d.sampleLoad}
            </Button>
          </PanelBody>
        </Panel>
      )}

      <Panel>
        <PanelHead label={d.updatesTitle} />
        <PanelBody className="space-y-3">
          <p className="text-[12px] leading-[1.65] text-ink-2">{d.updatesBody}</p>
          <Checkbox
            label={d.updatesToggle}
            checked={settings.checkUpdates}
            onChange={(e) => onSave({ ...settings, checkUpdates: e.currentTarget.checked })}
          />
          <Button onClick={check} disabled={checking}>
            {checking ? d.updatesChecking : d.updatesNow}
          </Button>
          {info && (
            <div className="space-y-1.5 border-t border-rule pt-3 text-[12px]">
              <p className={info.newer ? "text-brass" : "text-ink-2"}>
                {info.newer ? d.newer(info.latest, info.current) : d.upToDate(info.current)}
              </p>
              {info.newer && info.url && (
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-ink-3">{d.address}</span>
                  <code className="select-all rounded bg-surface-hi px-1.5 py-0.5 text-[11.5px] text-ink-2">
                    {info.url}
                  </code>
                  <Button
                    variant="ghost"
                    onClick={() => {
                      void navigator.clipboard?.writeText(info.url).then(() => setCopied(true));
                    }}
                  >
                    {copied ? d.copied : d.copy}
                  </Button>
                </div>
              )}
            </div>
          )}
        </PanelBody>
      </Panel>
    </>
  );
}
