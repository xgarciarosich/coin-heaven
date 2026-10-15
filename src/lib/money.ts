/**
 * Money crosses IPC as an integer count of minor units (cents) and is only
 * turned into a decimal here, at the very edge, for display. Nothing in the
 * app does arithmetic on the formatted value.
 */
export type Minor = number;

import { region } from "./region";

// Digit grouping and the decimal mark follow the region's locale; the symbol
// is placed by hand, where the region says, rather than by
// `style: "currency"` — which would also dictate spacing, and reads
// inconsistently next to the compact "€1,2M" forms.
const cache = new Map<string, Intl.NumberFormat>();
const number = (min: number, max: number) => {
  const key = `${region().locale}/${min}/${max}`;
  let f = cache.get(key);
  if (!f) {
    f = new Intl.NumberFormat(region().locale, {
      minimumFractionDigits: min,
      maximumFractionDigits: max,
    });
    cache.set(key, f);
  }
  return f;
};

/**
 * Privacy mode: every amount formatted anywhere becomes "€•••••". Done here,
 * at the one place money turns into text, so no screen can forget to hide
 * one — and dates and percentages, which say nothing on their own, stay.
 */
let hidden = false;
export const setMoneyHidden = (v: boolean) => {
  hidden = v;
};
const MASK = "•••••";

/** The region's currency symbol. */
export const sym = () => region().currencySymbol;

/** A formatted magnitude with the symbol where the region puts it. */
const place = (body: string) =>
  region().symbolFirst ? `${sym()}${body}` : `${body} ${sym()}`;

const withSymbol = (n: number, body: string) =>
  hidden ? place(MASK) : `${n < 0 ? "−" : ""}${place(body)}`;

/**
 * A plain decimal in the same convention as the money beside it. "10.6
 * years" next to "€184.500" reads as two different countries on one screen.
 */
export const fmtDecimal = (n: number, digits = 1) => number(digits, digits).format(n);

/** A whole number with the region's digit grouping, no symbol. */
export const fmtInt = (n: number) => number(0, 0).format(Math.round(n));

export const fmtMoney = (m: Minor) => withSymbol(m, number(0, 0).format(Math.abs(m) / 100));
export const fmtMoneyExact = (m: Minor) =>
  withSymbol(m, number(2, 2).format(Math.abs(m) / 100));

/** A figure in whole currency units, as charts carry them. */
export const fmtMajor = (major: number) =>
  Number.isFinite(major) ? fmtMoney(Math.round(major * 100)) : "—";

export const fmtCompact = (m: Minor) => {
  if (hidden) return place(MASK);
  const v = m / 100;
  const a = Math.abs(v);
  const sign = v < 0 ? "−" : "";
  if (a >= 1_000_000)
    return `${sign}${place(`${fmtDecimal(a / 1_000_000, a >= 10_000_000 ? 1 : 2)}M`)}`;
  if (a >= 1_000) return `${sign}${place(`${Math.round(a / 1_000)}k`)}`;
  return withSymbol(m, number(0, 0).format(a));
};

/**
 * Axis ticks: bare magnitude, no currency symbol — the axis label carries it.
 *
 * Keeps a decimal below 10k so a small range does not print the same label
 * twice: 2,400 and 1,800 both round to "2k", which makes an axis unreadable.
 */
export const fmtAxis = (major: number) => {
  if (hidden) return "•••";
  const a = Math.abs(major);
  if (a >= 1_000_000) return `${fmtDecimal(major / 1_000_000, 1)}M`;
  if (a >= 10_000) return `${Math.round(major / 1_000)}k`;
  if (a >= 1_000) return `${fmtDecimal(major / 1_000, 1)}k`;
  return String(Math.round(major));
};

/** Basis points -> human percentage. 280 -> "2.8%" */
export const fmtBps = (bps: number, decimals = 1) =>
  `${fmtDecimal(bps / 100, decimals)}%`;

export const majorToMinor = (major: number): Minor => Math.round(major * 100);
export const minorToMajor = (minor: Minor): number => minor / 100;
