//! End-to-end check against a realistic statement kept in the repository.
//!
//! The unit tests use small hand-written snippets, which is where detection
//! rules are easiest to reason about but also easiest to accidentally tune to.
//! This runs the whole pipeline over a file shaped like the real thing:
//! Windows-1252 accents, a two-line preamble, semicolon delimiters, comma
//! decimals with dot thousands, a running-balance column, and a junk total row
//! at the bottom.

use coin_heaven_lib::import::{parser, sniff, table};

const SAMPLE: &str = "../samples/movimientos-ejemplo.csv";

fn load() -> table::Table {
    let bytes = std::fs::read(SAMPLE)
        .unwrap_or_else(|e| panic!("sample statement missing at {SAMPLE}: {e}"));
    table::load(SAMPLE, &bytes, None, None).expect("the sample should load")
}

#[test]
fn a_realistic_spanish_statement_needs_no_configuration() {
    let t = load();
    let d = sniff::detect(&t);

    assert_eq!(d.mapping.delimiter, ';');
    assert_eq!(
        d.mapping.encoding, "windows-1252",
        "Latin-1 should be detected"
    );
    assert_eq!(d.mapping.date_col, 0);
    assert_eq!(d.mapping.date_format, "%d/%m/%Y");
    assert_eq!(d.mapping.description_col, 1);
    assert_eq!(
        d.mapping.amount_col,
        Some(2),
        "the balance column was picked instead"
    );
    assert_eq!(d.mapping.decimal_separator, ',');
    assert_eq!(d.mapping.thousands_separator, ".");
    assert!(d.mapping.has_header);
    assert_eq!(
        d.mapping.skip_rows, 2,
        "two preamble lines above the header"
    );
    assert!(d.confidence >= 80, "confidence was only {}", d.confidence);
}

#[test]
fn every_real_row_parses_and_only_the_junk_row_is_rejected() {
    let t = load();
    let d = sniff::detect(&t);
    let parsed = parser::parse(&t, &d.mapping);

    assert_eq!(parsed.rows.len(), 48, "expected every movement to parse");
    assert_eq!(
        parsed.rejected.len(),
        1,
        "only the trailing TOTAL row should fail, got {:?}",
        parsed.rejected
    );
    assert!(parsed.rejected[0].raw.contains("TOTAL"));
}

#[test]
fn accented_vendor_names_survive_the_round_trip() {
    let t = load();
    let d = sniff::detect(&t);
    let parsed = parser::parse(&t, &d.mapping);

    assert!(
        parsed
            .rows
            .iter()
            .any(|r| r.description.contains("Café Bilbao")),
        "Latin-1 accents were mangled"
    );
}

#[test]
fn signs_survive_so_income_and_spending_are_not_confused() {
    let t = load();
    let d = sniff::detect(&t);
    let parsed = parser::parse(&t, &d.mapping);

    let income = parsed
        .rows
        .iter()
        .filter(|r| r.amount_minor.minor() > 0)
        .count();
    let spend = parsed
        .rows
        .iter()
        .filter(|r| r.amount_minor.minor() < 0)
        .count();
    assert!(income > 0, "no incoming rows — sign handling is wrong");
    assert!(spend > income, "a statement should be mostly outgoings");

    let salary = parsed
        .rows
        .iter()
        .find(|r| r.description.contains("NOMINA"))
        .expect("the salary rows should be present");
    assert!(salary.amount_minor.minor() > 0, "salary should be positive");
}
