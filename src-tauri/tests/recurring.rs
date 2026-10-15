//! Recurring payments found in a real schema: a subscription at a steady
//! amount, a bill that wanders, and a shop visited often that is neither.

use chrono::NaiveDate;
use coin_heaven_lib::commands::recurring::detect;
use coin_heaven_lib::db::Db;
use rusqlite::params;
use secrecy::SecretString;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn book() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("r.db"),
        &SecretString::from("0".repeat(64)),
    )
    .unwrap();
    db.with(|c| {
        c.execute(
            "INSERT INTO account (id, name, kind, currency) VALUES (1, 'Main', 'checking', 'EUR')",
            [],
        )?;
        let mut id = 0;
        let mut add = |on: String, desc: &str, minor: i64| {
            id += 1;
            c.execute(
                "INSERT INTO txn (id, account_id, booked_on, description_raw, description_norm,
                                  amount_minor, currency, dedupe_hash)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, 'EUR', ?6)",
                params![
                    id,
                    on,
                    desc,
                    coin_heaven_lib::security::pii::normalise_description(desc),
                    minor,
                    format!("h{id}")
                ],
            )
            .unwrap();
        };
        // Twelve months of a streaming subscription on the 3rd, exactly 12,99.
        for m in 1..=12 {
            add(format!("2026-{m:02}-03"), "STREAMFLIX MONTHLY", -1299);
        }
        // A power bill on the 10th that wanders between 60 and 110.
        for (m, v) in [
            (1, 6100),
            (2, 9800),
            (3, 7200),
            (4, 10400),
            (5, 6500),
            (6, 8800),
        ] {
            add(format!("2026-{m:02}-10"), "ELECTRA ENERGIA", -v);
        }
        // A supermarket: often, irregular, and never the same amount.
        for (day, v) in [
            ("2026-01-02", 4120),
            ("2026-01-04", 1830),
            ("2026-01-11", 7710),
            ("2026-01-12", 2210),
            ("2026-01-19", 5530),
            ("2026-02-01", 3390),
            ("2026-02-03", 8800),
        ] {
            add(day.into(), "GREEN MARKET", -v);
        }
        // An insurance paid once a year, two years running.
        add("2025-03-15".into(), "SAFEHOME INSURANCE", -18_000);
        add("2026-03-15".into(), "SAFEHOME INSURANCE", -18_000);
        // A subscription that stopped in March.
        for m in 1..=3 {
            add(format!("2026-{m:02}-20"), "OLDGYM CLUB", -3000);
        }
        // Income is not a recurring payment.
        for m in 1..=12 {
            add(format!("2026-{m:02}-28"), "ACME PAYROLL", 250_000);
        }
        Ok(())
    })
    .unwrap();
    (dir, db)
}

#[test]
fn subscriptions_are_found_and_shopping_habits_are_not() {
    let (_d, db) = book();
    let r = db.with(|c| detect(c, d("2026-12-20"))).unwrap();
    let find = |n: &str| r.items.iter().find(|i| i.name.contains(n));

    let s = find("STREAMFLIX").expect("the subscription");
    assert_eq!(s.every, "monthly");
    assert_eq!(s.typical_minor, 1299);
    assert!(!s.varies);
    assert_eq!(s.count, 12);
    assert!(s.active);
    assert_eq!(s.yearly_minor, 1299 * 365 / 30);

    let p = find("ELECTRA").expect("the power bill");
    assert_eq!(p.every, "monthly");
    assert!(p.varies, "its amounts wander");

    let y = find("SAFEHOME").expect("the yearly insurance");
    assert_eq!(y.every, "yearly");
    assert_eq!(y.typical_minor, 18_000);
    assert_eq!(y.yearly_minor, 18_000);

    assert!(
        find("GREEN MARKET").is_none(),
        "a habit, not a subscription"
    );
    assert!(find("PAYROLL").is_none(), "income is not a payment");
}

#[test]
fn what_has_stopped_is_listed_but_not_counted() {
    let (_d, db) = book();
    let r = db.with(|c| detect(c, d("2026-12-20"))).unwrap();
    let gym = r.items.iter().find(|i| i.name.contains("OLDGYM")).unwrap();
    assert!(!gym.active, "last paid in March");
    assert_eq!(gym.last_on, "2026-03-20");

    let active_yearly: i64 = r
        .items
        .iter()
        .filter(|i| i.active)
        .map(|i| i.yearly_minor)
        .sum();
    assert_eq!(r.yearly_minor, active_yearly);
    assert_eq!(r.monthly_minor, active_yearly / 12);
    // Active ones come first.
    let first_stopped = r.items.iter().position(|i| !i.active).unwrap();
    assert!(r.items[first_stopped..].iter().all(|i| !i.active));
}
