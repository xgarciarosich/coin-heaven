//! The sample household: loaded into an empty book, never into one with
//! data, and removed without a trace.

use chrono::NaiveDate;
use coin_heaven_lib::commands::{budget, plan, recurring, sample};
use coin_heaven_lib::db::Db;
use secrecy::SecretString;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn book() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("s.db"),
        &SecretString::from("0".repeat(64)),
    )
    .unwrap();
    (dir, db)
}

fn count(db: &Db, table: &str) -> i64 {
    db.with(|c| Ok(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?))
        .unwrap()
}

#[test]
fn an_empty_book_is_offered_the_sample_and_it_looks_like_a_life() {
    let (_d, db) = book();
    assert!(db.with(sample::status).unwrap().can_load);

    db.with(|c| sample::load(c, d("2026-10-13"), "en", "USD"))
        .unwrap();
    let s = db.with(sample::status).unwrap();
    assert!(s.loaded && !s.can_load);

    assert!(count(&db, "txn") > 150, "a year and more of transactions");
    assert_eq!(count(&db, "holding"), 2);
    assert_eq!(count(&db, "wealth_snapshot"), 3);
    assert_eq!(count(&db, "plan_month"), 3);
    assert_eq!(count(&db, "budget"), 3);

    // Every transaction is filed: nothing waits for review in a demo.
    let unfiled: i64 = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM txn WHERE category_override_id IS NULL OR needs_review = 1",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(unfiled, 0);

    // The portfolio lines were computed, so net worth includes the holdings.
    let held: i64 = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM wealth_line WHERE source = 'holding'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(held >= 3, "got {held}");

    // Other screens find something to say about it.
    let rec = db.with(|c| recurring::detect(c, d("2026-10-13"))).unwrap();
    for name in ["STREAMFLIX", "FITCLUB", "FIBRELINE"] {
        assert!(
            rec.items.iter().any(|i| i.name.contains(name) && i.active),
            "{name} should be a recurring payment"
        );
    }
    let plans = db.with(plan::list).unwrap();
    assert!(plans.iter().all(|p| p.net_minor.minor() > 0));
    let b = db.with(|c| budget::plan(c, 2026, d("2026-10-13"))).unwrap();
    assert_eq!(b.len(), 3);
}

#[test]
fn removal_leaves_the_book_as_empty_as_it_was_and_it_can_come_back() {
    let (_d, db) = book();
    db.with(|c| sample::load(c, d("2026-10-13"), "es", "EUR"))
        .unwrap();
    db.with(sample::remove).unwrap();

    for t in [
        "txn",
        "account",
        "holding",
        "holding_txn",
        "holding_price",
        "wealth_snapshot",
        "wealth_line",
        "plan_month",
        "plan_item",
        "budget",
    ] {
        assert_eq!(count(&db, t), 0, "{t} should be empty");
    }
    let s = db.with(sample::status).unwrap();
    assert!(!s.loaded && s.can_load);
    // And once more, to prove nothing was left to collide with.
    db.with(|c| sample::load(c, d("2026-10-13"), "en", "EUR"))
        .unwrap();
    assert!(count(&db, "txn") > 150);
}

#[test]
fn a_book_with_data_is_never_offered_the_sample() {
    let (_d, db) = book();
    db.with(|c| {
        c.execute(
            "INSERT INTO account (name, kind, currency) VALUES ('Mine', 'checking', 'EUR')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    assert!(!db.with(sample::status).unwrap().can_load);
    assert!(
        db.with(|c| sample::load(c, d("2026-10-13"), "en", "EUR"))
            .is_err()
    );
    assert_eq!(count(&db, "txn"), 0, "nothing was added");
}

#[test]
fn removing_the_sample_does_not_touch_what_was_added_beside_it() {
    let (_d, db) = book();
    db.with(|c| sample::load(c, d("2026-10-13"), "en", "EUR"))
        .unwrap();
    db.with(|c| {
        c.execute(
            "INSERT INTO account (id, name, kind, currency) VALUES (900, 'Mine', 'checking', 'EUR')",
            [],
        )?;
        c.execute(
            "INSERT INTO txn (account_id, booked_on, description_raw, description_norm,
                              amount_minor, currency, dedupe_hash)
             VALUES (900, '2026-10-01', 'MY OWN', 'my own', -100, 'EUR', 'mine-1')",
            [],
        )?;
        plan::save(c, "2026-11", &[])?;
        Ok(())
    })
    .unwrap();
    db.with(sample::remove).unwrap();
    assert_eq!(count(&db, "txn"), 1);
    assert_eq!(count(&db, "account"), 1);
    assert_eq!(count(&db, "plan_month"), 1);
}
