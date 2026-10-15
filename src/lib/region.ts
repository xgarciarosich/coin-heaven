/**
 * Where the person lives: currency, number and date conventions, and the
 * defaults that depend on a country's rules.
 *
 * Every region is a JSON file in `/regions`. They are found at build time
 * rather than listed here, so adding a country is adding a file — see
 * `regions/README.md`. What a file may contain is in `regionSchema.ts`.
 */
import { parseRegion, type Region } from "./regionSchema";

export type { Region };

const files = import.meta.glob<unknown>("../../regions/*.json", {
  eager: true,
  import: "default",
});

const loaded = Object.entries(files)
  // The schema that describes region files lives beside them.
  .filter(([path]) => !path.endsWith(".schema.json"))
  .map(([path, raw]) => {
    const file = path.split("/").pop() ?? path;
    return { file, ...parseRegion(raw, file.replace(/\.json$/, "")) };
  });

/** Every region that ships with this build, by name. */
export const REGIONS: Region[] = loaded
  .flatMap((l) => (l.region ? [l.region] : []))
  .sort((a, b) => a.name.localeCompare(b.name));

/**
 * Files that are in the folder but could not be used, and why — shown in
 * Settings, so whoever is adding a country sees what to fix rather than
 * wondering why it did not appear.
 */
export const REGION_PROBLEMS: { file: string; problems: string[] }[] = loaded
  .filter((l) => l.problems.length > 0)
  .map((l) => ({ file: l.file, problems: l.problems }));

/** Used only when no region file can be read at all. */
const FALLBACK = "us";
const find = (id: string) =>
  REGIONS.find((r) => r.id === id) ?? REGIONS.find((r) => r.id === FALLBACK) ?? REGIONS[0]!;

let active: Region = find(FALLBACK);

/**
 * The region that best fits a language tag such as "en-GB" or "es-MX": the
 * one with that exact locale, else the one for that country, else the first
 * for that language. Nobody is asked: it is only a first guess, visible and
 * changeable in Settings.
 */
export const detectRegion = (tag: string): Region => {
  const [lang = "", country = ""] = tag.toLowerCase().split(/[-_]/);
  return (
    REGIONS.find((r) => r.locale.toLowerCase() === tag.toLowerCase().replace("_", "-")) ??
    (country ? REGIONS.find((r) => r.id === country) : undefined) ??
    REGIONS.find((r) => r.locale.toLowerCase().startsWith(`${lang}-`)) ??
    find(FALLBACK)
  );
};

/** The region in force. Read at the moment something is formatted. */
export const region = (): Region => active;

/** Switch region. An unknown id — a file since removed — falls back. */
export const setRegion = (id: string): Region => {
  active = find(id);
  return active;
};

/** "2026-06-30" the way this region writes a date. */
export const fmtDate = (iso: string): string => {
  const [y, m, d] = iso.slice(0, 10).split("-").map(Number);
  if (!y || !m || !d) return iso;
  return new Intl.DateTimeFormat(active.locale, {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    timeZone: "UTC",
  }).format(new Date(Date.UTC(y, m - 1, d)));
};
