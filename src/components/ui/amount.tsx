import { useState } from "react";
import { fmtMoney, fmtMoneyExact, sym } from "@/lib/money";

/** Accepts "0,015" and "0.015" alike, and a thousands dot before a comma. */
const parseDecimal = (s: string) =>
  Number(
    s
      .trim()
      .replace(/\./g, (m, i, all) => (all.indexOf(",") > i ? "" : m))
      .replace(",", "."),
  );

/**
 * A money amount that reads as money when it is at rest and as a plain number
 * while it is being typed, so "12.500 €" never has to be edited around.
 *
 * At rest it shows cents only when there are some: "€900" for a rent of nine
 * hundred, "€45,50" for a subscription. Rounding the second to "€46" would be
 * a number someone did not write.
 */
export function Amount({
  value,
  onChange,
  className,
  ariaLabel,
}: {
  value: number;
  onChange: (minor: number) => void;
  className?: string;
  ariaLabel?: string;
}) {
  const [text, setText] = useState<string | null>(null);
  const plain = (minor: number) => (minor === 0 ? "" : String(minor / 100).replace(".", ","));
  const rest = (minor: number) =>
    minor === 0 ? "" : minor % 100 === 0 ? fmtMoney(minor) : fmtMoneyExact(minor);
  return (
    <input
      inputMode="decimal"
      aria-label={ariaLabel}
      value={text ?? rest(value)}
      placeholder={fmtMoney(0)}
      onFocus={(e) => {
        setText(plain(value));
        const el = e.currentTarget;
        requestAnimationFrame(() => el.select());
      }}
      onBlur={() => setText(null)}
      onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
      onChange={(e) => {
        const raw = e.currentTarget.value;
        setText(raw);
        const v = raw.trim() === "" ? 0 : parseDecimal(raw.split(sym()).join("").replace(/\s/g, ""));
        if (Number.isFinite(v)) onChange(Math.round(v * 100));
      }}
      className={
        className ??
        "tnum w-full rounded-md border border-rule bg-surface px-2.5 py-1.5 text-right text-[12.5px] text-ink outline-none transition-colors placeholder:text-ink-3 focus:border-brass"
      }
    />
  );
}
