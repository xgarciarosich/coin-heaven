import {
  Bot,
  Building2,
  Eye,
  EyeOff,
  Flame,
  Landmark,
  Lock,
  Moon,
  Receipt,
  Settings as SettingsIcon,
  ShieldAlert,
  Sun,
} from "lucide-react";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import type { Dict } from "@/locales/en";
import type { Theme } from "@/lib/theme";
import type { AppStatus, ModuleToggles } from "@/lib/types";

export type Tab =
  | "simulator"
  | "events"
  | "wealth"
  | "fire"
  | "assistant"
  | "settings";

/// `modules` names the toggles that keep an entry visible — an entry stays if
/// *any* of them is on, because Cash flow holds both the importer and the
/// ledger and turning one off should not take the other with it. Settings has
/// none and is always reachable, otherwise a user could hide their way out of
/// the app.
const NAV: {
  id: Tab;
  label: keyof Dict["nav"];
  icon: typeof Building2;
  modules: (keyof ModuleToggles)[];
}[] = [
  { id: "events", label: "cashflow", icon: Receipt, modules: ["ledger", "import"] },
  { id: "wealth", label: "wealth", icon: Landmark, modules: ["wealth"] },
  { id: "fire", label: "fire", icon: Flame, modules: ["fire"] },
  { id: "simulator", label: "simulator", icon: Building2, modules: ["simulator"] },
  { id: "assistant", label: "assistant", icon: Bot, modules: ["assistant"] },
  { id: "settings", label: "settings", icon: SettingsIcon, modules: [] },
];

const shown = (n: (typeof NAV)[number], modules: ModuleToggles | null) =>
  n.modules.length === 0 || !modules || n.modules.some((m) => modules[m]);

export function visibleTabs(modules: ModuleToggles | null): Tab[] {
  return NAV.filter((n) => shown(n, modules)).map((n) => n.id);
}

export function Sidebar({
  tab,
  onTab,
  status,
  theme,
  onToggleTheme,
  privacy,
  onTogglePrivacy,
  phraseUnconfirmed,
  modules,
  newVersion,
}: {
  tab: Tab;
  onTab: (t: Tab) => void;
  status: AppStatus | null;
  theme: Theme;
  onToggleTheme: () => void;
  privacy: boolean;
  onTogglePrivacy: () => void;
  phraseUnconfirmed: boolean;
  modules: ModuleToggles | null;
  /** A newer release exists (only ever set when the person asked to be told). */
  newVersion?: string | null;
}) {
  const t = useT();
  return (
    <aside className="flex w-[232px] shrink-0 flex-col border-r border-rule bg-paper-sunk">
      {/* Clears the macOS traffic lights and gives the window a drag handle. */}
      <div data-tauri-drag-region className="h-9 shrink-0" />

      <div data-tauri-drag-region className="px-5 pt-1 pb-6">
        <h1 className="display text-[19px] leading-none text-ink">Coin Heaven</h1>
        <p className="mt-1.5 text-[11px] text-ink-3">{t.nav.tagline}</p>
      </div>

      <nav className="flex-1 px-2.5">
        {NAV.filter((n) => shown(n, modules)).map((item) => {
          const active = tab === item.id;
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => onTab(item.id)}
              aria-current={active ? "page" : undefined}
              className={cn(
                "group relative flex w-full items-center gap-2.5 rounded-md py-[7px] pr-2 pl-3 text-left text-[12.5px] transition-colors",
                active
                  ? "bg-surface text-ink shadow-[0_1px_2px_oklch(0_0_0/0.05)]"
                  : "text-ink-2 hover:bg-surface-hi hover:text-ink",
              )}
            >
              {/* The active marker is a hairline bar, not a filled pill. */}
              <span
                className={cn(
                  "absolute top-1/2 left-0 h-3.5 w-[2px] -translate-y-1/2 rounded-r-full bg-brass transition-opacity",
                  active ? "opacity-100" : "opacity-0",
                )}
              />
              <item.icon
                className={cn(
                  "size-[14px] transition-colors",
                  active ? "text-brass" : "text-ink-3 group-hover:text-ink-2",
                )}
                strokeWidth={1.75}
              />
              <span className="flex-1">{t.nav[item.label] as string}</span>
            </button>
          );
        })}
      </nav>

      <footer className="space-y-3 border-t border-rule px-5 py-4">
        {newVersion && (
          <button
            type="button"
            onClick={() => onTab("settings")}
            className="text-left text-[11px] leading-[1.5] text-brass underline-offset-2 hover:underline"
          >
            {t.settings.data.available(newVersion)}
          </button>
        )}
        {phraseUnconfirmed && (
          <p className="flex items-start gap-1.5 text-[11px] leading-[1.5] text-brass">
            <ShieldAlert className="mt-px size-3 shrink-0" strokeWidth={2} />
            <span>{t.nav.phraseUnconfirmed}</span>
          </p>
        )}
        <div className="flex items-center gap-1.5 text-[11px] text-ink-3">
          <Lock className="size-3" strokeWidth={2} />
          <span>{status?.unlocked ? t.nav.unlocked : t.nav.locked}</span>
        </div>
        <div className="flex items-center justify-between">
          <span className="tnum text-[11px] text-ink-3">
            {t.nav.version(status?.version ?? "—", String(status?.schemaVersion ?? "—"))}
          </span>
          <div className="flex items-center">
          <button
            type="button"
            onClick={onTogglePrivacy}
            aria-pressed={privacy}
            aria-label={privacy ? t.nav.privacyOff : t.nav.privacyOn}
            title={privacy ? t.nav.privacyOff : t.nav.privacyOn}
            className={cn(
              "rounded p-1 transition-colors hover:bg-surface-hi hover:text-ink",
              privacy ? "text-brass" : "text-ink-3",
            )}
          >
            {privacy ? <EyeOff className="size-3.5" strokeWidth={1.75} /> : <Eye className="size-3.5" strokeWidth={1.75} />}
          </button>
          <button
            type="button"
            onClick={onToggleTheme}
            aria-label={t.nav.toTheme(theme === "light")}
            title={t.nav.toTheme(theme === "light")}
            className="rounded p-1 text-ink-3 transition-colors hover:bg-surface-hi hover:text-ink"
          >
            {theme === "light" ? (
              <Moon className="size-3.5" strokeWidth={1.75} />
            ) : (
              <Sun className="size-3.5" strokeWidth={1.75} />
            )}
          </button>
          </div>
        </div>
      </footer>
    </aside>
  );
}
