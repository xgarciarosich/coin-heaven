//! The budget, matrix and auto-categorisation queries against a real database.
//!
//! These read through `v_txn_net`, which nets refunds and drops neutralised
//! transfers, and they slice by a text prefix of `booked_on`. Both are easy to
//! get subtly wrong in a way no type will catch — a refund counted twice, or a
//! December row landing in the wrong month — so they are exercised against an
//! actual migrated schema rather than a hand-built fixture.

use coin_heaven_lib::commands::{autocat, budget};
use coin_heaven_lib::db::Db;
use rusqlite::params;
use secrecy::SecretString;

fn ledger() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("atlas.db"),
        &SecretString::from("0".repeat(64)),
    )
    .unwrap();

    db.with(|c| {
        c.execute(
            "INSERT INTO account (id, name, kind, currency) VALUES (1, 'Corriente', 'checking', 'EUR')",
            [],
        )?;
        c.execute(
            "INSERT INTO pl_category (id, name, kind, sort_order) VALUES
               (900, 'Groceries', 'expense', 10), (901, 'Pay', 'income', 20)",
            [],
        )?;

        let add = |id: i64, on: &str, desc: &str, minor: i64, cat: Option<i64>| {
            c.execute(
                "INSERT INTO txn (id, account_id, booked_on, description_raw, description_norm,
                                  amount_minor, currency, category_override_id, dedupe_hash,
                                  needs_review)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, 'EUR', ?6, ?7, 1)",
                params![
                    id,
                    on,
                    desc,
                    coin_heaven_lib::security::pii::normalise_description(desc),
                    minor,
                    cat,
                    format!("h{id}")
                ],
            )
            .unwrap();
        };

        add(1, "2026-01-10", "COMPRA MERCADONA VALENCIA", -5000, Some(900));
        add(2, "2026-01-20", "COMPRA MERCADONA GETAFE", -3000, Some(900));
        add(3, "2026-12-31", "COMPRA MERCADONA VALENCIA", -1000, None);
        add(4, "2026-03-01", "NOMINA ACME IBERIA", 200_000, Some(901));
        add(5, "2025-06-01", "COMPRA MERCADONA VALENCIA", -9999, Some(900));
        add(6, "2026-06-05", "COMPRA MERCADONA SEVILLA", -1000, Some(900));
        Ok(())
    })
    .unwrap();

    (dir, db)
}

#[test]
fn the_matrix_puts_every_row_in_its_own_month_and_year() {
    let (_d, db) = ledger();
    let m = db.with(|c| budget::matrix(c, 2026)).unwrap();

    assert_eq!(m.periods.len(), 12);
    let groceries = m
        .rows
        .iter()
        .find(|r| r.category_id == Some(900))
        .expect("the groceries row");
    assert_eq!(groceries.cells[0].minor(), -8000, "January");
    assert_eq!(groceries.cells[1].minor(), 0, "February is empty");
    // The 2025 row must not leak in, and 31 December must not fall off the end.
    assert_eq!(groceries.cells[5].minor(), -1000, "June");
    assert_eq!(groceries.total_minor.minor(), -9000);
    assert_eq!(
        m.rows.iter().map(|r| r.total_minor.minor()).sum::<i64>(),
        190_000
    );

    let december: i64 = m.rows.iter().map(|r| r.cells[11].minor()).sum();
    assert_eq!(december, -1000, "31 December belongs to December");

    // January, March, June and December have activity; nothing else does.
    assert_eq!(m.active_periods, 4);
    assert_eq!(m.income_total.minor(), 200_000);
    assert_eq!(m.expense_total.minor(), -10_000);
}

#[test]
fn a_refund_is_netted_once_and_the_refund_itself_is_not_a_row() {
    let (_d, db) = ledger();
    db.with(|c| {
        c.execute(
            "INSERT INTO txn (id, account_id, booked_on, description_raw, description_norm,
                              amount_minor, currency, category_override_id, dedupe_hash,
                              parent_txn_id, link_kind, needs_review)
             VALUES (9, 1, '2026-01-15', 'DEVOLUCION', 'devolucion', 2000, 'EUR', 900, 'h9',
                     1, 'reimbursement', 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let m = db.with(|c| budget::matrix(c, 2026)).unwrap();
    let groceries = m.rows.iter().find(|r| r.category_id == Some(900)).unwrap();
    assert_eq!(
        groceries.cells[0].minor(),
        -6000,
        "the 20 refund should reduce January's 80 to 60, not appear beside it"
    );
}

fn on(d: &str) -> chrono::NaiveDate {
    chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").unwrap()
}

#[test]
fn a_budget_reports_pace_against_the_calendar() {
    let (_d, db) = ledger();
    db.with(|c| {
        budget::save(c, 900, "2026", 20_000)?;
        budget::save(c, 900, "2026-01", 6_000)
    })
    .unwrap();

    let plans = db
        .with(|c| budget::plan(c, 2026, on("2026-07-02")))
        .unwrap();
    assert_eq!(plans.len(), 1);
    let p = &plans[0];

    // Spending is stored negative; the budget is a magnitude, so the actual
    // has to be one too or every comparison inverts.
    assert_eq!(p.year.actual_minor.minor(), 9000, "January plus June");
    assert_eq!(p.year.remaining_minor.minor(), 11_000);
    assert_eq!(p.year.days_total, 365);
    assert!(p.year.own);
    assert!(
        (4900..=5100).contains(&p.year.elapsed_bps),
        "2 July is about half the year, got {}",
        p.year.elapsed_bps
    );

    let jan = &p.months[0];
    assert!(jan.own);
    assert_eq!(jan.actual_minor.minor(), 8000, "January only");
    assert_eq!(jan.remaining_minor.minor(), -2000, "2.000 over its 60");
    assert_eq!(jan.days_total, 31);
    assert_eq!(jan.elapsed_bps, 10_000, "a closed month is fully elapsed");
    assert_eq!(jan.days_left, 0);
}

#[test]
fn a_month_before_it_starts_has_not_elapsed_at_all() {
    let (_d, db) = ledger();
    db.with(|c| budget::save(c, 900, "2026-09", 5000)).unwrap();

    let plans = db
        .with(|c| budget::plan(c, 2026, on("2026-03-01")))
        .unwrap();
    let sep = &plans[0].months[8];
    assert_eq!(sep.elapsed_bps, 0);
    assert_eq!(sep.days_left, 30);
}

#[test]
fn a_year_spreads_over_its_months_and_they_add_back_up_to_it() {
    let (_d, db) = ledger();
    // 1.000,01 does not divide by twelve; not one cent may be lost.
    db.with(|c| budget::save(c, 900, "2026", 100_001)).unwrap();

    let plans = db
        .with(|c| budget::plan(c, 2026, on("2026-07-02")))
        .unwrap();
    let p = &plans[0];
    assert_eq!(p.months_set, 0);
    assert!(p.months.iter().all(|m| !m.own));
    let sum: i64 = p
        .months
        .iter()
        .map(|m| m.amount_minor.unwrap().minor())
        .sum();
    assert_eq!(sum, 100_001);
    assert_eq!(p.months_sum_minor.minor(), 100_001);
    assert_eq!(p.months[11].amount_minor.unwrap().minor(), 8333);
    assert_eq!(p.months[0].amount_minor.unwrap().minor(), 8334);
}

#[test]
fn a_month_with_its_own_figure_keeps_it_and_the_rest_share_what_is_left() {
    let (_d, db) = ledger();
    db.with(|c| {
        budget::save(c, 900, "2026", 120_000)?;
        // December is the expensive one.
        budget::save(c, 900, "2026-12", 31_000)
    })
    .unwrap();

    let plans = db
        .with(|c| budget::plan(c, 2026, on("2026-07-02")))
        .unwrap();
    let p = &plans[0];
    assert_eq!(p.year.amount_minor.unwrap().minor(), 120_000);
    assert_eq!(p.months[11].amount_minor.unwrap().minor(), 31_000);
    // (120.000 − 31.000) / 11 = 8.090,9…: eleven months, 89.000 between them.
    let rest: i64 = p.months[..11]
        .iter()
        .map(|m| m.amount_minor.unwrap().minor())
        .sum();
    assert_eq!(rest, 89_000);
    assert_eq!(p.months[10].amount_minor.unwrap().minor(), 8090);
    assert_eq!(p.months_sum_minor.minor(), 120_000);

    // Months written by hand that already exceed the year leave the others
    // nothing, and the plan says what the months really come to.
    db.with(|c| budget::save(c, 900, "2026-11", 100_000))
        .unwrap();
    let plans = db
        .with(|c| budget::plan(c, 2026, on("2026-07-02")))
        .unwrap();
    let p = &plans[0];
    assert_eq!(p.months[0].amount_minor.unwrap().minor(), 0);
    assert_eq!(p.year.amount_minor.unwrap().minor(), 120_000);
    assert_eq!(p.months_sum_minor.minor(), 131_000);
}

#[test]
fn months_alone_make_the_year_and_the_unplanned_ones_stay_unplanned() {
    let (_d, db) = ledger();
    db.with(|c| {
        budget::save(c, 900, "2026-01", 6_000)?;
        budget::save(c, 900, "2026-06", 4_000)
    })
    .unwrap();

    let plans = db
        .with(|c| budget::plan(c, 2026, on("2026-07-02")))
        .unwrap();
    let p = &plans[0];
    assert!(!p.year.own);
    assert_eq!(p.year.amount_minor.unwrap().minor(), 10_000);
    assert_eq!(p.months_set, 2);
    assert!(p.months[1].amount_minor.is_none(), "February has no plan");
    assert_eq!(p.months[1].actual_minor.minor(), 0);
}

#[test]
fn the_same_figure_every_month_replaces_the_year_and_the_other_way_round() {
    let (_d, db) = ledger();
    db.with(|c| {
        budget::save(c, 900, "2026", 20_000)?;
        budget::save_every_month(c, 900, 2026, 40_000)
    })
    .unwrap();
    let plans = db
        .with(|c| budget::plan(c, 2026, on("2026-07-02")))
        .unwrap();
    let p = &plans[0];
    assert!(!p.year.own, "the year now follows its months");
    assert_eq!(p.year.amount_minor.unwrap().minor(), 480_000);
    assert_eq!(p.months_set, 12);

    // A figure for the year over twelve written months takes over from them.
    db.with(|c| budget::save(c, 900, "2026", 600_000)).unwrap();
    let plans = db
        .with(|c| budget::plan(c, 2026, on("2026-07-02")))
        .unwrap();
    let p = &plans[0];
    assert!(p.year.own);
    assert_eq!(p.months_set, 0);
    assert_eq!(p.months[3].amount_minor.unwrap().minor(), 50_000);

    // The matrix reads the same year figure the plan shows.
    let m = db.with(|c| budget::matrix(c, 2026)).unwrap();
    let row = m.rows.iter().find(|r| r.category_id == Some(900)).unwrap();
    assert_eq!(row.budget_minor.unwrap().minor(), 600_000);
}

#[test]
fn suggestions_group_by_shop_and_learn_from_what_was_already_filed() {
    let (_d, db) = ledger();
    let s = db.with(autocat::suggest).unwrap();

    assert_eq!(s.uncategorised_total, 1, "only the 31 December row");
    let g = s
        .groups
        .iter()
        .find(|g| g.key == "mercadona")
        .expect("the uncategorised Mercadona row should be grouped");
    assert_eq!(g.txn_ids, vec![3]);
    assert_eq!(
        g.suggested_category_id,
        Some(900),
        "four earlier Mercadona rows were filed under Groceries"
    );
    assert_eq!(g.source, "history");
    assert_eq!(g.confidence_bps, 10_000);
    assert_eq!(g.evidence_count, 4);
    assert_eq!(s.covered, 1);
}

#[test]
fn nothing_uncategorised_means_nothing_to_decide() {
    let (_d, db) = ledger();
    db.with(|c| {
        c.execute("UPDATE txn SET category_override_id = 900 WHERE id = 3", [])?;
        Ok(())
    })
    .unwrap();

    let s = db.with(autocat::suggest).unwrap();
    assert_eq!(s.uncategorised_total, 0);
    assert!(s.groups.is_empty());
}

#[test]
fn a_merchant_filed_once_is_filed_the_same_way_from_then_on() {
    let (_d, db) = ledger();
    db.with(|c| {
        // New rows arrive: a known shop, and one nobody has seen before.
        c.execute(
            "INSERT INTO txn (id, account_id, booked_on, description_raw, description_norm,
                              amount_minor, currency, dedupe_hash, needs_review) VALUES
               (10, 1, '2027-01-03', 'COMPRA MERCADONA SUECA', 'compra mercadona sueca', -2000, 'EUR', 'h10', 1),
               (11, 1, '2027-01-04', 'FARMACIA LOPEZ', 'farmacia lopez', -900, 'EUR', 'h11', 1)",
            [],
        )?;
        // Rows 3 and 10 are Mercadona and unfiled; every earlier one is Groceries.
        assert_eq!(autocat::learn(c)?, 2);
        let cat = |id: i64| -> Option<i64> {
            c.query_row(
                "SELECT category_override_id FROM txn WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(cat(3), Some(900));
        assert_eq!(cat(10), Some(900));
        assert_eq!(cat(11), None, "a shop never seen before is left for a person");
        assert_eq!(autocat::learn(c)?, 0, "nothing left to learn");
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_merchant_filed_two_ways_is_not_guessed() {
    let (_d, db) = ledger();
    db.with(|c| {
        // Half of the Mercadona history now says something else.
        c.execute(
            "UPDATE txn SET category_override_id = 901 WHERE id IN (1, 2)",
            [],
        )?;
        assert_eq!(autocat::learn(c)?, 0);
        Ok(())
    })
    .unwrap();
}
