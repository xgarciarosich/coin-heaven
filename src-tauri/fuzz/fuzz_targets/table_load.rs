//! A file arrives: any bytes, any name, any forced encoding and delimiter.
//!
//! `import::table::load` is the first thing a hostile statement meets. It must
//! come back with a table or an error, never panic, and never hand on a grid
//! the rest of the importer cannot index.
#![no_main]
use coin_heaven_fuzz::{check_table, file_case};
use coin_heaven_lib::import::table::{self, Source};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(f) = file_case(data) else { return };

    if let Ok(t) = table::load(&f.name, f.bytes, f.encoding, f.delimiter) {
        check_table(&t, f.bytes.len());
        // Loading is a pure function of its inputs: the preview and the import
        // read the same file twice and must see the same rows.
        let again = table::load(&f.name, f.bytes, f.encoding, f.delimiter).unwrap();
        assert_eq!(t, again);
    }

    // The delimited reader on its own: a delimiter the person forced is the
    // delimiter used, whatever the file looks like.
    if let Ok(t) = table::from_delimited(f.bytes, f.encoding, f.delimiter) {
        check_table(&t, f.bytes.len());
        if let (Source::Delimited { delimiter, .. }, Some(want)) = (&t.source, f.delimiter) {
            assert_eq!(*delimiter, want);
        }
    }
});
