import { cn } from "@/lib/utils";
import { useId, useState } from "react";
import { ChevronDown } from "lucide-react";

/**
 * A labelled slider.
 *
 * The value sits on the same baseline as the label, right-aligned in tabular
 * figures so it never reflows while dragging — the single most noticeable
 * difference between a slider that feels solid and one that feels cheap.
 */
export function Slider({
  label,
  value,
  min,
  max,
  step = 1,
  onChange,
  format,
  note,
  className,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  format: (v: number) => string;
  note?: string;
  className?: string;
}) {
  const id = useId();
  const pct = max === min ? 0 : ((value - min) / (max - min)) * 100;

  return (
    <div className={cn("group", className)}>
      <div className="flex items-baseline justify-between gap-3">
        <label
          htmlFor={id}
          className="text-[12px] text-ink-2 transition-colors group-hover:text-ink"
        >
          {label}
        </label>
        <output
          htmlFor={id}
          className="tnum text-[12.5px] font-medium text-ink"
        >
          {format(value)}
        </output>
      </div>
      <input
        id={id}
        type="range"
        className="mt-1.5"
        style={{ "--pct": `${pct}%` } as React.CSSProperties}
        min={min}
        max={max}
        step={step}
        value={value}
        aria-valuetext={format(value)}
        onChange={(e) => onChange(Number(e.currentTarget.value))}
      />
      {note ? (
        <p className="mt-1 text-[11px] leading-[1.45] text-ink-3">{note}</p>
      ) : null}
    </div>
  );
}

/**
 * A titled run of sliders, separated from the next by a hairline.
 *
 * `collapsible` groups start shut: they hold the assumptions most people
 * never need to move, and a wall of twenty sliders hides the four that matter.
 */
export function FieldGroup({
  title,
  hint,
  collapsible = false,
  children,
}: {
  title: string;
  hint?: string;
  collapsible?: boolean;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(!collapsible);
  return (
    <div className="border-b border-rule px-5 py-4 last:border-b-0">
      {collapsible ? (
        <button
          type="button"
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
          className="flex w-full items-center gap-2 text-left"
        >
          <h3 className="eyebrow flex-1">{title}</h3>
          <ChevronDown
            className={cn("size-3.5 text-ink-3 transition-transform", !open && "-rotate-90")}
            strokeWidth={1.75}
          />
        </button>
      ) : (
        <h3 className="eyebrow">{title}</h3>
      )}
      {hint && (!collapsible || !open) && (
        <p className="mt-1 text-[11px] leading-[1.45] text-ink-3">{hint}</p>
      )}
      {open && <div className="mt-3.5 space-y-4">{children}</div>}
    </div>
  );
}
