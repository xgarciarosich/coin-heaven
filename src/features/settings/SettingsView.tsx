import { useCallback, useEffect, useState } from "react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { Banner, Checkbox, PageHead } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import type { AppSettings, ModuleToggles } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useLang, useT, type Lang } from "@/lib/i18n";
import { REGIONS, REGION_PROBLEMS, setRegion, type Region } from "@/lib/region";
import { AiSettingsPanel } from "./AiSettingsPanel";
import { DataPanels } from "./DataPanels";

/** "1.234,56 € · 31/12/2026": how this region writes a number and a date. */
function sample(r: Region) {
  const n = new Intl.NumberFormat(r.locale, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(1234.56);
  const d = new Intl.DateTimeFormat(r.locale, {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  }).format(new Date(Date.UTC(2026, 11, 31)));
  return `${r.symbolFirst ? `${r.currencySymbol}${n}` : `${n} ${r.currencySymbol}`} · ${d}`;
}

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

const MODULES: (keyof ModuleToggles)[] = [
  "import",
  "ledger",
  "wealth",
  "fire",
  "simulator",
  "assistant",
];

export function SettingsView({
  initialSection = "general",
  onChanged,
  onLanguage,
}: {
  initialSection?: "general" | "ai";
  onChanged: () => void;
  onLanguage: (l: Lang) => void;
}) {
  const t = useT();
  const lang = useLang();
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [section, setSection] = useState<"general" | "ai">(initialSection);

  const refresh = useCallback(() => {
    api
      .settingsGet()
      // Before the first-run default has been written, show the language the
      // screen is actually in rather than no selection at all.
      .then((s) => setSettings({ ...s, language: s.language || lang }))
      .catch((e: unknown) => setError(msg(e)));
  }, [lang]);

  useEffect(refresh, [refresh]);

  const save = (next: AppSettings) => {
    setSettings(next);
    setError(null);
    api
      .settingsSave(next)
      .then(() => {
        onChanged();
        refresh();
      })
      .catch((e: unknown) => {
        setError(msg(e));
        refresh();
      });
  };

  if (!settings) return null;
  const enabledCount = Object.values(settings.modules).filter(Boolean).length;

  return (
    <div className="min-w-0">
      <PageHead eyebrow={t.settings.eyebrow} title={t.settings.title} lede={t.settings.lede} />

      <div className="space-y-5 px-10 pb-12">
        {error && <Banner tone="bad">{error}</Banner>}

        <div className="flex items-center gap-1 border-b border-rule">
          {(
            [
              ["general", t.settings.tabs.general],
              ["ai", t.settings.tabs.ai],
            ] as const
          ).map(([id, label]) => (
            <button
              key={id}
              type="button"
              onClick={() => setSection(id)}
              className={cn(
                "-mb-px border-b-2 px-3 py-2 text-[12.5px] transition-colors",
                section === id
                  ? "border-brass text-ink"
                  : "border-transparent text-ink-3 hover:text-ink",
              )}
            >
              {label}
            </button>
          ))}
        </div>

        {section === "ai" ? (
          <AiSettingsPanel />
        ) : (
          <>
        <Panel>
          <PanelHead label={t.settings.language} />
          <PanelBody className="space-y-3">
            <div className="inline-flex rounded-md border border-rule bg-surface-hi p-0.5">
              {(
                [
                  ["es", "Español"],
                  ["en", "English"],
                ] as const
              ).map(([code, label]) => (
                <button
                  key={code}
                  type="button"
                  onClick={() => {
                    if (lang === code) return;
                    onLanguage(code);
                    save({ ...settings, language: code });
                  }}
                  className={cn(
                    "rounded px-3 py-1 text-[12.5px] transition-colors",
                    // The language the screen is in, not the stored value:
                    // they can only differ after a failed save, and then the
                    // screen is the truth the user is looking at.
                    lang === code
                      ? "bg-surface text-ink shadow-[0_1px_2px_oklch(0_0_0/0.06)]"
                      : "text-ink-3 hover:text-ink",
                  )}
                >
                  {label}
                </button>
              ))}
            </div>
            <p className="text-[11.5px] leading-[1.6] text-ink-3">{t.settings.languageHint}</p>
          </PanelBody>
        </Panel>

        <Panel>
          <PanelHead label={t.settings.modules} />
          <div className="divide-y divide-rule">
            {MODULES.map((id) => {
              const on = settings.modules[id];
              const [label, blurb] = t.settings.moduleList[id] ?? [id, ""];
              const last = on && enabledCount === 1;
              return (
                <div key={id} className="flex items-center gap-4 px-5 py-3">
                  <div className="min-w-0 flex-1">
                    <p className="text-[13px] text-ink">{label}</p>
                    <p className="text-[11.5px] text-ink-3">{blurb}</p>
                  </div>
                  <Checkbox
                    label={on ? t.settings.shown : t.settings.hidden}
                    checked={on}
                    disabled={last}
                    onChange={(e) =>
                      save({
                        ...settings,
                        modules: { ...settings.modules, [id]: e.currentTarget.checked },
                      })
                    }
                  />
                </div>
              );
            })}
          </div>
          <div className="border-t border-rule px-5 py-2.5">
            <p className="text-[11px] text-ink-3">{t.settings.atLeastOne}</p>
          </div>
        </Panel>

        <Panel>
          <PanelHead label={t.settings.region} />
          <div className="divide-y divide-rule">
            {REGIONS.map((r) => {
              const on = settings.region === r.id;
              return (
                <button
                  key={r.id}
                  type="button"
                  aria-pressed={on}
                  onClick={() => {
                    if (on) return;
                    setRegion(r.id);
                    api.regionApply(r.merchantNoise).catch(() => undefined);
                    save({
                      ...settings,
                      region: r.id,
                      country: r.country,
                      primaryCurrency: r.currency,
                      locale: r.locale,
                    });
                  }}
                  className={cn(
                    "flex w-full items-center gap-4 px-5 py-3 text-left transition-colors",
                    on ? "bg-brass-wash/40" : "hover:bg-surface-hi",
                  )}
                >
                  <span
                    className={cn(
                      "size-3 shrink-0 rounded-full border",
                      on ? "border-brass bg-brass" : "border-rule-strong",
                    )}
                  />
                  <span className="w-40 text-[13px] text-ink">{r.name}</span>
                  <span className="font-mono text-[12px] text-ink-2">
                    {r.currency} · {r.currencySymbol}
                  </span>
                  <span className="tnum flex-1 text-right text-[12px] text-ink-3">
                    {sample(r)}
                  </span>
                </button>
              );
            })}
          </div>
          {/* A file someone is writing that Coin Heaven could not read: say why,
              here, rather than leave them wondering where their country is. */}
          {REGION_PROBLEMS.map((p) => (
            <div key={p.file} className="border-t border-rule bg-negative/5 px-5 py-3">
              <p className="text-[12px] text-ink">{t.settings.regionBroken(p.file)}</p>
              <ul className="mt-1 list-disc space-y-0.5 pl-4 text-[11.5px] leading-[1.5] text-ink-2">
                {p.problems.map((x) => (
                  <li key={x}>{x}</li>
                ))}
              </ul>
            </div>
          ))}
          <p className="border-t border-rule px-5 py-3 text-[11.5px] leading-[1.6] text-ink-3">
            {t.settings.regionNote}
          </p>
        </Panel>

        <DataPanels settings={settings} onSave={save} onChanged={onChanged} />
          </>
        )}
      </div>
    </div>
  );
}
