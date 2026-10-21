/** Colours offered for categories, in the order they are handed out. */
export const PALETTE = [
  "#f97316", "#22c55e", "#a855f7", "#06b6d4", "#eab308", "#ef4444", "#3b82f6",
  "#ec4899", "#14b8a6", "#84cc16", "#8b5cf6", "#f43f5e", "#0ea5e9", "#d97706",
  "#10b981", "#6366f1", "#64748b", "#c026d3", "#65a30d", "#0891b2",
];

const norm = (c: string) => c.trim().toLowerCase();

/** `h` in degrees, `s` and `l` in 0..1, as "#rrggbb". */
function hsl(h: number, s: number, l: number) {
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  const hex = (x: number) =>
    Math.round(x * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${hex(f(0))}${hex(f(8))}${hex(f(4))}`;
}

/**
 * The next colour for a new category: the first one in the palette nobody is
 * using. When the palette is used up, new hues are spread around the colour
 * wheel by the golden angle, so each new one lands far from the others rather
 * than next to a neighbour.
 */
export function nextColour(used: readonly (string | null | undefined)[]): string {
  const taken = new Set(used.filter((c): c is string => !!c).map(norm));
  const free = PALETTE.find((c) => !taken.has(norm(c)));
  if (free) return free;
  for (let i = 1; i < 720; i++) {
    const c = hsl((i * 137.508) % 360, 0.62, 0.52);
    if (!taken.has(c)) return c;
  }
  return PALETTE[0]!;
}
