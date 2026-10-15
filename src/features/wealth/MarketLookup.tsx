import { useCallback, useEffect, useState } from "react";
import { Check, AlertTriangle } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { Banner, Button } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { fmtDate } from "@/lib/region";
import type { MarketAsk, MarketResult } from "@/lib/types";
import { useT } from "@/lib/i18n";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Prices for the buys that have none.
 *
 * A recurring buy of "€50 of SAP" does not say how many shares €50 bought;
 * that takes the closing price of the day. This finds the holdings in that
 * state, says what it would ask and of whom, and — on a click — fills in a
 * price for every such day at once, so no one has to work them out by hand.
 */
export function MarketLookup({ onChanged }: { onChanged: () => void }) {
  const t = useT();
  const m = t.portfolio.market;
  const [asks, setAsks] = useState<MarketAsk[]>([]);
  const [results, setResults] = useState<MarketResult[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .marketPreview()
      .then(setAsks)
      .catch(() => setAsks([]));
  }, []);
  useEffect(load, [load]);

  const run = () => {
    setBusy(true);
    setError(null);
    api
      .marketFetch(asks.map((a) => a.holdingId))
      .then((r) => {
        setResults(r);
        load();
        onChanged();
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setBusy(false));
  };

  const undo = (id: number) =>
    api
      .marketClear(id)
      .then(() => {
        setResults((r) => r?.filter((x) => x.holdingId !== id) ?? null);
        load();
        onChanged();
      })
      .catch((e: unknown) => setError(msg(e)));

  const total = results?.reduce((a, r) => a + r.stored, 0) ?? 0;

  if (asks.length === 0 && !results) return null;

  return (
    <Panel className="border-brass/50">
      <PanelHead label={asks.length > 0 ? m.title : t.portfolio.checkPrices} />
      <PanelBody className="space-y-3">
        {error && <Banner tone="bad">{error}</Banner>}

        {asks.length > 0 && (
          <>
            <p className="text-[12.5px] leading-[1.65] text-ink-2">
              {m.body(
                asks.reduce((a, x) => a + x.buysMissing, 0),
                asks.length,
              )}
            </p>
            <ul className="space-y-1 text-[12.5px] text-ink">
              {asks.map((a) => (
                <li key={a.holdingId} className="flex flex-wrap items-baseline gap-x-2">
                  <span>{m.item(a.name, a.buysMissing, fmtDate(a.from))}</span>
                  <span className="text-[11.5px] text-ink-3">
                    {a.symbol ? a.symbol : m.noSymbol}
                  </span>
                </li>
              ))}
            </ul>
            <p className="text-[11.5px] leading-[1.6] text-ink-3">{m.asks(asks[0]!.source)}</p>
            <Button variant="primary" onClick={run} disabled={busy}>
              {busy ? m.busy : m.go}
            </Button>
          </>
        )}

        {results && (
          <div className="space-y-2.5 border-t border-rule pt-3">
            {total > 0 && (
              <p className="flex items-center gap-1.5 text-[12px] text-positive">
                <Check className="size-3.5" strokeWidth={2} />
                {m.done(total)}
              </p>
            )}
            {results.map((r) => (
              <div key={r.holdingId} className="text-[12px]">
                {r.problem === null || r.stored > 0 ? (
                  <div className="space-y-0.5">
                    <p className="text-ink">
                      <span className="font-medium">{r.name}</span> ·{" "}
                      {m.found(
                        r.symbolUsed ?? "",
                        r.stored,
                        r.from ? fmtDate(r.from) : "",
                        r.to ? fmtDate(r.to) : "",
                      )}
                    </p>
                    {r.leftOut > 0 && <p className="text-ink-3">{m.leftOut(r.leftOut)}</p>}
                    <button
                      type="button"
                      onClick={() => void undo(r.holdingId)}
                      className="text-[11px] text-ink-3 underline-offset-2 hover:text-ink hover:underline"
                    >
                      {m.undo}
                    </button>
                  </div>
                ) : (
                  <p className="flex items-start gap-1.5 text-ink-2">
                    <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-negative" strokeWidth={2} />
                    <span>
                      <span className="font-medium text-ink">{r.name}</span> —{" "}
                      {m.problems[r.problem ?? "unreachable"]}
                    </span>
                  </p>
                )}
              </div>
            ))}
          </div>
        )}
      </PanelBody>
    </Panel>
  );
}
