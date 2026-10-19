import { useEffect, useId, useRef, useState } from "react";

/** `list` with the item at `from` moved to `to`. */
export function move<T>(list: readonly T[], from: number, to: number): T[] {
  const next = [...list];
  const [item] = next.splice(from, 1);
  if (item === undefined) return next;
  next.splice(to, 0, item);
  return next;
}

/**
 * Drag-to-reorder for a list, with the arrow keys as the keyboard route.
 *
 * Pointer events rather than the browser's drag-and-drop: the latter is
 * intercepted by the window on some platforms and cannot be driven from a
 * keyboard. The pointer is captured on the list, which stays put, because an
 * item that moves in the page loses a capture held on itself. Several lists
 * can be nested (classes containing items): each only looks at its own items.
 *
 * `onMove` reorders the list as the pointer passes over items; `onDrop` runs
 * once, when the drag or key press is finished, to save the new order.
 */
export function useReorder<K extends string | number, E extends HTMLElement = HTMLDivElement>(
  keys: readonly K[],
  onMove: (from: number, to: number) => void,
  onDrop: () => void,
) {
  const scope = useId();
  const listRef = useRef<E>(null);
  const [dragging, setDragging] = useState<K | null>(null);
  const refocus = useRef<K | null>(null);

  const items = () =>
    listRef.current
      ? [...listRef.current.querySelectorAll<HTMLElement>(`[data-reorder-item="${scope}"]`)]
      : [];

  useEffect(() => {
    if (refocus.current === null) return;
    listRef.current
      ?.querySelector<HTMLElement>(`[data-reorder-handle="${scope}:${String(refocus.current)}"]`)
      ?.focus();
    refocus.current = null;
  }, [keys, scope]);

  const listProps = {
    ref: listRef,
    onPointerMove: (e: React.PointerEvent) => {
      if (dragging === null) return;
      const els = items();
      if (els.length === 0) return;
      // The item whose middle is nearest the pointer: gaps need no special case.
      let target = 0;
      let best = Infinity;
      els.forEach((el, i) => {
        const r = el.getBoundingClientRect();
        const d = Math.abs(e.clientY - (r.top + r.height / 2));
        if (d < best) {
          best = d;
          target = i;
        }
      });
      const from = keys.indexOf(dragging);
      if (from !== -1 && target !== from) onMove(from, target);
    },
    onPointerUp: (e: React.PointerEvent) => {
      if (dragging === null) return;
      listRef.current?.releasePointerCapture?.(e.pointerId);
      setDragging(null);
      onDrop();
    },
    onPointerCancel: (e: React.PointerEvent) => {
      if (dragging === null) return;
      listRef.current?.releasePointerCapture?.(e.pointerId);
      setDragging(null);
      onDrop();
    },
  };

  /** Spread on each item's outer element. */
  const itemProps = { "data-reorder-item": scope };

  /** Spread on the grip a person grabs. */
  const handleProps = (key: K) => ({
    "data-reorder-handle": `${scope}:${String(key)}`,
    onPointerDown: (e: React.PointerEvent) => {
      if (e.button !== 0) return;
      e.preventDefault();
      e.stopPropagation();
      listRef.current?.setPointerCapture(e.pointerId);
      setDragging(key);
    },
    onKeyDown: (e: React.KeyboardEvent) => {
      if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
      e.preventDefault();
      e.stopPropagation();
      const from = keys.indexOf(key);
      const to = from + (e.key === "ArrowUp" ? -1 : 1);
      if (from === -1 || to < 0 || to >= keys.length) return;
      refocus.current = key;
      onMove(from, to);
      // After the moved list has rendered, so the save reads the new order.
      setTimeout(onDrop, 0);
    },
  });

  return { listProps, itemProps, handleProps, dragging };
}
