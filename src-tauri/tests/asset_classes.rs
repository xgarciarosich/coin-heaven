//! Net worth once asset classes are the user's to define.
//!
//! The arithmetic keys off a class's `kind`, not its name, and three sums read
//! the same rows in different ways: net worth, the FIRE liquid side, and
//! property equity. A liability counted once in one and twice in another is
//! the kind of discrepancy that only shows up as a number the user knows is
//! wrong but cannot explain, so it is pinned down here.

use coin_heaven_lib::commands::latest_balances;
use coin_heaven_lib::db::Db;
use rusqlite::params;
use secrecy::SecretString;

fn book() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("atlas.db"),
        &SecretString::from("0".repeat(64)),
    )
    .unwrap();
    db.with(|c| {
        c.execute(
            "INSERT INTO wealth_snapshot (id, taken_on, is_scheduled) VALUES (1, '2026-06-30', 1)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    (dir, db)
}

/// Euros as minor units, so the tests read in the units a person thinks in.
fn eur(major: i64) -> i64 {
    major * 100
}

fn line(db: &Db, bucket: &str, label: &str, gross: i64, debt: i64) {
    db.with(|c| {
        c.execute(
            "INSERT INTO wealth_line (snapshot_id, bucket, label, gross_value_minor, debt_minor)
             VALUES (1, ?1, ?2, ?3, ?4)",
            params![bucket, label, gross, debt],
        )?;
        Ok(())
    })
    .unwrap();
}

fn net_worth(db: &Db) -> (i64, i64, i64, i64, i64) {
    db.with(|c| {
        Ok(c.query_row(
            "SELECT gross_assets_minor, total_debt_minor, net_worth_minor,
                    liquid_minor, re_equity_minor
             FROM v_net_worth WHERE snapshot_id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?)
    })
    .unwrap()
}

#[test]
fn the_eight_original_buckets_survive_as_editable_classes() {
    let (_d, db) = book();
    let names: Vec<(String, String)> = db
        .with(|c| {
            let mut stmt = c.prepare("SELECT key, kind FROM asset_class ORDER BY sort_order")?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap();
    assert_eq!(names.len(), 8);
    assert_eq!(names[0], ("cash".into(), "liquid".into()));
    assert_eq!(names[4], ("real_estate".into(), "property".into()));
    assert_eq!(names[7], ("other_debt".into(), "liability".into()));
}

#[test]
fn a_property_is_worth_its_equity_and_a_loan_is_subtracted_once() {
    let (_d, db) = book();
    line(&db, "cash", "Current account", eur(10_000), 0);
    line(&db, "real_estate", "Flat", eur(200_000), eur(120_000));
    line(&db, "other_debt", "Car loan", 0, eur(8_000));

    let (gross, debt, net, liquid, equity) = net_worth(&db);
    assert_eq!(gross, eur(210_000), "the car loan is not an asset");
    assert_eq!(debt, eur(128_000), "mortgage plus car loan");
    assert_eq!(net, eur(10_000) + eur(80_000) - eur(8_000));
    assert_eq!(liquid, eur(10_000), "the flat is not liquid");
    assert_eq!(equity, eur(80_000));
}

#[test]
fn a_liability_entered_as_a_value_rather_than_a_debt_still_subtracts_once() {
    // Nothing stops a user typing the amount owed into the value field, and a
    // loan that added to net worth would be a spectacular way to be wrong.
    let (_d, db) = book();
    line(&db, "other_debt", "Car loan", eur(8_000), 0);
    let (gross, debt, net, _, _) = net_worth(&db);
    assert_eq!(gross, 0);
    assert_eq!(debt, eur(8_000));
    assert_eq!(net, -eur(8_000));
}

#[test]
fn a_class_the_user_invents_counts_in_net_worth_but_not_in_the_fire_target() {
    let (_d, db) = book();
    db.with(|c| {
        c.execute(
            "INSERT INTO asset_class (key, name, kind, sort_order)
             VALUES ('business', 'Business stake', 'other_asset', 90)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    line(&db, "cash", "Current account", eur(20_000), 0);
    line(&db, "business", "Half of the consultancy", eur(150_000), 0);

    let (_, _, net, liquid, _) = net_worth(&db);
    assert_eq!(net, eur(170_000));
    assert_eq!(
        liquid,
        eur(20_000),
        "an illiquid stake cannot fund withdrawals"
    );

    let b = db.with(latest_balances).unwrap().unwrap();
    assert_eq!(b.liquid.minor(), eur(20_000));
    assert_eq!(b.other_assets.minor(), eur(150_000));

    // Until its owner says it does. The kind stays what it was; only the
    // answer to "does this count toward FIRE" changes, and it changes in the
    // view and in the FIRE figures together.
    db.with(|c| {
        c.execute(
            "UPDATE asset_class SET counts_fire = 1 WHERE key = 'business'",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(net_worth(&db).3, eur(170_000));
    let b = db.with(latest_balances).unwrap().unwrap();
    assert_eq!(b.liquid.minor(), eur(170_000));
    assert_eq!(b.other_assets.minor(), 0, "counted once, not twice");
}

#[test]
fn unticking_a_pension_moves_it_out_of_the_fire_side() {
    let (_d, db) = book();
    line(&db, "cash", "Current account", eur(5_000), 0);
    line(&db, "pension", "Workplace pension", eur(90_000), 0);

    assert_eq!(net_worth(&db).3, eur(95_000), "liquid by default");

    db.with(|c| {
        c.execute(
            "UPDATE asset_class SET counts_fire = 0 WHERE key = 'pension'",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let (_, _, net, liquid, _) = net_worth(&db);
    assert_eq!(liquid, eur(5_000), "locked until a statutory age");
    assert_eq!(net, eur(95_000), "still owned, still counted");
}

#[test]
fn the_net_worth_view_and_the_fire_balances_agree_on_liquid() {
    let (_d, db) = book();
    line(&db, "cash", "Current account", eur(12_345), 0);
    line(&db, "equities", "Vanguard", eur(88_000), 0);
    line(&db, "pension", "Workplace", eur(40_000), 0);
    line(&db, "real_estate", "Flat", eur(200_000), eur(120_000));

    let view_liquid = net_worth(&db).3;
    let fire_liquid = db.with(latest_balances).unwrap().unwrap().liquid;
    assert_eq!(
        view_liquid,
        fire_liquid.minor(),
        "one definition of liquid, or the two screens contradict each other"
    );
}

#[test]
fn a_snapshot_line_cannot_name_a_class_that_does_not_exist() {
    let (_d, db) = book();
    let bad = db.with(|c| {
        Ok(c.execute(
            "INSERT INTO wealth_line (snapshot_id, bucket, label, gross_value_minor, debt_minor)
             VALUES (1, 'unicorns', 'Nope', 100, 0)",
            [],
        ))
    });
    assert!(bad.unwrap().is_err(), "the foreign key should refuse it");
}
