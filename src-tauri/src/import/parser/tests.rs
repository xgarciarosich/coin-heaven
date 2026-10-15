use super::*;
use crate::import::table;

/// Build a table the way the loader would, so the tests exercise the real path.
fn tbl(csv: &str, delim: char) -> Table {
    table::from_delimited(csv.as_bytes(), Some("utf-8"), Some(delim)).unwrap()
}

fn eur(major: f64) -> Money {
    Money::from_major(major)
}

/* ------------------------------------------------------------- amounts -- */

#[test]
fn spanish_format_with_dot_thousands_and_comma_decimals() {
    assert_eq!(parse_amount("1.234,56", ',', ".", 2), Some(eur(1234.56)));
    assert_eq!(parse_amount("-45,00", ',', ".", 2), Some(eur(-45.0)));
    assert_eq!(parse_amount("0,05", ',', ".", 2), Some(eur(0.05)));
}

#[test]
fn anglo_format_with_comma_thousands_and_dot_decimals() {
    assert_eq!(parse_amount("1,234.56", '.', ",", 2), Some(eur(1234.56)));
    assert_eq!(parse_amount("-45.00", '.', ",", 2), Some(eur(-45.0)));
}

#[test]
fn trailing_and_parenthesised_negatives() {
    // Both appear in real exports and both mean "money left".
    assert_eq!(parse_amount("45,00-", ',', ".", 2), Some(eur(-45.0)));
    assert_eq!(parse_amount("(45,00)", ',', ".", 2), Some(eur(-45.0)));
}

#[test]
fn currency_symbols_and_spaces_are_ignored() {
    assert_eq!(parse_amount("€ 1.234,56", ',', ".", 2), Some(eur(1234.56)));
    assert_eq!(
        parse_amount("1 234,56 EUR", ',', ".", 2),
        Some(eur(1234.56))
    );
}

#[test]
fn zero_decimal_currencies_do_not_gain_two_zeroes() {
    // 1.000 JPY is 1000 minor units, not 100000.
    assert_eq!(
        parse_amount("1.000", ',', ".", 0),
        Some(Money::from_minor(1000))
    );
}

#[test]
fn a_short_or_long_fraction_is_padded_and_truncated() {
    assert_eq!(parse_amount("12,5", ',', ".", 2), Some(eur(12.50)));
    assert_eq!(parse_amount("12,509", ',', ".", 2), Some(eur(12.50)));
}

#[test]
fn junk_is_rejected_rather_than_guessed_at() {
    assert_eq!(parse_amount("", ',', ".", 2), None);
    assert_eq!(parse_amount("   ", ',', ".", 2), None);
    assert_eq!(parse_amount("n/a", ',', ".", 2), None);
}

/* --------------------------------------------------------------- files -- */

fn santander_like() -> BankMapping {
    BankMapping {
        delimiter: ';',
        date_format: "%d/%m/%Y".into(),
        date_col: 0,
        description_col: 1,
        amount_col: Some(2),
        ..Default::default()
    }
}

#[test]
fn parses_a_signed_amount_export() {
    let csv = "Fecha;Concepto;Importe\n\
               05/01/2026;MERCADONA VALENCIA;-45,20\n\
               07/01/2026;NOMINA ENERO;2.400,00\n";
    let m = santander_like();
    let r = parse(&tbl(csv, m.delimiter), &m);

    assert_eq!(r.rejected, vec![]);
    assert_eq!(r.headers, vec!["Fecha", "Concepto", "Importe"]);
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rows[0].booked_on, "2026-01-05");
    assert_eq!(r.rows[0].amount_minor, eur(-45.20));
    assert_eq!(r.rows[0].description, "MERCADONA VALENCIA");
    assert_eq!(r.rows[1].amount_minor, eur(2400.0));
}

#[test]
fn a_mapping_with_no_amount_column_rejects_every_row_and_says_why() {
    // Separate debit/credit columns are no longer supported, so a mapping that
    // has no amount column must fail loudly rather than importing zeroes.
    let m = BankMapping {
        amount_col: None,
        ..santander_like()
    };
    let csv = "Fecha;Concepto;Debe;Haber\n05/01/2026;ALQUILER;850,00;\n";
    let r = parse(&tbl(csv, m.delimiter), &m);

    assert!(r.rows.is_empty());
    assert_eq!(r.rejected.len(), 1);
    assert!(
        r.rejected[0].reason.contains("amount column"),
        "got {:?}",
        r.rejected[0]
    );
}

#[test]
fn invert_sign_handles_exports_that_write_outgoings_positive() {
    let m = BankMapping {
        invert_sign: true,
        ..santander_like()
    };
    let csv = "Fecha;Concepto;Importe\n05/01/2026;COMPRA;45,20\n";
    assert_eq!(
        parse(&tbl(csv, m.delimiter), &m).rows[0].amount_minor,
        eur(-45.20)
    );
}

#[test]
fn a_bad_row_is_rejected_and_the_rest_still_import() {
    let csv = "Fecha;Concepto;Importe\n\
               05/01/2026;GOOD ONE;-10,00\n\
               not-a-date;BROKEN;-10,00\n\
               07/01/2026;;-10,00\n\
               08/01/2026;NO AMOUNT;\n\
               09/01/2026;ANOTHER GOOD;-20,00\n";
    let m = santander_like();
    let r = parse(&tbl(csv, m.delimiter), &m);

    assert_eq!(r.rows.len(), 2, "good rows must survive a bad neighbour");
    assert_eq!(r.rejected.len(), 3);
    assert!(r.rejected[0].reason.contains("does not match"));
    assert!(r.rejected[1].reason.contains("description"));
    assert!(r.rejected[2].reason.contains("amount"));
    // Line numbers point at the real file line so the user can find it.
    assert_eq!(r.rejected[0].line, 3);
}

#[test]
fn skip_rows_handles_preamble_before_the_header() {
    let m = BankMapping {
        skip_rows: 2,
        ..santander_like()
    };
    let csv = "Extracto de cuenta;;\n\
               Titular: X;;\n\
               Fecha;Concepto;Importe\n\
               05/01/2026;CAFE;-2,00\n";
    let r = parse(&tbl(csv, m.delimiter), &m);
    assert_eq!(r.rows.len(), 1);
    assert_eq!(r.headers, vec!["Fecha", "Concepto", "Importe"]);
}

#[test]
fn a_file_without_a_header_loses_no_rows() {
    let m = BankMapping {
        has_header: false,
        ..santander_like()
    };
    let csv = "05/01/2026;CAFE;-2,00\n06/01/2026;PAN;-1,50\n";
    assert_eq!(parse(&tbl(csv, m.delimiter), &m).rows.len(), 2);
}

#[test]
fn blank_lines_are_skipped_without_being_called_errors() {
    let csv = "Fecha;Concepto;Importe\n\
               05/01/2026;CAFE;-2,00\n\
               ;;\n\
               \n\
               06/01/2026;PAN;-1,50\n";
    let m = santander_like();
    let r = parse(&tbl(csv, m.delimiter), &m);
    assert_eq!(r.rows.len(), 2);
    assert_eq!(r.rejected, vec![]);
}

#[test]
fn ragged_rows_do_not_abort_the_file() {
    let csv = "Fecha;Concepto;Importe\n\
               05/01/2026;CAFE;-2,00;extra;columns\n\
               06/01/2026;PAN\n";
    let m = santander_like();
    let r = parse(&tbl(csv, m.delimiter), &m);
    assert_eq!(r.rows.len(), 1, "the over-long row is still usable");
    assert_eq!(r.rejected.len(), 1, "the short row has no amount");
}

#[test]
fn quoted_fields_containing_the_delimiter_stay_intact() {
    let csv = "Fecha;Concepto;Importe\n05/01/2026;\"BAR; RESTAURANTE\";-30,00\n";
    let m = santander_like();
    let r = parse(&tbl(csv, m.delimiter), &m);
    assert_eq!(r.rows[0].description, "BAR; RESTAURANTE");
}

/* ------------------------------------------------------------ encoding -- */

#[test]
fn latin1_accents_survive_decoding() {
    // "Café Bilbao" in Windows-1252 — still common from Spanish banks.
    let mut bytes = b"Fecha;Concepto;Importe\n05/01/2026;Caf".to_vec();
    bytes.push(0xE9); // é
    bytes.extend_from_slice(b" Bilbao;-3,50\n");

    let (text, enc) = table::decode(&bytes, None);
    assert!(text.contains("Café Bilbao"), "got {text}");
    assert_eq!(
        enc, "windows-1252",
        "invalid UTF-8 should fall back, not mangle"
    );

    let m = santander_like();
    let t = table::from_delimited(&bytes, None, Some(';')).unwrap();
    assert_eq!(parse(&t, &m).rows[0].description, "Café Bilbao");
}

#[test]
fn valid_utf8_is_detected_and_bad_bytes_do_not_crash() {
    let (text, enc) = table::decode(
        "Fecha;Concepto;Importe\n05/01/2026;Café;-3,50\n".as_bytes(),
        None,
    );
    assert!(text.contains("Café"));
    assert_eq!(enc, "utf-8");

    let (fallback, _) = table::decode(&[0xff, 0x41, 0x42], None);
    assert!(
        !fallback.is_empty(),
        "undecodable bytes must not produce nothing"
    );
}
