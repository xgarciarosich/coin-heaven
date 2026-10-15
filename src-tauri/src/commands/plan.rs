//! The monthly plan: boxes of income and spending, in the person's own order,
//! and what is left.
//!
//! Everything about a month is saved in one call and read back in one call.
//! The list is short (a month's worth of boxes), and rewriting it whole keeps
//! "what is on screen" and "what is stored" the same thing — the order
//! included — instead of merging edits.

use crate::error::{AppError, AppResult};
use crate::money::Money;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

/// More boxes than anyone has in a month; a guard against a runaway client,
/// not a limit anyone should meet.
const MAX_ITEMS: usize = 200;
const MAX_NAME_CHARS: usize = 80;
/// A trillion, in cents. Large enough for anything real, small enough that
/// twelve months of two hundred boxes cannot overflow.
const MAX_AMOUNT_MINOR: i64 = 100_000_000_000_000;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    /// "income" or "expense".
    pub kind: String,
    pub name: String,
    /// Always positive; the kind decides the direction.
    pub amount_minor: i64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MonthPlan {
    /// "2026-10".
    pub period: String,
    /// In the order the person arranged them.
    pub items: Vec<PlanItem>,
    pub income_minor: Money,
    pub expense_minor: Money,
    pub net_minor: Money,
}

fn check_period(period: &str) -> AppResult<()> {
    let ok = period.len() == 7
        && period.as_bytes()[4] == b'-'
        && period[..4].chars().all(|c| c.is_ascii_digit())
        && matches!(period[5..].parse::<u32>(), Ok(1..=12));
    if ok {
        Ok(())
    } else {
        Err(AppError::Invalid(format!(
            "a plan month looks like 2026-03, not {period:?}"
        )))
    }
}

/// Every month that has a plan, oldest first.
#[tauri::command]
pub fn plan_list(state: State<'_, AppState>) -> AppResult<Vec<MonthPlan>> {
    state.db()?.with(list)
}

pub fn list(c: &rusqlite::Connection) -> AppResult<Vec<MonthPlan>> {
    let mut plans: Vec<MonthPlan> = Vec::new();
    let mut stmt = c.prepare(
        "SELECT m.period, i.kind, i.name, i.amount_minor
         FROM plan_month m
         LEFT JOIN plan_item i ON i.period = m.period
         ORDER BY m.period, i.sort_order, i.id",
    )?;
    let mut rows = stmt.query([])?;
    while let Some(r) = rows.next()? {
        let period: String = r.get(0)?;
        if plans.last().is_none_or(|p| p.period != period) {
            plans.push(MonthPlan {
                period,
                items: Vec::new(),
                income_minor: Money::ZERO,
                expense_minor: Money::ZERO,
                net_minor: Money::ZERO,
            });
        }
        // A month with no boxes yet comes back from the join as one row of
        // NULLs.
        let Some(kind) = r.get::<_, Option<String>>(1)? else {
            continue;
        };
        let amount: i64 = r.get(3)?;
        let p = plans.last_mut().expect("pushed above");
        if kind == "income" {
            p.income_minor += Money::from_minor(amount);
        } else {
            p.expense_minor += Money::from_minor(amount);
        }
        p.items.push(PlanItem {
            kind,
            name: r.get(2)?,
            amount_minor: amount,
        });
    }
    for p in &mut plans {
        p.net_minor = p.income_minor - p.expense_minor;
    }
    Ok(plans)
}

/// Write a month's plan, replacing whatever was there. The array's order is
/// the order the boxes are kept in.
#[tauri::command]
pub fn plan_save(
    state: State<'_, AppState>,
    period: String,
    items: Vec<PlanItem>,
) -> AppResult<()> {
    state.db()?.with(|c| save(c, &period, &items))?;
    state.backup.mark_dirty();
    Ok(())
}

pub fn save(c: &rusqlite::Connection, period: &str, items: &[PlanItem]) -> AppResult<()> {
    check_period(period)?;
    if items.len() > MAX_ITEMS {
        return Err(AppError::Invalid(format!(
            "a month's plan can hold at most {MAX_ITEMS} boxes"
        )));
    }
    for it in items {
        if !matches!(it.kind.as_str(), "income" | "expense") {
            return Err(AppError::Invalid(format!(
                "a box is income or expense, not {:?}",
                it.kind
            )));
        }
        if it.amount_minor < 0 || it.amount_minor > MAX_AMOUNT_MINOR {
            return Err(AppError::Invalid(
                "a box holds an amount of zero or more; whether it is income or spending is its type"
                    .into(),
            ));
        }
    }
    let tx = c.unchecked_transaction()?;
    tx.execute(
        "INSERT OR IGNORE INTO plan_month (period) VALUES (?1)",
        rusqlite::params![period],
    )?;
    tx.execute(
        "DELETE FROM plan_item WHERE period = ?1",
        rusqlite::params![period],
    )?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO plan_item (period, kind, name, amount_minor, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for (i, it) in items.iter().enumerate() {
            let name: String = it.name.trim().chars().take(MAX_NAME_CHARS).collect();
            stmt.execute(rusqlite::params![
                period,
                it.kind,
                name,
                it.amount_minor,
                i as i64
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Remove a month's plan, with its boxes.
#[tauri::command]
pub fn plan_delete(state: State<'_, AppState>, period: String) -> AppResult<()> {
    state.db()?.with(|c| delete(c, &period))?;
    state.backup.mark_dirty();
    Ok(())
}

pub fn delete(c: &rusqlite::Connection, period: &str) -> AppResult<()> {
    check_period(period)?;
    c.execute(
        "DELETE FROM plan_month WHERE period = ?1",
        rusqlite::params![period],
    )?;
    Ok(())
}
