//! The FIRE page's suggestions, read from a real ledger.

use chrono::NaiveDate;
use coin_heaven_lib::commands::wealth::ledger_suggestion;
use coin_heaven_lib::db::Db;
use rusqlite::params;
use secrecy::SecretString;

fn ledger() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("a.db"),
        &SecretString::from("0".repeat(64)),
    )
    .unwrap();
    db.with(|c| {
        c.execute(
            "INSERT INTO account (id, name, kind, currency) VALUES (1, 'A', 'checking', 'EUR')",
            [],
        )?;
        c.execute(
            "INSERT INTO pl_category (id, name, kind, sort_order) VALUES
               (900, 'Food', 'expense', 1), (901, 'Pay', 'income', 2), (902, 'Broker', 'neutral', 3)",
            [],
        )?;
        let add = |id: i64, on: &str, minor: i64, cat: i64| {
            c.execute(
                "INSERT INTO txn (id, account_id, booked_on, description_raw, description_norm,
                                  amount_minor, currency, category_override_id, dedupe_hash)
                 VALUES (?1, 1, ?2, 'x', 'x', ?3, 'EUR', ?4, ?5)",
                params![id, on, minor, cat, format!("h{id}")],
            )
            .unwrap();
        };
        // Six months of data: 3,000 in and 2,000 out each month, plus a
        // 500 monthly transfer to a broker that is saving, not spending.
        for (i, m) in ["2026-03", "2026-04", "2026-05", "2026-06", "2026-07", "2026-08"]
            .iter()
            .enumerate()
        {
            let i = i as i64 * 10;
            add(i + 1, &format!("{m}-01"), 300_000, 901);
            add(i + 2, &format!("{m}-10"), -200_000, 900);
            add(i + 3, &format!("{m}-15"), -50_000, 902);
        }
        // Outside the window: this month is incomplete and must not count.
        add(999, "2026-09-05", -999_999, 900);
        Ok(())
    })
    .unwrap();
    (dir, db)
}

#[test]
fn spending_and_saving_come_from_the_last_complete_months_annualised() {
    let (_d, db) = ledger();
    let today = NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();
    let s = db.with(|c| ledger_suggestion(c, today)).unwrap();

    assert_eq!(s.to, "2026-08-31", "the current month is not complete");
    assert_eq!(s.from, "2025-09-01");
    assert_eq!(s.months_with_data, 6);
    // Six months of 2,000 read as a year of 24,000 — not a cheap 12,000.
    assert_eq!(s.annual_spending.minor(), 2_400_000);
    assert_eq!(s.annual_income.minor(), 3_600_000);
    assert_eq!(
        s.annual_saving.minor(),
        1_200_000,
        "the broker transfer is saving, so it must not count as spending"
    );
    assert_eq!(s.savings_rate_bps, Some(3_333));
}

#[test]
fn an_empty_ledger_suggests_nothing_rather_than_zero_spending() {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("b.db"),
        &SecretString::from("0".repeat(64)),
    )
    .unwrap();
    let s = db
        .with(|c| ledger_suggestion(c, NaiveDate::from_ymd_opt(2026, 9, 27).unwrap()))
        .unwrap();
    assert_eq!(s.months_with_data, 0);
    assert_eq!(s.savings_rate_bps, None);
}
