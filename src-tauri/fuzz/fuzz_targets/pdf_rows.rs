//! The text of a PDF statement, read into rows: the pure half of the PDF path.
//!
//! The half that opens the file runs in a child process (`--pdf-text`) and
//! cannot be fuzzed in-process; what it hands back is plain text from outside,
//! and this is everything done with that text. It goes text, then rows, then
//! tab-separated text, then the same loader, detection and parser as any other
//! statement, and every step has to account for every line it was given.
#![no_main]
use coin_heaven_fuzz::{check_detection, check_parse, check_table};
use coin_heaven_lib::import::pdf::{rows_from_text, to_tsv};
use coin_heaven_lib::import::{parser, sniff, table};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let rows = rows_from_text(&text);

    for r in &rows {
        let [date, description, amount, balance] = r;
        assert!(!date.is_empty() && date == date.trim(), "date {date:?}");
        assert!(
            !description.is_empty() && description == description.trim(),
            "description {description:?}"
        );
        // An amount is a sign, digits and separators: nothing from the line
        // around it, and never the empty string (a movement always has one).
        assert!(!amount.is_empty(), "a row without an amount");
        for a in [amount, balance] {
            let body = a.strip_prefix('-').unwrap_or(a);
            assert!(
                body.chars().all(|c| c.is_numeric() || matches!(c, '.' | ',')),
                "amount {a:?}"
            );
        }
    }

    let tsv = to_tsv(&text);
    // Fewer than two rows is "nothing could be read", never an import of one
    // line that may be a page number.
    assert_eq!(tsv.is_ok(), rows.len() >= 2);
    let Ok(tsv) = tsv else { return };

    let lines: Vec<&str> = tsv.lines().collect();
    assert_eq!(lines.len(), rows.len() + 1, "a line for each row, and the header");
    assert_eq!(lines[0], "Date\tDescription\tAmount\tBalance");
    for l in &lines {
        assert_eq!(l.matches('\t').count(), 3, "a cell grew a tab: {l:?}");
    }

    let t = table::from_delimited(tsv.as_bytes(), Some("utf-8"), Some('\t')).unwrap();
    check_table(&t, tsv.len());
    assert_eq!(t.rows.len(), rows.len() + 1);
    for (line, row) in lines.iter().zip(&t.rows) {
        assert!(row.len() <= 4, "{line:?} became {} cells", row.len());
    }
    // Without a quote in the text the cells come back exactly as they went in.
    // (A quote is the splitter's own syntax: a lone one swallows the tabs after
    // it and the row is rejected as having no amount, which the preview shows.)
    if !tsv.contains('"') {
        for (r, got) in rows.iter().zip(&t.rows[1..]) {
            // A tab inside a wrapped description was written as a space.
            let written: Vec<String> = r.iter().map(|c| c.replace('\t', " ")).collect();
            assert_eq!(got, &written);
        }
    }

    // And on to the same detection and parsing as every other file.
    let d = sniff::detect(&t);
    check_detection(&t, &d);
    let parsed = parser::parse(&t, &d.mapping);
    check_parse(&t, &d.mapping, &parsed);
});
