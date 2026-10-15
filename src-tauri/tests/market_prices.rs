//! Prices for buys that have none: a series of daily closes is written for
//! each buy date, units are worked out from them, and a price somebody typed
//! is never replaced.

use chrono::NaiveDate;
use coin_heaven_lib::commands::market::{apply, asks};
use coin_heaven_lib::commands::portfolio::portfolio_at;
use coin_heaven_lib::db::Db;
use coin_heaven_lib::market::parse_chart;
use rusqlite::params;
use secrecy::SecretString;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn sap_series() -> coin_heaven_lib::market::Series {
    let body: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string("tests/fixtures/yahoo_chart_sap_de.json").unwrap(),
    )
    .unwrap();
    parse_chart(&body).unwrap()
}

fn book() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("m.db"),
        &SecretString::from("0".repeat(64)),
    )
    .unwrap();
    db.with(|c| {
        c.execute(
            "INSERT INTO holding (id, name, symbol, asset_class) VALUES (1, 'SAP', 'SAP.DE', 'equities')",
            [],
        )?;
        // A weekly €100 buy, known by its amount only: no units, no price.
        for on in ["2024-03-04", "2024-03-11", "2024-03-18", "2024-03-25"] {
            c.execute(
                "INSERT INTO holding_txn (holding_id, on_date, kind, amount_minor)
                 VALUES (1, ?1, 'buy', 10000)",
                params![on],
            )?;
        }
        Ok(())
    })
    .unwrap();
    (dir, db)
}

#[test]
fn buys_without_a_price_are_found_and_then_priced_on_their_own_day() {
    let (_d, db) = book();

    let a = db.with(asks).unwrap();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].name, "SAP");
    assert_eq!(a[0].buys_missing, 4);
    assert_eq!(a[0].from, "2024-03-04");

    // Before: nothing to turn money into shares with.
    let before = db.with(|c| portfolio_at(c, d("2024-03-29"))).unwrap();
    assert!(!before.holdings[0].units_complete);

    let series = sap_series();
    let wanted = [
        d("2024-03-04"),
        d("2024-03-11"),
        d("2024-03-18"),
        d("2024-03-25"),
    ];
    let (stored, left_out) = db.with(|c| apply(c, 1, &series, &wanted)).unwrap();
    assert_eq!((stored, left_out), (4, 0));

    // A price on each buy date, from the market source.
    let rows: Vec<(String, i64, String)> = db
        .with(|c| {
            let mut st = c.prepare(
                "SELECT on_date, price_micro, source FROM holding_price WHERE holding_id = 1
                 ORDER BY on_date",
            )?;
            Ok(st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<_, _>>()?)
        })
        .unwrap();
    assert!(rows.len() >= 4);
    assert!(rows.iter().all(|r| r.2 == "market" && r.1 > 0));
    let on4 = rows.iter().find(|r| r.0 == "2024-03-04").unwrap();
    let close = series
        .closes
        .iter()
        .find(|c| c.0 == d("2024-03-04"))
        .unwrap()
        .1;
    assert_eq!(on4.1, (close * 1e6).round() as i64);

    // After: each €100 became the shares that day's close bought.
    let after = db.with(|c| portfolio_at(c, d("2024-03-29"))).unwrap();
    let h = &after.holdings[0];
    assert!(h.units_complete, "every buy has units now");
    let expected: f64 = wanted
        .iter()
        .map(|w| {
            let p = series.closes.iter().rev().find(|c| c.0 <= *w).unwrap().1;
            100.0 / p
        })
        .sum();
    let got = h.units_e8 as f64 / 1e8;
    assert!((got - expected).abs() < 0.001, "units {got} vs {expected}");

    // And it is nothing left to ask about.
    assert!(db.with(asks).unwrap().is_empty());
}

#[test]
fn a_price_typed_by_hand_survives_a_lookup() {
    let (_d, db) = book();
    db.with(|c| {
        c.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro, source)
             VALUES (1, '2024-03-11', 123456789, 'manual')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let wanted = [d("2024-03-04"), d("2024-03-11")];
    db.with(|c| apply(c, 1, &sap_series(), &wanted)).unwrap();
    let (p, src): (i64, String) = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT price_micro, source FROM holding_price
                 WHERE holding_id = 1 AND on_date = '2024-03-11'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!((p, src.as_str()), (123_456_789, "manual"));
}

#[test]
fn dates_before_the_instrument_existed_are_left_out_not_invented() {
    let (_d, db) = book();
    db.with(|c| {
        c.execute(
            "INSERT INTO holding_txn (holding_id, on_date, kind, amount_minor)
             VALUES (1, '2023-06-05', 'buy', 10000)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let wanted = [d("2023-06-05"), d("2024-03-04")];
    let (stored, left_out) = db.with(|c| apply(c, 1, &sap_series(), &wanted)).unwrap();
    assert_eq!((stored, left_out), (1, 1));
}
