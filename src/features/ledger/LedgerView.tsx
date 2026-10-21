import { useCallback, useEffect, useMemo, useState } from "react";
import { Plus, Settings2, Tag, Trash2, Wand2, X } from "lucide-react";
import { Panel, PanelBody, PanelHead } from "@/components/ui/panel";
import {
  Banner,
  Button,
  Empty,
  PageHead,
  Segmented,
  SelectField,
  TextField,
} from "@/components/ui/controls";
import { api } from "@/lib/ipc";
import { fmtCompact, fmtMoney, majorToMinor } from "@/lib/money";
import type {
  Account,
  Batch,
  CashflowSummary,
  Category,
  EventItem,
  ImportImpact,
  Ledger,
  LedgerRow,
  Matrix,
  ModuleToggles,
  PeriodTotals,
  PlSummary,
} from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import { ImportView } from "@/features/import/ImportView";
import { AutoCategorise } from "./AutoCategorise";
import { RemoveImport } from "@/features/import/RemoveImport";
import { BudgetsView } from "./BudgetsView";
import { CategoriesTab } from "./CategoriesTab";
import { MonthlyPlan } from "./MonthlyPlan";
import { RecurringView } from "./RecurringView";
import { CashFlowAnalytics } from "./CashFlowAnalytics";
import { TransactionTree, type Grouping } from "./TransactionTree";

const msg = (e: unknown) => (e instanceof Error ? e.message : String(e));
type SubTab = "overview" | "plan" | "transactions" | "import";
type OverviewView = "happened" | "budget" | "recurring";
const VIEW_KEY = "coinheaven.overview.view";

/**
 * Module 2 — everything that happens to a transaction.
 *
 * The page opens on the analytics, because the question someone arrives with
 * is "how is it going", not "what shall I load". Importing is a tab here
 * rather than its own destination in the sidebar: the act of loading a
 * statement sits beside the only screens that show what loading it did.
 */
export function LedgerView({ modules }: { modules: ModuleToggles | null }) {
  const t = useT();
  const showImport = modules?.import ?? true;
  const showLedger = modules?.ledger ?? true;
  const [sub, setSub] = useState<SubTab>(showLedger ? "overview" : "import");
  // Both views of the overview are an analysis of a year or a month; which
  // one someone wants is theirs to remember.
  const [view, setViewState] = useState<OverviewView>(() => {
    try {
      const v = localStorage.getItem(VIEW_KEY);
      return v === "budget" || v === "recurring" ? v : "happened";
    } catch {
      return "happened";
    }
  });
  const setView = (v: OverviewView) => {
    setViewState(v);
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      /* a view that is not remembered is still a view */
    }
  };
  const [managing, setManaging] = useState(false);

  const [categories, setCategories] = useState<Category[]>([]);
  const [events, setEvents] = useState<EventItem[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [years, setYears] = useState<number[]>([]);
  const [error, setError] = useState<string | null>(null);

  const [year, setYear] = useState<number>(new Date().getFullYear());
  const [accountId, setAccountId] = useState<number | null>(null);
  const [projectId, setProjectId] = useState<number | null>(null);
  const [categoryId, setCategoryId] = useState<number | null>(null);
  const [search, setSearch] = useState("");
  // Only what one import brought in — to check it, or to take it back out.
  const [batchId, setBatchId] = useState<number | null>(null);
  const [batches, setBatches] = useState<Batch[]>([]);
  const [removing, setRemoving] = useState<ImportImpact | null>(null);
  const [onlyReview, setOnlyReview] = useState(false);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [editing, setEditing] = useState<number | null>(null);
  const [grouping, setGrouping] = useState<Grouping>("month");
  const [tidying, setTidying] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const [ledger, setLedger] = useState<Ledger | null>(null);
  const [pl, setPl] = useState<PlSummary | null>(null);
  const [summary, setSummary] = useState<CashflowSummary | null>(null);
  const [monthly, setMonthly] = useState<PeriodTotals[]>([]);
  const [yearly, setYearly] = useState<PeriodTotals[]>([]);
  const [matrix, setMatrix] = useState<Matrix | null>(null);

  const range = useMemo(
    () => ({ from: `${year}-01-01`, to: `${year}-12-31` }),
    [year],
  );

  const loadStatic = useCallback(() => {
    Promise.all([
      api.categoryList(),
      api.eventList(),
      api.accountList(),
      api.txnYears(),
      api.importBatches(),
    ])
      .then(([c, e, a, ys, bs]) => {
        setCategories(c);
        setEvents(e);
        setAccounts(a);
        setBatches(bs);
        // The years with data, plus this one: a budget for a year that has
        // only just begun has to be writable before its first statement.
        const now = new Date().getFullYear();
        setYears(ys.includes(now) ? ys : [now, ...ys].sort((a, b) => b - a));
        // Land on a year that actually has data rather than an empty current year.
        if (ys.length > 0 && !ys.includes(year)) setYear(ys[0]!);
      })
      .catch((e: unknown) => setError(msg(e)));
  }, [year]);

  const loadData = useCallback(() => {
    const { from, to } = range;
    Promise.all([
      api.txnList({
        // One import is read whole, whatever years its lines fall in: a file
        // with a wrong column is exactly the one whose dates are off.
        from: batchId === null ? from : null,
        to: batchId === null ? to : null,
        importBatchId: batchId,
        accountId,
        projectId,
        categoryId,
        search: search || null,
        needsReview: onlyReview || null,
        // Grouping only helps if the group is complete, so the whole year is
        // fetched rather than the first screenful of it.
        limit: 5000,
      }),
      api.plSummary(from, to),
      api.cashflowSummary(from, to),
      api.cashflowMonthly(from, to),
      api.cashflowYearly(),
      api.plMatrix(year),
    ])
      .then(([l, p, s, m, y, mx]) => {
        setLedger(l);
        setPl(p);
        setSummary(s);
        setMonthly(m);
        setYearly(y);
        setMatrix(mx);
      })
      .catch((e: unknown) => setError(msg(e)));
  }, [range, year, accountId, projectId, categoryId, search, onlyReview, batchId]);

  useEffect(loadStatic, [loadStatic]);
  useEffect(() => {
    const t = setTimeout(loadData, 150);
    return () => clearTimeout(t);
  }, [loadData]);

  const refreshAll = () => {
    loadStatic();
    loadData();
    setSelected(new Set());
    setEditing(null);
    setDeleting(false);
  };

  const toggle = (id: number) =>
    setSelected((s) => {
      const next = new Set(s);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const selectMany = (ids: number[], on: boolean) =>
    setSelected((s) => {
      const next = new Set(s);
      for (const id of ids) {
        if (on) next.add(id);
        else next.delete(id);
      }
      return next;
    });

  // Everything shown, and everything shown from the merchants already
  // picked: the two bundles worth one click each.
  const shownIds = useMemo(() => ledger?.rows.map((r) => r.id) ?? [], [ledger]);
  const sameMerchantIds = useMemo(() => {
    if (!ledger || selected.size === 0) return [];
    const picked = new Set(
      ledger.rows.filter((r) => selected.has(r.id) && r.merchant).map((r) => r.merchant),
    );
    return ledger.rows
      .filter((r) => r.merchant && picked.has(r.merchant) && !selected.has(r.id))
      .map((r) => r.id);
  }, [ledger, selected]);

  const assign = (categoryTarget: number | null, project: number | null) =>
    api
      .txnAssign([...selected], categoryTarget, project)
      .then(refreshAll)
      .catch((e: unknown) => setError(msg(e)));

  return (
    <div className="min-w-0">
      <PageHead title={t.ledger.title} lede={t.ledger.lede} />

      <div className="space-y-5 px-10 pb-12">
        {error && <Banner tone="bad">{error}</Banner>}

        <div className="flex items-center gap-1 border-b border-rule">
          {(
            [
              ["overview", t.ledger.tabs.overview],
              ["plan", t.ledger.tabs.plan],
              ["transactions", t.ledger.tabs.transactions],
              ["import", t.ledger.tabs.import],
            ] as const
          )
            .filter(([id]) => (id === "import" ? showImport : showLedger))
            .map(([id, label]) => (
            <button
              key={id}
              type="button"
              onClick={() => setSub(id)}
              className={cn(
                "-mb-px border-b-2 px-3 py-2 text-[12.5px] transition-colors",
                sub === id
                  ? "border-brass text-ink"
                  : "border-transparent text-ink-3 hover:text-ink",
              )}
            >
                {label}
              </button>
            ))}
        </div>

        {(sub === "overview" || sub === "transactions") && (
        <Panel>
          <PanelHead
            label={sub === "overview" ? t.ledger.year : t.ledger.filters}
            aside={
              sub === "overview" ? (
                <div className="w-[26rem] max-w-full">
                  <Segmented
                    options={[
                      ["happened", t.ledger.views.happened],
                      ["budget", t.ledger.views.budget],
                      ["recurring", t.ledger.views.recurring],
                    ]}
                    value={view}
                    onChange={setView}
                  />
                </div>
              ) : undefined
            }
          />
          <PanelBody
            className={cn(
              "grid gap-3",
              sub === "overview" ? "sm:w-40" : "grid-cols-2 sm:grid-cols-5",
              // Recurring payments are read over the whole history, so there
              // is no year to pick.
              sub === "overview" && view === "recurring" && "hidden",
            )}
          >
            <SelectField
              label={t.ledger.year}
              value={year}
              onChange={(e) => setYear(Number(e.currentTarget.value))}
            >
              {(years.length > 0 ? years : [year]).map((y) => (
                <option key={y} value={y}>
                  {y}
                </option>
              ))}
            </SelectField>
            {sub === "transactions" && (
              <>
            <SelectField
              label={t.ledger.category}
              value={categoryId ?? ""}
              onChange={(e) =>
                setCategoryId(e.currentTarget.value ? Number(e.currentTarget.value) : null)
              }
            >
              <option value="">{t.ledger.allCategories}</option>
              {categories.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </SelectField>
            <SelectField
              label={t.ledger.event}
              value={projectId ?? ""}
              onChange={(e) =>
                setProjectId(e.currentTarget.value ? Number(e.currentTarget.value) : null)
              }
            >
              <option value="">{t.ledger.allEvents}</option>
              {events.map((e) => (
                <option key={e.id} value={e.id}>
                  {e.name}
                </option>
              ))}
            </SelectField>
            <SelectField
              label={t.ledger.account}
              value={accountId ?? ""}
              onChange={(e) =>
                setAccountId(e.currentTarget.value ? Number(e.currentTarget.value) : null)
              }
            >
              <option value="">{t.ledger.allAccounts}</option>
              {accounts.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </SelectField>
            <SelectField
              label={t.ledger.fromImport}
              value={batchId ?? ""}
              onChange={(e) => {
                setBatchId(e.currentTarget.value ? Number(e.currentTarget.value) : null);
                setRemoving(null);
              }}
            >
              <option value="">{t.ledger.anyImport}</option>
              {batches.map((b) => (
                <option key={b.id} value={b.id}>
                  {b.sourceFilename ?? "—"} · {b.importedAt.slice(0, 10)}
                </option>
              ))}
            </SelectField>
            <TextField
              label={t.ledger.search}
              value={search}
              placeholder={t.ledger.searchPlaceholder}
              onChange={(e) => setSearch(e.currentTarget.value)}
            />
              </>
            )}
          </PanelBody>
        </Panel>
        )}

        {sub === "import" ? (
          <ImportView embedded onImported={refreshAll} />
        ) : sub === "plan" ? (
          <MonthlyPlan />
        ) : sub === "overview" && view === "recurring" ? (
          <RecurringView />
        ) : sub === "overview" && view === "budget" ? (
          <BudgetsView year={year} categories={categories} onChanged={refreshAll} />
        ) : sub === "overview" ? (
          <CashFlowAnalytics
            summary={summary}
            monthly={monthly}
            yearly={yearly}
            pl={pl}
            matrix={matrix}
          />
        ) : (
          <>
            {/* What is done to the transactions, kept where they are: the
                categories they are filed under, and the events they can be
                gathered into. */}
            <div className="flex flex-wrap items-end justify-between gap-3">
              <Button variant={managing ? "primary" : "secondary"} onClick={() => setManaging((v) => !v)}>
                <Settings2 className="size-3.5" strokeWidth={1.75} />
                {t.ledger.tabs.categories}
              </Button>
              <NewEvent categories={categories} onSaved={refreshAll} onError={setError} />
            </div>

            {managing && (
              <div className="space-y-3 rounded-[10px] border border-brass/50 bg-surface p-4">
                <div className="flex items-center justify-between">
                  <p className="eyebrow">{t.ledger.tabs.categories}</p>
                  <Button variant="ghost" onClick={() => setManaging(false)} aria-label={t.common.close}>
                    <X className="size-3.5" strokeWidth={2} />
                  </Button>
                </div>
                <CategoriesTab categories={categories} onChanged={refreshAll} />
              </div>
            )}

            {events.length > 0 && (
              <Panel>
                <PanelHead label={t.ledger.events} />
                <div className="divide-y divide-rule">
                  {events.map((e) => (
                    <div key={e.id} className="flex items-center gap-4 px-5 py-2.5">
                      <span
                        className={cn(
                          "min-w-0 flex-1 truncate text-[12.5px]",
                          e.isClosed ? "text-ink-3 line-through" : "text-ink",
                        )}
                      >
                        {e.name}
                      </span>
                      <span className="rounded bg-brass-wash px-1.5 py-0.5 text-[10px] text-brass">
                        {e.defaultCategoryName}
                      </span>
                      <span className="text-[11px] text-ink-3">{t.common.txn(e.txnCount)}</span>
                      <span className="tnum text-[12.5px] text-ink">{fmtMoney(e.netMinor)}</span>
                      {e.budgetMinor !== null && (
                        <span className="tnum text-[11px] text-ink-3">
                          {t.ledger.ofBudget(fmtCompact(e.budgetMinor))}
                        </span>
                      )}
                      <Button
                        variant="ghost"
                        onClick={() =>
                          api
                            .eventSetClosed(e.id, !e.isClosed)
                            .then(refreshAll)
                            .catch((err: unknown) => setError(msg(err)))
                        }
                      >
                        {e.isClosed ? t.common.reopen : t.common.close}
                      </Button>
                    </div>
                  ))}
                </div>
              </Panel>
            )}

            {batchId !== null && (
              <div className="space-y-3">
                <Banner>
                  {t.ledger.showingImport(
                    batches.find((b) => b.id === batchId)?.sourceFilename ?? "—",
                    ledger?.total ?? 0,
                  )}
                  <span className="ml-3 inline-flex gap-2 align-middle">
                    <Button
                      variant="danger"
                      onClick={() =>
                        api
                          .importImpact(batchId)
                          .then(setRemoving)
                          .catch((e: unknown) => setError(msg(e)))
                      }
                    >
                      {t.importer.undo}
                    </Button>
                    <Button variant="ghost" onClick={() => setBatchId(null)}>
                      {t.ledger.showAll}
                    </Button>
                  </span>
                </Banner>
                {removing && (
                  <RemoveImport
                    batchId={batchId}
                    impact={removing}
                    onCancel={() => setRemoving(null)}
                    onDone={() => {
                      setRemoving(null);
                      setBatchId(null);
                      refreshAll();
                    }}
                    onError={setError}
                  />
                )}
              </div>
            )}

            {tidying && (
              <AutoCategorise
                categories={categories}
                events={events.filter((e) => !e.isClosed)}
                onApplied={refreshAll}
                onHide={() => setTidying(false)}
              />
            )}

            <Panel>
              <PanelHead
                label={t.ledger.transactions(ledger ? ledger.total : null)}
                aside={
                  <div className="flex items-center gap-2">
                    <div className="flex items-center gap-0.5">
                      {(
                        [
                          ["month", t.ledger.grouping.month],
                          ["category", t.ledger.grouping.category],
                          ["flat", t.ledger.grouping.flat],
                        ] as const
                      ).map(([id, label]) => (
                        <button
                          key={id}
                          type="button"
                          onClick={() => setGrouping(id)}
                          className={cn(
                            "rounded px-2 py-1 text-[11px] transition-colors",
                            grouping === id
                              ? "bg-brass-wash text-ink"
                              : "text-ink-3 hover:text-ink",
                          )}
                        >
                          {label}
                        </button>
                      ))}
                    </div>
                    {shownIds.length > 0 && selected.size === 0 && (
                      <Button variant="ghost" onClick={() => selectMany(shownIds, true)}>
                        {t.ledger.selectAll(shownIds.length)}
                      </Button>
                    )}
                    <Button
                      variant={onlyReview ? "primary" : "ghost"}
                      onClick={() => setOnlyReview((v) => !v)}
                    >
                      {t.ledger.needsReview}
                    </Button>
                    <Button
                      variant={tidying ? "primary" : "secondary"}
                      onClick={() => setTidying((v) => !v)}
                    >
                      <Wand2 className="size-3.5" strokeWidth={1.75} />
                      {t.ledger.autoCategorise}
                    </Button>
                  </div>
                }
              />

              {selected.size > 0 && (
                <div className="flex flex-wrap items-center gap-2 border-b border-rule bg-brass-wash/40 px-5 py-2.5">
                  <span className="text-[12px] text-ink">{t.ledger.selected(selected.size)}</span>
                  {selected.size < shownIds.length && (
                    <Button variant="ghost" onClick={() => selectMany(shownIds, true)}>
                      {t.ledger.selectAll(shownIds.length)}
                    </Button>
                  )}
                  {sameMerchantIds.length > 0 && (
                    <Button variant="ghost" onClick={() => selectMany(sameMerchantIds, true)}>
                      {t.ledger.sameMerchant(sameMerchantIds.length)}
                    </Button>
                  )}
                  <SelectField
                    value=""
                    className="w-44"
                    onChange={(e) =>
                      e.currentTarget.value && assign(Number(e.currentTarget.value), null)
                    }
                  >
                    <option value="">{t.ledger.setCategory}</option>
                    {categories.map((c) => (
                      <option key={c.id} value={c.id}>
                        {c.name}
                      </option>
                    ))}
                  </SelectField>
                  <SelectField
                    value=""
                    className="w-44"
                    onChange={(e) =>
                      e.currentTarget.value && assign(null, Number(e.currentTarget.value))
                    }
                  >
                    <option value="">{t.ledger.addToEvent}</option>
                    {events
                      .filter((e) => !e.isClosed)
                      .map((e) => (
                        <option key={e.id} value={e.id}>
                          {e.name}
                        </option>
                      ))}
                  </SelectField>
                  <Button
                    onClick={() =>
                      api
                        .txnSetNeutral([...selected], true)
                        .then(refreshAll)
                        .catch((e: unknown) => setError(msg(e)))
                    }
                  >
                    {t.ledger.markTransfer}
                  </Button>
                  <span className="flex-1" />
                  {deleting ? (
                    <>
                      <span className="text-[12px] text-ink-2">
                        {t.ledger.deleteSelectedSure(selected.size)}
                      </span>
                      <Button variant="ghost" onClick={() => setDeleting(false)}>
                        {t.common.cancel}
                      </Button>
                      <Button
                        variant="danger"
                        onClick={() =>
                          api
                            .txnDelete([...selected])
                            .then(refreshAll)
                            .catch((e: unknown) => setError(msg(e)))
                        }
                      >
                        {t.ledger.deleteSelected}
                      </Button>
                    </>
                  ) : (
                    <Button variant="ghost" onClick={() => setDeleting(true)}>
                      <Trash2 className="size-3" strokeWidth={1.75} />
                      {t.ledger.deleteSelected}
                    </Button>
                  )}
                  <Button
                    variant="ghost"
                    onClick={() => {
                      setSelected(new Set());
                      setDeleting(false);
                    }}
                  >
                    <X className="size-3" strokeWidth={2} />
                  </Button>
                </div>
              )}

              {!ledger || ledger.rows.length === 0 ? (
                <PanelBody>
                  <Empty
                    title={t.ledger.nothingHere}
                    body={t.ledger.nothingHereBody}
                  />
                </PanelBody>
              ) : (
                <TransactionTree
                  rows={ledger.rows}
                  grouping={grouping}
                  selected={selected}
                  onToggleSelect={toggle}
                  onSelectMany={selectMany}
                  editing={editing}
                  onEdit={(id) => setEditing(editing === id ? null : id)}
                  renderEditor={(row) => (
                    <RowEditor
                      row={row}
                      categories={categories}
                      events={events}
                      accounts={accounts}
                      onSaved={refreshAll}
                      onClose={() => setEditing(null)}
                      onError={setError}
                    />
                  )}
                />
              )}
            </Panel>
          </>
        )}
      </div>
    </div>
  );
}

/* --------------------------------------------------------------- editor -- */

function RowEditor({
  row,
  categories,
  events,
  accounts,
  onSaved,
  onClose,
  onError,
}: {
  row: LedgerRow;
  categories: Category[];
  events: EventItem[];
  accounts: Account[];
  onSaved: () => void;
  onClose: () => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const [bookedOn, setBookedOn] = useState(row.bookedOn);
  const [description, setDescription] = useState(row.description);
  const [amount, setAmount] = useState(String(row.amountMinor / 100));
  const [accountId, setAccountId] = useState(row.accountId);
  const [categoryId, setCategoryId] = useState<number | null>(row.categoryId);
  const [projectId, setProjectId] = useState<number | null>(row.projectId);
  const [neutral, setNeutral] = useState(row.isNeutralTransfer);
  const [notes, setNotes] = useState(row.notes ?? "");
  const [confirming, setConfirming] = useState(false);

  const save = () => {
    const value = Number(amount.trim().replace(",", "."));
    if (amount.trim() === "" || !Number.isFinite(value)) {
      onError(t.ledger.notAnAmount);
      return;
    }
    const minor = majorToMinor(value);
    api
      .txnUpdate(row.id, {
        bookedOn,
        description,
        // Only what changed is sent for the two fields a statement decides,
        // so saving a category never rewrites an amount by a rounding.
        ...(minor !== row.amountMinor ? { amountMinor: minor } : {}),
        ...(accountId !== row.accountId ? { accountId } : {}),
        categoryId,
        clearCategory: categoryId === null,
        projectId,
        clearProject: projectId === null,
        isNeutralTransfer: neutral,
        notes,
      })
      .then(onSaved)
      .catch((e: unknown) => onError(msg(e)));
  };

  return (
    <div className="space-y-3">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-6">
        <TextField
          label={t.ledger.date}
          type="date"
          value={bookedOn}
          onChange={(e) => setBookedOn(e.currentTarget.value)}
        />
        <TextField
          label={t.ledger.description}
          className="sm:col-span-3"
          value={description}
          onChange={(e) => setDescription(e.currentTarget.value)}
        />
        <TextField
          label={t.ledger.amount}
          className="sm:col-span-2"
          inputMode="decimal"
          value={amount}
          hint={t.ledger.amountHint}
          onChange={(e) => setAmount(e.currentTarget.value)}
          onKeyDown={(e) => e.key === "Enter" && save()}
        />
        <SelectField
          label={t.ledger.category}
          className="sm:col-span-2"
          value={categoryId ?? ""}
          onChange={(e) =>
            setCategoryId(e.currentTarget.value ? Number(e.currentTarget.value) : null)
          }
        >
          <option value="">{t.common.none}</option>
          {categories.map((c) => (
            <option key={c.id} value={c.id}>
              {c.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t.ledger.event}
          className="sm:col-span-2"
          value={projectId ?? ""}
          onChange={(e) =>
            setProjectId(e.currentTarget.value ? Number(e.currentTarget.value) : null)
          }
        >
          <option value="">{t.common.none}</option>
          {events.map((e) => (
            <option key={e.id} value={e.id}>
              {e.name}
            </option>
          ))}
        </SelectField>
        <SelectField
          label={t.ledger.account}
          className="sm:col-span-2"
          value={accountId}
          onChange={(e) => setAccountId(Number(e.currentTarget.value))}
        >
          {accounts.map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </SelectField>
        <TextField
          label={t.ledger.notes}
          className="col-span-2 sm:col-span-6"
          value={notes}
          onChange={(e) => setNotes(e.currentTarget.value)}
        />
      </div>

      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-4">
          <label className="flex cursor-pointer items-center gap-2">
            <input
              type="checkbox"
              checked={neutral}
              onChange={(e) => setNeutral(e.currentTarget.checked)}
              className="size-3.5 accent-[var(--brass)]"
            />
            <span className="text-[12px] text-ink-2">{t.ledger.internalTransfer}</span>
          </label>
          {row.parentTxnId && (
            <Button
              variant="ghost"
              onClick={() =>
                api
                  .txnUpdate(row.id, { clearParent: true })
                  .then(onSaved)
                  .catch((e: unknown) => onError(msg(e)))
              }
            >
              {t.ledger.unlinkFrom(row.parentTxnId)}
            </Button>
          )}
        </div>
        {confirming ? (
          <div className="flex items-center gap-2">
            <span className="text-[12px] text-ink-2">{t.ledger.deleteSure}</span>
            <Button variant="ghost" onClick={() => setConfirming(false)}>
              {t.common.cancel}
            </Button>
            <Button
              variant="danger"
              onClick={() =>
                api
                  .txnDelete([row.id])
                  .then(onSaved)
                  .catch((e: unknown) => onError(msg(e)))
              }
            >
              {t.ledger.deleteTxn}
            </Button>
          </div>
        ) : (
          <div className="flex gap-2">
            <Button variant="ghost" onClick={() => setConfirming(true)}>
              <Trash2 className="size-3" strokeWidth={1.75} />
              {t.ledger.deleteTxn}
            </Button>
            <Button variant="ghost" onClick={onClose}>
              {t.common.cancel}
            </Button>
            <Button variant="primary" onClick={save}>
              {t.common.save}
            </Button>
          </div>
        )}
      </div>
    </div>
  );
}

function NewEvent({
  categories,
  onSaved,
  onError,
}: {
  categories: Category[];
  onSaved: () => void;
  onError: (m: string) => void;
}) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [categoryId, setCategoryId] = useState<number | null>(null);
  const [budget, setBudget] = useState("");
  const [startsOn, setStartsOn] = useState("");

  useEffect(() => {
    if (categoryId === null && categories.length > 0) {
      setCategoryId(categories.find((c) => c.kind === "expense")?.id ?? categories[0]!.id);
    }
  }, [categories, categoryId]);

  if (!open) {
    return (
      <Button variant="primary" onClick={() => setOpen(true)} disabled={categories.length === 0}>
        <Plus className="size-3.5" strokeWidth={2} />
        {t.ledger.newEvent}
      </Button>
    );
  }
  return (
    <div className="flex items-end gap-2">
      <TextField
        label={t.ledger.name}
        value={name}
        autoFocus
        className="w-44"
        placeholder={t.ledger.eventPlaceholder}
        onChange={(e) => setName(e.currentTarget.value)}
      />
      <SelectField
        label={t.ledger.rollsUpTo}
        value={categoryId ?? ""}
        className="w-40"
        onChange={(e) => setCategoryId(Number(e.currentTarget.value))}
      >
        {categories.map((c) => (
          <option key={c.id} value={c.id}>
            {c.name}
          </option>
        ))}
      </SelectField>
      <TextField
        label={t.ledger.budget}
        value={budget}
        className="w-24"
        onChange={(e) => setBudget(e.currentTarget.value)}
      />
      <TextField
        label={t.ledger.starts}
        type="date"
        value={startsOn}
        className="w-36"
        onChange={(e) => setStartsOn(e.currentTarget.value)}
      />
      <Button
        variant="primary"
        onClick={() => {
          if (!categoryId) return;
          api
            .eventSave(null, {
              name,
              defaultCategoryId: categoryId,
              startsOn: startsOn || null,
              endsOn: null,
              budgetMinor: budget ? majorToMinor(Number(budget)) : null,
              notes: null,
            })
            .then(() => {
              setOpen(false);
              setName("");
              setBudget("");
              onSaved();
            })
            .catch((e: unknown) => onError(msg(e)));
        }}
      >
        <Tag className="size-3.5" strokeWidth={2} />
        {t.common.create}
      </Button>
      <Button variant="ghost" onClick={() => setOpen(false)}>
        <X className="size-3.5" strokeWidth={2} />
      </Button>
    </div>
  );
}
