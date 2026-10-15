import { Fragment, useMemo, useRef, useState } from "react";
import { ChevronDown, Link2, Pencil, X } from "lucide-react";
import { fmtMoney } from "@/lib/money";
import type { LedgerRow } from "@/lib/types";
import { cn } from "@/lib/utils";
import { useT } from "@/lib/i18n";
import type { Dict } from "@/locales/en";

export type Grouping = "month" | "category" | "flat";

const monthLabel = (t: Dict, period: string) => {
  const m = Number(period.slice(5, 7));
  return `${t.common.monthsLong[m - 1] ?? period} ${period.slice(0, 4)}`;
};

interface Node {
  key: string;
  label: string;
  rows: LedgerRow[];
  children: Node[];
  net: number;
}

const netOf = (rows: LedgerRow[]) => rows.reduce((n, r) => n + r.netMinor, 0);

function group(rows: LedgerRow[], by: (r: LedgerRow) => [string, string]): Node[] {
  const map = new Map<string, Node>();
  for (const r of rows) {
    const [key, label] = by(r);
    let node = map.get(key);
    if (!node) {
      node = { key, label, rows: [], children: [], net: 0 };
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
 * The ledger as a two-level tree.
 *
 * A year of statements is two or three thousand lines, and a flat list of
 * those is only ever scrolled past. Grouping by month and then by category
 * turns it into twelve rows that each open into something the size of a
 * receipt — and the subtotal on every header is the number people are
 * actually looking for when they scroll.
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
}) {
  const t = useT();
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const toggle = (key: string) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  const tree = useMemo<Node[]>(() => {
    if (grouping === "flat") return [];
    const [outer, inner] =
      grouping === "month" ? [byMonth(t), byCategory(t)] : [byCategory(t), byMonth(t)];
    return group(rows, outer)
      .map((n) => ({ ...n, children: sortNodes(group(n.rows, inner), grouping === "month" ? "inner-category" : "inner-month") }))
      .sort(cmp(grouping === "month" ? "month" : "category"));
  }, [rows, grouping, t]);

  // The rows in the order they are on screen, so a shift-click can pick
  // "from that one to this one" and mean what the eye sees.
  const visible = useMemo<number[]>(() => {
    if (grouping === "flat") return rows.map((r) => r.id);
    return tree.flatMap((outer) =>
      collapsed.has(outer.key)
        ? []
        : outer.children.flatMap((inner) =>
            collapsed.has(`${outer.key}/${inner.key}`) ? [] : inner.rows.map((r) => r.id),
          ),
    );
  }, [rows, grouping, tree, collapsed]);

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

  const body =
    grouping === "flat"
      ? rows.map((r) => (
          <RowWithEditor
            key={r.id}
            row={r}
            depth={0}
            selected={selected}
            onPick={pick}
            editing={editing}
            onEdit={onEdit}
            renderEditor={renderEditor}
          />
        ))
      : tree.map((outer) => (
          <Fragment key={outer.key}>
            <GroupHeader
              label={outer.label}
              count={outer.rows.length}
              net={outer.net}
              collapsed={collapsed.has(outer.key)}
              onToggle={() => toggle(outer.key)}
              level={0}
              selection={stateOf(outer.rows.map((r) => r.id))}
              onSelect={(on) => onSelectMany(outer.rows.map((r) => r.id), on)}
            />
            {!collapsed.has(outer.key) &&
              outer.children.map((inner) => {
                const key = `${outer.key}/${inner.key}`;
                return (
                  <Fragment key={key}>
                    <GroupHeader
                      label={inner.label}
                      count={inner.rows.length}
                      net={inner.net}
                      collapsed={collapsed.has(key)}
                      onToggle={() => toggle(key)}
                      level={1}
                      selection={stateOf(inner.rows.map((r) => r.id))}
                      onSelect={(on) => onSelectMany(inner.rows.map((r) => r.id), on)}
                    />
                    {!collapsed.has(key) &&
                      inner.rows.map((r) => (
                        <RowWithEditor
                          key={r.id}
                          row={r}
                          depth={2}
                          selected={selected}
                          onPick={pick}
                          editing={editing}
                          onEdit={onEdit}
                          renderEditor={renderEditor}
                        />
                      ))}
                  </Fragment>
                );
              })}
          </Fragment>
        ));

  return (
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
  );
}

/** Months run forward; categories run by weight, biggest first. */
function cmp(kind: "month" | "category") {
  return (a: Node, b: Node) =>
    kind === "month"
      ? a.key.localeCompare(b.key)
      : Math.abs(b.net) - Math.abs(a.net) || a.label.localeCompare(b.label);
}

function sortNodes(nodes: Node[], kind: "inner-month" | "inner-category") {
  return [...nodes].sort(cmp(kind === "inner-month" ? "month" : "category"));
}

function GroupHeader({
  label,
  count,
  net,
  collapsed,
  onToggle,
  level,
  selection,
  onSelect,
}: {
  label: string;
  count: number;
  net: number;
  collapsed: boolean;
  onToggle: () => void;
  level: 0 | 1;
  selection: "all" | "some" | "none";
  onSelect: (on: boolean) => void;
}) {
  const t = useT();
  return (
    <tr
      className={cn(
        "cursor-pointer border-b border-rule transition-colors",
        level === 0 ? "bg-surface-hi" : "bg-surface-hi/40",
      )}
      onClick={onToggle}
    >
      <td colSpan={5} className={cn("py-2", level === 0 ? "pl-5" : "pl-10")}>
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
          <span
            className={cn(
              level === 0 ? "text-[12.5px] text-ink" : "text-[12px] text-ink-2",
            )}
          >
            {label}
          </span>
          <span className="tnum text-[10.5px] text-ink-3">{count}</span>
        </div>
      </td>
      <td className="py-2 text-right">
        <span
          className={cn(
            "tnum text-[12.5px]",
            net < 0 ? "text-ink" : "text-positive",
            level === 0 && "font-medium",
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
}: {
  row: LedgerRow;
  depth: number;
  selected: Set<number>;
  onPick: (id: number, shift: boolean) => void;
  editing: number | null;
  onEdit: (id: number) => void;
  renderEditor: (row: LedgerRow) => React.ReactNode;
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
      />
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
  depth = 0,
}: {
  row: LedgerRow;
  checked: boolean;
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
        <span className="text-[10.5px] text-ink-3">{row.accountName}</span>
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
      <td className="py-1.5 pr-5 pl-3 text-right">
        <button
          type="button"
          onClick={onEdit}
          aria-label={t.ledger.editTxn}
          className="rounded p-1 text-ink-3 transition-colors hover:text-ink"
        >
          {isEditing ? (
            <X className="size-3" strokeWidth={2} />
          ) : (
            <Pencil className="size-3" strokeWidth={1.75} />
          )}
        </button>
      </td>
    </tr>
  );
}
