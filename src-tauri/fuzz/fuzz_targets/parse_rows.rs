//! Row parsing against a mapping the fuzzer writes: any columns, separators,
//! currency and date format, over a grid of tab-separated text.
//!
//! The first ten bytes are the mapping (see `coin_heaven_fuzz::mapping`), the
//! rest is the grid. Besides the shared parse invariants this checks that
//! flipping `invert_sign` negates every amount and nothing else, and that the
//! dedupe hashes of the rows read are all different — two coffees on one day
//! must stay two rows.
#![no_main]
use coin_heaven_fuzz::{Input, check_parse, grid, mapping};
use coin_heaven_lib::import::dedupe::{self, RawRow};
use coin_heaven_lib::import::parser;
use coin_heaven_lib::security::pii::normalise_description;
use libfuzzer_sys::fuzz_target;
use std::collections::HashSet;

fuzz_target!(|data: &[u8]| {
    let mut inp = Input::new(data);
    let m = mapping(&mut inp);
    let text = String::from_utf8_lossy(inp.rest());
    let t = grid(&text);

    let r = parser::parse(&t, &m);
    check_parse(&t, &m, &r);

    let flipped = parser::parse(
        &t,
        &parser::BankMapping {
            invert_sign: !m.invert_sign,
            ..m.clone()
        },
    );
    assert_eq!(r.rejected, flipped.rejected);
    assert_eq!(r.rows.len(), flipped.rows.len());
    for (a, b) in r.rows.iter().zip(&flipped.rows) {
        assert_eq!((a.line, &a.booked_on, &a.description), (b.line, &b.booked_on, &b.description));
        assert_eq!(a.amount_minor, -b.amount_minor);
    }

    let raw: Vec<RawRow> = r
        .rows
        .iter()
        .map(|p| RawRow {
            booked_on: p.booked_on.clone(),
            amount_minor: p.amount_minor,
            description: p.description.clone(),
        })
        .collect();
    let prepared = dedupe::prepare(1, &raw);
    assert_eq!(prepared.len(), raw.len());
    let hashes: HashSet<&str> = prepared.iter().map(|p| p.dedupe_hash.as_str()).collect();
    assert_eq!(hashes.len(), prepared.len(), "two different rows share a hash");
    for p in &prepared {
        assert_eq!(p.dedupe_hash.len(), 64);
        assert!(p.dedupe_hash.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(p.description_norm, normalise_description(&p.description_raw));
    }
});
