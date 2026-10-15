//! Budgets and the category x period matrix.
//!
//! Both answer the same question from different directions — "where did the
//! money go, month by month" and "where was it meant to go" — so they read the
//! ledger through the same view (`v_txn_net`) and the same period prefix. A
//! budget line that disagreed with the matrix beside it would be worse than no
//! budget at all.

use crate::error::{AppError, AppResult};
use crate::money::Money;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

/* ------------------------------------------------------------- periods -- */

/// Validate a budget period and say how wide it is.
///
/// `"2026"` covers a year, `"2026-03"` a month. The length is the substr()
/// width used against `booked_on`, which is why nothing else is accepted.
fn period_width(period: &str) -> AppResult<usize> {
    let ok = match period.len() {
        4 => period.chars().all(|c| c.is_ascii_digit()),
        // `get`, not `split_at`: a multi-byte character straddling byte 4
        // is "not a period", not a panic.
        7 => match (period.get(..4), period.get(4..)) {
            (Some(y), Some(m)) => {
                y.chars().all(|c| c.is_ascii_digit())
                    && m.strip_prefix('-')
                        .is_some_and(|mm| matches!(mm.parse::<u32>(), Ok(1..=12)))
            }
            _ => false,
        },
        _ => false,
    };
    if ok {
        Ok(period.len())
    } else {
        Err(AppError::Invalid(format!(
            "a budget period is a year (2026) or a month (2026-03), not {period:?}"
        )))
    }
}

/// The months of a year, in order. Used to lay the matrix out densely: a month
/// with no transactions still gets a column, because a gap in the middle of a
/// year is information.
fn months_of(year: i32) -> Vec<String> {
    (1..=12).map(|m| format!("{year}-{m:02}")).collect()
}

/* -------------------------------------------------------------- matrix -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixRow {
    pub category_id: Option<i64>,
    pub category_name: String,
    pub kind: String,
    pub color: Option<String>,
    /// One entry per period, in the same order as `Matrix::periods`.
    pub cells: Vec<Money>,
    pub counts: Vec<i64>,
    pub total_minor: Money,
    /// The plan for this category over the whole year, if one is set.
    pub budget_minor: Option<Money>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Matrix {
    /// "2026-01" … "2026-12".
    pub periods: Vec<String>,
    pub rows: Vec<MatrixRow>,
    pub income_by_period: Vec<Money>,
    /// Negative.
    pub expense_by_period: Vec<Money>,
    pub net_by_period: Vec<Money>,
    pub income_total: Money,
    pub expense_total: Money,
    pub net_total: Money,
    /// How many of the twelve months have any activity at all. Dividing a
    /// year's spending by 12 when only five months were imported would make
    /// every average wrong by more than half.
    pub active_periods: i64,
}

/// Every category's net, month by month, for one year.
#[tauri::command]
pub fn pl_matrix(state: State<'_, AppState>, year: i32) -> AppResult<Matrix> {
    state.db()?.with(|c| matrix(c, year))
}

/// The body of `pl_matrix`, against a connection, so it can be tested against
/// a real migrated database rather than only through the IPC layer.
pub fn matrix(c: &rusqlite::Connection, year: i32) -> AppResult<Matrix> {
    let periods = months_of(year);
    let index: HashMap<&str, usize> = periods
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), i))
        .collect();
    let from = format!("{year}-01-01");
    let to = format!("{year}-12-31");

    {
        let budgets = year_budgets(c, year)?;

        let mut stmt = c.prepare(
            "SELECT n.effective_category_id, COALESCE(k.name, 'Uncategorised'),
                    COALESCE(k.kind, 'expense'), k.color, k.sort_order,
                    substr(n.booked_on, 1, 7) AS period,
                    SUM(n.net_minor), count(*)
             FROM v_txn_net n
             LEFT JOIN pl_category k ON k.id = n.effective_category_id
             WHERE n.booked_on >= ?1 AND n.booked_on <= ?2
             GROUP BY n.effective_category_id, period",
        )?;

        // (sort_order, name) keeps the matrix in the order the user arranged
        // their categories in, with Uncategorised pinned last.
        struct Acc {
            name: String,
            kind: String,
            color: Option<String>,
            sort: i64,
            cells: Vec<Money>,
            counts: Vec<i64>,
        }
        let mut acc: HashMap<Option<i64>, Acc> = HashMap::new();

        let mut rows = stmt.query(rusqlite::params![from, to])?;
        while let Some(r) = rows.next()? {
            let id: Option<i64> = r.get(0)?;
            let period: String = r.get(5)?;
            let Some(&col) = index.get(period.as_str()) else {
                continue;
            };
            let entry = acc.entry(id).or_insert_with(|| Acc {
                name: String::new(),
                kind: String::new(),
                color: None,
                sort: i64::MAX,
                cells: vec![Money::ZERO; 12],
                counts: vec![0; 12],
            });
            entry.name = r.get(1)?;
            entry.kind = r.get(2)?;
            entry.color = r.get(3)?;
            entry.sort = r.get::<_, Option<i64>>(4)?.unwrap_or(i64::MAX);
            entry.cells[col] += Money::from_minor(r.get(6)?);
            entry.counts[col] += r.get::<_, i64>(7)?;
        }

        let mut out: Vec<MatrixRow> = acc
            .into_iter()
            .map(|(id, a)| MatrixRow {
                total_minor: a.cells.iter().copied().sum(),
                budget_minor: id.and_then(|i| budgets.get(&i).copied()),
                category_id: id,
                category_name: a.name,
                kind: a.kind,
                color: a.color,
                cells: a.cells,
                counts: a.counts,
            })
            .collect();
        out.sort_by(|a, b| {
            // Income first, then expenses, each in the user's own order.
            let rank = |r: &MatrixRow| match (r.category_id, r.kind.as_str()) {
                (None, _) => 3,
                (_, "income") => 0,
                (_, "neutral") => 2,
                _ => 1,
            };
            rank(a)
                .cmp(&rank(b))
                .then_with(|| {
                    b.total_minor
                        .minor()
                        .abs()
                        .cmp(&a.total_minor.minor().abs())
                })
                .then_with(|| a.category_name.cmp(&b.category_name))
        });

        // Rows that a budget exists for but that saw no money this year still
        // belong in the table — "budgeted 1.200, spent nothing" is a result.
        for (cat_id, amount) in &budgets {
            if out.iter().any(|r| r.category_id == Some(*cat_id)) {
                continue;
            }
            if let Some((name, kind, color)) = category_meta(c, *cat_id)? {
                out.push(MatrixRow {
                    category_id: Some(*cat_id),
                    category_name: name,
                    kind,
                    color,
                    cells: vec![Money::ZERO; 12],
                    counts: vec![0; 12],
                    total_minor: Money::ZERO,
                    budget_minor: Some(*amount),
                });
            }
        }

        let mut income = vec![Money::ZERO; 12];
        let mut expense = vec![Money::ZERO; 12];
        for r in &out {
            for (i, v) in r.cells.iter().enumerate() {
                if v.minor() > 0 {
                    income[i] += *v;
                } else {
                    expense[i] += *v;
                }
            }
        }
        let net: Vec<Money> = income.iter().zip(&expense).map(|(i, e)| *i + *e).collect();
        let active = (0..12)
            .filter(|&i| out.iter().any(|r| r.counts[i] > 0))
            .count() as i64;

        Ok(Matrix {
            income_total: income.iter().copied().sum(),
            expense_total: expense.iter().copied().sum(),
            net_total: net.iter().copied().sum(),
            income_by_period: income,
            expense_by_period: expense,
            net_by_period: net,
            active_periods: active,
            periods,
            rows: out,
        })
    }
}

fn category_meta(
    c: &rusqlite::Connection,
    id: i64,
) -> rusqlite::Result<Option<(String, String, Option<String>)>> {
    c.query_row(
        "SELECT name, kind, color FROM pl_category WHERE id = ?1",
        rusqlite::params![id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(other),
    })
}

/// A category's plan for a whole year: the annual figure if one is set,
/// otherwise the twelve monthly ones added up.
fn year_budgets(c: &rusqlite::Connection, year: i32) -> rusqlite::Result<HashMap<i64, Money>> {
    let mut annual: HashMap<i64, Money> = HashMap::new();
    let mut monthly: HashMap<i64, Money> = HashMap::new();

    let mut stmt = c.prepare(
        "SELECT category_id, period, amount_minor FROM budget
         WHERE period = ?1 OR period LIKE ?2",
    )?;
    let mut rows = stmt.query(rusqlite::params![year.to_string(), format!("{year}-%")])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let period: String = r.get(1)?;
        let amount = Money::from_minor(r.get(2)?);
        if period.len() == 4 {
            annual.insert(id, amount);
        } else {
            *monthly.entry(id).or_insert(Money::ZERO) += amount;
        }
    }
    for (id, total) in monthly {
        annual.entry(id).or_insert(total);
    }
    Ok(annual)
}

/* ------------------------------------------------------------- budgets -- */

/// One period of one category's plan: a year, or a month of it.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BudgetCell {
    pub period: String,
    /// The plan for this period, or nothing when there is none: a category
    /// budgeted for three months of a year has no plan for the other nine.
    pub amount_minor: Option<Money>,
    /// Whether this figure was written for this period, rather than worked
    /// out from the others.
    pub own: bool,
    /// What the ledger actually did in this period, as a positive magnitude:
    /// money spent for an expense category, money received for an income one.
    pub actual_minor: Money,
    /// Budget minus actual. Negative means over budget for an expense, or
    /// short of target for income — in both cases, the unwelcome direction.
    pub remaining_minor: Money,
    pub txn_count: i64,
    /// Elapsed share of the period, in basis points, as of today. 0 before it
    /// starts, 10000 once it is over.
    pub elapsed_bps: i64,
    pub days_total: i64,
    pub days_left: i64,
}

/// A category's budget for a year, seen both ways at once.
///
/// Some people plan by the year and some by the month, and the two are one
/// plan rather than two. A year's figure with nothing else spreads evenly over
/// its months; a month given its own figure keeps it, and the months without
/// one share what the year has left. With no figure for the year, the year is
/// simply what its months add up to.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetPlan {
    pub category_id: i64,
    pub category_name: String,
    pub kind: String,
    pub color: Option<String>,
    /// How many months carry a figure of their own.
    pub months_set: i64,
    /// What the twelve months come to. Equal to the year's figure unless the
    /// months written by hand already exceed it.
    pub months_sum_minor: Money,
    pub year: BudgetCell,
    /// January to December.
    pub months: Vec<BudgetCell>,
}

/// Inclusive calendar bounds of a period.
fn period_bounds(period: &str) -> AppResult<(chrono::NaiveDate, chrono::NaiveDate)> {
    use chrono::NaiveDate;
    let width = period_width(period)?;
    let invalid = || AppError::Invalid(format!("{period:?} is not a real period"));
    if width == 4 {
        let y: i32 = period.parse().map_err(|_| invalid())?;
        Ok((
            NaiveDate::from_ymd_opt(y, 1, 1).ok_or_else(invalid)?,
            NaiveDate::from_ymd_opt(y, 12, 31).ok_or_else(invalid)?,
        ))
    } else {
        let y: i32 = period[..4].parse().map_err(|_| invalid())?;
        let m: u32 = period[5..].parse().map_err(|_| invalid())?;
        let start = NaiveDate::from_ymd_opt(y, m, 1).ok_or_else(invalid)?;
        let next = if m == 12 {
            NaiveDate::from_ymd_opt(y + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(y, m + 1, 1)
        }
        .ok_or_else(invalid)?;
        Ok((start, next.pred_opt().ok_or_else(invalid)?))
    }
}

fn cell(
    period: String,
    amount: Option<Money>,
    own: bool,
    actual: Money,
    txn_count: i64,
    today: chrono::NaiveDate,
) -> AppResult<BudgetCell> {
    let (start, end) = period_bounds(&period)?;
    let days_total = (end - start).num_days() + 1;
    let elapsed_days = (today - start).num_days() + 1;
    Ok(BudgetCell {
        period,
        amount_minor: amount,
        own,
        actual_minor: actual,
        remaining_minor: amount.unwrap_or(Money::ZERO) - actual,
        txn_count,
        elapsed_bps: (elapsed_days.clamp(0, days_total) * 10_000) / days_total.max(1),
        days_total,
        days_left: (days_total - elapsed_days.max(0)).max(0),
    })
}

/// Every category budgeted in a year, with the year and its twelve months.
#[tauri::command]
pub fn budget_plan(state: State<'_, AppState>, year: i32) -> AppResult<Vec<BudgetPlan>> {
    let today = chrono::Local::now().date_naive();
    state.db()?.with(|c| plan(c, year, today))
}

/// `today` is a parameter rather than read from the clock so that pacing —
/// the part of a budget line most likely to be wrong — can be tested.
pub fn plan(
    c: &rusqlite::Connection,
    year: i32,
    today: chrono::NaiveDate,
) -> AppResult<Vec<BudgetPlan>> {
    struct Raw {
        name: String,
        kind: String,
        color: Option<String>,
        annual: Option<Money>,
        months: [Option<Money>; 12],
    }
    let mut order: Vec<i64> = Vec::new();
    let mut raw: HashMap<i64, Raw> = HashMap::new();
    {
        let mut stmt = c.prepare(
            "SELECT b.category_id, k.name, k.kind, k.color, b.period, b.amount_minor
             FROM budget b JOIN pl_category k ON k.id = b.category_id
             WHERE b.period = ?1 OR b.period LIKE ?2
             ORDER BY k.sort_order, k.name",
        )?;
        let mut rows = stmt.query(rusqlite::params![year.to_string(), format!("{year}-%")])?;
        while let Some(r) = rows.next()? {
            let id: i64 = r.get(0)?;
            let period: String = r.get(4)?;
            let amount = Money::from_minor(r.get(5)?);
            if !raw.contains_key(&id) {
                order.push(id);
            }
            let entry = raw.entry(id).or_insert(Raw {
                name: r.get(1)?,
                kind: r.get(2)?,
                color: r.get(3)?,
                annual: None,
                months: [None; 12],
            });
            if period.len() == 4 {
                entry.annual = Some(amount);
            } else if let Ok(m @ 1..=12) = period[5..].parse::<usize>() {
                entry.months[m - 1] = Some(amount);
            }
        }
    }

    // What the ledger did, by category and month. The plan is a magnitude, so
    // the actual has to be one too: spending 600 against a 500 budget is 100
    // over, whichever sign the ledger stores it with. The sign is dropped per
    // period, after netting, so a refund still reduces what was spent.
    let mut actual: HashMap<(i64, usize), (i64, i64)> = HashMap::new();
    {
        let mut stmt = c.prepare(
            "SELECT effective_category_id, substr(booked_on, 6, 2),
                    COALESCE(SUM(net_minor), 0), count(*)
             FROM v_txn_net
             WHERE effective_category_id IS NOT NULL AND substr(booked_on, 1, 4) = ?1
             GROUP BY effective_category_id, substr(booked_on, 6, 2)",
        )?;
        let mut rows = stmt.query(rusqlite::params![year.to_string()])?;
        while let Some(r) = rows.next()? {
            let month: String = r.get(1)?;
            if let Ok(m @ 1..=12) = month.parse::<usize>() {
                actual.insert((r.get(0)?, m - 1), (r.get(2)?, r.get(3)?));
            }
        }
    }

    let mut out = Vec::with_capacity(order.len());
    for id in order {
        let Some(r) = raw.remove(&id) else { continue };
        let set: Vec<Money> = r.months.iter().flatten().copied().collect();
        let written: Money = set.iter().copied().sum();

        // The months without a figure share what the year has left, to the
        // cent: the twelve add back up to the year exactly.
        let free = 12 - set.len();
        let mut shares = match r.annual {
            Some(a) if free > 0 => {
                Money::from_minor((a - written).minor().max(0)).split_evenly(free as u32)
            }
            _ => Vec::new(),
        }
        .into_iter();

        let mut months = Vec::with_capacity(12);
        let (mut year_signed, mut year_count) = (0i64, 0i64);
        for (i, own) in r.months.iter().enumerate() {
            let (signed, count) = actual.get(&(id, i)).copied().unwrap_or((0, 0));
            year_signed += signed;
            year_count += count;
            let amount = match own {
                Some(v) => Some(*v),
                None => shares.next(),
            };
            months.push(cell(
                format!("{year}-{:02}", i + 1),
                amount,
                own.is_some(),
                Money::from_minor(signed.abs()),
                count,
                today,
            )?);
        }
        let months_sum: Money = months.iter().filter_map(|m| m.amount_minor).sum();

        out.push(BudgetPlan {
            category_id: id,
            category_name: r.name,
            kind: r.kind,
            color: r.color,
            months_set: set.len() as i64,
            months_sum_minor: months_sum,
            year: cell(
                year.to_string(),
                Some(r.annual.unwrap_or(written)),
                r.annual.is_some(),
                Money::from_minor(year_signed.abs()),
                year_count,
                today,
            )?,
            months,
        });
    }
    Ok(out)
}

/// Write the figure for one period.
///
/// A year's figure written over twelve months that each had their own
/// replaces them: with every month spoken for there would be nothing left for
/// the year to decide, and two totals that disagree.
#[tauri::command]
pub fn budget_save(
    state: State<'_, AppState>,
    category_id: i64,
    period: String,
    amount_minor: i64,
) -> AppResult<()> {
    state
        .db()?
        .with(|c| save(c, category_id, &period, amount_minor))?;
    state.backup.mark_dirty();
    Ok(())
}

pub fn save(
    c: &rusqlite::Connection,
    category_id: i64,
    period: &str,
    amount_minor: i64,
) -> AppResult<()> {
    let width = period_width(period)?;
    if amount_minor < 0 {
        return Err(AppError::Invalid(
            "a budget is a plan to spend or earn, so it cannot be negative".into(),
        ));
    }
    let tx = c.unchecked_transaction()?;
    if width == 4 {
        let months: i64 = tx.query_row(
            "SELECT count(*) FROM budget WHERE category_id = ?1 AND period LIKE ?2",
            rusqlite::params![category_id, format!("{period}-%")],
            |r| r.get(0),
        )?;
        if months >= 12 {
            tx.execute(
                "DELETE FROM budget WHERE category_id = ?1 AND period LIKE ?2",
                rusqlite::params![category_id, format!("{period}-%")],
            )?;
        }
    }
    tx.execute(
        "INSERT INTO budget (category_id, period, amount_minor) VALUES (?1, ?2, ?3)
         ON CONFLICT(category_id, period) DO UPDATE SET amount_minor = excluded.amount_minor",
        rusqlite::params![category_id, period, amount_minor],
    )?;
    tx.commit()?;
    Ok(())
}

/// The same figure for every month of a year: how someone who thinks in
/// months writes a budget. The year then is what the months add up to, so any
/// separate figure for the year is dropped.
#[tauri::command]
pub fn budget_save_every_month(
    state: State<'_, AppState>,
    category_id: i64,
    year: i32,
    amount_minor: i64,
) -> AppResult<()> {
    state
        .db()?
        .with(|c| save_every_month(c, category_id, year, amount_minor))?;
    state.backup.mark_dirty();
    Ok(())
}

pub fn save_every_month(
    c: &rusqlite::Connection,
    category_id: i64,
    year: i32,
    amount_minor: i64,
) -> AppResult<()> {
    if amount_minor < 0 {
        return Err(AppError::Invalid(
            "a budget is a plan to spend or earn, so it cannot be negative".into(),
        ));
    }
    let tx = c.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM budget WHERE category_id = ?1 AND period = ?2",
        rusqlite::params![category_id, year.to_string()],
    )?;
    for period in months_of(year) {
        tx.execute(
            "INSERT INTO budget (category_id, period, amount_minor) VALUES (?1, ?2, ?3)
             ON CONFLICT(category_id, period) DO UPDATE SET amount_minor = excluded.amount_minor",
            rusqlite::params![category_id, period, amount_minor],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Take a figure away.
///
/// A month goes back to following the year (or to having no plan, if the
/// year has none). A year takes its months with it: removing a category's
/// budget for a year means the whole of it.
#[tauri::command]
pub fn budget_remove(
    state: State<'_, AppState>,
    category_id: i64,
    period: String,
) -> AppResult<()> {
    let width = period_width(&period)?;
    state.db()?.with(|c| {
        c.execute(
            "DELETE FROM budget WHERE category_id = ?1 AND period = ?2",
            rusqlite::params![category_id, period],
        )?;
        if width == 4 {
            c.execute(
                "DELETE FROM budget WHERE category_id = ?1 AND period LIKE ?2",
                rusqlite::params![category_id, format!("{period}-%")],
            )?;
        }
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetSeed {
    pub category_id: i64,
    pub amount_minor: i64,
}

/// Create a year's budgets in one go from what was actually spent.
///
/// Last year's outcome is the only honest starting point most people have; the
/// UI offers it pre-filled and the user edits from there.
#[tauri::command]
pub fn budget_seed_from_year(
    state: State<'_, AppState>,
    source_year: i32,
    target_year: i32,
) -> AppResult<Vec<BudgetSeed>> {
    state
        .db()?
        .with(|c| {
            let mut stmt = c.prepare(
                "SELECT effective_category_id, ABS(SUM(net_minor))
             FROM v_txn_net
             WHERE effective_category_id IS NOT NULL
               AND substr(booked_on, 1, 4) = ?1
             GROUP BY effective_category_id
             HAVING ABS(SUM(net_minor)) > 0",
            )?;
            let seeds = stmt
                .query_map(rusqlite::params![source_year.to_string()], |r| {
                    Ok(BudgetSeed {
                        category_id: r.get(0)?,
                        amount_minor: r.get(1)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;

            let tx = c.unchecked_transaction()?;
            for s in &seeds {
                tx.execute(
                    "INSERT INTO budget (category_id, period, amount_minor) VALUES (?1, ?2, ?3)
                 ON CONFLICT(category_id, period) DO NOTHING",
                    rusqlite::params![s.category_id, target_year.to_string(), s.amount_minor],
                )?;
            }
            tx.commit()?;
            Ok(seeds)
        })
        .inspect(|_| state.backup.mark_dirty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_years_and_months_only() {
        assert_eq!(period_width("2026").unwrap(), 4);
        assert_eq!(period_width("2026-03").unwrap(), 7);
        assert!(period_width("2026-13").is_err());
        assert!(period_width("2026-00").is_err());
        assert!(period_width("26-03").is_err());
        assert!(period_width("2026-03-01").is_err());
    }

    #[test]
    fn month_bounds_handle_february_and_december() {
        let (s, e) = period_bounds("2024-02").unwrap();
        assert_eq!(s.to_string(), "2024-02-01");
        assert_eq!(e.to_string(), "2024-02-29");

        let (s, e) = period_bounds("2026-12").unwrap();
        assert_eq!(s.to_string(), "2026-12-01");
        assert_eq!(e.to_string(), "2026-12-31");

        let (s, e) = period_bounds("2026").unwrap();
        assert_eq!(s.to_string(), "2026-01-01");
        assert_eq!(e.to_string(), "2026-12-31");
    }
}
