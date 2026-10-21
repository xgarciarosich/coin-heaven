import { useCallback, useEffect, useRef, useState } from "react";
import { Plus, Trash2, X } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import { Banner, Button, SelectField, TextField } from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import type { Category, CategoryUsage, Rule } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { PALETTE, nextColour } from "@/lib/colours";
import { move } from "@/lib/useReorder";
import { Grip, Reorderable } from "@/components/ui/reorderable";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));

const SWATCHES = PALETTE;

/**
 * The categories transactions are filed under, and the rules that file them.
 *
 * These used to live in Settings, a screen away from the only place they are
 * used. They sit beside the transactions now, laid out the way the asset
 * classes are in Net worth: a colour, a name, what kind of thing it is and
 * what that kind does to the totals.
 */
export function CategoriesTab({
  categories,
  onChanged,
}: {
  categories: Category[];
  onChanged: () => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  return (
    <div className="space-y-5">
      {error && <Banner tone="bad">{error}</Banner>}
      {notice && <Banner tone="good">{notice}</Banner>}
      <CategoryManager
        categories={categories}
        onChanged={onChanged}
        onError={setError}
        onNotice={setNotice}
      />
      <RuleManager categories={categories} onError={setError} onNotice={setNotice} />
    </div>
  );
}

/* ---------------------------------------------------------- categories -- */

type Draft = { name: string; kind: string; color: string };
const blank = (used: (string | null)[]): Draft => ({
  name: "",
  kind: "expense",
  // A colour nobody is using yet, so a new category is told apart at a glance.
  color: nextColour(used),
});

function CategoryManager({
  categories,
  onChanged,
  onError,
  onNotice,
}: {
  categories: Category[];
  onChanged: () => void;
  onError: (m: string) => void;
  onNotice: (m: string | null) => void;
}) {
  const t = useT();
  const [editing, setEditing] = useState<number | null>(null);
  const [adding, setAdding] = useState(false);
  const [deleting, setDeleting] = useState<{ cat: Category; usage: CategoryUsage } | null>(null);
  // Categories in the order shown, moved live while one is dragged.
  const [order, setOrder] = useState<number[]>(() => categories.map((c) => c.id));
  useEffect(() => setOrder(categories.map((c) => c.id)), [categories]);
  const orderNow = useRef(order);
  orderNow.current = order;
  const shown = order
    .map((id) => categories.find((c) => c.id === id))
    .filter((c): c is Category => c !== undefined);

  const commit = (id: number | null, d: Draft) =>
    api
      .categorySave(id, d.name, d.kind, d.color)
      .then(() => {
        setEditing(null);
        setAdding(false);
        onChanged();
      })
      .catch((e: unknown) => onError(msg(e)));

  const askDelete = (c: Category) =>
    api
      .categoryUsage(c.id)
      .then((usage) => setDeleting({ cat: c, usage }))
      .catch((e: unknown) => onError(msg(e)));

  return (
    <>
      <Panel>
        <PanelHead
          label={t.settings.categories(categories.length)}
          aside={
            <Button
              variant="primary"
              onClick={() => {
                setAdding((v) => !v);
                setEditing(null);
              }}
            >
              {adding ? (
                <X className="size-3.5" strokeWidth={2} />
              ) : (
                <Plus className="size-3.5" strokeWidth={2} />
              )}
              {adding ? t.common.cancel : t.settings.addCategory}
            </Button>
          }
        />

        {adding && (
          <CategoryForm
            initial={blank(categories.map((c) => c.color))}
            isNew
            onSave={(d) => commit(null, d)}
          />
        )}

        <Reorderable
          keys={order}
          className="divide-y divide-rule"
          onMove={(from, to) => setOrder((o) => move(o, from, to))}
          onDrop={() =>
            api
              .categoryReorder(orderNow.current)
              .then(onChanged)
              .catch((e: unknown) => onError(msg(e)))
          }
        >
          {(id, _i, item, grip, dragging) => {
            const c = shown.find((x) => x.id === id);
            if (!c) return null;
            return editing === c.id ? (
              <CategoryForm
                key={c.id}
                initial={{ name: c.name, kind: c.kind, color: c.color ?? SWATCHES[0]! }}
                onSave={(d) => commit(c.id, d)}
                onCancel={() => setEditing(null)}
              />
            ) : (
              <div
                key={c.id}
                {...item}
                className={cn(
                  "group flex items-center gap-4 px-5 py-2.5",
                  dragging && "bg-brass-wash/40",
                )}
              >
                <Grip handle={grip} label={t.settings.dragCategory} className="-ml-2" />
                <span
                  className="size-2.5 shrink-0 rounded-full"
                  style={{ background: c.color ?? "var(--ink-3)" }}
                />
                <span className="w-44 shrink-0 truncate text-[12.5px] text-ink">{c.name}</span>
                <span
                  className={cn(
                    "w-24 shrink-0 rounded px-1.5 py-0.5 text-center text-[10px]",
                    c.kind === "income" && "bg-positive/10 text-positive",
                    c.kind === "expense" && "bg-brass-wash text-brass",
                    c.kind === "neutral" && "bg-surface-hi text-ink-3",
                  )}
                >
                  {t.common.categoryKind[c.kind] ?? c.kind}
                </span>
                <span className="min-w-0 flex-1 truncate text-[11px] text-ink-3">
                  {t.settings.kindNotes[c.kind]}
                </span>
                <span className="tnum shrink-0 text-[11px] text-ink-3">
                  {t.common.transactions(c.txnCount)}
                </span>
                <Button
                  variant="ghost"
                  onClick={() => {
                    setEditing(c.id);
                    setAdding(false);
                  }}
                >
                  {t.common.edit}
                </Button>
                <Button
                  variant="ghost"
                  onClick={() => askDelete(c)}
                  aria-label={t.settings.deleteTitle(c.name)}
                >
                  <Trash2 className="size-3" strokeWidth={1.75} />
                </Button>
              </div>
            );
          }}
        </Reorderable>

        <p className="border-t border-rule px-5 py-3 text-[11px] leading-[1.6] text-ink-3">
          {t.settings.categoriesNote}
        </p>
      </Panel>

      {deleting && (
        <DeletePrompt
          cat={deleting.cat}
          usage={deleting.usage}
          options={categories.filter((c) => c.id !== deleting.cat.id)}
          onCancel={() => setDeleting(null)}
          onDone={(moved) => {
            setDeleting(null);
            onNotice(moved);
            onChanged();
          }}
          onError={onError}
        />
      )}
    </>
  );
}

function CategoryForm({
  initial,
  isNew,
  onSave,
  onCancel,
}: {
  initial: Draft;
  isNew?: boolean;
  onSave: (d: Draft) => void;
  onCancel?: () => void;
}) {
  const t = useT();
  const [draft, setDraft] = useState<Draft>(initial);
  return (
    <div className="space-y-3 border-b border-rule bg-brass-wash/30 px-5 py-4">
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-4">
        <TextField
          label={t.settings.name}
          value={draft.name}
          autoFocus
          className="sm:col-span-2"
          onChange={(e) => setDraft({ ...draft, name: e.currentTarget.value })}
        />
        <SelectField
          label={t.settings.kind}
          value={draft.kind}
          onChange={(e) => setDraft({ ...draft, kind: e.currentTarget.value })}
        >
          {(["expense", "income", "neutral"] as const).map((k) => (
            <option key={k} value={k}>
              {t.common.categoryKind[k]}
            </option>
          ))}
        </SelectField>
        <div className="space-y-1.5">
          <span className="eyebrow block">{t.settings.colour}</span>
          <div className="flex flex-wrap gap-1 pt-1">
            {SWATCHES.map((s) => (
              <button
                key={s}
                type="button"
                aria-label={`colour ${s}`}
                onClick={() => setDraft({ ...draft, color: s })}
                className={cn(
                  "size-5 rounded-full border-2 transition-transform",
                  draft.color === s ? "scale-110 border-ink" : "border-transparent",
                )}
                style={{ background: s }}
              />
            ))}
          </div>
        </div>
      </div>

      <div className="flex items-center justify-between gap-4">
        <p className="text-[11px] leading-[1.6] text-ink-3">{t.settings.kindNotes[draft.kind]}</p>
        <div className="flex shrink-0 gap-2">
          {onCancel && (
            <Button variant="ghost" onClick={onCancel}>
              {t.common.cancel}
            </Button>
          )}
          <Button variant="primary" disabled={!draft.name.trim()} onClick={() => onSave(draft)}>
            {isNew ? t.common.create : t.common.save}
          </Button>
        </div>
      </div>
    </div>
  );
}

function DeletePrompt({
  cat,
  usage,
  options,
  onCancel,
  onDone,
  onError,
}: {
  cat: Category;
  usage: CategoryUsage;
  options: Category[];
  onCancel: () => void;
  onDone: (notice: string | null) => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const total = usage.transactions + usage.events + usage.rules;
  const [target, setTarget] = useState<number | null>(options[0]?.id ?? null);

  return (
    <Panel className="border-negative/40">
      <PanelHead label={t.settings.deleteTitle(cat.name)} />
      <PanelBody className="space-y-3">
        {total === 0 ? (
          <p className="text-[12.5px] text-ink-2">{t.settings.nothingUses}</p>
        ) : (
          <>
            <p className="text-[12.5px] leading-[1.6] text-ink-2">
              {t.settings.usage(usage.transactions, usage.events, usage.rules)}
            </p>
            <SelectField
              label={t.settings.moveThemTo}
              value={target ?? ""}
              className="w-56"
              onChange={(e) => setTarget(Number(e.currentTarget.value))}
            >
              {options.map((o) => (
                <option key={o.id} value={o.id}>
                  {o.name}
                </option>
              ))}
            </SelectField>
          </>
        )}
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={onCancel}>
            {t.settings.keepIt}
          </Button>
          <Button
            variant="danger"
            disabled={total > 0 && target === null}
            onClick={() =>
              api
                .categoryDelete(cat.id, total > 0 ? target : null)
                .then(() =>
                  onDone(
                    total > 0
                      ? t.settings.moved(
                          total,
                          options.find((o) => o.id === target)?.name ??
                            t.settings.anotherCategory,
                        )
                      : null,
                  ),
                )
                .catch((e: unknown) => onError(msg(e)))
            }
          >
            {t.settings.deleteCategory}
          </Button>
        </div>
      </PanelBody>
    </Panel>
  );
}

/* --------------------------------------------------------------- rules -- */

const BLANK_RULE: Rule = {
  id: null,
  name: "",
  priority: 100,
  matchKind: "contains",
  pattern: "",
  accountId: null,
  minAmountMinor: null,
  maxAmountMinor: null,
  setCategoryId: null,
  setProjectId: null,
  setNeutralTransfer: false,
  isEnabled: true,
};

function RuleManager({
  categories,
  onError,
  onNotice,
}: {
  categories: Category[];
  onError: (m: string) => void;
  onNotice: (m: string | null) => void;
}) {
  const t = useT();
  const [rules, setRules] = useState<Rule[]>([]);
  const [draft, setDraft] = useState<Rule | null>(null);

  const load = useCallback(() => {
    api.ruleList().then(setRules).catch((e: unknown) => onError(msg(e)));
  }, [onError]);
  useEffect(load, [load]);

  const save = (r: Rule) =>
    api
      .ruleSave(r)
      .then(() => {
        setDraft(null);
        load();
      })
      .catch((e: unknown) => onError(msg(e)));

  return (
    <Panel>
      <PanelHead
        label={t.settings.rules(rules.length)}
        aside={
          <div className="flex gap-2">
            <Button
              onClick={() =>
                api
                  .rulesApply()
                  .then((n) =>
                    onNotice(
                      n === 0 ? t.settings.nothingForRules : t.settings.categorised(n),
                    ),
                  )
                  .catch((e: unknown) => onError(msg(e)))
              }
            >
              {t.settings.runOnExisting}
            </Button>
            {!draft && (
              <Button onClick={() => setDraft({ ...BLANK_RULE })}>
                <Plus className="size-3" strokeWidth={2} />
                {t.settings.addRule}
              </Button>
            )}
          </div>
        }
      />

      {draft && (
        <div className="border-b border-rule bg-brass-wash/30 px-5 py-3">
          <RuleEditor
            draft={draft}
            categories={categories}
            onChange={setDraft}
            onSave={() => save(draft)}
            onCancel={() => setDraft(null)}
          />
        </div>
      )}

      {rules.length === 0 && !draft ? (
        <PanelBody>
          <p className="text-[12px] leading-[1.6] text-ink-3">{t.settings.rulesEmpty}</p>
        </PanelBody>
      ) : (
        <div className="divide-y divide-rule">
          {rules.map((r) => (
            <div key={r.id} className="flex items-center gap-3 px-5 py-2.5 text-[12.5px]">
              <span className="tnum w-10 text-[11px] text-ink-3">{r.priority}</span>
              <span className="w-40 truncate text-ink">{r.name}</span>
              <span className="flex-1 truncate font-mono text-[11.5px] text-ink-3">
                {t.settings.matchKinds[r.matchKind] ?? r.matchKind} &ldquo;{r.pattern}&rdquo;
              </span>
              <span className="text-[11px] text-ink-2">
                {r.setNeutralTransfer
                  ? t.settings.toTransfer
                  : `→ ${categories.find((c) => c.id === r.setCategoryId)?.name ?? "—"}`}
              </span>
              <Button variant="ghost" onClick={() => setDraft(r)}>
                {t.common.edit}
              </Button>
              <Button
                variant="ghost"
                onClick={() =>
                  r.id !== null &&
                  api.ruleDelete(r.id).then(load).catch((e: unknown) => onError(msg(e)))
                }
              >
                <Trash2 className="size-3" strokeWidth={1.75} />
              </Button>
            </div>
          ))}
        </div>
      )}
    </Panel>
  );
}

function RuleEditor({
  draft,
  categories,
  onChange,
  onSave,
  onCancel,
}: {
  draft: Rule;
  categories: Category[];
  onChange: (r: Rule) => void;
  onSave: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const set = <K extends keyof Rule>(k: K, v: Rule[K]) => onChange({ ...draft, [k]: v });
  return (
    <div className="space-y-3">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-5">
        <TextField
          label={t.settings.name}
          value={draft.name}
          autoFocus
          onChange={(e) => set("name", e.currentTarget.value)}
        />
        <SelectField
          label={t.settings.match}
          value={draft.matchKind}
          onChange={(e) => set("matchKind", e.currentTarget.value as Rule["matchKind"])}
        >
          {(["contains", "starts_with", "exact", "regex"] as const).map((k) => (
            <option key={k} value={k}>
              {t.settings.matchKinds[k]}
            </option>
          ))}
        </SelectField>
        <TextField
          label={t.settings.pattern}
          value={draft.pattern}
          placeholder="mercadona"
          onChange={(e) => set("pattern", e.currentTarget.value)}
        />
        <SelectField
          label={t.settings.thenSet}
          value={draft.setNeutralTransfer ? "neutral" : (draft.setCategoryId ?? "")}
          onChange={(e) => {
            const v = e.currentTarget.value;
            if (v === "neutral") onChange({ ...draft, setNeutralTransfer: true, setCategoryId: null });
            else
              onChange({
                ...draft,
                setNeutralTransfer: false,
                setCategoryId: v ? Number(v) : null,
              });
          }}
        >
          <option value="">—</option>
          {categories.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
          <option value="neutral">{t.settings.internalTransfer}</option>
        </SelectField>
        <TextField
          label={t.settings.priority}
          type="number"
          value={draft.priority}
          onChange={(e) => set("priority", Number(e.currentTarget.value))}
        />
      </div>
      <div className="flex items-center justify-between">
        <p className="text-[11px] text-ink-3">{t.settings.ruleHint}</p>
        <div className="flex gap-2">
          <Button variant="ghost" onClick={onCancel}>
            {t.common.cancel}
          </Button>
          <Button variant="primary" onClick={onSave} disabled={!draft.name.trim() || !draft.pattern.trim()}>
            {t.settings.saveRule}
          </Button>
        </div>
      </div>
    </div>
  );
}
