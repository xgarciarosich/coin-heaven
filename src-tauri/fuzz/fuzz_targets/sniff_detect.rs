//! Format detection and the parse that follows it, on whatever file arrives.
//!
//! This is the "pick a file and it is understood" path of the import screen:
//! load, detect, parse with the detected mapping. Detection must point at
//! columns that exist, and parsing must account for every line it was given.
#![no_main]
use coin_heaven_fuzz::{check_detection, check_parse, check_table, file_case};
use coin_heaven_lib::import::{parser, sniff, table};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some(f) = file_case(data) else { return };
    let Ok(t) = table::load(&f.name, f.bytes, f.encoding, f.delimiter) else {
        return;
    };
    check_table(&t, f.bytes.len());

    let d = sniff::detect(&t);
    check_detection(&t, &d);
    assert_eq!(sniff::detect(&t), d, "detection is deterministic");

    let parsed = parser::parse(&t, &d.mapping);
    check_parse(&t, &d.mapping, &parsed);
});
