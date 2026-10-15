//! The monthly plan against a real schema: order kept, totals right, a month
//! replaced rather than merged, and bad input refused.

use coin_heaven_lib::commands::plan::{self, PlanItem};
use coin_heaven_lib::db::Db;
use secrecy::SecretString;

fn book() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("p.db"),
        &SecretString::from("0".repeat(64)),
    )
    .unwrap();
    (dir, db)
}

fn item(kind: &str, name: &str, minor: i64) -> PlanItem {
    PlanItem {
        kind: kind.into(),
        name: name.into(),
        amount_minor: minor,
    }
}

#[test]
fn boxes_come_back_in_the_order_they_were_arranged_with_the_net() {
    let (_d, db) = book();
    // Deliberately not income-first and not alphabetical.
    let items = vec![
        item("expense", "Rent", 90_000),
        item("income", "Salary", 240_000),
        item("expense", "Food", 35_000),
        item("income", "Side job", 15_000),
        item("expense", "Zebra insurance", 4_550),
    ];
    db.with(|c| plan::save(c, "2026-10", &items)).unwrap();

    let plans = db.with(plan::list).unwrap();
    assert_eq!(plans.len(), 1);
    let p = &plans[0];
    let names: Vec<&str> = p.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(
        names,
        ["Rent", "Salary", "Food", "Side job", "Zebra insurance"]
    );
    assert_eq!(p.income_minor.minor(), 255_000);
    assert_eq!(p.expense_minor.minor(), 129_550);
    assert_eq!(p.net_minor.minor(), 125_450);
}

#[test]
fn saving_again_replaces_the_month_and_reordering_sticks() {
    let (_d, db) = book();
    db.with(|c| {
        plan::save(
            c,
            "2026-10",
            &[item("income", "A", 100), item("expense", "B", 50)],
        )
    })
    .unwrap();
    // B first, A second, and C is new; nothing from before lingers.
    db.with(|c| {
        plan::save(
            c,
            "2026-10",
            &[
                item("expense", "B", 70),
                item("income", "A", 100),
                item("expense", "C", 10),
            ],
        )
    })
    .unwrap();
    let p = &db.with(plan::list).unwrap()[0];
    let got: Vec<(&str, i64)> = p
        .items
        .iter()
        .map(|i| (i.name.as_str(), i.amount_minor))
        .collect();
    assert_eq!(got, [("B", 70), ("A", 100), ("C", 10)]);
    assert_eq!(p.net_minor.minor(), 20);
}

#[test]
fn months_are_kept_apart_and_listed_oldest_first_and_an_empty_month_is_kept() {
    let (_d, db) = book();
    db.with(|c| {
        plan::save(c, "2026-03", &[item("income", "Pay", 500)])?;
        plan::save(
            c,
            "2024-11",
            &[item("income", "Pay", 300), item("expense", "Rent", 100)],
        )?;
        plan::save(c, "2025-01", &[])
    })
    .unwrap();
    let plans = db.with(plan::list).unwrap();
    let periods: Vec<&str> = plans.iter().map(|p| p.period.as_str()).collect();
    assert_eq!(periods, ["2024-11", "2025-01", "2026-03"]);
    assert_eq!(plans[0].net_minor.minor(), 200, "two years back, as it was");
    assert!(plans[1].items.is_empty());
    assert_eq!(plans[1].net_minor.minor(), 0);
}

#[test]
fn deleting_a_month_takes_its_boxes_and_nothing_else() {
    let (_d, db) = book();
    db.with(|c| {
        plan::save(c, "2026-01", &[item("income", "Pay", 500)])?;
        plan::save(c, "2026-02", &[item("income", "Pay", 600)])?;
        plan::delete(c, "2026-01")
    })
    .unwrap();
    let plans = db.with(plan::list).unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].period, "2026-02");
    let orphans: i64 = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM plan_item WHERE period = '2026-01'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(orphans, 0);
}

#[test]
fn bad_input_is_refused_and_leaves_the_month_as_it_was() {
    let (_d, db) = book();
    db.with(|c| plan::save(c, "2026-05", &[item("income", "Pay", 500)]))
        .unwrap();

    let refused = |r: coin_heaven_lib::error::AppResult<()>| r.is_err();
    assert!(refused(db.with(|c| plan::save(c, "2026-13", &[]))));
    assert!(refused(db.with(|c| plan::save(c, "26-05", &[]))));
    assert!(refused(db.with(|c| plan::save(
        c,
        "2026-05",
        &[item("transfer", "x", 1)]
    ))));
    assert!(refused(db.with(|c| plan::save(
        c,
        "2026-05",
        &[item("income", "x", -1)]
    ))));
    let too_many: Vec<PlanItem> = (0..201)
        .map(|i| item("expense", &format!("{i}"), 1))
        .collect();
    assert!(refused(db.with(|c| plan::save(c, "2026-05", &too_many))));

    let p = &db.with(plan::list).unwrap()[0];
    assert_eq!(p.items.len(), 1, "a refused save changes nothing");
    assert_eq!(p.items[0].name, "Pay");
}

#[test]
fn names_are_trimmed_and_may_be_empty() {
    let (_d, db) = book();
    db.with(|c| {
        plan::save(
            c,
            "2026-06",
            &[item("expense", "   Gym  ", 100), item("expense", "", 5)],
        )
    })
    .unwrap();
    let p = &db.with(plan::list).unwrap()[0];
    assert_eq!(p.items[0].name, "Gym");
    assert_eq!(p.items[1].name, "");
}
