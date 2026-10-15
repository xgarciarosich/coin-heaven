//! Snapshot lines computed from the portfolio.
//!
//! This is the code that writes into net worth without anyone typing the
//! number, so it is tested against a real schema: a holding valued at units ×
//! price, hand lines left alone, a second sync changing nothing, and the
//! value through time.

use chrono::NaiveDate;
use coin_heaven_lib::commands::portfolio::{
    materialise, portfolio_at, portfolio_history, resync_all, sync_snapshot,
};
use coin_heaven_lib::db::Db;
use secrecy::SecretString;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn book() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("n.db"),
        &SecretString::from("0".repeat(64)),
    )
    .unwrap();
    (dir, db)
}

#[test]
fn a_sync_builds_the_snapshot_from_its_sources_and_leaves_hand_lines_alone() {
    let (_d, db) = book();
    db.with(|c| {
        c.execute(
            "INSERT INTO holding (id, name, symbol, asset_class) VALUES (1, 'Bitcoin', 'BTC', 'crypto')",
            [],
        )?;
        c.execute(
            "INSERT INTO holding_txn (holding_id, on_date, kind, amount_minor, units_e8)
             VALUES (1, '2026-01-10', 'buy', 100000, 1500000)",
            [],
        )?;
        c.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES (1, '2026-06-30', 90000000000)",
            [],
        )?;
        // A line the user typed into the same snapshot.
        c.execute("INSERT INTO wealth_snapshot (id, taken_on) VALUES (7, '2026-06-30')", [])?;
        c.execute(
            "INSERT INTO wealth_line (snapshot_id, bucket, label, gross_value_minor, debt_minor)
             VALUES (7, 'other_asset', 'Car', 800000, 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let report = db.with(|c| sync_snapshot(c, d("2026-06-30"))).unwrap();
    assert_eq!(report.snapshot_id, 7, "the existing snapshot is reused");
    assert_eq!(report.lines, 1, "the holding");
    assert!(report.unpriced.is_empty() && report.incomplete.is_empty());

    let lines = |db: &Db| -> Vec<(String, String, i64, i64)> {
        db.with(|c| {
            let mut st = c.prepare(
                "SELECT source, label, gross_value_minor, debt_minor FROM wealth_line
                 WHERE snapshot_id = 7 ORDER BY source, label",
            )?;
            Ok(st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap()
    };
    let first = lines(&db);
    assert!(
        first.contains(&("holding".into(), "Bitcoin".into(), 135_000, 0)),
        "0.015 × €90,000"
    );
    assert!(
        first.contains(&("manual".into(), "Car".into(), 800_000, 0)),
        "hand line kept"
    );

    db.with(|c| sync_snapshot(c, d("2026-06-30"))).unwrap();
    assert_eq!(lines(&db), first, "a second sync changes nothing");
}

#[test]
fn recurring_buys_are_written_once_and_a_skip_survives() {
    let (_d, db) = book();
    db.with(|c| {
        c.execute(
            "INSERT INTO holding (id, name, asset_class) VALUES (1, 'Bitcoin', 'crypto')",
            [],
        )?;
        c.execute(
            "INSERT INTO holding_schedule (id, holding_id, amount_minor, every_days, start_date)
             VALUES (1, 1, 5000, 15, '2025-01-01')",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let until = d("2026-09-27");
    let first = db.with(|c| materialise(c, until)).unwrap();
    assert_eq!(first, 43);
    assert_eq!(db.with(|c| materialise(c, until)).unwrap(), 0, "idempotent");

    db.with(|c| {
        c.execute(
            "UPDATE holding_txn SET skipped = 1 WHERE on_date = '2025-01-16'",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        db.with(|c| materialise(c, until)).unwrap(),
        0,
        "a skipped buy is not rewritten"
    );

    let pv = db.with(|c| portfolio_at(c, d("2026-06-30"))).unwrap();
    let btc = &pv.holdings[0];
    // 1 Jan 2025 to 30 Jun 2026 every 15 days is 37 dates; one was skipped.
    assert_eq!(btc.invested.minor(), 36 * 5_000);
    assert!(!btc.units_complete, "amounts known, units not");
    assert_eq!(pv.unpriced, vec!["Bitcoin".to_string()]);
}

#[test]
fn reconciling_units_turns_scheduled_buys_into_a_value() {
    let (_d, db) = book();
    db.with(|c| {
        c.execute("INSERT INTO holding (id, name, asset_class) VALUES (1, 'Bitcoin', 'crypto')", [])?;
        c.execute(
            "INSERT INTO holding_schedule (holding_id, amount_minor, every_days, start_date)
             VALUES (1, 5000, 15, '2025-01-01')",
            [],
        )?;
        materialise(c, d("2026-09-27"))?;
        // Read off the exchange: 0.02 BTC held on 30 June.
        c.execute(
            "INSERT INTO holding_txn (holding_id, on_date, kind, units_e8) VALUES (1, '2026-06-30', 'units', 2000000)",
            [],
        )?;
        c.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES (1, '2026-06-30', 90000000000)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let pv = db.with(|c| portfolio_at(c, d("2026-06-30"))).unwrap();
    let btc = &pv.holdings[0];
    assert!(btc.units_complete);
    assert_eq!(btc.value.unwrap().minor(), 180_000, "0.02 × €90,000");
    assert_eq!(btc.gain.unwrap().minor(), 180_000 - 37 * 5_000);
    assert!(btc.xirr_bps.is_some());
}

#[test]
fn blind_recurring_buys_are_valued_from_a_price_until_units_are_read_off() {
    let (_d, db) = book();
    db.with(|c| {
        c.execute("INSERT INTO holding (id, name, asset_class) VALUES (1, 'Bitcoin', 'crypto')", [])?;
        c.execute(
            "INSERT INTO holding_schedule (holding_id, amount_minor, every_days, start_date)
             VALUES (1, 5000, 15, '2025-01-01')",
            [],
        )?;
        materialise(c, d("2026-09-27"))?;
        c.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES (1, '2026-06-30', 90000000000)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let pv = db.with(|c| portfolio_at(c, d("2026-06-30"))).unwrap();
    let btc = &pv.holdings[0];
    // With one price and no unit counts, every buy is taken at that price:
    // the holding reads at cost rather than at nothing.
    assert!(btc.units_complete && btc.units_estimated);
    let v = btc.value.unwrap().minor();
    assert!((v - 37 * 5_000).abs() <= 37, "at cost, to rounding: {v}");
    assert!(pv.unpriced.is_empty());
}

#[test]
fn history_follows_what_went_in_and_what_it_was_worth() {
    let (_d, db) = book();
    db.with(|c| {
        c.execute(
            "INSERT INTO holding (id, name, asset_class) VALUES (1, 'Fund', 'equities')",
            [],
        )?;
        c.execute(
            "INSERT INTO holding_txn (holding_id, on_date, kind, amount_minor, units_e8) VALUES
               (1, '2026-01-15', 'buy', 100000, 1000000000),
               (1, '2026-03-10', 'buy', 100000,  800000000)",
            [],
        )?;
        c.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES
               (1, '2026-02-28', 110000000), (1, '2026-04-20', 125000000)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let h = db.with(|c| portfolio_history(c, d("2026-04-30"))).unwrap();
    let at = |date: &str| {
        let p = h
            .iter()
            .find(|p| p.date == date)
            .unwrap_or_else(|| panic!("no point {date}"));
        (p.value.minor(), p.invested.minor())
    };
    assert_eq!(h.first().unwrap().date, "2026-01-15");
    assert_eq!(
        at("2026-01-15"),
        (100_000, 100_000),
        "at cost before any price"
    );
    assert_eq!(at("2026-01-31"), (100_000, 100_000));
    assert_eq!(at("2026-02-28"), (110_000, 100_000), "10 units × €110");
    assert_eq!(at("2026-03-31"), (198_000, 200_000), "18 units × €110");
    assert_eq!(at("2026-04-20"), (225_000, 200_000), "18 units × €125");
    assert_eq!(h.last().unwrap().date, "2026-04-30");
}

#[test]
fn a_new_price_revalues_recorded_balances_without_creating_one() {
    let (_d, db) = book();
    db.with(|c| {
        c.execute("INSERT INTO holding (id, name, asset_class) VALUES (1, 'Fund', 'equities')", [])?;
        c.execute(
            "INSERT INTO holding_txn (holding_id, on_date, kind, amount_minor, units_e8)
             VALUES (1, '2026-01-15', 'buy', 100000, 1000000000)",
            [],
        )?;
        c.execute("INSERT INTO wealth_snapshot (id, taken_on) VALUES (1, '2026-06-30')", [])?;
        c.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES (1, '2026-06-01', 120000000)",
            [],
        )?;
        resync_all(c)?;
        // A later price, after the balance's date: it must not touch it.
        c.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES (1, '2026-09-01', 200000000)",
            [],
        )?;
        resync_all(c)?;
        let (n, v): (i64, i64) = c.query_row(
            "SELECT (SELECT count(*) FROM wealth_snapshot),
                    (SELECT gross_value_minor FROM wealth_line WHERE source = 'holding')",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(n, 1, "no balance invented for the price's date");
        assert_eq!(v, 120_000, "10 units × €120, the price on or before 30 June");
        Ok(())
    })
    .unwrap();
}
