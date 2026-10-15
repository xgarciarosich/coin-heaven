//! Excel workbooks: the sheet XML, shared strings, styles and workbook part
//! are fuzzed inside a valid ZIP, so the mutations reach the spreadsheet reader
//! instead of stopping at a checksum.
//!
//! The first byte picks the mode. Even: the four parts, NUL-separated (an empty
//! part is replaced by a plain one). Odd: the rest is the whole file, for
//! damage to the container itself.
#![no_main]
use coin_heaven_fuzz::{Input, check_table, known_findings, xlsx};
use coin_heaven_lib::import::table;
use libfuzzer_sys::fuzz_target;

/// Whether every cell reference in some XML stays inside a modest grid.
///
/// Known defect: the reader lays a sheet out as a dense grid from the first
/// cell to the last, so one cell at `XFD1048576` asks for the whole
/// 17-billion-cell rectangle (see `fuzz/regressions/xlsx_load/`). The fuzzer is
/// kept off that one shape unless `COIN_HEAVEN_FUZZ_KNOWN=1`.
fn references_stay_small(xml: &[u8]) -> bool {
    let mut at = 0;
    while let Some(p) = xml[at..].windows(3).position(|w| w == b"r=\"" || w == b"f=\"") {
        let start = at + p + 3;
        let end = xml[start..]
            .iter()
            .position(|&b| b == b'"')
            .map_or(xml.len(), |e| start + e);
        // "B7", "A1:C3", or a row number on its own.
        for part in xml[start..end].split(|&b| b == b':') {
            let letters = part.iter().take_while(|b| b.is_ascii_alphabetic()).count();
            let digits = &part[letters..];
            let row = digits
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .fold(0u64, |n, &b| n.saturating_mul(10).saturating_add(u64::from(b - b'0')));
            if letters > 2 || row > 2_000 {
                return false;
            }
        }
        at = end.min(xml.len());
    }
    true
}

fuzz_target!(|data: &[u8]| {
    let mut inp = Input::new(data);
    let raw = inp.byte() & 1 == 1;
    let file = if raw {
        inp.rest().to_vec()
    } else {
        let (sheet, shared, styles, book) = (inp.field(), inp.field(), inp.field(), inp.field());
        if !known_findings() && !references_stay_small(sheet) {
            return;
        }
        xlsx::workbook(sheet, shared, styles, book)
    };

    let named = table::load("book.xlsx", &file, None, None);
    if let Ok(t) = &named {
        check_table(t, file.len());
        assert!(matches!(t.source, table::Source::Spreadsheet { .. }));
    }
    // The same bytes under a name that lies are still read by their content.
    if file.starts_with(b"PK\x03\x04") {
        let other = table::load("book.csv", &file, None, None);
        assert_eq!(named.as_ref().ok(), other.as_ref().ok());
    }
});
