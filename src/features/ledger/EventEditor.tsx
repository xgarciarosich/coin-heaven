import { useEffect, useState } from "react";
import { Tag, X } from "lucide-react";
import { Banner, Button, Checkbox, SelectField, TextField } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { majorToMinor, minorToMajor } from "@/lib/money";
import type { Category, EventInput, EventItem, EventUsage } from "@/lib/types";
import { useT } from "@/lib/i18n";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

/**
 * An amount as people type it: "1500", "1.500,50", "1,500.50", "€ 1 500".
 * `null` for nothing at all, `undefined` for something that is not a number.
 */
export function parseAmount(text: string): number | null | undefined {
  const s = text.replace(/[^\d.,-]/g, "");
  if (s === "" || s === "-") return text.trim() === "" ? null : undefined;
  const lastDot = s.lastIndexOf(".");
  const lastComma = s.lastIndexOf(",");
  let normalised = s;
  if (lastDot !== -1 && lastComma !== -1) {
    // Whichever comes last is the decimal mark; the other groups thousands.
    const decimal = lastDot > lastComma ? "." : ",";
    const group = decimal === "." ? "," : ".";
    normalised = s.split(group).join("").replace(decimal, ".");
  } else if (lastComma !== -1) {
    // One comma: a decimal mark, unless it is clearly grouping ("1,500").
    const tail = s.length - lastComma - 1;
    normalised = tail === 3 && s.indexOf(",") === lastComma && s.length > 4 ? s.replace(",", "") : s.replace(",", ".");
  }
  const n = Number(normalised);
  return Number.isFinite(n) ? n : undefined;
}

/** Create an event, or change one: the same fields either way. */
export function EventForm({
  initial,
  categories,
  onSaved,
  onCancel,
}: {
  /** The event being changed; omitted for a new one. */
  initial?: EventItem;
  categories: Category[];
  onSaved: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const l = t.ledger;
  const [name, setName] = useState(initial?.name ?? "");
  const [categoryId, setCategoryId] = useState<number | null>(initial?.defaultCategoryId ?? null);
  const [budget, setBudget] = useState(
    initial?.budgetMinor != null ? String(minorToMajor(initial.budgetMinor)) : "",
  );
  const [startsOn, setStartsOn] = useState(initial?.startsOn ?? "");
  const [endsOn, setEndsOn] = useState(initial?.endsOn ?? "");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (categoryId === null && categories.length > 0) {
      setCategoryId(categories.find((c) => c.kind === "expense")?.id ?? categories[0]!.id);
    }
  }, [categories, categoryId]);

  const save = () => {
    if (!name.trim()) return setError(l.eventNeedsName);
    if (categoryId === null) return;
    const amount = parseAmount(budget);
    if (amount === undefined) return setError(l.notAnAmount);
    if (startsOn && endsOn && endsOn < startsOn) return setError(l.eventEndsBeforeStart);
    setError(null);
    setSaving(true);
    const input: EventInput = {
      name: name.trim(),
      defaultCategoryId: categoryId,
      startsOn: startsOn || null,
      endsOn: endsOn || null,
      budgetMinor: amount === null ? null : majorToMinor(amount),
      notes: null,
    };
    api
      .eventSave(initial?.id ?? null, input)
      .then(onSaved)
      .catch((e: unknown) => setError(msg(e)))
      .finally(() => setSaving(false));
  };

  return (
    <div className="space-y-3 border-b border-rule bg-brass-wash/30 px-5 py-4">
      {error && <Banner tone="bad">{error}</Banner>}
      <div className="flex flex-wrap items-end gap-3">
        <TextField
          label={l.name}
          value={name}
          autoFocus
          className="w-52"
          placeholder={l.eventPlaceholder}
          onChange={(e) => setName(e.currentTarget.value)}
          onKeyDown={(e) => e.key === "Enter" && save()}
        />
        <SelectField
          label={l.rollsUpTo}
          value={categoryId ?? ""}
          className="w-44"
          onChange={(e) => setCategoryId(Number(e.currentTarget.value))}
        >
          {categories.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </SelectField>
        <TextField
          label={l.budget}
          value={budget}
          inputMode="decimal"
          className="w-28"
          placeholder={l.optional}
          onChange={(e) => setBudget(e.currentTarget.value)}
        />
        <TextField
          label={l.starts}
          type="date"
          value={startsOn}
          className="w-40"
          onChange={(e) => setStartsOn(e.currentTarget.value)}
        />
        <TextField
          label={l.ends}
          type="date"
          value={endsOn}
          min={startsOn || undefined}
          className="w-40"
          onChange={(e) => setEndsOn(e.currentTarget.value)}
        />
        <div className="flex gap-2">
          <Button variant="primary" onClick={save} disabled={saving}>
            <Tag className="size-3.5" strokeWidth={2} />
            {initial ? t.common.save : t.common.create}
          </Button>
          <Button variant="ghost" onClick={onCancel}>
            <X className="size-3.5" strokeWidth={2} />
            {t.common.cancel}
          </Button>
        </div>
      </div>
      <p className="text-[11px] leading-[1.6] text-ink-3">{l.eventRollsUpNote}</p>
    </div>
  );
}

/** What deleting an event does, said first, with the one choice there is. */
export function DeleteEvent({
  event,
  onCancel,
  onDeleted,
  onError,
}: {
  event: EventItem;
  onCancel: () => void;
  onDeleted: () => void;
  onError: (message: string) => void;
}) {
  const t = useT();
  const l = t.ledger;
  const [usage, setUsage] = useState<EventUsage | null>(null);
  const [keep, setKeep] = useState(true);

  useEffect(() => {
    api
      .eventUsage(event.id)
      .then(setUsage)
      .catch((e: unknown) => onError(msg(e)));
  }, [event.id, onError]);

  if (!usage) return null;
  return (
    <div
      role="alertdialog"
      aria-label={l.deleteEventTitle(usage.name)}
      className="space-y-3 border-b border-rule bg-negative/[0.05] px-5 py-4"
    >
      <p className="text-[13px] font-medium text-ink">{l.deleteEventTitle(usage.name)}</p>
      <div className="space-y-1.5 text-[12.5px] leading-[1.65] text-ink-2">
        <p>{usage.txnCount > 0 ? l.deleteEventStay(usage.txnCount) : l.deleteEventEmpty}</p>
        {usage.ruleCount > 0 && <p>{l.deleteEventRules(usage.ruleCount)}</p>}
      </div>
      {usage.txnOnDefault > 0 && (
        <Checkbox
          label={l.deleteEventKeep(usage.txnOnDefault, usage.defaultCategoryName)}
          checked={keep}
          onChange={(e) => setKeep(e.currentTarget.checked)}
        />
      )}
      <div className="flex gap-2">
        <Button variant="ghost" onClick={onCancel}>
          {t.common.cancel}
        </Button>
        <Button
          variant="danger"
          onClick={() =>
            api
              .eventDelete(event.id, usage.txnOnDefault > 0 ? keep : true)
              .then(onDeleted)
              .catch((e: unknown) => onError(msg(e)))
          }
        >
          {l.deleteEventGo}
        </Button>
      </div>
    </div>
  );
}
