//! Module 3 — net-worth snapshots and the FIRE picture.
//!
//! Snapshots accept any date. The original specification fixed them to June 30
//! and December 31; that is kept as a *prompt*, not a constraint — twenty data
//! points over ten years makes for a jagged chart and a noisy trend, and there
//! is no reason to refuse a valuation taken on a different day.
//!
//! Buckets are rows rather than columns, so adding an asset class later is a
//! row rather than a migration.

use crate::error::{AppError, AppResult};
use crate::fire::{self, Balances, FireParams, FireResult};
use crate::money::Money;
use crate::state::AppState;
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};
use tauri::State;

/* ------------------------------------------------------- asset classes -- */

/// A user-defined asset class.
///
/// `key` is a stable slug that `wealth_line.bucket` stores; `name` is what the
/// user sees and may rename freely. `kind` is the part the arithmetic depends
/// on and is drawn from a fixed set — see `0010_asset_classes.sql`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetClass {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub color: Option<String>,
    pub sort_order: i64,
    /// Whether what is held in it counts toward the FIRE portfolio.
    pub counts_fire: bool,
    /// How many snapshot lines currently use it.
    pub line_count: i64,
}

pub const KINDS: [&str; 4] = ["liquid", "property", "other_asset", "liability"];

#[tauri::command]
pub fn asset_class_list(state: State<'_, AppState>) -> AppResult<Vec<AssetClass>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT k.key, k.name, k.kind, k.color, k.sort_order,
                    (SELECT count(*) FROM wealth_line l WHERE l.bucket = k.key), k.counts_fire
             FROM asset_class k WHERE k.is_archived = 0
             ORDER BY k.sort_order, k.name",
        )?;
        Ok(stmt
            .query_map([], |r| {
                Ok(AssetClass {
                    key: r.get(0)?,
                    name: r.get(1)?,
                    kind: r.get(2)?,
                    color: r.get(3)?,
                    sort_order: r.get(4)?,
                    line_count: r.get(5)?,
                    counts_fire: r.get::<_, i64>(6)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    })
}

/// Turn a name into a slug that is safe as a key and readable in a backup.
fn slugify(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_dash = true;
    for ch in name.chars() {
        let c = ch.to_lowercase().next().unwrap_or(ch);
        if c.is_ascii_alphanumeric() {
            out.push(c);
            last_dash = false;
        } else if !last_dash {
            out.push('_');
            last_dash = true;
        }
    }
    out.trim_matches('_').to_string()
}

/// Create a class, or rename and restyle an existing one.
///
/// `key` is never rewritten on an edit: the lines that reference it would
/// otherwise have to be rewritten too, and a rename is a display change.
#[tauri::command]
pub fn asset_class_save(
    state: State<'_, AppState>,
    key: Option<String>,
    name: String,
    kind: String,
    color: Option<String>,
    counts_fire: bool,
) -> AppResult<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::Invalid("an asset class needs a name".into()));
    }
    if !KINDS.contains(&kind.as_str()) {
        return Err(AppError::Invalid(format!(
            "unknown asset class kind: {kind}"
        )));
    }
    // A debt is never capital to live on, whatever the box says.
    let counts = i64::from(counts_fire && kind != "liability");

    let key = state.db()?.with(|c| match key {
        Some(existing) => {
            let n = c.execute(
                "UPDATE asset_class SET name = ?1, kind = ?2, color = ?3, counts_fire = ?4
                 WHERE key = ?5",
                rusqlite::params![name, kind, color, counts, existing],
            )?;
            if n == 0 {
                return Err(AppError::Invalid(
                    "that asset class no longer exists".into(),
                ));
            }
            Ok(existing)
        }
        None => {
            let base = slugify(&name);
            let base = if base.is_empty() {
                "class".to_string()
            } else {
                base
            };
            // Two classes called "Other" must not collide, and the second one
            // must not silently overwrite the first.
            let mut candidate = base.clone();
            let mut n = 2;
            while c.query_row(
                "SELECT count(*) FROM asset_class WHERE key = ?1",
                rusqlite::params![candidate],
                |r| r.get::<_, i64>(0),
            )? > 0
            {
                candidate = format!("{base}_{n}");
                n += 1;
            }
            c.execute(
                "INSERT INTO asset_class (key, name, kind, color, sort_order, counts_fire)
                 VALUES (?1, ?2, ?3, ?4,
                         (SELECT COALESCE(MAX(sort_order), 0) + 10 FROM asset_class), ?5)",
                rusqlite::params![candidate, name, kind, color, counts],
            )?;
            Ok(candidate)
        }
    })?;
    state.backup.mark_dirty();
    Ok(key)
}

/// Remove a class, moving any snapshot lines using it somewhere else first.
///
/// Same contract as deleting a category: silently stripping the class off
/// historic lines would change what past snapshots mean.
#[tauri::command]
pub fn asset_class_delete(
    state: State<'_, AppState>,
    key: String,
    reassign_to: Option<String>,
) -> AppResult<()> {
    if reassign_to.as_deref() == Some(key.as_str()) {
        return Err(AppError::Invalid(
            "a class cannot be reassigned to itself".into(),
        ));
    }
    state.db()?.with(|c| {
        let tx = c.unchecked_transaction()?;
        let in_use: i64 = tx.query_row(
            "SELECT count(*) FROM wealth_line WHERE bucket = ?1",
            rusqlite::params![key],
            |r| r.get(0),
        )?;
        match reassign_to.as_deref() {
            Some(target) => {
                let exists: i64 = tx.query_row(
                    "SELECT count(*) FROM asset_class WHERE key = ?1",
                    rusqlite::params![target],
                    |r| r.get(0),
                )?;
                if exists == 0 {
                    return Err(AppError::Invalid(
                        "the replacement class does not exist".into(),
                    ));
                }
                tx.execute(
                    "UPDATE wealth_line SET bucket = ?2 WHERE bucket = ?1",
                    rusqlite::params![key, target],
                )?;
            }
            None if in_use > 0 => {
                return Err(AppError::Invalid(format!(
                    "{in_use} snapshot line(s) still use this class — choose a replacement first"
                )));
            }
            None => {}
        }
        tx.execute(
            "DELETE FROM asset_class WHERE key = ?1",
            rusqlite::params![key],
        )?;
        tx.commit()?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

#[tauri::command]
pub fn asset_class_reorder(state: State<'_, AppState>, ordered_keys: Vec<String>) -> AppResult<()> {
    state.db()?.with(|c| {
        let tx = c.unchecked_transaction()?;
        for (i, key) in ordered_keys.iter().enumerate() {
            tx.execute(
                "UPDATE asset_class SET sort_order = ?1 WHERE key = ?2",
                rusqlite::params![(i as i64 + 1) * 10, key],
            )?;
        }
        tx.commit()?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/* ---------------------------------------------------------- snapshots -- */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WealthLine {
    pub id: Option<i64>,
    pub bucket: String,
    pub label: String,
    pub gross_value_minor: i64,
    pub debt_minor: i64,
    pub currency: String,
    /// "manual", or "holding" for a line computed from the portfolio.
    /// Automatic lines are recomputed, never edited.
    #[serde(default = "manual")]
    pub source: String,
}

fn manual() -> String {
    "manual".into()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: i64,
    pub taken_on: String,
    pub label: Option<String>,
    pub is_scheduled: bool,
    pub gross_assets_minor: Money,
    pub total_debt_minor: Money,
    pub net_worth_minor: Money,
    pub liquid_minor: Money,
    pub re_equity_minor: Money,
    pub lines: Vec<WealthLine>,
}

fn load_lines(c: &rusqlite::Connection, snapshot_id: i64) -> rusqlite::Result<Vec<WealthLine>> {
    let mut stmt = c.prepare(
        "SELECT id, bucket, label, gross_value_minor, debt_minor, currency, source
         FROM wealth_line WHERE snapshot_id = ?1 ORDER BY source <> 'manual', bucket, label",
    )?;
    stmt.query_map(rusqlite::params![snapshot_id], |r| {
        Ok(WealthLine {
            id: r.get(0)?,
            bucket: r.get(1)?,
            label: r.get(2)?,
            gross_value_minor: r.get(3)?,
            debt_minor: r.get(4)?,
            currency: r.get(5)?,
            source: r.get(6)?,
        })
    })?
    .collect()
}

#[tauri::command]
pub fn snapshot_list(state: State<'_, AppState>) -> AppResult<Vec<Snapshot>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT s.id, s.taken_on, s.label, s.is_scheduled,
                    v.gross_assets_minor, v.total_debt_minor, v.net_worth_minor,
                    v.liquid_minor, v.re_equity_minor
             FROM wealth_snapshot s
             JOIN v_net_worth v ON v.snapshot_id = s.id
             ORDER BY s.taken_on",
        )?;
        let mut out = stmt
            .query_map([], |r| {
                Ok(Snapshot {
                    id: r.get(0)?,
                    taken_on: r.get(1)?,
                    label: r.get(2)?,
                    is_scheduled: r.get::<_, i64>(3)? != 0,
                    gross_assets_minor: Money::from_minor(r.get::<_, Option<i64>>(4)?.unwrap_or(0)),
                    total_debt_minor: Money::from_minor(r.get::<_, Option<i64>>(5)?.unwrap_or(0)),
                    net_worth_minor: Money::from_minor(r.get::<_, Option<i64>>(6)?.unwrap_or(0)),
                    liquid_minor: Money::from_minor(r.get::<_, Option<i64>>(7)?.unwrap_or(0)),
                    re_equity_minor: Money::from_minor(r.get::<_, Option<i64>>(8)?.unwrap_or(0)),
                    lines: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for s in &mut out {
            s.lines = load_lines(c, s.id)?;
        }
        Ok(out)
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInput {
    pub taken_on: String,
    pub label: Option<String>,
    pub lines: Vec<WealthLine>,
}

/// Create or replace a snapshot. Lines are rewritten wholesale, which keeps
/// "what I entered" and "what is stored" identical rather than trying to merge.
#[tauri::command]
pub fn snapshot_save(
    state: State<'_, AppState>,
    id: Option<i64>,
    input: SnapshotInput,
) -> AppResult<i64> {
    let date = NaiveDate::parse_from_str(&input.taken_on, "%Y-%m-%d")
        .map_err(|_| AppError::Invalid("date must be YYYY-MM-DD".into()))?;
    for l in &input.lines {
        if l.debt_minor < 0 {
            return Err(AppError::Invalid("debt cannot be negative".into()));
        }
    }
    // June 30 and December 31 are the dates the app nudges toward; recording
    // that lets the UI tell a routine snapshot from an ad-hoc one.
    let scheduled =
        (date.month() == 6 && date.day() == 30) || (date.month() == 12 && date.day() == 31);

    let id = state.db()?.with(|c| {
        let tx = c.unchecked_transaction()?;
        // Checked here rather than left to the foreign key, so a stale class in
        // an open editor says which one rather than "FOREIGN KEY constraint
        // failed".
        for l in &input.lines {
            let known: i64 = tx.query_row(
                "SELECT count(*) FROM asset_class WHERE key = ?1",
                rusqlite::params![l.bucket],
                |r| r.get(0),
            )?;
            if known == 0 {
                return Err(AppError::Invalid(format!(
                    "unknown asset class: {}",
                    l.bucket
                )));
            }
        }
        let sid = match id {
            Some(existing) => {
                tx.execute(
                    "UPDATE wealth_snapshot SET taken_on = ?1, label = ?2, is_scheduled = ?3
                     WHERE id = ?4",
                    rusqlite::params![input.taken_on, input.label, i64::from(scheduled), existing],
                )?;
                existing
            }
            None => {
                tx.execute(
                    "INSERT INTO wealth_snapshot (taken_on, label, is_scheduled)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(taken_on) DO UPDATE SET label = excluded.label",
                    rusqlite::params![input.taken_on, input.label, i64::from(scheduled)],
                )?;
                tx.query_row(
                    "SELECT id FROM wealth_snapshot WHERE taken_on = ?1",
                    rusqlite::params![input.taken_on],
                    |r| r.get::<_, i64>(0),
                )?
            }
        };
        tx.execute(
            // Only the hand-typed lines are the editor's to replace; the
            // automatic ones belong to the holdings they were computed from.
            "DELETE FROM wealth_line WHERE snapshot_id = ?1 AND source = 'manual'",
            rusqlite::params![sid],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO wealth_line
                   (snapshot_id, bucket, label, gross_value_minor, debt_minor, currency)
                 VALUES (?1,?2,?3,?4,?5,?6)",
            )?;
            // Automatic lines sent back by the editor are ignored: they are
            // recomputed from their source, not saved as typed.
            for l in input.lines.iter().filter(|l| l.source == "manual") {
                stmt.execute(rusqlite::params![
                    sid,
                    l.bucket,
                    l.label,
                    l.gross_value_minor,
                    l.debt_minor,
                    l.currency,
                ])?;
            }
        }
        // The portfolio's lines are written in the same breath, so a balance
        // is never saved without what the holdings were worth that day.
        super::portfolio::sync_snapshot(&tx, date)?;
        tx.commit()?;
        Ok(sid)
    })?;
    state.backup.mark_dirty();
    Ok(id)
}

/// Rename an item on every date it appears.
///
/// An item is its class and its name; the same flat on five dates is five
/// lines that agree on both. Renaming it on one date only would split its
/// history in two, so a rename is applied to all of them or to none.
#[tauri::command]
pub fn wealth_line_rename(
    state: State<'_, AppState>,
    bucket: String,
    from: String,
    to: String,
) -> AppResult<usize> {
    let to = to.trim().to_string();
    if to.is_empty() {
        return Err(AppError::Invalid("an item needs a name".into()));
    }
    let n = state.db()?.with(|c| {
        Ok(c.execute(
            "UPDATE wealth_line SET label = ?1
             WHERE bucket = ?2 AND label = ?3 AND source = 'manual'",
            rusqlite::params![to, bucket, from],
        )?)
    })?;
    state.backup.mark_dirty();
    Ok(n)
}

#[tauri::command]
pub fn snapshot_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.db()?.with(|c| {
        c.execute(
            "DELETE FROM wealth_snapshot WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/// Is a bi-annual snapshot due? Used for the gentle nudge, never to block.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotPrompt {
    pub due: bool,
    pub suggested_date: String,
    pub label: String,
}

#[tauri::command]
pub fn snapshot_prompt(state: State<'_, AppState>) -> AppResult<SnapshotPrompt> {
    let today = chrono::Local::now().date_naive();
    let year = today.year();
    // The most recent checkpoint that has already passed.
    let (suggested, label) = if today.month() >= 12 && today.day() >= 31 {
        (NaiveDate::from_ymd_opt(year, 12, 31), format!("H2 {year}"))
    } else if today.month() > 6 || (today.month() == 6 && today.day() >= 30) {
        (NaiveDate::from_ymd_opt(year, 6, 30), format!("H1 {year}"))
    } else {
        (
            NaiveDate::from_ymd_opt(year - 1, 12, 31),
            format!("H2 {}", year - 1),
        )
    };
    let suggested = suggested.unwrap_or(today).format("%Y-%m-%d").to_string();

    let exists: i64 = state.db()?.with(|c| {
        Ok(c.query_row(
            "SELECT count(*) FROM wealth_snapshot WHERE taken_on = ?1",
            rusqlite::params![suggested],
            |r| r.get(0),
        )?)
    })?;

    Ok(SnapshotPrompt {
        due: exists == 0,
        suggested_date: suggested,
        label,
    })
}

/* ---------------------------------------------------------------- FIRE -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FireView {
    pub params: FireParams,
    pub balances: Balances,
    pub result: FireResult,
    pub has_snapshot: bool,
}

/// The FIRE picture from the latest snapshot.
///
/// Two figures, never blended: the liquid target the portfolio must cover, and
/// total wealth including property equity. Rental income is subtracted from
/// the spending target, so counting property equity on the assets side as well
/// would be the same asset twice.
#[tauri::command]
pub fn fire_view(state: State<'_, AppState>, params: Option<FireParams>) -> AppResult<FireView> {
    let stored = super::fire_latest_balances(state.clone())?;
    let has_snapshot = stored.is_some();
    let balances = stored.unwrap_or(Balances {
        liquid: Money::ZERO,
        real_estate_equity: Money::ZERO,
        other_assets: Money::ZERO,
        other_debt: Money::ZERO,
    });
    let params = params.unwrap_or_default();
    let result = fire::calculate(&params, &balances)
        .map_err(|e| AppError::Invalid(format!("{} — {}", e[0].field, e[0].message)))?;
    Ok(FireView {
        params,
        balances,
        result,
        has_snapshot,
    })
}

/* ----------------------------------------------------------- the plan -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirePlanView {
    pub params: FireParams,
    pub plan_params: fire::PlanParams,
    pub balances: Balances,
    pub result: FireResult,
    pub plan: fire::Plan,
    pub has_snapshot: bool,
    /// Nothing has been saved yet: the plan is the built-in starting point,
    /// which the page may replace with the person's own country's.
    pub plan_is_new: bool,
    /// Nothing has been saved yet: the figures are the built-in starting
    /// point, which the page replaces with the person's country's.
    pub params_is_new: bool,
}

fn setting_json<T: serde::de::DeserializeOwned>(c: &rusqlite::Connection, key: &str) -> Option<T> {
    c.query_row(
        "SELECT value_json FROM app_setting WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| serde_json::from_str(&v).ok())
}

/// The whole independence picture: where you are, and when you arrive.
///
/// Passing `None` for either parameter set reads what was last saved, so the
/// page opens on the user's own assumptions rather than on a default they
/// already rejected.
#[tauri::command]
pub fn fire_plan(
    state: State<'_, AppState>,
    params: Option<FireParams>,
    plan_params: Option<fire::PlanParams>,
) -> AppResult<FirePlanView> {
    let stored = super::fire_latest_balances(state.clone())?;
    let has_snapshot = stored.is_some();
    let balances = stored.unwrap_or(Balances {
        liquid: Money::ZERO,
        real_estate_equity: Money::ZERO,
        other_assets: Money::ZERO,
        other_debt: Money::ZERO,
    });

    let (saved_params, saved_plan) = state.db()?.with(|c| {
        Ok((
            setting_json::<FireParams>(c, "fire.params"),
            setting_json::<fire::PlanParams>(c, "fire.plan"),
        ))
    })?;

    let plan_is_new = saved_plan.is_none();
    let params_is_new = saved_params.is_none() && params.is_none();
    let params = params.or(saved_params).unwrap_or_default();
    let plan_params = plan_params.or(saved_plan).unwrap_or_default();

    let describe =
        |e: Vec<fire::FireError>| AppError::Invalid(format!("{} — {}", e[0].field, e[0].message));
    let result = fire::calculate(&params, &balances).map_err(describe)?;
    let plan = fire::plan(&params, &balances, &plan_params).map_err(describe)?;

    Ok(FirePlanView {
        params,
        plan_params,
        balances,
        result,
        plan,
        has_snapshot,
        plan_is_new,
        params_is_new,
    })
}

/// Remember the assumptions, so they are there again tomorrow.
#[tauri::command]
pub fn fire_params_save(
    state: State<'_, AppState>,
    params: FireParams,
    plan_params: fire::PlanParams,
) -> AppResult<()> {
    // Validated before storing: a saved set that cannot be calculated would
    // make the page fail to open with no obvious way back.
    let zero = Balances {
        liquid: Money::ZERO,
        real_estate_equity: Money::ZERO,
        other_assets: Money::ZERO,
        other_debt: Money::ZERO,
    };
    fire::plan(&params, &zero, &plan_params)
        .map_err(|e| AppError::Invalid(format!("{} — {}", e[0].field, e[0].message)))?;

    state.db()?.with(|c| {
        let put = |key: &str, value: String| -> rusqlite::Result<()> {
            c.execute(
                "INSERT INTO app_setting (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
                rusqlite::params![key, value],
            )?;
            Ok(())
        };
        put(
            "fire.params",
            serde_json::to_string(&params).unwrap_or_default(),
        )?;
        put(
            "fire.plan",
            serde_json::to_string(&plan_params).unwrap_or_default(),
        )?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/* ------------------------------------------------ from the ledger -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerSuggestion {
    pub from: String,
    pub to: String,
    /// Months in the window that have any transaction at all.
    pub months_with_data: i64,
    /// Both annualised from the months that have data, positive magnitudes.
    pub annual_spending: Money,
    pub annual_income: Money,
    pub annual_saving: Money,
    pub savings_rate_bps: Option<i64>,
}

/// What the ledger says you actually spend and save, for the FIRE page.
///
/// The last twelve complete months, net of refunds (through `v_txn_net`), with
/// transfers and anything filed under a neutral category — internal moves,
/// investment flows — left out, since moving money to a broker is saving, not
/// spending. Fewer than twelve months of history is annualised rather than
/// read as a cheap year.
pub fn ledger_suggestion(
    c: &rusqlite::Connection,
    today: NaiveDate,
) -> AppResult<LedgerSuggestion> {
    let first_of_month = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap_or(today);
    let to = first_of_month.pred_opt().unwrap_or(first_of_month);
    let from = NaiveDate::from_ymd_opt(to.year() - 1, to.month(), 1)
        .and_then(|d| d.checked_add_months(chrono::Months::new(1)))
        .unwrap_or(to);

    let (spend, income, months): (i64, i64, i64) = c.query_row(
        "SELECT COALESCE(SUM(CASE WHEN n.net_minor < 0 THEN -n.net_minor ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN n.net_minor > 0 THEN n.net_minor ELSE 0 END), 0),
                COUNT(DISTINCT substr(n.booked_on, 1, 7))
         FROM v_txn_net n
         LEFT JOIN pl_category k ON k.id = n.effective_category_id
         WHERE n.booked_on >= ?1 AND n.booked_on <= ?2
           AND COALESCE(k.kind, 'expense') <> 'neutral'",
        rusqlite::params![from.to_string(), to.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let annualise = |v: i64| {
        if months == 0 {
            Money::ZERO
        } else {
            Money::from_minor(v * 12 / months)
        }
    };
    let (spend, income) = (annualise(spend), annualise(income));
    let saving = income - spend;
    Ok(LedgerSuggestion {
        from: from.to_string(),
        to: to.to_string(),
        months_with_data: months,
        annual_spending: spend,
        annual_income: income,
        annual_saving: saving,
        savings_rate_bps: (income.minor() > 0).then(|| saving.minor() * 10_000 / income.minor()),
    })
}

#[tauri::command]
pub fn fire_ledger_suggestion(state: State<'_, AppState>) -> AppResult<LedgerSuggestion> {
    let today = chrono::Local::now().date_naive();
    state.db()?.with(|c| ledger_suggestion(c, today))
}
