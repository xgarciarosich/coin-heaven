use super::*;
use crate::import::table;

fn detect_csv(csv: &str) -> Detection {
    detect(&table::from_delimited(csv.as_bytes(), Some("utf-8"), None).unwrap())
}

/* ------------------------------------------------------------ the easy -- */

#[test]
fn a_spanish_signed_amount_export_is_read_without_being_told_anything() {
    let d = detect_csv(
        "Fecha;Concepto;Importe\n\
         05/01/2026;MERCADONA VALENCIA;-45,20\n\
         07/01/2026;NOMINA ENERO;2.400,00\n\
         20/01/2026;TRATTORIA;-38,00\n",
    );
    let m = &d.mapping;
    assert_eq!(m.delimiter, ';');
    assert_eq!(m.date_col, 0);
    assert_eq!(m.date_format, "%d/%m/%Y");
    assert_eq!(m.description_col, 1);
    assert_eq!(m.amount_col, Some(2));
    assert_eq!(m.decimal_separator, ',');
    assert_eq!(m.thousands_separator, ".");
    assert!(m.has_header);
    assert_eq!(m.skip_rows, 0);
    assert!(d.confidence >= 80, "confidence was {}", d.confidence);
}

#[test]
fn an_anglo_iso_export_is_read_too() {
    let d = detect_csv(
        "Date,Description,Amount\n\
         2026-01-05,TESCO METRO,-45.20\n\
         2026-01-07,SALARY,2400.00\n",
    );
    assert_eq!(d.mapping.delimiter, ',');
    assert_eq!(d.mapping.date_format, "%Y-%m-%d");
    assert_eq!(d.mapping.decimal_separator, '.');
    assert_eq!(d.mapping.thousands_separator, ",");
    assert_eq!(d.mapping.amount_col, Some(2));
}

/* --------------------------------------------------------- the awkward -- */

#[test]
fn a_named_debit_credit_file_is_flagged_rather_than_quietly_half_read() {
    // Support was removed deliberately. Picking either column alone would drop
    // half the statement while still looking like a successful import, so the
    // shape is recognised only to refuse it clearly.
    let d = detect_csv(
        "Fecha;Concepto;Debe;Haber\n\
         05/01/2026;ALQUILER;850,00;\n\
         07/01/2026;TRANSFERENCIA;;1.200,00\n",
    );
    assert!(
        d.warnings.iter().any(|w| w.contains("debit and credit")),
        "expected a warning about the unsupported shape, got {:?}",
        d.warnings
    );
}

#[test]
fn two_columns_never_both_filled_are_flagged_even_without_helpful_headers() {
    let d = detect_csv(
        "A;B;C;D\n\
         05/01/2026;ALQUILER;850,00;\n\
         07/01/2026;NOMINA;;1.200,00\n\
         09/01/2026;COMPRA;45,00;\n",
    );
    assert!(
        d.warnings.iter().any(|w| w.contains("debit and credit")),
        "got {:?}",
        d.warnings
    );
}

#[test]
fn a_running_balance_column_is_not_mistaken_for_the_amount() {
    // Reading the balance as the amount turns a statement into nonsense, so
    // this is the single most damaging thing detection could get wrong.
    let d = detect_csv(
        "Fecha;Concepto;Importe;Saldo\n\
         05/01/2026;MERCADONA;-45,20;1.954,80\n\
         07/01/2026;NOMINA;2.400,00;4.354,80\n\
         09/01/2026;LUZ;-80,00;4.274,80\n",
    );
    assert_eq!(d.mapping.amount_col, Some(2), "picked the balance column");
    assert!(d.notes.iter().any(|n| n.contains("balance")));
}

#[test]
fn preamble_lines_above_the_header_are_skipped() {
    let d = detect_csv(
        "Extracto de cuenta;;\n\
         Titular: X;;\n\
         ;;\n\
         Fecha;Concepto;Importe\n\
         05/01/2026;CAFE;-2,00\n\
         06/01/2026;PAN;-1,50\n",
    );
    assert!(d.mapping.has_header);
    assert_eq!(d.mapping.skip_rows, 3, "the header sits on line 4");
    assert_eq!(d.mapping.date_col, 0);
    assert!(d.notes.iter().any(|n| n.contains("preamble")));
}

#[test]
fn a_file_with_no_header_is_still_read() {
    let d = detect_csv("05/01/2026;CAFE;-2,00\n06/01/2026;PAN;-1,50\n");
    assert!(!d.mapping.has_header);
    assert_eq!(d.mapping.skip_rows, 0);
    assert_eq!(d.mapping.amount_col, Some(2));
}

#[test]
fn the_description_is_the_wordiest_column_when_headers_do_not_say() {
    let d = detect_csv(
        "A;B;C;D\n\
         05/01/2026;X;SUPERMERCADO EL CORTE INGLES MADRID;-45,20\n\
         07/01/2026;Y;TRANSFERENCIA RECIBIDA DE NOMINA;2.400,00\n",
    );
    assert_eq!(d.mapping.description_col, 2);
}

#[test]
fn a_value_date_column_does_not_displace_the_booking_date() {
    let d = detect_csv(
        "Fecha;F. Valor;Concepto;Importe\n\
         05/01/2026;06/01/2026;MERCADONA;-45,20\n\
         07/01/2026;08/01/2026;NOMINA;2.400,00\n",
    );
    assert_eq!(
        d.mapping.date_col, 0,
        "the first named date column should win"
    );
}

/* ------------------------------------------------------- the ambiguous -- */

#[test]
fn an_undecidable_day_or_month_order_is_flagged_rather_than_guessed_silently() {
    // Every day here is <= 12, so 03/04 could be 3 April or 4 March.
    let d = detect_csv(
        "Fecha;Concepto;Importe\n\
         03/04/2026;CAFE;-2,00\n\
         05/06/2026;PAN;-1,50\n",
    );
    assert!(
        d.warnings.iter().any(|w| w.contains("day-first")),
        "expected an ambiguity warning, got {:?}",
        d.warnings
    );
}

#[test]
fn a_day_above_twelve_settles_the_order_with_no_warning() {
    let d = detect_csv(
        "Fecha;Concepto;Importe\n\
         25/04/2026;CAFE;-2,00\n\
         05/06/2026;PAN;-1,50\n",
    );
    assert_eq!(d.mapping.date_format, "%d/%m/%Y");
    assert!(d.warnings.is_empty(), "got {:?}", d.warnings);
}

#[test]
fn a_file_that_is_not_a_statement_reports_low_confidence() {
    let d = detect_csv("hello world\nthis is not a bank export\n");
    assert!(d.confidence < 50, "confidence was {}", d.confidence);
}

/* -------------------------------------------------------------- pieces -- */

#[test]
fn number_shapes_are_told_apart() {
    assert_eq!(number_shape("1.234,56"), Some(','));
    assert_eq!(number_shape("1,234.56"), Some('.'));
    assert_eq!(number_shape("-45,00"), Some(','));
    assert_eq!(number_shape("€ 1.234,56"), Some(','));
    assert_eq!(number_shape("45,00-"), Some(','));
    assert_eq!(number_shape("MERCADONA"), None);
    assert_eq!(number_shape(""), None);
}

#[test]
fn thousands_only_numbers_do_not_vote_the_wrong_way() {
    // "1.234" is one thousand two hundred, not 1.234 — three trailing digits
    // rule out a decimal mark.
    assert_eq!(number_shape("1.234"), Some(','));
    assert_eq!(number_shape("1,234"), Some('.'));
}

#[test]
fn every_supported_date_format_is_recognised() {
    for (s, f) in [
        ("2026-01-05", "%Y-%m-%d"),
        ("05/01/2026", "%d/%m/%Y"),
        ("05-01-2026", "%d-%m-%Y"),
        ("05.01.2026", "%d.%m.%Y"),
        ("20260105", "%Y%m%d"),
    ] {
        assert_eq!(detect_date_format(s), Some(f), "for {s}");
    }
    assert_eq!(detect_date_format("MERCADONA"), None);
    assert_eq!(detect_date_format("45,20"), None);
}

#[test]
fn detection_output_feeds_straight_back_into_the_parser() {
    // The contract that matters: whatever detection returns must actually
    // parse the file it was derived from.
    let csv = "Fecha;Concepto;Importe;Saldo\n\
               05/01/2026;MERCADONA;-45,20;1.954,80\n\
               07/01/2026;NOMINA;2.400,00;4.354,80\n";
    let t = table::from_delimited(csv.as_bytes(), Some("utf-8"), None).unwrap();
    let d = detect(&t);
    let parsed = crate::import::parser::parse(&t, &d.mapping);

    assert_eq!(
        parsed.rejected,
        vec![],
        "detection produced an unusable mapping"
    );
    assert_eq!(parsed.rows.len(), 2);
    assert_eq!(parsed.rows[0].description, "MERCADONA");
    assert_eq!(
        parsed.rows[0].amount_minor,
        crate::money::Money::from_major(-45.20)
    );
    assert_eq!(
        parsed.rows[1].amount_minor,
        crate::money::Money::from_major(2400.0)
    );
}

/* --------------------------------------------------- dates with a time -- */

#[test]
fn a_statement_whose_dates_carry_a_time_is_detected_and_parsed() {
    let csv = "Fecha;Concepto;Importe\n\
               2026-01-05 14:30:00;MERCADONA;-45,20\n\
               2026-01-07 09:15:00;NOMINA;2.400,00\n";
    let t = table::from_delimited(csv.as_bytes(), Some("utf-8"), None).unwrap();
    let d = detect(&t);

    assert_eq!(d.mapping.date_format, "%Y-%m-%d %H:%M:%S");
    let parsed = crate::import::parser::parse(&t, &d.mapping);
    assert_eq!(parsed.rejected, vec![]);
    assert_eq!(
        parsed.rows[0].booked_on, "2026-01-05",
        "the time should be dropped"
    );
    assert_eq!(parsed.rows[1].booked_on, "2026-01-07");
}

#[test]
fn an_iso_timestamp_column_is_detected() {
    let csv = "date,description,amount\n\
               2026-01-05T14:30:00,TESCO,-45.20\n\
               2026-01-07T09:15:00,SALARY,2400.00\n";
    let t = table::from_delimited(csv.as_bytes(), Some("utf-8"), None).unwrap();
    let d = detect(&t);
    assert_eq!(d.mapping.date_format, "%Y-%m-%dT%H:%M:%S");
    assert_eq!(crate::import::parser::parse(&t, &d.mapping).rows.len(), 2);
}

#[test]
fn a_hand_picked_date_format_still_reads_a_column_that_has_a_time() {
    // The user picks "Year-month-day" and the column turns out to be stamped.
    // Rejecting every row over that would be unhelpful and surprising.
    let csv = "Fecha;Concepto;Importe\n2026-01-05 14:30:00;CAFE;-2,00\n";
    let t = table::from_delimited(csv.as_bytes(), Some("utf-8"), None).unwrap();
    let mut m = detect(&t).mapping;
    m.date_format = "%Y-%m-%d".into();

    let parsed = crate::import::parser::parse(&t, &m);
    assert_eq!(parsed.rejected, vec![]);
    assert_eq!(parsed.rows[0].booked_on, "2026-01-05");
}

#[test]
fn an_american_month_first_file_is_detected_when_the_day_settles_it() {
    let csv = "Date,Description,Amount\n\
               03/25/2026,WHOLE FOODS,-45.20\n\
               04/01/2026,PAYCHECK,2400.00\n";
    let t = table::from_delimited(csv.as_bytes(), Some("utf-8"), None).unwrap();
    let d = detect(&t);
    assert_eq!(d.mapping.date_format, "%m/%d/%Y");
    assert_eq!(crate::import::parser::parse(&t, &d.mapping).rows.len(), 2);
}
