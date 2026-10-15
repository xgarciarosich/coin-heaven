use super::*;
use crate::import::parser::{BankMapping, parse};
use crate::import::table;
use secrecy::SecretString;

fn db() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(
        &dir.path().join("atlas.db"),
        &SecretString::from("d".repeat(64)),
    )
    .unwrap();
    db.with(|c| {
        c.execute(
            "INSERT INTO account (name, kind) VALUES ('Main','checking')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    (dir, db)
}

fn mapping() -> BankMapping {
    BankMapping {
        delimiter: ';',
        date_format: "%d/%m/%Y".into(),
        date_col: 0,
        description_col: 1,
        amount_col: Some(2),
        ..Default::default()
    }
}

const JAN: &str = "Fecha;Concepto;Importe\n\
                   05/01/2026;MERCADONA;-45,20\n\
                   07/01/2026;NOMINA;2.400,00\n\
                   20/01/2026;TRATTORIA;-38,00\n";

const JAN_FEB: &str = "Fecha;Concepto;Importe\n\
                       05/01/2026;MERCADONA;-45,20\n\
                       07/01/2026;NOMINA;2.400,00\n\
                       20/01/2026;TRATTORIA;-38,00\n\
                       02/02/2026;GASOLINA;-60,00\n";

fn import(db: &Db, csv: &str, name: &str) -> ImportSummary {
    let m = mapping();
    let t = table::from_delimited(csv.as_bytes(), Some("utf-8"), Some(m.delimiter)).unwrap();
    let parsed = parse(&t, &m);
    run(
        db,
        1,
        None,
        name,
        &file_sha256(csv.as_bytes()),
        &parsed,
        "EUR",
    )
    .unwrap()
}

#[test]
fn a_first_import_inserts_every_row() {
    let (_d, db) = db();
    let s = import(&db, JAN, "jan.csv");
    assert_eq!(s.row_count, 3);
    assert_eq!(s.inserted, 3);
    assert_eq!(s.duplicates, 0);
    assert_eq!(s.rejected, 0);
}

#[test]
fn re_importing_the_same_file_inserts_nothing() {
    let (_d, db) = db();
    import(&db, JAN, "jan.csv");
    let again = import(&db, JAN, "jan.csv");

    assert_eq!(again.inserted, 0, "a repeat import must not double-count");
    assert_eq!(again.duplicates, 3);

    let total: i64 = db
        .with(|c| Ok(c.query_row("SELECT count(*) FROM txn", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(total, 3);
}

#[test]
fn an_overlapping_statement_inserts_only_what_is_new() {
    // The case the original specification had no answer for.
    let (_d, db) = db();
    import(&db, JAN, "jan.csv");
    let s = import(&db, JAN_FEB, "jan-feb.csv");

    assert_eq!(s.inserted, 1);
    assert_eq!(s.duplicates, 3);

    let total: i64 = db
        .with(|c| Ok(c.query_row("SELECT count(*) FROM txn", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(total, 4);
}

#[test]
fn two_identical_purchases_on_one_day_both_survive() {
    let (_d, db) = db();
    let csv = "Fecha;Concepto;Importe\n\
               05/01/2026;CAFE CENTRAL;-2,00\n\
               05/01/2026;CAFE CENTRAL;-2,00\n";
    let s = import(&db, csv, "coffee.csv");
    assert_eq!(
        s.inserted, 2,
        "these are two real transactions, not a duplicate"
    );
}

#[test]
fn rejected_rows_are_counted_and_recorded_on_the_batch() {
    let (_d, db) = db();
    let csv = "Fecha;Concepto;Importe\n\
               05/01/2026;GOOD;-10,00\n\
               garbage;BAD;-10,00\n";
    let s = import(&db, csv, "mixed.csv");
    assert_eq!(s.inserted, 1);
    assert_eq!(s.rejected, 1);

    let (rows, ins, rej): (i64, i64, i64) = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT row_count, inserted_count, rejected_count FROM import_batch WHERE id = ?1",
                rusqlite::params![s.batch_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!((rows, ins, rej), (1, 1, 1));
}

#[test]
fn imported_rows_are_flagged_for_review() {
    let (_d, db) = db();
    import(&db, JAN, "jan.csv");
    let n: i64 = db
        .with(|c| {
            Ok(
                c.query_row("SELECT count(*) FROM txn WHERE needs_review = 1", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(n, 3, "nothing is silently considered categorised");
}

#[test]
fn undo_removes_the_batch_and_its_untouched_rows() {
    let (_d, db) = db();
    let s = import(&db, JAN, "jan.csv");
    let removed = undo(&db, s.batch_id, false).unwrap();

    assert_eq!(removed, 3);
    let total: i64 = db
        .with(|c| Ok(c.query_row("SELECT count(*) FROM txn", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(total, 0);
    let batches: i64 = db
        .with(|c| Ok(c.query_row("SELECT count(*) FROM import_batch", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(batches, 0);
}

#[test]
fn undo_leaves_rows_the_user_has_already_worked_on() {
    let (_d, db) = db();
    let s = import(&db, JAN, "jan.csv");
    db.with(|c| {
        c.execute(
            "UPDATE txn SET categorized_by = 'user', category_override_id = 1
             WHERE description_raw = 'TRATTORIA'",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let removed = undo(&db, s.batch_id, false).unwrap();
    assert_eq!(removed, 2, "the categorised row must be kept");

    let survivor: String = db
        .with(|c| Ok(c.query_row("SELECT description_raw FROM txn", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(survivor, "TRATTORIA");
}

#[test]
fn an_import_can_always_be_removed_whole_and_says_what_that_costs() {
    let (_d, db) = db();
    let s = import(&db, JAN, "jan.csv");
    db.with(|c| {
        // One row filed by the person, and a refund linked to another.
        c.execute(
            "UPDATE txn SET categorized_by = 'user', category_override_id = 1
             WHERE description_raw = 'TRATTORIA'",
            [],
        )?;
        c.execute(
            "INSERT INTO txn (account_id, booked_on, description_raw, description_norm,
                              amount_minor, currency, dedupe_hash, parent_txn_id, link_kind)
             SELECT 1, '2026-01-21', 'DEVOLUCION', 'devolucion', 1500, 'EUR', 'refund-1',
                    id, 'reimbursement' FROM txn WHERE description_raw = 'MERCADONA'",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let i = db.with(|c| impact(c, s.batch_id)).unwrap();
    assert_eq!(i.filename.as_deref(), Some("jan.csv"));
    assert_eq!(i.total, 3);
    assert_eq!(i.touched, 1, "the filed one");
    assert_eq!(i.linked_from_elsewhere, 1, "the refund");

    // Everything goes, the filed row included; the refund survives, unlinked.
    let removed = undo(&db, s.batch_id, true).unwrap();
    assert_eq!(removed, 3);
    let (left, parent, kind): (i64, Option<i64>, Option<String>) = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT count(*), max(parent_txn_id), max(link_kind) FROM txn",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap();
    assert_eq!((left, parent, kind), (1, None, None));

    // Nothing was remembered as deleted: the same file comes back whole.
    let again = import(&db, JAN, "jan.csv");
    assert_eq!(again.inserted, 3);
}

#[test]
fn the_file_hash_is_recorded_so_an_import_is_traceable() {
    let (_d, db) = db();
    let s = import(&db, JAN, "jan.csv");
    let (name, sha): (String, String) = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT source_filename, source_sha256 FROM import_batch WHERE id = ?1",
                rusqlite::params![s.batch_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(name, "jan.csv");
    assert_eq!(sha, file_sha256(JAN.as_bytes()));
    assert_eq!(sha.len(), 64);
}

#[test]
fn a_deleted_row_stays_deleted_and_a_corrected_one_stays_corrected() {
    let (_d, db) = db();
    import(&db, JAN, "jan.csv");

    db.with(|c| {
        // The trattoria is deleted; the Mercadona amount is corrected by hand.
        let trattoria: i64 = c.query_row(
            "SELECT id FROM txn WHERE description_raw = 'TRATTORIA'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(crate::commands::ledger::delete(c, &[trattoria])?, 1);
        c.execute(
            "UPDATE txn SET amount_minor = -4250 WHERE description_raw = 'MERCADONA'",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    // The overlapping statement brings back neither.
    let s = import(&db, JAN_FEB, "jan-feb.csv");
    assert_eq!(s.inserted, 1, "only February's row is new");
    assert_eq!(s.duplicates, 3);

    let (rows, mercadona): (i64, i64) = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT count(*), SUM(CASE WHEN description_raw = 'MERCADONA'
                                           THEN amount_minor ELSE 0 END) FROM txn",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(rows, 3);
    assert_eq!(mercadona, -4250);
}

#[test]
fn deleting_a_bill_leaves_its_refund_standing_on_its_own() {
    let (_d, db) = db();
    import(&db, JAN, "jan.csv");
    db.with(|c| {
        c.execute(
            "UPDATE txn SET parent_txn_id = (SELECT id FROM txn WHERE description_raw = 'TRATTORIA'),
                            link_kind = 'reimbursement'
             WHERE description_raw = 'NOMINA'",
            [],
        )?;
        let bill: i64 = c.query_row(
            "SELECT id FROM txn WHERE description_raw = 'TRATTORIA'",
            [],
            |r| r.get(0),
        )?;
        crate::commands::ledger::delete(c, &[bill])?;
        let (parent, kind): (Option<i64>, Option<String>) = c.query_row(
            "SELECT parent_txn_id, link_kind FROM txn WHERE description_raw = 'NOMINA'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!((parent, kind), (None, None));
        Ok(())
    })
    .unwrap();
}
