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
        owner: "shared".into(),
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

#[test]
fn a_plan_for_two_keeps_who_each_box_belongs_to_and_how_they_split() {
    let (_d, db) = book();
    let mine = |kind: &str, name: &str, minor: i64, owner: &str| PlanItem {
        owner: owner.into(),
        ..item(kind, name, minor)
    };
    db.with(|c| {
        plan::save(
            c,
            "2026-11",
            &[
                mine("income", "Ana's salary", 300_000, "a"),
                mine("income", "Ben's salary", 200_000, "b"),
                mine("expense", "Rent", 120_000, "shared"),
                mine("expense", "Gym", 4_000, "b"),
            ],
        )?;
        plan::save_setup(
            c,
            "2026-11",
            &plan::PlanSetup {
                mode: "couple".into(),
                person_a: " Ana ".into(),
                person_b: "Ben".into(),
                split: "income".into(),
                custom_a_bps: 5_000,
            },
        )
    })
    .unwrap();
    let p = &db.with(plan::list).unwrap()[0];
    assert_eq!(p.setup.mode, "couple");
    assert_eq!(
        (p.setup.person_a.as_str(), p.setup.person_b.as_str()),
        ("Ana", "Ben")
    );
    assert_eq!(p.setup.split, "income");
    let owners: Vec<&str> = p.items.iter().map(|i| i.owner.as_str()).collect();
    assert_eq!(owners, ["a", "b", "shared", "b"]);
    // Totals are the household's, whoever pays.
    assert_eq!(p.net_minor.minor(), 500_000 - 124_000);

    // A new month starts as a plan for one, and nonsense is refused.
    db.with(|c| plan::save(c, "2026-12", &[])).unwrap();
    assert_eq!(
        db.with(plan::list).unwrap()[1].setup,
        plan::PlanSetup::default()
    );
    let bad = db.with(|c| plan::save(c, "2026-12", &[mine("income", "x", 1, "c")]));
    assert!(bad.is_err());
    let bad_setup = db.with(|c| {
        plan::save_setup(
            c,
            "2026-12",
            &plan::PlanSetup {
                split: "random".into(),
                ..plan::PlanSetup::default()
            },
        )
    });
    assert!(bad_setup.is_err());
}

#[test]
fn a_box_saved_by_an_older_version_without_an_owner_is_shared() {
    let json = r#"{"kind":"income","name":"Pay","amountMinor":100}"#;
    let it: PlanItem = serde_json::from_str(json).unwrap();
    assert_eq!(it.owner, "shared");
}

#[test]
fn a_couple_can_agree_their_own_split() {
    let (_d, db) = book();
    let custom = |bps: i64| plan::PlanSetup {
        mode: "couple".into(),
        split: "custom".into(),
        custom_a_bps: bps,
        ..plan::PlanSetup::default()
    };
    db.with(|c| {
        plan::save(c, "2026-11", &[item("expense", "Rent", 100_000)])?;
        plan::save_setup(c, "2026-11", &custom(6_500))
    })
    .unwrap();
    let p = &db.with(plan::list).unwrap()[0];
    assert_eq!(
        (p.setup.split.as_str(), p.setup.custom_a_bps),
        ("custom", 6_500)
    );
    assert_eq!(p.items.len(), 1, "the boxes survive the table rebuild");
    for bad in [-1, 10_001] {
        assert!(
            db.with(|c| plan::save_setup(c, "2026-11", &custom(bad)))
                .is_err()
        );
    }
    // Deleting the month still takes its boxes with it.
    db.with(|c| plan::delete(c, "2026-11")).unwrap();
    let left: i64 = db
        .with(|c| Ok(c.query_row("SELECT count(*) FROM plan_item", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(left, 0);
}
