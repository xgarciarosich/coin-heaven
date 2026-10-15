use super::*;

/* ----------------------------------------------------------- delimiter -- */

#[test]
fn a_semicolon_file_is_not_fooled_by_commas_inside_amounts() {
    // The classic European trap: every row contains commas, but they are
    // decimal marks. Counting occurrences would pick ',' and shred the file.
    let csv = "Fecha;Concepto;Importe\n\
               05/01/2026;MERCADONA;-45,20\n\
               07/01/2026;NOMINA;2.400,00\n";
    assert_eq!(sniff_delimiter(csv), ';');
}

#[test]
fn a_comma_file_is_detected_as_comma() {
    let csv = "Date,Description,Amount\n\
               2026-01-05,TESCO,-45.20\n\
               2026-01-07,SALARY,2400.00\n";
    assert_eq!(sniff_delimiter(csv), ',');
}

#[test]
fn a_tab_file_is_detected() {
    let tsv = "Date\tDescription\tAmount\n2026-01-05\tSHOP\t-4.00\n2026-01-06\tBUS\t-2.00\n";
    assert_eq!(sniff_delimiter(tsv), '\t');
}

#[test]
fn a_quoted_field_containing_the_delimiter_stays_one_cell() {
    let t = from_delimited(
        b"Fecha;Concepto;Importe\n05/01/2026;\"BAR; RESTAURANTE\";-30,00\n",
        Some("utf-8"),
        None,
    )
    .unwrap();
    assert_eq!(t.cell(1, 1), "BAR; RESTAURANTE");
    assert_eq!(t.rows[1].len(), 3);
}

#[test]
fn doubled_quotes_inside_a_quoted_field_are_unescaped() {
    let t = from_delimited(b"a;b\n1;\"He said \"\"hi\"\"\"\n", Some("utf-8"), None).unwrap();
    assert_eq!(t.cell(1, 1), r#"He said "hi""#);
}

/* ------------------------------------------------------------ encoding -- */

#[test]
fn valid_utf8_is_recognised_without_being_told() {
    let (text, enc) = decode("Café Bilbao".as_bytes(), None);
    assert_eq!(text, "Café Bilbao");
    assert_eq!(enc, "utf-8");
}

#[test]
fn invalid_utf8_falls_back_to_windows_1252_rather_than_mangling() {
    let mut bytes = b"Caf".to_vec();
    bytes.push(0xE9); // é in Latin-1
    bytes.extend_from_slice(b" Bilbao");

    let (text, enc) = decode(&bytes, None);
    assert_eq!(text, "Café Bilbao");
    assert_eq!(enc, "windows-1252");
}

#[test]
fn a_utf8_bom_is_consumed_not_left_in_the_first_cell() {
    let mut bytes = vec![0xEF, 0xBB, 0xBF];
    bytes.extend_from_slice(b"Fecha;Concepto\n05/01/2026;CAFE\n");
    let t = from_delimited(&bytes, None, None).unwrap();
    assert_eq!(
        t.cell(0, 0),
        "Fecha",
        "a BOM must not become part of the header"
    );
}

#[test]
fn a_forced_encoding_overrides_detection() {
    let bytes = vec![0xE9];
    let (text, enc) = decode(&bytes, Some("windows-1252"));
    assert_eq!(text, "é");
    assert_eq!(enc, "windows-1252");
}

/* ------------------------------------------------------------- routing -- */

#[test]
fn spreadsheet_magic_bytes_route_to_the_spreadsheet_loader() {
    // A .csv extension on a real workbook must not be parsed as text: the
    // content decides, not the name.
    let zip_magic = b"PK\x03\x04not-really-a-workbook";
    let err = load("statement.csv", zip_magic, None, None).unwrap_err();
    assert!(
        err.to_string().contains("spreadsheet") || err.to_string().contains("workbook"),
        "expected a spreadsheet error, got: {err}"
    );
}

#[test]
fn an_xlsx_extension_routes_to_the_spreadsheet_loader() {
    let err = load("book.xlsx", b"definitely not a zip", None, None).unwrap_err();
    assert!(
        err.to_string().contains("cannot read spreadsheet"),
        "got: {err}"
    );
}

#[test]
fn plain_text_routes_to_the_delimited_loader() {
    let t = load("export.txt", b"a\tb\n1\t2\n", None, None).unwrap();
    assert!(matches!(
        t.source,
        Source::Delimited {
            delimiter: '\t',
            ..
        }
    ));
    assert_eq!(t.cell(1, 1), "2");
}

#[test]
fn an_empty_file_is_reported_rather_than_silently_accepted() {
    assert!(load("empty.csv", b"", None, None).is_err());
}

/* --------------------------------------------------------------- shape -- */

#[test]
fn width_and_data_rows_ignore_blank_lines() {
    let t = from_delimited(b"a;b;c\n1;2;3\n\n;;\n4;5;6\n", Some("utf-8"), None).unwrap();
    assert_eq!(t.width(), 3);
    assert_eq!(
        t.data_rows().count(),
        3,
        "blank and all-empty rows carry nothing"
    );
}

#[test]
fn reading_past_the_edge_returns_empty_rather_than_panicking() {
    let t = from_delimited(b"a;b\n1;2\n", Some("utf-8"), None).unwrap();
    assert_eq!(t.cell(99, 99), "");
    assert_eq!(t.cell(0, 5), "");
}

/* --------------------------------------------------------------- xlsx -- */

/// A real, minimal workbook: header row, two data rows, dates stored as Excel
/// serials with a date number format. Embedded rather than read from disk so
/// the test is self-contained.
const SAMPLE_XLSX_B64: &str = "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";

fn sample_xlsx() -> Vec<u8> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(SAMPLE_XLSX_B64)
        .expect("the embedded workbook should decode")
}

#[test]
fn an_excel_workbook_is_read_into_the_same_shape_as_a_csv() {
    let t = from_spreadsheet(&sample_xlsx()).unwrap();
    assert!(matches!(&t.source, Source::Spreadsheet { sheet } if sheet == "Movimientos"));
    assert_eq!(t.width(), 3);
    assert_eq!(t.cell(0, 0), "Fecha");
    assert_eq!(t.cell(0, 2), "Importe");
    assert_eq!(t.cell(1, 1), "MERCADONA VALENCIA");
}

#[test]
fn excel_date_serials_become_iso_dates_not_raw_numbers() {
    // Left as "46027" the date detector would never recognise the column.
    let t = from_spreadsheet(&sample_xlsx()).unwrap();
    let d = t.cell(1, 0);
    assert!(
        d.starts_with("20") && d.len() == 10 && d.contains('-'),
        "expected an ISO date, got {d:?}"
    );
}

#[test]
fn an_xlsx_is_detected_and_parsed_end_to_end_with_no_configuration() {
    use crate::import::{parser, sniff};

    let t = load("movimientos.xlsx", &sample_xlsx(), None, None).unwrap();
    let d = sniff::detect(&t);
    assert_eq!(d.mapping.date_col, 0);
    assert_eq!(d.mapping.description_col, 1);
    assert_eq!(d.mapping.amount_col, Some(2));
    assert_eq!(
        d.mapping.date_format, "%Y-%m-%d",
        "spreadsheet dates arrive normalised"
    );

    let parsed = parser::parse(&t, &d.mapping);
    assert_eq!(
        parsed.rejected,
        vec![],
        "detection produced an unusable mapping"
    );
    assert_eq!(parsed.rows.len(), 2);
    assert_eq!(parsed.rows[0].description, "MERCADONA VALENCIA");
    assert_eq!(
        parsed.rows[0].amount_minor,
        crate::money::Money::from_major(-45.20)
    );
    assert_eq!(
        parsed.rows[1].amount_minor,
        crate::money::Money::from_major(2400.0)
    );
}

/// A ZIP whose central directory claims `size` bytes for one entry. Nothing is
/// inflated by the guard, so the entry's data need not exist.
fn zip_claiming(size: u32, entries: u16) -> Vec<u8> {
    let mut cd = Vec::new();
    for _ in 0..entries {
        cd.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        cd.extend_from_slice(&[0u8; 16]); // versions, flags, method, time, date, crc
        cd.extend_from_slice(&0u32.to_le_bytes()); // compressed size
        cd.extend_from_slice(&size.to_le_bytes()); // uncompressed size
        cd.extend_from_slice(&[0u8; 18]); // name/extra/comment lengths, disk, attrs, offset
    }
    let mut out = cd.clone();
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 4]);
    out.extend_from_slice(&entries.to_le_bytes());
    out.extend_from_slice(&entries.to_le_bytes());
    out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[test]
fn a_workbook_that_inflates_to_gigabytes_is_refused_before_it_is_read() {
    let bomb = zip_claiming(u32::MAX - 1, 4);
    let r = from_spreadsheet(&bomb);
    assert!(
        matches!(&r, Err(AppError::Invalid(m)) if m.contains("expands")),
        "{r:?}"
    );
    let many = zip_claiming(10, 5_000);
    assert!(from_spreadsheet(&many).is_err());
}

#[test]
fn the_declared_size_is_read_from_the_directory_only() {
    let z = zip_claiming(1_000, 3);
    assert_eq!(zip_declared_size(&z), Some((3, 3_000)));
    assert_eq!(zip_declared_size(b"not a zip"), None);
}
