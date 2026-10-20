//! The way out: everything written to CSV a spreadsheet opens, and read back
//! the same.

use coin_heaven_lib::commands::export::{Style, export_all};
use coin_heaven_lib::commands::plan::{self, PlanItem};
use coin_heaven_lib::db::Db;
use rusqlite::params;
use secrecy::SecretString;

fn book() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("e.db"),
        &SecretString::from("0".repeat(64)),
    )
    .unwrap();
    db.with(|c| {
        c.execute(
            "INSERT INTO account (id, name, kind, currency) VALUES (1, 'Main', 'checking', 'EUR')",
            [],
        )?;
        c.execute(
            "INSERT INTO account (id, name, kind, currency) VALUES (2, 'Tokyo', 'checking', 'JPY')",
            [],
        )?;
        let add = |id: i64, acc: i64, on: &str, desc: &str, minor: i64, cur: &str| {
            c.execute(
                "INSERT INTO txn (id, account_id, booked_on, description_raw, description_norm,
                                  amount_minor, currency, dedupe_hash, notes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'a, \"quoted\" note')",
                params![
                    id,
                    acc,
                    on,
                    desc,
                    coin_heaven_lib::security::pii::normalise_description(desc),
                    minor,
                    cur,
                    format!("h{id}")
                ],
            )
            .unwrap();
        };
        add(1, 1, "2026-01-05", "CAFÉ, CENTRAL", -227, "EUR");
        add(2, 1, "2026-01-07", "NOMINA", 240_000, "EUR");
        add(3, 2, "2026-01-09", "RAMEN", -1_500, "JPY");
        Ok(())
    })
    .unwrap();
    (dir, db)
}

fn read(dir: &std::path::Path, name: &str) -> String {
    let bytes = std::fs::read(dir.join(name)).unwrap();
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "{name}: a BOM, for Excel");
    String::from_utf8(bytes[3..].to_vec()).unwrap()
}

#[test]
fn transactions_are_written_with_the_amounts_a_spreadsheet_can_read() {
    let (_d, db) = book();
    let out = tempfile::tempdir().unwrap();

    let files = db
        .with(|c| {
            export_all(
                c,
                out.path(),
                Style {
                    delimiter: b',',
                    decimal: '.',
                },
            )
        })
        .unwrap();
    assert!(
        files
            .iter()
            .any(|f| f.name == "transactions.csv" && f.rows == 3)
    );

    let csv = read(out.path(), "transactions.csv");
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(
        lines[0],
        "id,date,account,description,counterparty,amount,currency,category,event,internal_transfer,notes"
    );
    // A comma inside a field is quoted, and so is a quote inside a note.
    assert!(lines[1].contains("\"CAFÉ, CENTRAL\""), "{}", lines[1]);
    assert!(lines[1].contains("-2.27"), "{}", lines[1]);
    assert!(
        lines[1].contains("\"a, \"\"quoted\"\" note\""),
        "{}",
        lines[1]
    );
    assert!(lines[2].contains(",2400.00,"));
    // Yen have no decimals: 1.500 yen is 1500, not 15.
    assert!(lines[3].contains(",-1500,JPY,"), "{}", lines[3]);
}

#[test]
fn a_comma_decimal_region_gets_semicolons_and_decimal_commas() {
    let (_d, db) = book();
    let out = tempfile::tempdir().unwrap();
    db.with(|c| {
        export_all(
            c,
            out.path(),
            Style {
                delimiter: b';',
                decimal: ',',
            },
        )
    })
    .unwrap();
    let csv = read(out.path(), "transactions.csv");
    let line = csv.lines().nth(1).unwrap();
    assert!(line.contains(";-2,27;EUR;"), "{line}");
    assert!(line.starts_with("1;2026-01-05;Main;"), "{line}");
}

#[test]
fn plans_and_the_other_tables_come_out_too() {
    let (_d, db) = book();
    db.with(|c| {
        plan::save(
            c,
            "2026-10",
            &[
                PlanItem {
                    kind: "income".into(),
                    name: "Pay".into(),
                    amount_minor: 100_000,
                    owner: "shared".into(),
                },
                PlanItem {
                    kind: "expense".into(),
                    name: "Rent".into(),
                    amount_minor: 45_050,
                    owner: "shared".into(),
                },
            ],
        )?;
        c.execute(
            "INSERT INTO wealth_snapshot (id, taken_on) VALUES (1, '2026-06-30')",
            [],
        )?;
        c.execute(
            "INSERT INTO wealth_line (snapshot_id, bucket, label, gross_value_minor, debt_minor, currency)
             VALUES (1, 'real_estate', 'Flat', 20000000, 15000000, 'EUR')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    let out = tempfile::tempdir().unwrap();
    let files = db
        .with(|c| {
            export_all(
                c,
                out.path(),
                Style {
                    delimiter: b',',
                    decimal: '.',
                },
            )
        })
        .unwrap();
    let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
    for n in [
        "transactions.csv",
        "categories.csv",
        "net_worth.csv",
        "holdings.csv",
        "holding_movements.csv",
        "holding_prices.csv",
        "budgets.csv",
        "monthly_plans.csv",
    ] {
        assert!(names.contains(&n), "missing {n}");
    }
    let plans = read(out.path(), "monthly_plans.csv");
    assert!(plans.contains("2026-10,1,income,Pay,1000.00"), "{plans}");
    assert!(plans.contains("2026-10,2,expense,Rent,450.50"), "{plans}");
    let nw = read(out.path(), "net_worth.csv");
    assert!(nw.contains("2026-06-30,"), "{nw}");
    assert!(nw.contains(",Flat,200000.00,150000.00,EUR,manual"), "{nw}");
}
