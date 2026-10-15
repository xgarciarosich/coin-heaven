//! A made-up household, so the app can be looked at before anyone has
//! brought a statement to it.
//!
//! Everything here is invented and deterministic. It is only offered to a
//! database with nothing in it, and everything it creates is recorded in a
//! manifest so it can be removed again without touching a thing the person
//! has added since.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

const MANIFEST: &str = "sample.manifest";
const MONTHS: i32 = 14;

#[derive(Serialize, Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    account_id: i64,
    snapshot_ids: Vec<i64>,
    holding_ids: Vec<i64>,
    plan_months: Vec<String>,
    budget_ids: Vec<i64>,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SampleStatus {
    /// Sample data is in the database now.
    pub loaded: bool,
    /// The database is empty, so loading would not mix it with real data.
    pub can_load: bool,
}

fn count(c: &rusqlite::Connection, table: &str) -> AppResult<i64> {
    Ok(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?)
}

fn manifest(c: &rusqlite::Connection) -> AppResult<Option<Manifest>> {
    let raw: Option<String> = c
        .query_row(
            "SELECT value_json FROM app_setting WHERE key = ?1",
            params![MANIFEST],
            |r| r.get(0),
        )
        .ok();
    Ok(raw.and_then(|s| serde_json::from_str(&s).ok()))
}

pub fn status(c: &rusqlite::Connection) -> AppResult<SampleStatus> {
    let loaded = manifest(c)?.is_some();
    let empty = [
        "txn",
        "account",
        "wealth_snapshot",
        "holding",
        "plan_month",
        "budget",
    ]
    .iter()
    .try_fold(true, |acc, t| count(c, t).map(|n| acc && n == 0))?;
    Ok(SampleStatus {
        loaded,
        can_load: !loaded && empty,
    })
}

/// A small deterministic wobble, so amounts look like life without a RNG.
fn wobble(seed: i64, lo: i64, hi: i64) -> i64 {
    let x = (seed.wrapping_mul(1_103_515_245).wrapping_add(12_345)).rem_euclid(2_147_483_648);
    lo + x % (hi - lo + 1)
}

fn add_months(d: NaiveDate, by: i32) -> NaiveDate {
    let idx = d.year() * 12 + d.month0() as i32 + by;
    NaiveDate::from_ymd_opt(idx.div_euclid(12), idx.rem_euclid(12) as u32 + 1, 1).unwrap()
}

fn month_end(first: NaiveDate) -> NaiveDate {
    add_months(first, 1) - Duration::days(1)
}

fn category(c: &rusqlite::Connection, sort_order: i64) -> AppResult<i64> {
    Ok(c.query_row(
        "SELECT id FROM pl_category WHERE sort_order = ?1",
        params![sort_order],
        |r| r.get(0),
    )?)
}

pub fn load(
    c: &rusqlite::Connection,
    today: NaiveDate,
    lang: &str,
    currency: &str,
) -> AppResult<()> {
    if !status(c)?.can_load {
        return Err(AppError::Invalid(
            "sample data can only be added to an empty book".into(),
        ));
    }
    let es = lang == "es";
    let t = |en: &'static str, es_: &'static str| if es { es_ } else { en };
    let exp = crate::currency::lookup(currency).map_or(2, |c| u32::from(c.exponent));
    // Whole currency units to minor units.
    let m = |major: i64| major * 10i64.pow(exp);
    let mc = |cents: i64| cents * 10i64.pow(exp) / 100;

    let tx = c.unchecked_transaction()?;
    let mut man = Manifest::default();

    tx.execute(
        "INSERT INTO account (name, kind, currency) VALUES (?1, 'checking', ?2)",
        params![
            t("Sample current account", "Cuenta corriente de ejemplo"),
            currency
        ],
    )?;
    man.account_id = tx.last_insert_rowid();
    let acct = man.account_id;

    let (salary, housing, food, transport, leisure, health, subs) = (
        category(&tx, 10)?,
        category(&tx, 40)?,
        category(&tx, 50)?,
        category(&tx, 60)?,
        category(&tx, 70)?,
        category(&tx, 80)?,
        category(&tx, 90)?,
    );

    let this = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
    let mut n = 0i64;
    let mut put = |on: NaiveDate, desc: &str, minor: i64, cat: i64| -> rusqlite::Result<()> {
        if on > today {
            return Ok(());
        }
        n += 1;
        tx.execute(
            "INSERT INTO txn (account_id, booked_on, description_raw, description_norm,
                              amount_minor, currency, category_override_id, categorized_by,
                              needs_review, dedupe_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'user', 0, ?8)",
            params![
                acct,
                on.to_string(),
                desc,
                crate::security::pii::normalise_description(desc),
                minor,
                currency,
                cat,
                format!("sample-{n}")
            ],
        )?;
        Ok(())
    };

    for k in (0..MONTHS).rev() {
        let first = add_months(this, -k);
        let day = |d: u32| first.with_day(d.min(28)).unwrap();
        let s = i64::from(k) * 31;

        put(day(28), "ACME PAYROLL", m(2_850), salary)?;
        put(
            day(1),
            t(
                "MAPLE PROPERTY MANAGEMENT RENT",
                "ALQUILER INMOBILIARIA ARCE",
            ),
            -m(1_050),
            housing,
        )?;
        put(
            day(5),
            t("ELECTRA ENERGY", "ELECTRA ENERGIA"),
            -mc(wobble(s, 6_800, 11_500)),
            housing,
        )?;
        put(day(8), "FIBRELINE INTERNET", -mc(3_990), subs)?;
        put(day(3), "STREAMFLIX", -mc(1_299), subs)?;
        put(day(12), "MUSICBOX PLUS", -mc(999), subs)?;
        put(day(15), "FITCLUB GYM", -mc(3_400), health)?;
        put(
            day(2),
            t("CITY TRANSIT PASS", "ABONO TRANSPORTE"),
            -m(40),
            transport,
        )?;
        for (i, d) in [4u32, 9, 14, 19, 24, 27].iter().enumerate() {
            put(
                day(*d),
                t("GREEN MARKET SUPERMARKET", "SUPERMERCADO MERCADO VERDE"),
                -mc(wobble(s + i as i64, 3_400, 9_600)),
                food,
            )?;
        }
        put(
            day(11),
            t("CORNER BAKERY", "PANADERIA LA ESQUINA"),
            -mc(wobble(s + 7, 600, 1_400)),
            food,
        )?;
        put(
            day(10),
            t("FUEL STATION", "GASOLINERA"),
            -mc(wobble(s + 9, 4_200, 6_200)),
            transport,
        )?;
        put(
            day(25),
            t("FUEL STATION", "GASOLINERA"),
            -mc(wobble(s + 11, 4_200, 6_200)),
            transport,
        )?;
        put(
            day(13),
            "BISTRO LUNA",
            -mc(wobble(s + 13, 2_800, 6_500)),
            leisure,
        )?;
        put(
            day(21),
            "CINEMA CENTRAL",
            -mc(wobble(s + 15, 1_800, 2_800)),
            leisure,
        )?;
        if k % 3 == 0 {
            put(
                day(17),
                t("CITY PHARMACY", "FARMACIA CENTRAL"),
                -mc(wobble(s + 17, 800, 2_200)),
                health,
            )?;
        }
        if k == 5 {
            put(
                day(20),
                t("SKYWAYS FLIGHT BOOKING", "VUELOS SKYWAYS"),
                -m(312),
                leisure,
            )?;
        }
    }

    // Holdings: a monthly buy of an index fund, and three buys of a coin.
    let fund_price = |k: i32| 100_000_000 + i64::from(MONTHS - k) * 900_000; // micro-units
    tx.execute(
        "INSERT INTO holding (name, symbol, asset_class) VALUES (?1, 'GEIF', 'equities')",
        params![t("Global Equity Index Fund", "Fondo Indexado Global")],
    )?;
    let fund = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO holding (name, symbol, asset_class) VALUES ('Bitcoin', 'BTC', 'crypto')",
        [],
    )?;
    let coin = tx.last_insert_rowid();
    man.holding_ids = vec![fund, coin];
    for k in (0..MONTHS).rev() {
        let on = add_months(this, -k).with_day(6).unwrap();
        if on > today {
            continue;
        }
        let buy = m(250);
        let price = fund_price(k);
        // units ×1e8 = amount / price, with the price in millionths.
        let units = (i128::from(buy) * 100_000_000i128 * 1_000_000i128
            / i128::from(price)
            / 10i128.pow(exp)) as i64;
        tx.execute(
            "INSERT INTO holding_txn (holding_id, on_date, kind, amount_minor, units_e8)
             VALUES (?1, ?2, 'buy', ?3, ?4)",
            params![fund, on.to_string(), buy, units],
        )?;
        tx.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES (?1, ?2, ?3)
             ON CONFLICT DO NOTHING",
            params![fund, on.to_string(), price],
        )?;
    }
    // A price at every month end too, so the value is known when a balance
    // is recorded then.
    for k in 0..MONTHS {
        let on = month_end(add_months(this, -k)).min(today);
        tx.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES (?1, ?2, ?3)
             ON CONFLICT DO NOTHING",
            params![fund, on.to_string(), fund_price(k)],
        )?;
        // The coin moves a lot more.
        let coin_price = 38_000_000_000i64
            + (i64::from(MONTHS - k) * 1_650_000_000)
            + wobble(i64::from(k), 0, 2_500_000_000);
        tx.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES (?1, ?2, ?3)
             ON CONFLICT DO NOTHING",
            params![coin, on.to_string(), coin_price],
        )?;
    }
    for (k, units_e8) in [(11, 3_000_000i64), (7, 2_500_000), (2, 2_000_000)] {
        let on = add_months(this, -k).with_day(18).unwrap();
        if on > today {
            continue;
        }
        tx.execute(
            "INSERT INTO holding_txn (holding_id, on_date, kind, amount_minor, units_e8)
             VALUES (?1, ?2, 'buy', ?3, ?4)",
            params![coin, on.to_string(), m(units_e8 / 10_000), units_e8],
        )?;
    }

    // Net worth: a balance three times over the year.
    let snaps = [
        (12, [3_800, 14_000, 21_000, 240_000, 171_000]),
        (6, [4_300, 17_500, 23_100, 244_000, 166_000]),
        (0, [5_100, 21_000, 25_200, 248_000, 161_000]),
    ];
    for (k, v) in snaps {
        let on = month_end(add_months(this, -k)).min(today);
        tx.execute(
            "INSERT INTO wealth_snapshot (taken_on) VALUES (?1)
             ON CONFLICT(taken_on) DO NOTHING",
            params![on.to_string()],
        )?;
        let sid: i64 = tx.query_row(
            "SELECT id FROM wealth_snapshot WHERE taken_on = ?1",
            params![on.to_string()],
            |r| r.get(0),
        )?;
        man.snapshot_ids.push(sid);
        let lines = [
            ("cash", t("Current account", "Cuenta corriente"), v[0], 0),
            ("cash", t("Savings account", "Cuenta de ahorro"), v[1], 0),
            (
                "pension",
                t("Workplace pension", "Plan de pensiones"),
                v[2],
                0,
            ),
            ("real_estate", t("Apartment", "Vivienda"), v[3], v[4]),
        ];
        for (bucket, label, gross, debt) in lines {
            tx.execute(
                "INSERT INTO wealth_line (snapshot_id, bucket, label, gross_value_minor,
                                          debt_minor, currency, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'manual')",
                params![sid, bucket, label, m(gross), m(debt), currency],
            )?;
        }
        // The fund and the coin, valued from what was bought and the prices.
        super::portfolio::sync_snapshot(&tx, on)?;
    }

    // Plans and budgets for the months it is looked at in.
    for k in 0..3 {
        let per = add_months(this, -k).format("%Y-%m").to_string();
        tx.execute(
            "INSERT OR IGNORE INTO plan_month (period) VALUES (?1)",
            params![per],
        )?;
        let items: [(&str, String, i64); 6] = [
            ("income", t("Salary", "Nómina").into(), m(2_850)),
            ("expense", t("Rent", "Alquiler").into(), m(1_050)),
            (
                "expense",
                t("Food", "Comida").into(),
                m(520 + i64::from(k) * 10),
            ),
            (
                "expense",
                t("Bills and subscriptions", "Recibos y suscripciones").into(),
                m(190),
            ),
            ("expense", t("Transport", "Transporte").into(), m(140)),
            (
                "expense",
                t("Savings and investing", "Ahorro e inversión").into(),
                m(600),
            ),
        ];
        for (i, (kind, name, amount)) in items.iter().enumerate() {
            tx.execute(
                "INSERT INTO plan_item (period, kind, name, amount_minor, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![per, kind, name, amount, i as i64],
            )?;
        }
        man.plan_months.push(per);
    }
    for (cat, amount) in [(food, m(6_600)), (leisure, m(3_000)), (transport, m(1_800))] {
        tx.execute(
            "INSERT INTO budget (category_id, period, amount_minor) VALUES (?1, ?2, ?3)",
            params![cat, today.year().to_string(), amount],
        )?;
        man.budget_ids.push(tx.last_insert_rowid());
    }

    tx.execute(
        "INSERT INTO app_setting (key, value_json) VALUES (?1, ?2)",
        params![MANIFEST, serde_json::to_string(&man).unwrap_or_default()],
    )?;
    tx.commit()?;
    Ok(())
}

/// Remove exactly what `load` made, and nothing else.
pub fn remove(c: &rusqlite::Connection) -> AppResult<()> {
    let Some(man) = manifest(c)? else {
        return Ok(());
    };
    let tx = c.unchecked_transaction()?;
    let ids = |v: &[i64]| v.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
    if !man.holding_ids.is_empty() {
        // Lines computed from these holdings may sit in a snapshot the person
        // made meanwhile; once the holdings go they would name nothing.
        tx.execute_batch(&format!(
            "DELETE FROM wealth_line WHERE source = 'holding' AND source_id IN ({0});
             DELETE FROM holding WHERE id IN ({0})",
            ids(&man.holding_ids)
        ))?;
    }
    if !man.snapshot_ids.is_empty() {
        tx.execute_batch(&format!(
            "DELETE FROM wealth_snapshot WHERE id IN ({})",
            ids(&man.snapshot_ids)
        ))?;
    }
    if !man.budget_ids.is_empty() {
        tx.execute_batch(&format!(
            "DELETE FROM budget WHERE id IN ({})",
            ids(&man.budget_ids)
        ))?;
    }
    for p in &man.plan_months {
        tx.execute("DELETE FROM plan_month WHERE period = ?1", params![p])?;
    }
    // Only the rows the sample wrote (they carry its own hash prefix). The
    // account is the one place a first statement or a first typed expense
    // naturally goes, so whatever else is in it is the person's: the account
    // stays, under a plain name, instead of taking those rows with it.
    tx.execute(
        "DELETE FROM txn WHERE account_id = ?1 AND dedupe_hash LIKE 'sample-%'",
        params![man.account_id],
    )?;
    let left: i64 = tx.query_row(
        "SELECT count(*) FROM txn WHERE account_id = ?1",
        params![man.account_id],
        |r| r.get(0),
    )?;
    if left == 0 {
        tx.execute("DELETE FROM account WHERE id = ?1", params![man.account_id])?;
    } else {
        tx.execute(
            "UPDATE account SET name = CASE WHEN name = 'Cuenta corriente de ejemplo'
                                            THEN 'Cuenta corriente' ELSE 'Current account' END
              WHERE id = ?1",
            params![man.account_id],
        )?;
    }
    tx.execute("DELETE FROM app_setting WHERE key = ?1", params![MANIFEST])?;
    tx.commit()?;
    Ok(())
}

#[tauri::command]
pub fn sample_status(state: State<'_, AppState>) -> AppResult<SampleStatus> {
    state.db()?.with(status)
}

#[tauri::command]
pub fn sample_load(
    state: State<'_, AppState>,
    language: String,
    currency: String,
) -> AppResult<()> {
    if !crate::currency::is_supported(&currency) {
        return Err(AppError::Invalid(format!(
            "unsupported currency: {currency}"
        )));
    }
    let today = chrono::Local::now().date_naive();
    state.db()?.with(|c| load(c, today, &language, &currency))?;
    state.backup.mark_dirty();
    Ok(())
}

#[tauri::command]
pub fn sample_remove(state: State<'_, AppState>) -> AppResult<()> {
    state.db()?.with(remove)?;
    state.backup.mark_dirty();
    Ok(())
}
