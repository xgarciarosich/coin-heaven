import { Fragment, useMemo, useRef, useState } from "react";
import { ChevronDown, Link2, Pencil, Search, Trash2, X } from "lucide-react";
import { fmtMoney } from "@/lib/money";
import type { LedgerRow } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import type { Dict } from "@/locales/en";

export type Grouping = "month" | "category";

const monthLabel = (t: Dict, period: string) => {
  const m = Number(period.slice(5, 7));
  return `${t.common.monthsLong[m - 1] ?? period} ${period.slice(0, 4)}`;
};

interface Node {
  key: string;
  label: string;
  rows: LedgerRow[];
  net: number;
}

const netOf = (rows: LedgerRow[]) => rows.reduce((n, r) => n + r.netMinor, 0);

function group(rows: LedgerRow[], by: (r: LedgerRow) => [string, string]): Node[] {
  const map = new Map<string, Node>();
  for (const r of rows) {
    const [key, label] = by(r);
    let node = map.get(key);
    if (!node) {
      node = { key, label, rows: [], net: 0 };
      map.set(key, node);
    }
    node.rows.push(r);
  }
  return [...map.values()].map((n) => ({ ...n, net: netOf(n.rows) }));
}

const byMonth =
  (t: Dict) =>
  (r: LedgerRow): [string, string] => {
    const p = r.bookedOn.slice(0, 7);
    return [p, monthLabel(t, p)];
  };
const byCategory =
  (t: Dict) =>
  (r: LedgerRow): [string, string] => [
    String(r.categoryId ?? "none"),
    r.categoryName ?? t.common.uncategorised,
  ];

/**
 * The ledger, grouped by month or by category, and closed by default.
 *
 * A year of statements is two or three thousand lines, and there is not much
 * room: the groups are shown as one line each, with the subtotal that people
 * are actually looking for, and open into the lines behind them when asked.
 * Months run in calendar order; categories by weight, biggest first. Inside a
 * group the lines are in date order.
 */
export function TransactionTree({
  rows,
  grouping,
  selected,
  onToggleSelect,
  onSelectMany,
  editing,
  onEdit,
  renderEditor,
  deleting,
  onAskDelete,
  renderDeleteConfirm,
  onSearchSame,
  openAll,
}: {
  rows: LedgerRow[];
  grouping: Grouping;
  selected: Set<number>;
  onToggleSelect: (id: number) => void;
  /** Select or deselect a whole bundle at once. */
  onSelectMany: (ids: number[], on: boolean) => void;
  editing: number | null;
  onEdit: (id: number) => void;
  renderEditor: (row: LedgerRow) => React.ReactNode;
  /** The line a delete is being confirmed for. */
  deleting: number | null;
  onAskDelete: (id: number) => void;
  renderDeleteConfirm: (row: LedgerRow) => React.ReactNode;
  /** "Show me everything with this name." */
  onSearchSame: (row: LedgerRow) => void;
  /** The list has been narrowed (a search, an import): show the lines open. */
  openAll?: boolean;
}) {
  const t = useT();
  // Open groups. Nothing is open until it is asked for; the choice is kept
  // while the data is refreshed, and starts over when the grouping changes.
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [openFor, setOpenFor] = useState(grouping);
  if (openFor !== grouping) {
    setOpenFor(grouping);
    setOpen(new Set());
  }
  const [autoFor, setAutoFor] = useState(false);
  const keysNow = (): string[] => {
    const seen = new Set<string>();
    for (const r of rows) {
      seen.add(grouping === "month" ? r.bookedOn.slice(0, 7) : String(r.categoryId ?? "none"));
    }
    return [...seen];
  };
  // A narrowed list is the answer to a question: show it, not a column of
  // closed groups. It can still be folded afterwards.
  if ((openAll ?? false) !== autoFor) {
    setAutoFor(openAll ?? false);
    if (openAll) setOpen(new Set(keysNow()));
  }
  const [lastRows, setLastRows] = useState(rows);
  if (lastRows !== rows) {
    setLastRows(rows);
    // New results while narrowed (the search was edited): open what is there.
    if (openAll) setOpen(new Set(keysNow()));
  }
  const toggle = (key: string) =>
    setOpen((c) => {
      const next = new Set(c);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const tree = useMemo<Node[]>(() => {
    const dated = [...rows].sort((a, b) => a.bookedOn.localeCompare(b.bookedOn) || a.id - b.id);
    return group(dated, grouping === "month" ? byMonth(t) : byCategory(t)).sort(cmp(grouping));
  }, [rows, grouping, t]);

  // An editor is on a line; its group has to be open for it to be seen.
  const editingGroup = useMemo(
    () => tree.find((n) => n.rows.some((r) => r.id === editing))?.key ?? null,
    [tree, editing],
  );
  const isOpen = (key: string) => open.has(key) || key === editingGroup;
  const allOpen = tree.length > 0 && tree.every((n) => isOpen(n.key));

  // The rows in the order they are on screen, so a shift-click can pick
  // "from that one to this one" and mean what the eye sees.
  const visible = useMemo<number[]>(
    () => tree.flatMap((n) => (isOpen(n.key) ? n.rows.map((r) => r.id) : [])),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tree, open, editingGroup],
  );

  const anchor = useRef<number | null>(null);
  const pick = (id: number, shift: boolean) => {
    const from = anchor.current === null ? -1 : visible.indexOf(anchor.current);
    const to = visible.indexOf(id);
    if (shift && from !== -1 && to !== -1) {
      const [a, b] = from < to ? [from, to] : [to, from];
      // The range takes the state the clicked row is about to have.
      onSelectMany(visible.slice(a, b + 1), !selected.has(id));
    } else {
      onToggleSelect(id);
    }
    anchor.current = id;
  };
  const stateOf = (ids: number[]): "all" | "some" | "none" => {
    const n = ids.filter((i) => selected.has(i)).length;
    return n === 0 ? "none" : n === ids.length ? "all" : "some";
  };

  const body = tree.map((n) => (
    <Fragment key={n.key}>
      <GroupHeader
        label={n.label}
        count={n.rows.length}
        net={n.net}
        collapsed={!isOpen(n.key)}
        onToggle={() => toggle(n.key)}
        selection={stateOf(n.rows.map((r) => r.id))}
        onSelect={(on) => onSelectMany(n.rows.map((r) => r.id), on)}
      />
      {isOpen(n.key) &&
        n.rows.map((r) => (
          <RowWithEditor
            key={r.id}
            row={r}
            depth={1}
            selected={selected}
            onPick={pick}
            editing={editing}
            onEdit={onEdit}
            renderEditor={renderEditor}
            deleting={deleting}
            onAskDelete={onAskDelete}
            renderDeleteConfirm={renderDeleteConfirm}
            onSearchSame={onSearchSame}
          />
        ))}
    </Fragment>
  ));

  return (
    <div>
      {tree.length > 1 && (
        <div className="flex justify-end border-b border-rule px-5 py-1.5">
          <button
            type="button"
            onClick={() => setOpen(allOpen ? new Set() : new Set(tree.map((n) => n.key)))}
            className="eyebrow underline-offset-2 hover:text-ink hover:underline"
          >
            {allOpen ? t.ledger.closeAll : t.ledger.openAll}
          </button>
        </div>
      )}
      <div className="max-h-[620px] overflow-auto">
        <table className="w-full">
          <thead className="sticky top-0 z-10 bg-surface">
            <tr className="border-b border-rule">
              <th className="w-8 py-2.5 pl-5" />
              {t.ledger.columns.map((h, i) => (
                <th
                  key={h || i}
                  className={cn(
                    "eyebrow py-2.5 font-medium",
                    i === 4 ? "text-right" : "text-left",
                    i === 5 && "pr-5",
                  )}
                >
                  {h}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>{body}</tbody>
        </table>
      </div>
    </div>
  );
}

/** Months run forward; categories run by weight, biggest first. */
function cmp(kind: "month" | "category") {
  return (a: Node, b: Node) =>
    kind === "month"
      ? a.key.localeCompare(b.key)
      : Math.abs(b.net) - Math.abs(a.net) || a.label.localeCompare(b.label);
}

function GroupHeader({
  label,
  count,
  net,
  collapsed,
  onToggle,
  selection,
  onSelect,
}: {
  label: string;
  count: number;
  net: number;
  collapsed: boolean;
  onToggle: () => void;
  selection: "all" | "some" | "none";
  onSelect: (on: boolean) => void;
}) {
  const t = useT();
  return (
    <tr
      className="cursor-pointer border-b border-rule bg-surface-hi transition-colors hover:bg-surface-hi/70"
      onClick={onToggle}
    >
      <td colSpan={5} className="py-2 pl-5">
        <div className="flex items-center gap-2">
          {/* A whole bundle at once: the group's checkbox sits before its
              name, and does not fold the group. */}
          <input
            type="checkbox"
            checked={selection === "all"}
            ref={(el) => {
              if (el) el.indeterminate = selection === "some";
            }}
            onClick={(e) => e.stopPropagation()}
            onChange={() => onSelect(selection !== "all")}
            aria-label={t.ledger.selectGroup(label, count)}
            title={t.ledger.selectGroup(label, count)}
            className="size-3.5 accent-[var(--brass)]"
          />
          <ChevronDown
            className={cn(
              "size-3.5 shrink-0 text-ink-3 transition-transform",
              collapsed && "-rotate-90",
            )}
            strokeWidth={1.75}
          />
          <span className="text-[12.5px] text-ink">{label}</span>
          <span className="tnum text-[10.5px] text-ink-3">{count}</span>
        </div>
      </td>
      <td className="py-2 text-right">
        <span
          className={cn(
            "tnum text-[12.5px]",
            net < 0 ? "text-ink" : "text-positive",
            "font-medium",
          )}
        >
          {fmtMoney(net)}
        </span>
      </td>
      <td className="py-2 pr-5" />
    </tr>
  );
}

function RowWithEditor({
  row,
  depth,
  selected,
  onPick,
  editing,
  onEdit,
  renderEditor,
  deleting,
  onAskDelete,
  renderDeleteConfirm,
  onSearchSame,
}: {
  row: LedgerRow;
  depth: number;
  selected: Set<number>;
  onPick: (id: number, shift: boolean) => void;
  editing: number | null;
  onEdit: (id: number) => void;
  renderEditor: (row: LedgerRow) => React.ReactNode;
  deleting: number | null;
  onAskDelete: (id: number) => void;
  renderDeleteConfirm: (row: LedgerRow) => React.ReactNode;
  onSearchSame: (row: LedgerRow) => void;
}) {
  const isEditing = editing === row.id;
  return (
    <Fragment>
      <Row
        row={row}
        depth={depth}
        checked={selected.has(row.id)}
        onPick={(shift) => onPick(row.id, shift)}
        onEdit={() => onEdit(row.id)}
        isEditing={isEditing}
        onDelete={() => onAskDelete(row.id)}
        onSearchSame={() => onSearchSame(row)}
      />
      {deleting === row.id && (
        <tr className="border-b border-rule bg-negative/[0.05]">
          <td colSpan={7} className="px-5 py-3">
            {renderDeleteConfirm(row)}
          </td>
        </tr>
      )}
      {isEditing && (
        <tr className="border-b border-rule bg-brass-wash/30">
          <td colSpan={7} className="px-5 py-4">
            {renderEditor(row)}
          </td>
        </tr>
      )}
    </Fragment>
  );
}

export function Row({
  row,
  checked,
  onPick,
  onEdit,
  isEditing,
  onDelete,
  onSearchSame,
  depth = 0,
}: {
  row: LedgerRow;
  checked: boolean;
  onDelete?: () => void;
  onSearchSame?: () => void;
  /** `shift` is true for a shift-click: select the range from the last one. */
  onPick: (shift: boolean) => void;
  onEdit: () => void;
  isEditing: boolean;
  depth?: number;
}) {
  const t = useT();
  const netted = row.netMinor !== row.amountMinor;
  return (
    <tr
      className={cn(
        "border-b border-rule/60 transition-colors hover:bg-surface-hi",
        checked && "bg-brass-wash/50",
        row.isNeutralTransfer && "opacity-45",
      )}
      // Two clicks on a line open it for editing, as the pencil does.
      onDoubleClick={(e) => {
        if ((e.target as HTMLElement).closest("button, input, select, a")) return;
        if (!isEditing) onEdit();
      }}
    >
      <td className={cn("py-1.5", depth > 0 ? "pl-10" : "pl-5")}>
        <input
          type="checkbox"
          checked={checked}
          onClick={(e) => onPick(e.shiftKey)}
          onChange={() => undefined}
          title={t.ledger.shiftHint}
          className="size-3.5 accent-[var(--brass)]"
        />
      </td>
      <td className="tnum py-1.5 pr-3 text-[12px] whitespace-nowrap text-ink-3">
        {row.bookedOn}
      </td>
      <td className="max-w-0 py-1.5 pr-3">
        <div className="flex items-center gap-1.5">
          <span className="truncate text-[12.5px] text-ink">{row.description}</span>
          {row.needsReview && (
            <span className="size-1.5 shrink-0 rounded-full bg-brass" title={t.ledger.needsReviewDot} />
          )}
          {row.parentTxnId && <Link2 className="size-3 shrink-0 text-ink-3" strokeWidth={1.75} />}
        </div>
        <span className="text-[10.5px] text-ink-3">
          {row.accountName}
          {row.importFile && (
            <span title={t.ledger.importedFrom(row.importFile)}> · {row.importFile}</span>
          )}
        </span>
      </td>
      <td className="py-1.5 pr-3 text-[12px] text-ink-2">
        {row.categoryName ?? <span className="text-ink-3">—</span>}
      </td>
      <td className="py-1.5 pr-3 text-[12px] text-ink-2">
        {row.projectName ?? <span className="text-ink-3">—</span>}
      </td>
      <td className="py-1.5 text-right">
        <span
          className={cn(
            "tnum block text-[12.5px]",
            row.amountMinor < 0 ? "text-ink" : "text-positive",
          )}
        >
          {fmtMoney(row.amountMinor)}
        </span>
        {netted && (
          <span className="tnum block text-[10.5px] text-brass">
            {t.ledger.net(fmtMoney(row.netMinor))}
          </span>
        )}
      </td>
      <td className="py-1.5 pr-5 pl-3 text-right whitespace-nowrap">
        {onSearchSame && (
          <button
            type="button"
            onClick={onSearchSame}
            aria-label={t.ledger.searchSame}
            title={t.ledger.searchSame}
            className="rounded p-1 text-ink-3 transition-colors hover:text-ink"
          >
            <Search className="size-3" strokeWidth={1.75} />
          </button>
        )}
        <button
          type="button"
          onClick={onEdit}
          aria-label={t.ledger.editTxn}
          title={t.ledger.editTxn}
          className="rounded p-1 text-ink-3 transition-colors hover:text-ink"
        >
          {isEditing ? (
            <X className="size-3" strokeWidth={2} />
          ) : (
            <Pencil className="size-3" strokeWidth={1.75} />
          )}
        </button>
        {onDelete && (
          <button
            type="button"
            onClick={onDelete}
            aria-label={t.ledger.deleteTxn}
            title={t.ledger.deleteTxn}
            className="rounded p-1 text-ink-3 transition-colors hover:text-negative"
          >
            <Trash2 className="size-3" strokeWidth={1.75} />
          </button>
        )}
      </td>
    </tr>
  );
}
