import { cn } from "@/lib/utils";
import type { ComponentProps, ReactNode } from "react";

export function Panel({ className, ...props }: ComponentProps<"section">) {
  return (
    <section
      className={cn(
        "rounded-[10px] border border-rule bg-surface shadow-card",
        className,
      )}
      {...props}
    />
  );
}

/**
 * Panel headers are a hairline rule with a small-caps label sitting on it —
 * no heavy title bar, no background fill.
 */
export function PanelHead({
  label,
  aside,
  className,
}: {
  label: string;
  aside?: ReactNode;
  className?: string;
}) {
  return (
    <header
      className={cn(
        "flex items-center justify-between gap-4 border-b border-rule px-5 py-3",
        className,
      )}
    >
      <h2 className="eyebrow">{label}</h2>
      {aside}
    </header>
  );
}

export function PanelBody({ className, ...props }: ComponentProps<"div">) {
  return <div className={cn("px-5 py-4", className)} {...props} />;
}
