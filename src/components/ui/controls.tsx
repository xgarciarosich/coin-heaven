import { cn } from "@/lib/utils";
import type { ComponentProps, ReactNode } from "react";
import { useId, useState } from "react";
import { ChevronDown } from "lucide-react";

/* ------------------------------------------------------------- buttons -- */

type Variant = "primary" | "secondary" | "ghost" | "danger";

const VARIANTS: Record<Variant, string> = {
  primary: "bg-ink text-paper hover:opacity-90",
  secondary: "border border-rule bg-surface text-ink hover:border-rule-strong",
  ghost: "text-ink-2 hover:bg-surface-hi hover:text-ink",
  danger: "border border-negative/40 text-negative hover:bg-negative/10",
};

export function Button({
  variant = "secondary",
  className,
  ...props
}: ComponentProps<"button"> & { variant?: Variant }) {
  return (
    <button
      type="button"
      className={cn(
        "inline-flex items-center justify-center gap-1.5 rounded-md px-3 py-1.5",
        "text-[12.5px] font-medium transition-colors",
        "disabled:pointer-events-none disabled:opacity-35",
        VARIANTS[variant],
        className,
      )}
      {...props}
    />
  );
}

/* -------------------------------------------------------------- fields -- */

export function Label({ children }: { children: ReactNode }) {
  return <span className="eyebrow">{children}</span>;
}

const inputBase =
  "w-full rounded-md border border-rule bg-surface px-2.5 py-1.5 text-[12.5px] " +
  "text-ink outline-none transition-colors placeholder:text-ink-3 focus:border-brass";

export function TextField({
  label,
  hint,
  className,
  ...props
}: ComponentProps<"input"> & { label?: string; hint?: string }) {
  const id = useId();
  return (
    <div className={cn("space-y-1.5", className)}>
      {label && (
        <label htmlFor={id} className="eyebrow block">
          {label}
        </label>
      )}
      <input id={id} className={inputBase} {...props} />
      {hint && <p className="text-[11px] leading-snug text-ink-3">{hint}</p>}
    </div>
  );
}

export function SelectField({
  label,
  hint,
  children,
  className,
  ...props
}: ComponentProps<"select"> & { label?: string; hint?: string }) {
  const id = useId();
  return (
    <div className={cn("space-y-1.5", className)}>
      {label && (
        <label htmlFor={id} className="eyebrow block">
          {label}
        </label>
      )}
      <select id={id} className={cn(inputBase, "appearance-none pr-7")} {...props}>
        {children}
      </select>
      {hint && <p className="text-[11px] leading-snug text-ink-3">{hint}</p>}
    </div>
  );
}

export function Checkbox({
  label,
  ...props
}: ComponentProps<"input"> & { label: string }) {
  const id = useId();
  return (
    <label htmlFor={id} className="flex cursor-pointer items-center gap-2">
      <input
        id={id}
        type="checkbox"
        className="size-3.5 accent-[var(--brass)]"
        {...props}
      />
      <span className="text-[12.5px] text-ink-2">{label}</span>
    </label>
  );
}

/* --------------------------------------------------------------- state -- */

export function Empty({
  title,
  body,
  action,
}: {
  title: string;
  body: string;
  action?: ReactNode;
}) {
  return (
    <div className="rounded-[10px] border border-dashed border-rule px-6 py-10 text-center">
      <p className="text-[13px] text-ink">{title}</p>
      <p className="mx-auto mt-1.5 max-w-sm text-[12px] leading-[1.6] text-ink-3">
        {body}
      </p>
      {action && <div className="mt-4 flex justify-center">{action}</div>}
    </div>
  );
}

export function Banner({
  tone = "info",
  children,
}: {
  tone?: "info" | "good" | "bad";
  children: ReactNode;
}) {
  return (
    <div
      className={cn(
        "rounded-[10px] border px-4 py-3 text-[12.5px] leading-[1.6]",
        tone === "good" && "border-positive/35 bg-positive/[0.07] text-ink",
        tone === "bad" && "border-negative/35 bg-negative/[0.07] text-ink",
        tone === "info" && "border-rule bg-surface text-ink-2",
      )}
    >
      {children}
    </div>
  );
}

/** A page heading in the editorial style used across the app. */
export function PageHead({
  eyebrow,
  title,
  lede,
  actions,
}: {
  eyebrow?: string;
  title: string;
  lede?: string;
  actions?: ReactNode;
}) {
  return (
    <header data-tauri-drag-region className="px-10 pt-9 pb-7">
      {eyebrow && <p className="eyebrow">{eyebrow}</p>}
      <div className={cn(eyebrow && "mt-2.5", "flex items-end justify-between gap-6")}>
        <h1 className="display text-[30px] leading-[1.1] text-ink">{title}</h1>
        {actions && <div className="flex shrink-0 gap-2 pb-1">{actions}</div>}
      </div>
      {lede && (
        <p className="mt-3 max-w-2xl text-[13px] leading-[1.65] text-ink-2">
          {lede}
        </p>
      )}
    </header>
  );
}

/* ---------------------------------------------------------- disclosure -- */

/**
 * A section that stays shut until it is wanted.
 *
 * The line it shows while shut is not a label but the current answer — "Rent
 * €11,800 a year · pension from 67" — so the detail can be left closed
 * without leaving the reader unsure what is in force.
 */
export function Disclosure({
  title,
  summary,
  defaultOpen = false,
  children,
}: {
  title: string;
  summary?: ReactNode;
  defaultOpen?: boolean;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <section className="rounded-[10px] border border-rule bg-surface shadow-card">
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center gap-3 px-5 py-3.5 text-left"
      >
        <ChevronDown
          className={cn("size-3.5 shrink-0 text-ink-3 transition-transform", !open && "-rotate-90")}
          strokeWidth={1.75}
        />
        <span className="shrink-0 text-[13px] text-ink">{title}</span>
        {summary && (
          <span className="min-w-0 flex-1 truncate text-right text-[11.5px] text-ink-3">
            {summary}
          </span>
        )}
      </button>
      {open && <div className="border-t border-rule">{children}</div>}
    </section>
  );
}

/** A small set of mutually exclusive choices, shown all at once. */
export function Segmented<T extends string | number>({
  options,
  value,
  onChange,
}: {
  options: readonly (readonly [T, string])[];
  value: T | null;
  onChange: (v: T) => void;
}) {
  return (
    <div className="inline-flex w-full rounded-md border border-rule bg-surface-hi p-0.5">
      {options.map(([id, label]) => (
        <button
          key={String(id)}
          type="button"
          aria-pressed={value === id}
          onClick={() => onChange(id)}
          className={cn(
            "flex-1 rounded px-2 py-1 text-[12px] whitespace-nowrap transition-colors",
            value === id
              ? "bg-surface text-ink shadow-[0_1px_2px_oklch(0_0_0/0.06)]"
              : "text-ink-3 hover:text-ink",
          )}
        >
          {label}
        </button>
      ))}
    </div>
  );
}
