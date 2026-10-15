//! Module 2 — categories, event containers, and the transaction ledger.
//!
//! Two views in the schema do the thinking, so every screen agrees:
//!
//!   * `v_txn_effective` resolves category precedence — a transaction's own
//!     override beats the default category of the event it belongs to.
//!   * `v_txn_net` nets reimbursements against the bill they offset, excludes
//!     the reimbursements themselves so nothing is counted twice, and drops
//!     neutralised transfers.
//!
//! Nothing here recomputes either rule; a category total that disagreed with
//! the ledger would be worse than no total at all.

use crate::error::{AppError, AppResult};
use crate::money::Money;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

/* ---------------------------------------------------------- categories -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub kind: String,
    pub color: Option<String>,
    pub sort_order: i64,
    pub txn_count: i64,
}

#[tauri::command]
pub fn category_list(state: State<'_, AppState>) -> AppResult<Vec<Category>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT k.id, k.parent_id, k.name, k.kind, k.color, k.sort_order,
                    (SELECT count(*) FROM v_txn_effective e
                      WHERE e.effective_category_id = k.id)
             FROM pl_category k WHERE k.is_archived = 0
             ORDER BY k.sort_order, k.name",
        )?;
        Ok(stmt
            .query_map([], |r| {
                Ok(Category {
                    id: r.get(0)?,
                    parent_id: r.get(1)?,
                    name: r.get(2)?,
                    kind: r.get(3)?,
                    color: r.get(4)?,
                    sort_order: r.get(5)?,
                    txn_count: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    })
}

#[tauri::command]
pub fn category_save(
    state: State<'_, AppState>,
    id: Option<i64>,
    name: String,
    kind: String,
    color: Option<String>,
) -> AppResult<i64> {
    if name.trim().is_empty() {
        return Err(AppError::Invalid("a category needs a name".into()));
    }
    if !matches!(kind.as_str(), "income" | "expense" | "neutral") {
        return Err(AppError::Invalid(format!("unknown category kind: {kind}")));
    }
    let id = state.db()?.with(|c| match id {
        Some(existing) => {
            c.execute(
                "UPDATE pl_category SET name = ?1, kind = ?2, color = ?3 WHERE id = ?4",
                rusqlite::params![name.trim(), kind, color, existing],
            )?;
            Ok(existing)
        }
        None => {
            c.execute(
                "INSERT INTO pl_category (name, kind, color, sort_order)
                 VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(sort_order), 0) + 10 FROM pl_category))",
                rusqlite::params![name.trim(), kind, color],
            )?;
            Ok(c.last_insert_rowid())
        }
    })?;
    state.backup.mark_dirty();
    Ok(id)
}

/// Archive rather than delete: a category still referenced by transactions or
/// events cannot be removed without silently rewriting history.
#[tauri::command]
pub fn category_archive(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.db()?.with(|c| {
        c.execute(
            "UPDATE pl_category SET is_archived = 1 WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/* -------------------------------------------------------------- events -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: i64,
    pub name: String,
    pub default_category_id: i64,
    pub default_category_name: String,
    pub starts_on: Option<String>,
    pub ends_on: Option<String>,
    pub budget_minor: Option<Money>,
    pub is_closed: bool,
    pub txn_count: i64,
    /// Net of any reimbursements booked against its transactions.
    pub net_minor: Money,
}

#[tauri::command]
pub fn event_list(state: State<'_, AppState>) -> AppResult<Vec<Event>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT p.id, p.name, p.default_category_id, k.name,
                    p.starts_on, p.ends_on, p.budget_minor, p.is_closed,
                    (SELECT count(*) FROM txn t WHERE t.project_id = p.id),
                    COALESCE((SELECT SUM(n.net_minor) FROM v_txn_net n
                               WHERE n.project_id = p.id), 0)
             FROM project_event p
             JOIN pl_category k ON k.id = p.default_category_id
             ORDER BY p.is_closed, COALESCE(p.starts_on, '9999') DESC, p.name",
        )?;
        Ok(stmt
            .query_map([], |r| {
                Ok(Event {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    default_category_id: r.get(2)?,
                    default_category_name: r.get(3)?,
                    starts_on: r.get(4)?,
                    ends_on: r.get(5)?,
                    budget_minor: r.get::<_, Option<i64>>(6)?.map(Money::from_minor),
                    is_closed: r.get::<_, i64>(7)? != 0,
                    txn_count: r.get(8)?,
                    net_minor: Money::from_minor(r.get(9)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventInput {
    pub name: String,
    pub default_category_id: i64,
    pub starts_on: Option<String>,
    pub ends_on: Option<String>,
    pub budget_minor: Option<i64>,
    pub notes: Option<String>,
}

fn check_date(label: &str, v: &Option<String>) -> AppResult<()> {
    if let Some(d) = v.as_deref().filter(|s| !s.is_empty()) {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| AppError::Invalid(format!("{label} must be YYYY-MM-DD")))?;
    }
    Ok(())
}

#[tauri::command]
pub fn event_save(
    state: State<'_, AppState>,
    id: Option<i64>,
    input: EventInput,
) -> AppResult<i64> {
    if input.name.trim().is_empty() {
        return Err(AppError::Invalid("an event needs a name".into()));
    }
    check_date("start date", &input.starts_on)?;
    check_date("end date", &input.ends_on)?;
    let blank = |v: Option<String>| v.filter(|s| !s.trim().is_empty());

    let id = state.db()?.with(|c| {
        let p = rusqlite::params![
            input.name.trim(),
            input.default_category_id,
            blank(input.starts_on.clone()),
            blank(input.ends_on.clone()),
            input.budget_minor,
            blank(input.notes.clone()),
        ];
        match id {
            Some(existing) => {
                c.execute(
                    "UPDATE project_event SET name=?1, default_category_id=?2, starts_on=?3,
                            ends_on=?4, budget_minor=?5, notes=?6 WHERE id=?7",
                    rusqlite::params_from_iter(
                        p.iter()
                            .copied()
                            .chain(std::iter::once(&existing as &dyn rusqlite::ToSql)),
                    ),
                )?;
                Ok(existing)
            }
            None => {
                c.execute(
                    "INSERT INTO project_event
                       (name, default_category_id, starts_on, ends_on, budget_minor, notes)
                     VALUES (?1,?2,?3,?4,?5,?6)",
                    p,
                )?;
                Ok(c.last_insert_rowid())
            }
        }
    })?;
    state.backup.mark_dirty();
    Ok(id)
}

#[tauri::command]
pub fn event_set_closed(state: State<'_, AppState>, id: i64, closed: bool) -> AppResult<()> {
    state.db()?.with(|c| {
        c.execute(
            "UPDATE project_event SET is_closed = ?1 WHERE id = ?2",
            rusqlite::params![i64::from(closed), id],
        )?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/// Deleting an event releases its transactions (the FK is ON DELETE SET NULL)
/// rather than taking them with it.
#[tauri::command]
pub fn event_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.db()?.with(|c| {
        c.execute(
            "DELETE FROM project_event WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/* -------------------------------------------------------------- ledger -- */

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TxnFilter {
    pub account_id: Option<i64>,
    pub project_id: Option<i64>,
    pub category_id: Option<i64>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub search: Option<String>,
    pub needs_review: Option<bool>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerRow {
    pub id: i64,
    pub booked_on: String,
    pub description: String,
    pub counterparty: Option<String>,
    pub amount_minor: Money,
    /// Amount after reimbursements; equal to `amount_minor` when there are none.
    pub net_minor: Money,
    pub currency: String,
    pub account_id: i64,
    pub account_name: String,
    pub notes: Option<String>,
    /// The shop a line is from, worked out from its description: the same
    /// word the importer groups by. It is what "everything from this
    /// merchant" selects on.
    pub merchant: Option<String>,
    pub category_id: Option<i64>,
    pub category_name: Option<String>,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
    pub is_neutral_transfer: bool,
    pub needs_review: bool,
    pub parent_txn_id: Option<i64>,
    pub reimbursement_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ledger {
    pub rows: Vec<LedgerRow>,
    pub total: i64,
}

#[tauri::command]
pub fn txn_list(state: State<'_, AppState>, filter: TxnFilter) -> AppResult<Ledger> {
    let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
    let offset = filter.offset.unwrap_or(0).max(0);

    // Every clause is parameterised; only the fixed fragments are concatenated,
    // never a user value.
    let mut where_sql = String::from(" WHERE 1=1");
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let mut push = |sql: &str, v: Box<dyn rusqlite::ToSql>, w: &mut String| {
        w.push_str(sql);
        args.push(v);
    };
    if let Some(v) = filter.account_id {
        push(" AND t.account_id = ?", Box::new(v), &mut where_sql);
    }
    if let Some(v) = filter.project_id {
        push(" AND t.project_id = ?", Box::new(v), &mut where_sql);
    }
    if let Some(v) = filter.category_id {
        push(
            " AND e.effective_category_id = ?",
            Box::new(v),
            &mut where_sql,
        );
    }
    if let Some(v) = filter.from.filter(|s| !s.is_empty()) {
        push(" AND t.booked_on >= ?", Box::new(v), &mut where_sql);
    }
    if let Some(v) = filter.to.filter(|s| !s.is_empty()) {
        push(" AND t.booked_on <= ?", Box::new(v), &mut where_sql);
    }
    if let Some(v) = filter.search.filter(|s| !s.trim().is_empty()) {
        push(
            " AND t.description_norm LIKE ?",
            Box::new(format!(
                "%{}%",
                crate::security::pii::normalise_description(&v)
            )),
            &mut where_sql,
        );
    }
    if filter.needs_review == Some(true) {
        where_sql.push_str(" AND t.needs_review = 1");
    }

    state.db()?.with(|c| {
        let base = "FROM txn t
             JOIN account a ON a.id = t.account_id
             LEFT JOIN v_txn_effective e ON e.id = t.id
             LEFT JOIN pl_category k ON k.id = e.effective_category_id
             LEFT JOIN project_event p ON p.id = t.project_id";

        let count_sql = format!("SELECT count(*) {base}{where_sql}");
        let total: i64 = c.query_row(
            &count_sql,
            rusqlite::params_from_iter(args.iter().map(|b| b.as_ref())),
            |r| r.get(0),
        )?;

        let sql = format!(
            "SELECT t.id, t.booked_on, t.description_raw, t.counterparty, t.amount_minor,
                    COALESCE((SELECT n.net_minor FROM v_txn_net n WHERE n.id = t.id),
                             t.amount_minor),
                    t.currency, a.name, e.effective_category_id, k.name,
                    t.project_id, p.name, t.is_neutral_transfer, t.needs_review,
                    t.parent_txn_id,
                    (SELECT count(*) FROM txn r
                      WHERE r.parent_txn_id = t.id AND r.link_kind = 'reimbursement'),
                    t.account_id, t.notes, t.description_norm
             {base}{where_sql}
             ORDER BY t.booked_on DESC, t.id DESC LIMIT ? OFFSET ?"
        );
        let mut stmt = c.prepare(&sql)?;
        let rows = stmt
            .query_map(
                rusqlite::params_from_iter(args.iter().map(|b| b.as_ref()).chain([
                    &limit as &dyn rusqlite::ToSql,
                    &offset as &dyn rusqlite::ToSql,
                ])),
                |r| {
                    Ok(LedgerRow {
                        id: r.get(0)?,
                        booked_on: r.get(1)?,
                        description: r.get(2)?,
                        counterparty: r.get(3)?,
                        amount_minor: Money::from_minor(r.get(4)?),
                        net_minor: Money::from_minor(r.get(5)?),
                        currency: r.get(6)?,
                        account_name: r.get(7)?,
                        category_id: r.get(8)?,
                        category_name: r.get(9)?,
                        project_id: r.get(10)?,
                        project_name: r.get(11)?,
                        is_neutral_transfer: r.get::<_, i64>(12)? != 0,
                        needs_review: r.get::<_, i64>(13)? != 0,
                        parent_txn_id: r.get(14)?,
                        reimbursement_count: r.get(15)?,
                        account_id: r.get(16)?,
                        notes: r.get(17)?,
                        merchant: crate::import::merchant::key(&r.get::<_, String>(18)?),
                    })
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Ledger { rows, total })
    })
}

/// Assign category and/or event to a set of transactions.
///
/// `None` leaves a field untouched; clearing is explicit via the `clear_*`
/// flags, so "no change" and "set to nothing" can never be confused.
#[tauri::command]
pub fn txn_assign(
    state: State<'_, AppState>,
    ids: Vec<i64>,
    category_id: Option<i64>,
    project_id: Option<i64>,
    clear_category: Option<bool>,
    clear_project: Option<bool>,
) -> AppResult<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let n = state.db()?.with(|c| {
        let tx = c.unchecked_transaction()?;
        let mut changed = 0;
        for id in &ids {
            if category_id.is_some() || clear_category == Some(true) {
                changed += tx.execute(
                    "UPDATE txn SET category_override_id = ?1, categorized_by = 'user',
                            needs_review = 0 WHERE id = ?2",
                    rusqlite::params![category_id, id],
                )?;
            }
            if project_id.is_some() || clear_project == Some(true) {
                tx.execute(
                    "UPDATE txn SET project_id = ?1, needs_review = 0 WHERE id = ?2",
                    rusqlite::params![project_id, id],
                )?;
            }
        }
        tx.commit()?;
        // A category set by hand is a decision about the merchant: carry it
        // to everything else from the same one that is still unfiled.
        if category_id.is_some() {
            changed += super::autocat::learn(c)?;
        }
        Ok(changed.max(ids.len()))
    })?;
    state.backup.mark_dirty();
    Ok(n)
}

/// Mark a transaction as an internal transfer, or undo that.
#[tauri::command]
pub fn txn_set_neutral(state: State<'_, AppState>, ids: Vec<i64>, neutral: bool) -> AppResult<()> {
    state.db()?.with(|c| {
        let tx = c.unchecked_transaction()?;
        for id in &ids {
            tx.execute(
                "UPDATE txn SET is_neutral_transfer = ?1, needs_review = 0 WHERE id = ?2",
                rusqlite::params![i64::from(neutral), id],
            )?;
        }
        tx.commit()?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/// Link a reimbursement to the bill it offsets, or unlink it.
#[tauri::command]
pub fn txn_link_reimbursement(
    state: State<'_, AppState>,
    parent_id: Option<i64>,
    child_id: i64,
) -> AppResult<()> {
    if parent_id == Some(child_id) {
        return Err(AppError::Invalid(
            "a transaction cannot reimburse itself".into(),
        ));
    }
    state.db()?.with(|c| {
        match parent_id {
            Some(p) => c.execute(
                "UPDATE txn SET parent_txn_id = ?1, link_kind = 'reimbursement',
                        needs_review = 0 WHERE id = ?2",
                rusqlite::params![p, child_id],
            )?,
            None => c.execute(
                "UPDATE txn SET parent_txn_id = NULL, link_kind = NULL WHERE id = ?1",
                rusqlite::params![child_id],
            )?,
        };
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/* ----------------------------------------------------------- P&L roll-up */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlLine {
    pub category_id: Option<i64>,
    pub category_name: String,
    pub kind: String,
    pub color: Option<String>,
    pub net_minor: Money,
    pub txn_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlSummary {
    pub lines: Vec<PlLine>,
    pub income_minor: Money,
    pub expense_minor: Money,
    pub net_minor: Money,
    pub uncategorised_count: i64,
}

/// Category totals for a period, read straight from `v_txn_net` so
/// reimbursements and neutralised transfers are handled once, centrally.
#[tauri::command]
pub fn pl_summary(state: State<'_, AppState>, from: String, to: String) -> AppResult<PlSummary> {
    check_date("from", &Some(from.clone()))?;
    check_date("to", &Some(to.clone()))?;

    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT n.effective_category_id, COALESCE(k.name, 'Uncategorised'),
                    COALESCE(k.kind, 'expense'), k.color,
                    SUM(n.net_minor), count(*)
             FROM v_txn_net n
             LEFT JOIN pl_category k ON k.id = n.effective_category_id
             WHERE n.booked_on >= ?1 AND n.booked_on <= ?2
             GROUP BY n.effective_category_id
             ORDER BY SUM(n.net_minor)",
        )?;
        let lines = stmt
            .query_map(rusqlite::params![from, to], |r| {
                Ok(PlLine {
                    category_id: r.get(0)?,
                    category_name: r.get(1)?,
                    kind: r.get(2)?,
                    color: r.get(3)?,
                    net_minor: Money::from_minor(r.get(4)?),
                    txn_count: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let income = lines
            .iter()
            .filter(|l| l.net_minor.minor() > 0)
            .map(|l| l.net_minor)
            .sum::<Money>();
        let expense = lines
            .iter()
            .filter(|l| l.net_minor.minor() < 0)
            .map(|l| l.net_minor)
            .sum::<Money>();
        let uncategorised = lines
            .iter()
            .filter(|l| l.category_id.is_none())
            .map(|l| l.txn_count)
            .sum();

        Ok(PlSummary {
            income_minor: income,
            expense_minor: expense,
            net_minor: income + expense,
            uncategorised_count: uncategorised,
            lines,
        })
    })
}

/* --------------------------------------------------- editing a row ------ */

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxnEdit {
    pub booked_on: Option<String>,
    pub description: Option<String>,
    pub amount_minor: Option<i64>,
    pub account_id: Option<i64>,
    pub counterparty: Option<String>,
    pub category_id: Option<i64>,
    pub clear_category: Option<bool>,
    pub project_id: Option<i64>,
    pub clear_project: Option<bool>,
    pub parent_txn_id: Option<i64>,
    pub clear_parent: Option<bool>,
    pub is_neutral_transfer: Option<bool>,
    pub notes: Option<String>,
}

/// Edit a single transaction — any part of it.
///
/// The amount and the account can be corrected like everything else: a
/// statement can be wrong, a row can land in the wrong account, and a ledger
/// its owner cannot put right is one they stop trusting. What keeps a re-import
/// honest is the dedupe hash, which is left as it was imported: loading the
/// same statement again still recognises the row and leaves the correction
/// alone.
#[tauri::command]
pub fn txn_update(state: State<'_, AppState>, id: i64, edit: TxnEdit) -> AppResult<()> {
    if let Some(d) = edit.booked_on.as_deref() {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d")
            .map_err(|_| AppError::Invalid("date must be YYYY-MM-DD".into()))?;
    }
    if edit.parent_txn_id == Some(id) {
        return Err(AppError::Invalid(
            "a transaction cannot be linked to itself".into(),
        ));
    }

    state.db()?.with(|c| {
        let tx = c.unchecked_transaction()?;
        if let Some(v) = &edit.booked_on {
            tx.execute(
                "UPDATE txn SET booked_on = ?1 WHERE id = ?2",
                rusqlite::params![v, id],
            )?;
        }
        if let Some(v) = &edit.description {
            if v.trim().is_empty() {
                return Err(AppError::Invalid("a description is required".into()));
            }
            tx.execute(
                "UPDATE txn SET description_raw = ?1, description_norm = ?2 WHERE id = ?3",
                rusqlite::params![v.trim(), crate::security::pii::normalise_description(v), id],
            )?;
        }
        if let Some(v) = edit.amount_minor {
            if v == 0 {
                return Err(AppError::Invalid(
                    "an amount of zero is not a transaction — delete it instead".into(),
                ));
            }
            tx.execute(
                "UPDATE txn SET amount_minor = ?1 WHERE id = ?2",
                rusqlite::params![v, id],
            )?;
        }
        if let Some(v) = edit.account_id {
            // Moving a row leaves its old account remembering it, or the next
            // statement for that account would import it a second time.
            tx.execute(
                "INSERT OR IGNORE INTO txn_removed (account_id, dedupe_hash)
                 SELECT account_id, dedupe_hash FROM txn WHERE id = ?1 AND account_id <> ?2",
                rusqlite::params![id, v],
            )?;
            tx.execute(
                "UPDATE txn SET account_id = ?1 WHERE id = ?2",
                rusqlite::params![v, id],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(f, _)
                    if f.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    AppError::Invalid(
                        "that account already has this exact transaction, or no longer exists"
                            .into(),
                    )
                }
                other => other.into(),
            })?;
        }
        if let Some(v) = &edit.counterparty {
            tx.execute(
                "UPDATE txn SET counterparty = ?1 WHERE id = ?2",
                rusqlite::params![
                    if v.trim().is_empty() {
                        None
                    } else {
                        Some(v.trim())
                    },
                    id
                ],
            )?;
        }
        if edit.category_id.is_some() || edit.clear_category == Some(true) {
            tx.execute(
                "UPDATE txn SET category_override_id = ?1, categorized_by = 'user',
                        needs_review = 0 WHERE id = ?2",
                rusqlite::params![edit.category_id, id],
            )?;
        }
        if edit.project_id.is_some() || edit.clear_project == Some(true) {
            tx.execute(
                "UPDATE txn SET project_id = ?1, needs_review = 0 WHERE id = ?2",
                rusqlite::params![edit.project_id, id],
            )?;
        }
        if edit.parent_txn_id.is_some() || edit.clear_parent == Some(true) {
            match edit.parent_txn_id {
                Some(p) => tx.execute(
                    "UPDATE txn SET parent_txn_id = ?1, link_kind = 'reimbursement',
                            needs_review = 0 WHERE id = ?2",
                    rusqlite::params![p, id],
                )?,
                None => tx.execute(
                    "UPDATE txn SET parent_txn_id = NULL, link_kind = NULL WHERE id = ?1",
                    rusqlite::params![id],
                )?,
            };
        }
        if let Some(v) = edit.is_neutral_transfer {
            tx.execute(
                "UPDATE txn SET is_neutral_transfer = ?1, needs_review = 0 WHERE id = ?2",
                rusqlite::params![i64::from(v), id],
            )?;
        }
        if let Some(v) = &edit.notes {
            tx.execute(
                "UPDATE txn SET notes = ?1 WHERE id = ?2",
                rusqlite::params![
                    if v.trim().is_empty() {
                        None
                    } else {
                        Some(v.trim())
                    },
                    id
                ],
            )?;
        }
        tx.commit()?;
        if edit.category_id.is_some() {
            super::autocat::learn(c)?;
        }
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/// Delete transactions for good.
///
/// Their hashes are remembered so an overlapping statement does not bring
/// them back, and anything linked to them — a refund booked against a bill —
/// is unlinked first and stands on its own again rather than vanishing with it.
#[tauri::command]
pub fn txn_delete(state: State<'_, AppState>, ids: Vec<i64>) -> AppResult<usize> {
    let n = state.db()?.with(|c| delete(c, &ids))?;
    state.backup.mark_dirty();
    Ok(n)
}

pub fn delete(c: &rusqlite::Connection, ids: &[i64]) -> AppResult<usize> {
    let tx = c.unchecked_transaction()?;
    let mut removed = 0;
    for id in ids {
        tx.execute(
            "UPDATE txn SET parent_txn_id = NULL, link_kind = NULL WHERE parent_txn_id = ?1",
            rusqlite::params![id],
        )?;
        tx.execute(
            "INSERT OR IGNORE INTO txn_removed (account_id, dedupe_hash)
             SELECT account_id, dedupe_hash FROM txn WHERE id = ?1",
            rusqlite::params![id],
        )?;
        removed += tx.execute("DELETE FROM txn WHERE id = ?1", rusqlite::params![id])?;
    }
    tx.commit()?;
    Ok(removed)
}

/* ------------------------------------------------------- cash flow ------ */

/// Years that actually have data.
///
/// Derived from the ledger rather than hardcoded, so importing 2027 makes 2027
/// appear in the filters with no code change and no reload.
#[tauri::command]
pub fn txn_years(state: State<'_, AppState>) -> AppResult<Vec<i32>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT DISTINCT CAST(substr(booked_on, 1, 4) AS INTEGER) AS y
             FROM txn ORDER BY y DESC",
        )?;
        Ok(stmt
            .query_map([], |r| r.get::<_, i32>(0))?
            .collect::<Result<Vec<_>, _>>()?)
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PeriodTotals {
    /// "2026-03" for a month, "2026" for a year.
    pub period: String,
    pub income_minor: Money,
    /// Negative.
    pub expense_minor: Money,
    pub net_minor: Money,
}

fn totals_by(
    state: &AppState,
    from: &str,
    to: &str,
    length: usize,
) -> AppResult<Vec<PeriodTotals>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT substr(booked_on, 1, ?3) AS period,
                    COALESCE(SUM(CASE WHEN net_minor > 0 THEN net_minor ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN net_minor < 0 THEN net_minor ELSE 0 END), 0)
             FROM v_txn_net
             WHERE booked_on >= ?1 AND booked_on <= ?2
             GROUP BY period ORDER BY period",
        )?;
        Ok(stmt
            .query_map(rusqlite::params![from, to, length as i64], |r| {
                let income: i64 = r.get(1)?;
                let expense: i64 = r.get(2)?;
                Ok(PeriodTotals {
                    period: r.get(0)?,
                    income_minor: Money::from_minor(income),
                    expense_minor: Money::from_minor(expense),
                    net_minor: Money::from_minor(income + expense),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    })
}

/// Month-by-month income, spending and what is left.
#[tauri::command]
pub fn cashflow_monthly(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> AppResult<Vec<PeriodTotals>> {
    totals_by(&state, &from, &to, 7)
}

/// Year-by-year totals, for the comparison view.
#[tauri::command]
pub fn cashflow_yearly(state: State<'_, AppState>) -> AppResult<Vec<PeriodTotals>> {
    totals_by(&state, "0000-01-01", "9999-12-31", 4)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CashflowSummary {
    pub income_minor: Money,
    pub expense_minor: Money,
    pub net_minor: Money,
    /// Net ÷ income, in basis points. `None` when nothing came in, because a
    /// savings rate on zero income is not a number worth showing.
    pub savings_rate_bps: Option<i64>,
    pub transaction_count: i64,
}

#[tauri::command]
pub fn cashflow_summary(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> AppResult<CashflowSummary> {
    state.db()?.with(|c| {
        let (income, expense, count): (i64, i64, i64) = c.query_row(
            "SELECT COALESCE(SUM(CASE WHEN net_minor > 0 THEN net_minor ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN net_minor < 0 THEN net_minor ELSE 0 END), 0),
                    count(*)
             FROM v_txn_net WHERE booked_on >= ?1 AND booked_on <= ?2",
            rusqlite::params![from, to],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let net = income + expense;
        Ok(CashflowSummary {
            income_minor: Money::from_minor(income),
            expense_minor: Money::from_minor(expense),
            net_minor: Money::from_minor(net),
            savings_rate_bps: (income > 0).then(|| net * 10_000 / income),
            transaction_count: count,
        })
    })
}
