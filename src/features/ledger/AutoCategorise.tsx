import { useCallback, useEffect, useMemo, useState } from "react";
import { Bot, Check, ChevronDown, Sparkles, X } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { Banner, Button, Empty, SelectField } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { fmtBps, fmtMoney } from "@/lib/money";
import type {
  Category,
  EgressPreview,
  EventItem,
  MerchantSuggestion,
  SuggestedGroup,
  Suggestions,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * Bulk categorisation by merchant.
 *
 * Every uncategorised transaction is grouped by the word most likely to name
 * the shop, and each group gets one decision instead of one per row. Nothing
 * is written until Apply: the samples are there so a grouping can be checked
 * rather than trusted, which matters most for the groups the app is least
 * sure about.
 *
 * Suggestions come from three places, most trusted first: a rule the person
 * wrote, how they filed the same merchant before, and — only if they ask —
 * a model, for the merchants neither of those knows. The model is given the
 * category list, examples of the person's own filing and one line per
 * merchant; what it proposes lands in this same list, marked, to be accepted
 * or changed like any other.
 */
export function AutoCategorise({
  categories,
  events,
  onApplied,
  onHide,
}: {
  categories: Category[];
  /** Open events a bundle can be put in. */
  events: EventItem[];
  onApplied: () => void;
  onHide: () => void;
}) {
  const t = useT();
  const [data, setData] = useState<Suggestions | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [choice, setChoice] = useState<Record<string, number | null>>({});
  const [eventChoice, setEventChoice] = useState<Record<string, number | null>>({});
  const [rules, setRules] = useState<Record<string, boolean>>({});
  const [open, setOpen] = useState<string | null>(null);
  const [preview, setPreview] = useState<EgressPreview | null>(null);
  const [byAi, setByAi] = useState<Record<string, MerchantSuggestion>>({});
  const [asking, setAsking] = useState(false);

  const load = useCallback(() => {
    setBusy(true);
    setNote(null);
    api
      .autocatSuggest()
      .then((s) => {
        setData(s);
        // Pre-select what the app believes; the user vetoes rather than fills in.
        setChoice(
          Object.fromEntries(s.groups.map((g) => [g.key, g.suggestedCategoryId])),
        );
        setRules(
          Object.fromEntries(
            s.groups.map((g) => [g.key, g.source !== "rule" && g.txnCount > 1]),
          ),
        );
        setEventChoice({});
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setBusy(false));
  }, []);

  useEffect(load, [load]);

  const ready = useMemo(
    () =>
      (data?.groups ?? []).filter((g) => choice[g.key] != null || eventChoice[g.key] != null),
    [data, choice, eventChoice],
  );
  const readyTxns = useMemo(
    () => ready.reduce((n, g) => n + g.txnCount, 0),
    [ready],
  );

  const unknown = (data?.groups ?? []).filter(
    (g) => g.suggestedCategoryId == null && !byAi[g.key],
  ).length;

  const askAi = (pv: EgressPreview) => {
    setAsking(true);
    setError(null);
    api
      .aiAutocatSend(pv)
      .then((found) => {
        setPreview(null);
        setByAi((prev) => ({ ...prev, ...Object.fromEntries(found.map((f) => [f.key, f])) }));
        // Proposed, not applied: each lands in its row's selector, and only
        // where the person has not already chosen something themselves.
        setChoice((c) => {
          const next = { ...c };
          for (const f of found) if (next[f.key] == null) next[f.key] = f.categoryId;
          return next;
        });
        setRules((r) => ({ ...r, ...Object.fromEntries(found.map((f) => [f.key, true])) }));
        setNote(t.ledger.autocat.aiFound(found.length));
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setAsking(false));
  };

  const apply = () => {
    setBusy(true);
    api
      .autocatApply(
        ready.map((g) => ({
          txnIds: g.txnIds,
          categoryId: choice[g.key] ?? null,
          eventId: eventChoice[g.key] ?? null,
          // A rule needs a category to point at.
          createRule: choice[g.key] != null && (rules[g.key] ?? false),
          key: g.key,
        })),
      )
      .then((o) => {
        setNote(t.ledger.autocat.filed(o.categorised, o.rulesCreated));
        onApplied();
        load();
      })
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setBusy(false));
  };

  return (
    <Panel>
      <PanelHead
        label={t.ledger.autocat.title}
        aside={
          <div className="flex items-center gap-2">
            {data && (
              <span className="text-[11px] text-ink-3">
                {t.ledger.autocat.coverage(data.uncategorisedTotal, data.covered)}
              </span>
            )}
            <Button variant="ghost" onClick={load} disabled={busy}>
              {t.ledger.autocat.rescan}
            </Button>
            {unknown > 0 && (
              <Button
                disabled={busy || asking || preview !== null}
                title={t.ledger.autocat.aiHint}
                onClick={() => {
                  setError(null);
                  api
                    .aiAutocatPreview()
                    .then(setPreview)
                    .catch((e: unknown) => setError(msg(e)));
                }}
              >
                <Bot className="size-3.5" strokeWidth={1.75} />
                {t.ledger.autocat.askAi(unknown)}
              </Button>
            )}
            <Button variant="primary" onClick={apply} disabled={busy || ready.length === 0}>
              <Check className="size-3.5" strokeWidth={2} />
              {t.ledger.autocat.apply(readyTxns)}
            </Button>
            <Button variant="ghost" onClick={onHide} title={t.ledger.autocat.hide}>
              <X className="size-3.5" strokeWidth={1.75} />
              {t.ledger.autocat.hide}
            </Button>
          </div>
        }
      />

      {error && (
        <div className="px-5 pt-4">
          <Banner tone="bad">{error}</Banner>
        </div>
      )}
      {note && (
        <div className="px-5 pt-4">
          <Banner tone="good">{note}</Banner>
        </div>
      )}

      {preview && (
        <div className="space-y-2.5 border-b border-rule bg-brass-wash/30 px-5 py-4">
          <p className="text-[12px] leading-[1.6] text-ink-2">
            {t.ledger.autocat.aiExplain(preview.provider, preview.rowCount)}
          </p>
          <pre
            data-selectable
            className="max-h-56 overflow-auto rounded-md border border-rule bg-paper-sunk p-3 font-mono text-[11px] leading-[1.5] text-ink-2"
          >
            {preview.payload}
          </pre>
          <div className="flex justify-end gap-2">
            <Button
              variant="ghost"
              onClick={() => {
                api.aiDecline(preview).catch(() => undefined);
                setPreview(null);
              }}
            >
              {t.assistant.dontSend}
            </Button>
            <Button variant="primary" disabled={asking} onClick={() => askAi(preview)}>
              {asking ? t.assistant.sending : t.assistant.send}
            </Button>
          </div>
        </div>
      )}

      {!data || data.groups.length === 0 ? (
        <PanelBody>
          <Empty
            title={busy ? t.ledger.autocat.reading : t.ledger.autocat.allFiled}
            body={t.ledger.autocat.allFiledBody}
          />
        </PanelBody>
      ) : (
        <div className="max-h-[520px] divide-y divide-rule overflow-auto">
          {data.groups.map((g) => (
            <Group
              key={g.key}
              group={g}
              categories={categories}
              events={events}
              ai={byAi[g.key] ?? null}
              value={choice[g.key] ?? null}
              eventValue={eventChoice[g.key] ?? null}
              onEventChange={(v) => setEventChoice((c) => ({ ...c, [g.key]: v }))}
              createRule={rules[g.key] ?? false}
              expanded={open === g.key}
              onToggleExpand={() => setOpen(open === g.key ? null : g.key)}
              onChange={(v) => setChoice((c) => ({ ...c, [g.key]: v }))}
              onRuleChange={(v) => setRules((r) => ({ ...r, [g.key]: v }))}
            />
          ))}
        </div>
      )}
    </Panel>
  );
}

function Group({
  group: g,
  categories,
  events,
  ai,
  value,
  eventValue,
  onEventChange,
  createRule,
  expanded,
  onToggleExpand,
  onChange,
  onRuleChange,
}: {
  group: SuggestedGroup;
  categories: Category[];
  events: EventItem[];
  ai: MerchantSuggestion | null;
  value: number | null;
  eventValue: number | null;
  onEventChange: (v: number | null) => void;
  createRule: boolean;
  expanded: boolean;
  onToggleExpand: () => void;
  onChange: (v: number | null) => void;
  onRuleChange: (v: boolean) => void;
}) {
  const t = useT();
  return (
    <div className={cn("px-5 py-3", value == null && eventValue == null && "opacity-70")}>
      <div className="flex flex-wrap items-center gap-3">
        <button
          type="button"
          onClick={onToggleExpand}
          className="flex min-w-0 flex-1 items-center gap-2 text-left"
        >
          <ChevronDown
            className={cn(
              "size-3.5 shrink-0 text-ink-3 transition-transform",
              !expanded && "-rotate-90",
            )}
            strokeWidth={1.75}
          />
          <span className="truncate text-[13px] text-ink">{g.key}</span>
          {ai ? (
            <span className="flex shrink-0 items-center gap-1 rounded bg-surface-hi px-1.5 py-0.5 text-[10px] text-ink-2">
              <Bot className="size-2.5" strokeWidth={2} />
              {t.ledger.autocat.byAi(ai.categoryName, fmtBps(ai.confidenceBps, 0))}
            </span>
          ) : (
            <Evidence group={g} />
          )}
        </button>

        <span className="tnum shrink-0 text-[11.5px] text-ink-3">
          {t.common.txn(g.txnCount)}
        </span>
        <span
          className={cn(
            "tnum shrink-0 text-[12.5px]",
            g.totalMinor < 0 ? "text-ink" : "text-positive",
          )}
        >
          {fmtMoney(g.totalMinor)}
        </span>

        <SelectField
          value={value ?? ""}
          className="w-44 shrink-0"
          onChange={(e) =>
            onChange(e.currentTarget.value ? Number(e.currentTarget.value) : null)
          }
        >
          <option value="">{t.ledger.autocat.skip}</option>
          {categories.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </SelectField>

        {events.length > 0 && (
          <SelectField
            value={eventValue ?? ""}
            className="w-40 shrink-0"
            aria-label={t.ledger.autocat.event}
            onChange={(e) =>
              onEventChange(e.currentTarget.value ? Number(e.currentTarget.value) : null)
            }
          >
            <option value="">{t.ledger.autocat.noEvent}</option>
            {events.map((ev) => (
              <option key={ev.id} value={ev.id}>
                {ev.name}
              </option>
            ))}
          </SelectField>
        )}

        <label
          className={cn(
            "flex shrink-0 cursor-pointer items-center gap-1.5",
            value == null && "pointer-events-none opacity-40",
          )}
          title={t.ledger.autocat.ruleTitle(g.key)}
        >
          <input
            type="checkbox"
            checked={createRule}
            onChange={(e) => onRuleChange(e.currentTarget.checked)}
            className="size-3.5 accent-[var(--brass)]"
          />
          <span className="text-[11px] text-ink-3">{t.ledger.autocat.remember}</span>
        </label>
      </div>

      {expanded && (
        <div className="mt-2.5 ml-5.5 space-y-1 border-l border-rule pl-3">
          {g.samples.map((s) => (
            <div key={s.id} className="flex items-baseline gap-3 text-[11.5px]">
              <span className="tnum shrink-0 text-ink-3">{s.bookedOn}</span>
              <span className="min-w-0 flex-1 truncate text-ink-2">{s.description}</span>
              <span className="tnum shrink-0 text-ink">{fmtMoney(s.amountMinor)}</span>
            </div>
          ))}
          {g.txnCount > g.samples.length && (
            <p className="text-[11px] text-ink-3">
              {t.ledger.autocat.more(g.txnCount - g.samples.length, g.firstOn, g.lastOn)}
            </p>
          )}
        </div>
      )}
    </div>
  );
}

/** Why the app thinks what it thinks — never just a percentage on its own. */
function Evidence({ group: g }: { group: SuggestedGroup }) {
  const t = useT();
  if (g.source === "rule") {
    return (
      <span className="flex shrink-0 items-center gap-1 rounded bg-brass-wash px-1.5 py-0.5 text-[10px] text-brass">
        <Sparkles className="size-2.5" strokeWidth={2} />
        {t.ledger.autocat.byRule(g.ruleName ?? "")}
      </span>
    );
  }
  if (g.source === "history") {
    return (
      <span className="shrink-0 text-[10.5px] text-ink-3">
        {t.ledger.autocat.byHistory(
          g.evidenceCount,
          g.confidenceBps < 10_000 ? fmtBps(g.confidenceBps, 0) : null,
        )}
      </span>
    );
  }
  return (
    <span className="flex shrink-0 items-center gap-1 text-[10.5px] text-ink-3">
      <X className="size-2.5" strokeWidth={2} />
      {t.ledger.autocat.nothing}
    </span>
  );
}
