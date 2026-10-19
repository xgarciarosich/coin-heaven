import type { ReactNode } from "react";
import { GripVertical } from "lucide-react";
import { useReorder } from "@/lib/useReorder";
import { cn } from "@/lib/utils";

type Item = { "data-reorder-item": string };
type Handle = ReturnType<ReturnType<typeof useReorder>["handleProps"]>;

/**
 * A list whose items can be dragged into a new order by a grip, or moved with
 * the arrow keys once the grip has focus. `children` renders each item, given
 * the props for its outer element and its grip.
 */
export function Reorderable<K extends string | number>({
  keys,
  onMove,
  onDrop,
  className,
  children,
}: {
  keys: readonly K[];
  onMove: (from: number, to: number) => void;
  onDrop: () => void;
  className?: string;
  children: (key: K, index: number, item: Item, grip: Handle, dragging: boolean) => ReactNode;
}) {
  const r = useReorder(keys, onMove, onDrop);
  return (
    <div
      {...r.listProps}
      className={cn(className, r.dragging !== null && "cursor-grabbing select-none")}
    >
      {keys.map((k, i) => children(k, i, r.itemProps, r.handleProps(k), r.dragging === k))}
    </div>
  );
}

/** The grip itself: a small handle that reads as "drag me". */
export function Grip({ handle, label, className }: { handle: Handle; label: string; className?: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      {...handle}
      // A grip sits inside rows that open on click; grabbing it is not a click.
      onClick={(e) => e.stopPropagation()}
      className={cn(
        "shrink-0 cursor-grab touch-none rounded text-ink-3 opacity-40 outline-none transition-opacity",
        "group-hover:opacity-100 hover:text-ink focus-visible:opacity-100 focus-visible:ring-1 focus-visible:ring-brass active:cursor-grabbing",
        className,
      )}
    >
      <GripVertical className="size-3.5" strokeWidth={1.75} />
    </button>
  );
}
