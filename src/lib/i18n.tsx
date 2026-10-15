/**
 * The interface language.
 *
 * Dictionaries are plain objects rather than string keys: `t.ledger.title`
 * is checked by the compiler, and `es` is typed as the shape of `en`, so a
 * phrase missing from the Spanish dictionary is a build error rather than a
 * blank label discovered by a user. Phrases that carry a value are functions,
 * which is also where plurals are decided — per language, because Spanish and
 * English do not agree on them.
 */
import { createContext, useContext, type ReactNode } from "react";
import { en, type Dict } from "@/locales/en";
import { es } from "@/locales/es";

export type Lang = "en" | "es";

const DICTS: Record<Lang, Dict> = { en, es };

/** Remembered outside the database so the unlock screen can use it too. */
const STORE_KEY = "coinheaven.lang";

/**
 * The language to use before the settings are readable: the last one chosen
 * on this machine, else the system's.
 */
export function initialLang(): Lang {
  try {
    const stored = localStorage.getItem(STORE_KEY);
    if (stored === "en" || stored === "es") return stored;
  } catch {
    // Storage can be unavailable; the system language is a fine fallback.
  }
  const system = typeof navigator !== "undefined" ? navigator.language : "en";
  return system.toLowerCase().startsWith("es") ? "es" : "en";
}

export function rememberLang(lang: Lang) {
  try {
    localStorage.setItem(STORE_KEY, lang);
  } catch {
    // Only a convenience for the next launch.
  }
}

const Ctx = createContext<{ lang: Lang; t: Dict }>({ lang: "en", t: en });

export function I18nProvider({ lang, children }: { lang: Lang; children: ReactNode }) {
  return <Ctx.Provider value={{ lang, t: DICTS[lang] }}>{children}</Ctx.Provider>;
}

export const useT = () => useContext(Ctx).t;
export const useLang = () => useContext(Ctx).lang;

/** For code outside React (formatters) that still needs the words. */
export const dictFor = (lang: Lang) => DICTS[lang];
