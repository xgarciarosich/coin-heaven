//! Holdings, recurring buys and prices — and the snapshot lines computed from
//! them.
//!
//! The arithmetic lives in `crate::portfolio`; this file reads and writes the
//! facts it works from. The rule both follow: a number Coin Heaven can derive is
//! derived, never typed twice. A holding's value is units times a price.

use crate::error::{AppError, AppResult};
use crate::money::Money;
use crate::portfolio::{self as pf, Kind, Txn};
use crate::state::AppState;
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use tauri::State;

/// id, holding, amount, every N days, day of month, start, end.
type ScheduleRow = (
    i64,
    i64,
    i64,
    Option<u32>,
    Option<u32>,
    String,
    Option<String>,
);
/// price (µ€), date, source, source URL.
type PriceRow = (i64, String, String, Option<String>);
fn date(label: &str, s: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map_err(|_| AppError::Invalid(format!("{label} must be YYYY-MM-DD")))
}

fn today() -> NaiveDate {
    chrono::Local::now().date_naive()
}

fn class_exists(c: &Connection, key: &str) -> AppResult<()> {
    let n: i64 = c.query_row(
        "SELECT count(*) FROM asset_class WHERE key = ?1",
        params![key],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(AppError::Invalid(format!("unknown asset class: {key}")));
    }
    Ok(())
}

/* ----------------------------------------------------------- schedules -- */

/// Write out every scheduled buy that has fallen due, up to `until`.
///
/// Idempotent: the unique (schedule, date) pair means a buy already written —
/// or skipped — is never written again, so this can run every time the
/// portfolio is read.
pub fn materialise(c: &Connection, until: NaiveDate) -> AppResult<usize> {
    let schedules: Vec<ScheduleRow> = {
        let mut st = c.prepare(
            "SELECT id, holding_id, amount_minor, every_days, day_of_month, start_date, end_date
             FROM holding_schedule",
        )?;
        st.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<Result<_, _>>()?
    };
    let mut written = 0;
    for (id, holding, amount, every, dom, start, end) in schedules {
        let start = date("start date", &start)?;
        let end = end.as_deref().map(|e| date("end date", e)).transpose()?;
        for d in pf::schedule_dates(every, dom, start, end, until) {
            written += c.execute(
                "INSERT OR IGNORE INTO holding_txn
                   (holding_id, on_date, kind, amount_minor, units_e8, schedule_id)
                 VALUES (?1, ?2, 'buy', ?3, NULL, ?4)",
                params![holding, d.to_string(), amount, id],
            )?;
        }
    }
    Ok(written)
}

/* ------------------------------------------------------------ holdings -- */

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    pub id: Option<i64>,
    pub holding_id: i64,
    pub amount_minor: i64,
    pub every_days: Option<u32>,
    pub day_of_month: Option<u32>,
    pub start_date: String,
    pub end_date: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingView {
    pub id: i64,
    pub name: String,
    pub symbol: Option<String>,
    pub asset_class: String,
    pub units_e8: i64,
    /// False when some buy has neither units nor anything to work them out
    /// from, so the count is a floor.
    pub units_complete: bool,
    /// True when the count leans on a worked-out figure: a reconciliation
    /// shared over earlier buys, or an amount divided by a price.
    pub units_estimated: bool,
    pub price_micro: Option<i64>,
    pub price_date: Option<String>,
    pub price_source: Option<String>,
    pub price_url: Option<String>,
    pub value: Option<Money>,
    pub invested: Money,
    /// Value less net cash put in. `None` without a price.
    pub gain: Option<Money>,
    /// Money-weighted annual return, in basis points.
    pub xirr_bps: Option<i64>,
    pub txn_count: i64,
    pub schedules: Vec<Schedule>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioView {
    pub as_of: String,
    pub holdings: Vec<HoldingView>,
    pub value: Money,
    pub invested: Money,
    pub gain: Money,
    pub xirr_bps: Option<i64>,
    /// Holdings left out of `value` for want of a price on or before `as_of`.
    pub unpriced: Vec<String>,
    pub checkpoint: String,
}

fn load_txns(c: &Connection, holding: i64) -> AppResult<Vec<Txn>> {
    let mut st = c.prepare(
        "SELECT on_date, kind, amount_minor, units_e8, skipped
         FROM holding_txn WHERE holding_id = ?1 ORDER BY on_date, id",
    )?;
    let rows: Vec<(String, String, i64, Option<i64>, i64)> = st
        .query_map(params![holding], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(|(d, k, a, u, s)| {
            Ok(Txn {
                on: date("date", &d)?,
                kind: match k.as_str() {
                    "sell" => Kind::Sell,
                    "units" => Kind::Units,
                    _ => Kind::Buy,
                },
                amount: Money::from_minor(a),
                units_e8: u,
                skipped: s != 0,
            })
        })
        .collect()
}

/// Every price recorded for a holding, oldest first.
fn load_prices(c: &Connection, holding: i64) -> AppResult<Vec<(NaiveDate, i64)>> {
    let mut st = c.prepare(
        "SELECT on_date, price_micro FROM holding_price WHERE holding_id = ?1 ORDER BY on_date",
    )?;
    let rows: Vec<(String, i64)> = st
        .query_map(params![holding], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    rows.into_iter()
        .map(|(d, p)| Ok((date("date", &d)?, p)))
        .collect()
}

/// The dates of movements known only by their amount: the ones a price turns
/// into units, and so the ones that need a price on their own day.
pub fn blind_dates(c: &Connection, holding: i64) -> AppResult<Vec<NaiveDate>> {
    let mut dates: Vec<NaiveDate> = load_txns(c, holding)?
        .iter()
        .filter(|t| !t.skipped && t.kind != Kind::Units && t.units_e8.is_none())
        .map(|t| t.on)
        .collect();
    dates.sort();
    dates.dedup();
    Ok(dates)
}

/// The month-end dates a price lookup should ask about for one holding.
///
/// Only a holding with buys or sells recorded by their cost alone needs
/// them: those are the movements a price turns into units. One that has its
/// units throughout is valued exactly already.
pub fn history_dates_wanted(
    c: &Connection,
    holding: i64,
    today: NaiveDate,
    cap: usize,
) -> AppResult<Vec<NaiveDate>> {
    let txns = load_txns(c, holding)?;
    let blind = txns
        .iter()
        .any(|t| !t.skipped && t.kind != Kind::Units && t.units_e8.is_none());
    let Some(first) = txns.iter().filter(|t| !t.skipped).map(|t| t.on).min() else {
        return Ok(Vec::new());
    };
    if !blind {
        return Ok(Vec::new());
    }
    let priced: Vec<NaiveDate> = load_prices(c, holding)?.into_iter().map(|p| p.0).collect();
    Ok(pf::month_ends_to_price(first, &priced, today, cap))
}

/// Latest price on or before `d`.
fn price_on_or_before(c: &Connection, holding: i64, d: NaiveDate) -> AppResult<Option<PriceRow>> {
    Ok(c.query_row(
        "SELECT price_micro, on_date, source, source_url FROM holding_price
         WHERE holding_id = ?1 AND on_date <= ?2 ORDER BY on_date DESC LIMIT 1",
        params![holding, d.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .optional()?)
}

fn cash_flows(txns: &[Txn], d: NaiveDate) -> Vec<(NaiveDate, f64)> {
    txns.iter()
        .filter(|t| t.on <= d && !t.skipped)
        .filter_map(|t| match t.kind {
            Kind::Buy => Some((t.on, -t.amount.as_major_f64())),
            Kind::Sell => Some((t.on, t.amount.as_major_f64())),
            Kind::Units => None,
        })
        .collect()
}

pub fn portfolio_at(c: &Connection, as_of: NaiveDate) -> AppResult<PortfolioView> {
    // Due buys are written up to today whatever date is being valued, so a
    // past valuation and today's read the same history.
    materialise(c, today())?;
    let heads: Vec<(i64, String, Option<String>, String)> = {
        let mut st = c.prepare(
            "SELECT id, name, symbol, asset_class FROM holding WHERE is_archived = 0 ORDER BY name",
        )?;
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<_, _>>()?
    };

    let mut holdings = Vec::with_capacity(heads.len());
    let mut all_flows = Vec::new();
    let (mut value, mut invested) = (Money::ZERO, Money::ZERO);
    let mut unpriced = Vec::new();

    for (id, name, symbol, class) in heads {
        let (txns, guessed) = pf::fill_units(&load_txns(c, id)?, &load_prices(c, id)?);
        let units = pf::units_at(&txns, as_of);
        let price = price_on_or_before(c, id, as_of)?;
        let val = price.as_ref().map(|p| pf::value(units.e8, p.0));
        let inv = pf::contributions_at(&txns, as_of);
        let mut flows = cash_flows(&txns, as_of);
        let xirr = val.and_then(|v| {
            flows.push((as_of, v.as_major_f64()));
            pf::xirr(&flows)
        });
        all_flows.extend(cash_flows(&txns, as_of));
        match val {
            Some(v) => value += v,
            None if units.e8 > 0 || !inv.is_zero() => unpriced.push(name.clone()),
            None => {}
        }
        invested += inv;

        let schedules = {
            let mut st = c.prepare(
                "SELECT id, holding_id, amount_minor, every_days, day_of_month, start_date, end_date
                 FROM holding_schedule WHERE holding_id = ?1 ORDER BY start_date",
            )?;
            st.query_map(params![id], |r| {
                Ok(Schedule {
                    id: r.get(0)?,
                    holding_id: r.get(1)?,
                    amount_minor: r.get(2)?,
                    every_days: r.get(3)?,
                    day_of_month: r.get(4)?,
                    start_date: r.get(5)?,
                    end_date: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
        };

        holdings.push(HoldingView {
            id,
            name,
            symbol,
            asset_class: class,
            units_e8: units.e8,
            units_complete: units.complete,
            units_estimated: pf::estimated_at(&txns, &guessed, as_of),
            price_micro: price.as_ref().map(|p| p.0),
            price_date: price.as_ref().map(|p| p.1.clone()),
            price_source: price.as_ref().map(|p| p.2.clone()),
            price_url: price.as_ref().and_then(|p| p.3.clone()),
            gain: val.map(|v| v - inv),
            value: val,
            invested: inv,
            xirr_bps: xirr.map(|r| (r * 10_000.0).round() as i64),
            txn_count: txns.iter().filter(|t| !t.skipped).count() as i64,
            schedules,
        });
    }

    // The portfolio's own return only counts what is priced, or a missing
    // price would read as a total loss.
    let priced_flows: Vec<(NaiveDate, f64)> = if unpriced.is_empty() {
        let mut f = all_flows;
        f.push((as_of, value.as_major_f64()));
        f
    } else {
        Vec::new()
    };

    Ok(PortfolioView {
        as_of: as_of.to_string(),
        gain: value - invested,
        xirr_bps: pf::xirr(&priced_flows).map(|r| (r * 10_000.0).round() as i64),
        value,
        invested,
        unpriced,
        holdings,
        checkpoint: pf::last_checkpoint(today()).to_string(),
    })
}

#[tauri::command]
pub fn portfolio_view(
    state: State<'_, AppState>,
    as_of: Option<String>,
) -> AppResult<PortfolioView> {
    let as_of = match as_of {
        Some(d) => date("date", &d)?,
        None => today(),
    };
    let v = state.db()?.with(|c| portfolio_at(c, as_of))?;
    state.backup.mark_dirty();
    Ok(v)
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPoint {
    pub date: String,
    pub value: Money,
    pub invested: Money,
}

/// What the portfolio was worth, and what had gone into it, through time.
///
/// One point per month end, plus every date a price was recorded and today —
/// the dates on which the value can actually have been seen to change. A
/// holding is valued at its latest price on or before each date; before its
/// first price it is carried at cost, which is the only figure there is and
/// keeps the line from starting at zero and leaping.
pub fn portfolio_history(c: &Connection, until: NaiveDate) -> AppResult<Vec<HistoryPoint>> {
    materialise(c, today())?;
    let ids: Vec<i64> = {
        let mut st = c.prepare("SELECT id FROM holding WHERE is_archived = 0")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    let mut books = Vec::with_capacity(ids.len());
    for id in ids {
        let prices = load_prices(c, id)?;
        let (txns, _) = pf::fill_units(&load_txns(c, id)?, &prices);
        books.push((txns, prices));
    }
    let Some(first) = books
        .iter()
        .flat_map(|(t, _)| t.iter().filter(|t| !t.skipped).map(|t| t.on))
        .min()
    else {
        return Ok(Vec::new());
    };

    let mut dates = vec![first, until];
    let (mut y, mut m) = (first.year(), first.month());
    loop {
        (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        let Some(end) = NaiveDate::from_ymd_opt(y, m, 1).map(|d| d - Duration::days(1)) else {
            break;
        };
        if end >= until {
            break;
        }
        dates.push(end);
    }
    for (_, prices) in &books {
        dates.extend(prices.iter().map(|p| p.0));
    }
    dates.retain(|d| *d >= first && *d <= until);
    dates.sort_unstable();
    dates.dedup();

    Ok(dates
        .into_iter()
        .map(|d| {
            let (mut value, mut invested) = (Money::ZERO, Money::ZERO);
            for (txns, prices) in &books {
                let inv = pf::contributions_at(txns, d);
                invested += inv;
                value += match prices.iter().rfind(|p| p.0 <= d) {
                    Some(p) => pf::value(pf::units_at(txns, d).e8, p.1),
                    None => inv,
                };
            }
            HistoryPoint {
                date: d.to_string(),
                value,
                invested,
            }
        })
        .collect())
}

#[tauri::command]
pub fn portfolio_history_view(state: State<'_, AppState>) -> AppResult<Vec<HistoryPoint>> {
    state.db()?.with(|c| portfolio_history(c, today()))
}

/// Bring the portfolio lines of every recorded balance back in line with the
/// portfolio. Called after anything that changes what a holding was worth, so
/// no screen needs an "update" button and no balance shows a stale value.
pub fn resync_all(c: &Connection) -> AppResult<()> {
    let dates: Vec<String> = {
        let mut st = c.prepare("SELECT taken_on FROM wealth_snapshot")?;
        st.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    for d in dates {
        sync_snapshot(c, date("date", &d)?)?;
    }
    Ok(())
}

#[tauri::command]
pub fn holding_save(
    state: State<'_, AppState>,
    id: Option<i64>,
    name: String,
    symbol: Option<String>,
    asset_class: String,
) -> AppResult<i64> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(AppError::Invalid("a holding needs a name".into()));
    }
    let symbol = symbol
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let id = state.db()?.with(|c| {
        class_exists(c, &asset_class)?;
        match id {
            Some(existing) => {
                c.execute(
                    "UPDATE holding SET name = ?1, symbol = ?2, asset_class = ?3 WHERE id = ?4",
                    params![name, symbol, asset_class, existing],
                )?;
                Ok(existing)
            }
            None => {
                c.execute(
                    "INSERT INTO holding (name, symbol, asset_class) VALUES (?1, ?2, ?3)",
                    params![name, symbol, asset_class],
                )?;
                Ok(c.last_insert_rowid())
            }
        }
    })?;
    state.backup.mark_dirty();
    Ok(id)
}

#[tauri::command]
pub fn holding_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.db()?.with(|c| {
        c.execute("DELETE FROM holding WHERE id = ?1", params![id])?;
        // Its snapshot lines go with it; a value with no holding behind it
        // could never be recomputed.
        c.execute(
            "DELETE FROM wealth_line WHERE source = 'holding' AND source_id = ?1",
            params![id],
        )?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/* ---------------------------------------------------------- transactions -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingTxn {
    pub id: i64,
    pub on_date: String,
    pub kind: String,
    pub amount_minor: i64,
    pub units_e8: Option<i64>,
    pub scheduled: bool,
    pub skipped: bool,
}

#[tauri::command]
pub fn holding_txns(state: State<'_, AppState>, holding_id: i64) -> AppResult<Vec<HoldingTxn>> {
    state.db()?.with(|c| {
        materialise(c, today())?;
        let mut st = c.prepare(
            "SELECT id, on_date, kind, amount_minor, units_e8, schedule_id IS NOT NULL, skipped
             FROM holding_txn WHERE holding_id = ?1 ORDER BY on_date DESC, id DESC",
        )?;
        Ok(st
            .query_map(params![holding_id], |r| {
                Ok(HoldingTxn {
                    id: r.get(0)?,
                    on_date: r.get(1)?,
                    kind: r.get(2)?,
                    amount_minor: r.get(3)?,
                    units_e8: r.get(4)?,
                    scheduled: r.get(5)?,
                    skipped: r.get::<_, i64>(6)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    })
}

#[tauri::command]
pub fn holding_txn_save(
    state: State<'_, AppState>,
    id: Option<i64>,
    holding_id: i64,
    on_date: String,
    kind: String,
    amount_minor: i64,
    units_e8: Option<i64>,
) -> AppResult<i64> {
    date("date", &on_date)?;
    if !matches!(kind.as_str(), "buy" | "sell" | "units") {
        return Err(AppError::Invalid(format!(
            "unknown holding movement: {kind}"
        )));
    }
    if amount_minor < 0 || units_e8.is_some_and(|u| u < 0) {
        return Err(AppError::Invalid(
            "amounts and units cannot be negative".into(),
        ));
    }
    if kind == "units" && units_e8.is_none() {
        return Err(AppError::Invalid(
            "a reconciliation needs the number of units held".into(),
        ));
    }
    let id = state.db()?.with(|c| match id {
        Some(existing) => {
            c.execute(
                "UPDATE holding_txn SET on_date = ?1, kind = ?2, amount_minor = ?3, units_e8 = ?4,
                        skipped = 0
                 WHERE id = ?5",
                params![on_date, kind, amount_minor, units_e8, existing],
            )?;
            Ok(existing)
        }
        None => {
            c.execute(
                "INSERT INTO holding_txn (holding_id, on_date, kind, amount_minor, units_e8)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![holding_id, on_date, kind, amount_minor, units_e8],
            )?;
            Ok(c.last_insert_rowid())
        }
    })?;
    state.db()?.with(resync_all)?;
    state.backup.mark_dirty();
    Ok(id)
}

/// Remove a movement. A scheduled buy is marked skipped instead, or the
/// schedule would simply write it again.
#[tauri::command]
pub fn holding_txn_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.db()?.with(|c| {
        let scheduled: bool = c.query_row(
            "SELECT schedule_id IS NOT NULL FROM holding_txn WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        if scheduled {
            c.execute(
                "UPDATE holding_txn SET skipped = 1 WHERE id = ?1",
                params![id],
            )?;
        } else {
            c.execute("DELETE FROM holding_txn WHERE id = ?1", params![id])?;
        }
        resync_all(c)
    })?;
    state.backup.mark_dirty();
    Ok(())
}

#[tauri::command]
pub fn schedule_save(state: State<'_, AppState>, schedule: Schedule) -> AppResult<i64> {
    let start = date("start date", &schedule.start_date)?;
    if let Some(e) = schedule.end_date.as_deref().filter(|e| !e.is_empty()) {
        if date("end date", e)? < start {
            return Err(AppError::Invalid(
                "a schedule cannot end before it starts".into(),
            ));
        }
    }
    if schedule.amount_minor <= 0 {
        return Err(AppError::Invalid("a recurring buy needs an amount".into()));
    }
    if schedule.every_days.is_some() == schedule.day_of_month.is_some() {
        return Err(AppError::Invalid(
            "a recurring buy repeats every so many days or on a day of the month — one of the two"
                .into(),
        ));
    }
    let end = schedule.end_date.clone().filter(|e| !e.is_empty());
    let id = state.db()?.with(|c| {
        let id = match schedule.id {
            Some(existing) => {
                c.execute(
                    "UPDATE holding_schedule SET amount_minor = ?1, every_days = ?2,
                            day_of_month = ?3, start_date = ?4, end_date = ?5
                     WHERE id = ?6",
                    params![
                        schedule.amount_minor,
                        schedule.every_days,
                        schedule.day_of_month,
                        schedule.start_date,
                        end,
                        existing
                    ],
                )?;
                // Buys already written under the old terms, and not yet
                // touched by hand, are rewritten under the new ones.
                c.execute(
                    "DELETE FROM holding_txn WHERE schedule_id = ?1 AND units_e8 IS NULL
                       AND skipped = 0",
                    params![existing],
                )?;
                existing
            }
            None => {
                c.execute(
                    "INSERT INTO holding_schedule
                       (holding_id, amount_minor, every_days, day_of_month, start_date, end_date)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        schedule.holding_id,
                        schedule.amount_minor,
                        schedule.every_days,
                        schedule.day_of_month,
                        schedule.start_date,
                        end
                    ],
                )?;
                c.last_insert_rowid()
            }
        };
        materialise(c, today())?;
        resync_all(c)?;
        Ok(id)
    })?;
    state.backup.mark_dirty();
    Ok(id)
}

/// Stop a recurring buy. The buys it already wrote are history and stay.
#[tauri::command]
pub fn schedule_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.db()?.with(|c| {
        c.execute("DELETE FROM holding_schedule WHERE id = ?1", params![id])?;
        resync_all(c)
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/* --------------------------------------------------------------- prices -- */

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PriceInput {
    pub holding_id: i64,
    pub on_date: String,
    pub price_micro: i64,
    pub source: String,
    pub source_url: Option<String>,
}

/// Record prices, then bring every recorded balance up to date with them.
///
/// A price is a fact about a date, not about a balance: saving today's price
/// does not create a balance for today, it only revalues the ones on or after
/// the price's date.
#[tauri::command]
pub fn prices_save(state: State<'_, AppState>, prices: Vec<PriceInput>) -> AppResult<usize> {
    for p in &prices {
        date("date", &p.on_date)?;
        if p.price_micro < 0 {
            return Err(AppError::Invalid("a price cannot be negative".into()));
        }
        if p.source != "manual" && p.price_micro == 0 {
            return Err(AppError::Invalid("a looked-up price cannot be zero".into()));
        }
        if !matches!(p.source.as_str(), "manual" | "assistant" | "market") {
            return Err(AppError::Invalid(format!(
                "unknown price source: {}",
                p.source
            )));
        }
    }
    state.db()?.with(|c| {
        let tx = c.unchecked_transaction()?;
        for p in &prices {
            tx.execute(
                "INSERT INTO holding_price (holding_id, on_date, price_micro, source, source_url)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(holding_id, on_date) DO UPDATE SET
                   price_micro = excluded.price_micro, source = excluded.source,
                   source_url = excluded.source_url,
                   recorded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
                 -- a price the person typed is theirs: only they replace it
                 WHERE holding_price.source <> 'manual' OR excluded.source = 'manual'",
                params![
                    p.holding_id,
                    p.on_date,
                    p.price_micro,
                    p.source,
                    p.source_url
                ],
            )?;
        }
        resync_all(&tx)?;
        tx.commit()?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(prices.len())
}

/* ------------------------------------------------------------- the sync -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub snapshot_id: i64,
    pub taken_on: String,
    pub lines: usize,
    /// Holdings with units but no price on or before the date.
    pub unpriced: Vec<String>,
    /// Holdings whose unit count is only a floor — some buy says its amount
    /// and there is nothing to work its units out from.
    pub incomplete: Vec<String>,
}

/// Recompute the portfolio lines of the snapshot on `d`, creating the
/// snapshot if there is none. Hand-typed lines are left exactly as they are.
pub fn sync_snapshot(c: &Connection, d: NaiveDate) -> AppResult<SyncReport> {
    let scheduled = (d.month() == 6 && d.day() == 30) || (d.month() == 12 && d.day() == 31);
    c.execute(
        "INSERT INTO wealth_snapshot (taken_on, is_scheduled) VALUES (?1, ?2)
         ON CONFLICT(taken_on) DO NOTHING",
        params![d.to_string(), i64::from(scheduled)],
    )?;
    let sid: i64 = c.query_row(
        "SELECT id FROM wealth_snapshot WHERE taken_on = ?1",
        params![d.to_string()],
        |r| r.get(0),
    )?;
    c.execute(
        "DELETE FROM wealth_line WHERE snapshot_id = ?1 AND source <> 'manual'",
        params![sid],
    )?;

    // Lines are in the person's own currency, not a hard-coded one.
    let cur: String = c
        .query_row(
            "SELECT value_json FROM app_setting WHERE key = 'ui.currency.primary'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|j| serde_json::from_str::<String>(&j).ok())
        .filter(|code| crate::currency::is_supported(code))
        .unwrap_or_else(|| crate::currency::DEFAULT.to_string());
    let mut lines = 0;
    let mut insert = |class: &str,
                      label: &str,
                      gross: i64,
                      debt: i64,
                      source: &str,
                      id: i64|
     -> AppResult<()> {
        c.execute(
            "INSERT INTO wealth_line
               (snapshot_id, bucket, label, gross_value_minor, debt_minor, currency, source, source_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?8, ?6, ?7)",
            params![sid, class, label, gross, debt, source, id, cur],
        )?;
        lines += 1;
        Ok(())
    };

    // Holdings.
    let pv = portfolio_at(c, d)?;
    let mut incomplete = Vec::new();
    for h in &pv.holdings {
        if !h.units_complete && h.invested.minor() > 0 {
            incomplete.push(h.name.clone());
        }
        if let Some(v) = h.value.filter(|v| v.minor() > 0) {
            insert(&h.asset_class, &h.name, v.minor(), 0, "holding", h.id)?;
        }
    }

    Ok(SyncReport {
        snapshot_id: sid,
        taken_on: d.to_string(),
        lines,
        unpriced: pv.unpriced,
        incomplete,
    })
}

/// Share of the portfolio's value that is gain, for the FIRE tax estimate.
#[tauri::command]
pub fn portfolio_gain_share(state: State<'_, AppState>) -> AppResult<Option<i64>> {
    let pv = state.db()?.with(|c| portfolio_at(c, today()))?;
    Ok((pv.value.minor() > 0 && pv.unpriced.is_empty())
        .then(|| (pv.gain.minor().max(0) * 10_000 / pv.value.minor()).min(10_000)))
}
