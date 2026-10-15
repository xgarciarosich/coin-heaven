import { useCallback, useEffect, useState } from "react";
import { api } from "@/lib/ipc";
import { useTheme } from "@/lib/theme";
import { setMoneyHidden } from "@/lib/money";
import { I18nProvider, initialLang, rememberLang, useT, type Lang } from "@/lib/i18n";
import type { AppSettings, AppStatus, SimParams, UnlockStatus } from "@/lib/types";
import { Sidebar, visibleTabs, type Tab } from "@/components/Sidebar";
import { SimulatorView } from "@/features/simulator/SimulatorView";
import { SetupGate } from "@/features/setup/SetupGate";
import { LedgerView } from "@/features/ledger/LedgerView";
import { WealthView } from "@/features/wealth/WealthView";
import { FireView } from "@/features/fire/FireView";
import { ChatView } from "@/features/assistant/ChatView";
import { SettingsView } from "@/features/settings/SettingsView";
import { REGIONS, detectRegion, setRegion } from "@/lib/region";

export default function App() {
  const { theme, toggle } = useTheme();
  const [tab, setTab] = useState<Tab>("events");
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [unlock, setUnlock] = useState<UnlockStatus | null>(null);
  const [defaults, setDefaults] = useState<SimParams | null>(null);
  const [fatal, setFatal] = useState<string | null>(null);
  const [newVersion, setNewVersion] = useState<string | null>(null);
  const [settingsSection, setSettingsSection] = useState<"general" | "ai">("general");
  const [lang, setLang] = useState<Lang>(initialLang);
  const [privacy, setPrivacy] = useState<boolean>(() => {
    try {
      return localStorage.getItem("coinheaven.privacy") === "1";
    } catch {
      return false;
    }
  });
  // Set during render, before children format anything, so the first paint
  // after a toggle is already masked.
  setMoneyHidden(privacy);
  useEffect(() => {
    document.documentElement.classList.toggle("privacy", privacy);
    try {
      localStorage.setItem("coinheaven.privacy", privacy ? "1" : "0");
    } catch {
      // Only a convenience for the next launch.
    }
  }, [privacy]);

  const load = useCallback(() => {
    api
      .unlockStatus()
      .then((u) => {
        setUnlock(u);
        // Nothing else is meaningful until the database is open and the
        // recovery phrase has been acknowledged.
        if (u.state !== "opened" || !u.phraseAcknowledged) return undefined;
        return Promise.all([
          api.appStatus(),
          api.simDefaults(),
          api.settingsGet(),
        ]).then(([s, d, cfg0]) => {
          let cfg = cfg0;
          setStatus(s);
          // The region decides how every amount and date is written, and
          // what a new plan starts from; it is set before anything renders.
          //
          // On first run nobody has chosen a country, so one is proposed from
          // the system's locale and written down; it is visible and
          // changeable in Settings.
          const known = REGIONS.some((x) => x.id === cfg.region);
          const r = known ? setRegion(cfg.region) : setRegion(detectRegion(navigator.language).id);
          if (!known) {
            cfg = {
              ...cfg,
              region: r.id,
              country: r.country,
              primaryCurrency: r.currency,
              locale: r.locale,
            };
          }
          api.regionApply(r.merchantNoise).catch(() => undefined);
          const { transferTaxBps: _perProperty, ...general } = r.simulator;
          setDefaults({ ...d, ...general });
          if (cfg.language === "en" || cfg.language === "es") {
            setSettings(cfg);
            if (!known) api.settingsSave(cfg).catch(() => undefined);
            setLang(cfg.language);
            rememberLang(cfg.language);
          } else {
            // First run: nobody has chosen yet, so the system language is
            // written down as the choice. It is visible and changeable in
            // Settings, and from here on the backend speaks it too.
            const chosen = { ...cfg, language: initialLang() };
            setSettings(chosen);
            api.settingsSave(chosen).catch(() => undefined);
          }
        });
      })
      .catch((e: unknown) =>
        setFatal(e instanceof Error ? e.message : String(e)),
      );
  }, []);

  useEffect(load, [load]);

  // Only when the person has asked for it, and at most once a day: a single
  // request to the project's public release page, carrying nothing about them.
  const checkUpdates = settings?.checkUpdates ?? false;
  useEffect(() => {
    if (!checkUpdates) return;
    try {
      const last = Number(localStorage.getItem("coinheaven.update.checked") ?? 0);
      if (Date.now() - last < 24 * 3600 * 1000) return;
    } catch {
      /* no memory of a last check: check */
    }
    api
      .updateCheck()
      .then((u) => {
        try {
          localStorage.setItem("coinheaven.update.checked", String(Date.now()));
        } catch {
          /* ignored */
        }
        setNewVersion(u.newer ? u.latest : null);
      })
      .catch(() => undefined);
  }, [checkUpdates]);

  // A module hidden while you are looking at it should move you somewhere
  // real rather than leaving a blank pane.
  useEffect(() => {
    if (!settings) return;
    const allowed = visibleTabs(settings.modules);
    if (!allowed.includes(tab)) setTab(allowed[0] ?? "settings");
  }, [settings, tab]);

  return <I18nProvider lang={lang}>{body()}</I18nProvider>;

  function body() {
  if (fatal) return <Fatal message={fatal} />;

  if (unlock && (unlock.state !== "opened" || !unlock.phraseAcknowledged)) {
    return (
      <SetupGate
        status={unlock}
        onDone={load}
        theme={theme}
        onToggleTheme={toggle}
      />
    );
  }

  return (
    <div className="flex h-full">
      <Sidebar
        tab={tab}
        onTab={setTab}
        status={status}
        theme={theme}
        onToggleTheme={toggle}
        privacy={privacy}
        onTogglePrivacy={() => setPrivacy((v) => !v)}
        phraseUnconfirmed={unlock ? !unlock.phraseConfirmed : false}
        modules={settings?.modules ?? null}
        newVersion={newVersion}
      />
      {/* Keyed on privacy so every screen re-formats its amounts at once. */}
      <main key={privacy ? "hidden" : "shown"} className="flex-1 overflow-y-auto">
        {tab === "simulator" ? (
          defaults ? <SimulatorView initial={defaults} /> : <Waiting />
        ) : tab === "events" ? (
          <LedgerView modules={settings?.modules ?? null} />
        ) : tab === "wealth" ? (
          <WealthView />
        ) : tab === "fire" ? (
          <FireView />
        ) : tab === "settings" ? (
          <SettingsView
            // Remounted when the assistant sends someone here, so it opens on
            // the section they were sent to.
            key={settingsSection}
            initialSection={settingsSection}
            onChanged={load}
            onLanguage={(l) => {
              setLang(l);
              rememberLang(l);
            }}
          />
        ) : (
          <ChatView
            onOpenSettings={() => {
              setSettingsSection("ai");
              setTab("settings");
            }}
          />
        )}
      </main>
    </div>
  );
  }
}

function Waiting() {
  const t = useT();
  return (
    <div className="flex h-full items-center justify-center">
      <p className="text-[12px] text-ink-3">{t.common.loadingScenario}</p>
    </div>
  );
}

function Fatal({ message }: { message: string }) {
  const t = useT();
  return (
    <div className="flex h-full items-center justify-center p-10">
      <div className="max-w-sm">
        <h1 className="display text-[22px] text-ink">{t.common.backendDown}</h1>
        <p
          data-selectable
          className="mt-2 font-mono text-[11.5px] leading-relaxed text-ink-2"
        >
          {message}
        </p>
      </div>
    </div>
  );
}
