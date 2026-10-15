# Design

The interface is meant to read as a ledger — paper, ink, hairline rules — not
as a dashboard. Four rules hold it together. New modules should follow them so
the app stays one thing rather than five.

## 1. The number is the headline

The largest element on the simulator is the projected net worth, set in the
system serif at 42px. Not a card, not a label with a value underneath — the
figure itself is the page title, with the supporting facts as a single line of
prose beneath it.

Everything numeric uses `.tnum` (tabular, lining figures). This is not a
detail: proportional figures shift width as a slider moves, and the resulting
jitter is the difference between a control that feels solid and one that feels
cheap.

## 2. One accent, and it means something

Brass marks **property equity** — the thing the app exists to grow. Green marks
income. Red marks a real problem: a breached debt-to-income ratio, negative net
rent. Nothing is coloured for decoration.

Everything else is a warm neutral. Every grey carries a little yellow
(`oklch(… 0.006 70)`), which is what makes the surface read as paper rather
than as a screen.

## 3. Hairlines, not boxes

Structure comes from 1px rules and small-caps labels sitting on them. Panels
have a single hairline border and an almost imperceptible shadow. The stat
strip is one panel divided by vertical rules, not four floating cards — four
cards give four equal things, and these are not equal.

Radii stay small (10px on panels, 6px on controls). Large radii read as
consumer software; this is an instrument.

## 4. Type does the hierarchy

| Role | Face | Notes |
|---|---|---|
| Display | `ui-serif` → New York, Georgia | Headline figures and page titles only |
| Interface | `-apple-system` → SF Pro, Segoe UI | 12–13px, the working size |
| Figures | `ui-monospace` → SF Mono | Tables, recovery phrase, file paths |
| Labels | SF Pro, 10px, 0.09em, uppercase | `.eyebrow` — load-bearing, used sparingly |

No webfonts. Loading one would mean either a network request — which this app
refuses to make — or shipping a licensed binary. The system serif is excellent
on both platforms and costs nothing.

## Themes

Light ("paper") is the default; dark ("ink") is a toggle in the sidebar footer,
persisted in `localStorage`. Both come from one set of semantic tokens in
`src/index.css`; nothing hardcodes a colour. Adding a surface means adding a
token to both blocks, never a hex value in a component.

Light is the default deliberately. A dark dashboard with a saturated accent is
the default look of generated software, and this app should not look generated.

## Charts

Axis lines and tick marks are off. Horizontal gridlines only, at hairline
weight. Areas are 1.25–1.5px strokes over a gradient fading to near-zero
opacity — enough to read the shape, not enough to shout. Tooltips are a real
panel with the same border and shadow as everything else, never the library
default.

## Working on the UI

```bash
pnpm dev
```

Opens the frontend on `localhost:1420` in a plain browser with **no Rust
behind it**. `src/lib/ipc.ts` detects the absence of Tauri and serves fixtures
from `src/lib/mock.ts`, so you can iterate on layout without a Rust rebuild
between changes.

Those fixtures are development-only twice over: guarded by
`import.meta.env.DEV`, and aliased to a throwing stub (`mock.stub.ts`) in
release builds. A finance app has no business carrying invented figures in its
binary. The numbers they produce are deliberately *not* the real model — the
engine in `src-tauri/src/sim/` is the only source of truth for anything a user
sees.

## Accessibility

Focus rings are a 2px brass outline with 2px offset, never removed. Every
slider carries `aria-valuetext` with the formatted value, so a screen reader
announces "2.80%" rather than "280". `prefers-reduced-motion` collapses every
transition. Body text is 13px minimum; 10px is reserved for eyebrow labels that
always sit beside a larger value.

The one known gap: colour currently carries some meaning on its own — a
breached DTI is red text. Each of those also states its number, but a
non-colour indicator would be better.
