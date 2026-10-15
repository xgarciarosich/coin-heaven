//! Dates and amounts: the two parsers every imported cell goes through.
//!
//! The first byte picks one of four jobs:
//!
//! 0. any text against any catalogue pattern (and against detection),
//! 1. a real date written in every date-only pattern and read back,
//! 2. any text as an amount, with any separators and exponent,
//! 3. a real amount written the ways banks write them and read back exactly.
#![no_main]
use chrono::NaiveDate;
use coin_heaven_fuzz::{Input, known_findings};
use coin_heaven_lib::import::dates::{self, catalogue};
use coin_heaven_lib::import::parser::parse_amount;
use coin_heaven_lib::money::Money;
use libfuzzer_sys::fuzz_target;

fn arbitrary_text(inp: &mut Input) {
    let patterns = dates::patterns();
    let pattern = patterns[inp.byte() as usize % patterns.len()];
    let value = inp.text();

    if let Some(found) = dates::detect(&value) {
        assert!(dates::is_known(found));
        assert!(dates::parse_strict(&value, found).is_some());
        assert!(value.trim().len() >= 6, "{value:?} is too short to be a date");
    }
    let strict = dates::parse_strict(&value, pattern);
    let lenient = dates::parse_lenient(&value, pattern);
    if let Some(d) = strict {
        assert_eq!(lenient, Some(d), "lenient must accept what strict does");
    }
    // An unknown pattern may be rejected, but it must not panic: an AI reply
    // and a hand-typed mapping both put arbitrary text here.
    let odd = inp.text();
    let _ = dates::parse_strict(&value, &odd);
    let _ = dates::parse_lenient(&value, &odd);
}

fn real_date(inp: &mut Input) {
    let year = 1 + (u16::from(inp.byte()) << 8 | u16::from(inp.byte())) % 9999;
    let month = 1 + u32::from(inp.byte()) % 12;
    let day = 1 + u32::from(inp.byte()) % 31;
    let Some(date) = NaiveDate::from_ymd_opt(i32::from(year), month, day) else {
        return;
    };
    for f in catalogue().into_iter().filter(|f| !f.has_time) {
        // A two-digit year folds a century away; that is the format, not a bug.
        if f.pattern.contains("%y") {
            continue;
        }
        let written = date.format(f.pattern).to_string();
        assert_eq!(
            dates::parse_strict(&written, f.pattern),
            Some(date),
            "{written:?} as {}",
            f.pattern
        );
        assert!(dates::detect(&written).is_some(), "{written:?} not detected");
        if !f.pattern.contains([' ', 'T']) {
            let with_time = format!("{written} 14:30:00");
            assert_eq!(dates::parse_lenient(&with_time, f.pattern), Some(date));
        }
    }
}

fn arbitrary_amount(inp: &mut Input) {
    let exponent = u32::from(inp.byte() % 9);
    let decimals = if known_findings() {
        &[',', '.', '٫', '·', '5'][..]
    } else {
        &[',', '.', ';', '5', '-'][..]
    };
    let decimal = inp.pick(decimals);
    let thousands = inp.text();
    let raw = inp.text();

    let parsed = parse_amount(&raw, decimal, &thousands, exponent);
    // Surrounding whitespace is never part of the number.
    assert_eq!(parsed, parse_amount(&format!(" \t{raw}\u{a0} "), decimal, &thousands, exponent));
    if let Some(m) = parsed {
        // `Money` negation is `-i64`; a parsed amount must never be i64::MIN.
        assert_ne!(m.minor(), i64::MIN);
    }
}

/// `1234567` with `sep` between groups of three.
fn grouped(digits: &str, sep: &str) -> String {
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push_str(sep);
        }
        out.push(c);
    }
    out
}

fn real_amount(inp: &mut Input) {
    let mut raw = [0u8; 8];
    raw.iter_mut().for_each(|b| *b = inp.byte());
    let minor = i64::from_le_bytes(raw) >> u32::from(inp.byte() % 63);
    if minor == i64::MIN {
        // |i64::MIN| does not fit; refusing it (None) is the right answer.
        assert_eq!(parse_amount(&minor.to_string(), '.', "", 0), None);
        return;
    }
    let exponent = u32::from(inp.byte() % 5);
    let (decimal, thousands) = if inp.byte() & 1 == 0 { ('.', ",") } else { (',', ".") };
    let sep = inp.pick(&[thousands, "", " "]);
    let style = inp.byte() % 3; // sign in front, sign behind, parentheses

    let abs = minor.unsigned_abs();
    let scale = 10u64.pow(exponent);
    let mut body = grouped(&(abs / scale).to_string(), sep);
    if exponent > 0 {
        body.push(decimal);
        body.push_str(&format!("{:0w$}", abs % scale, w = exponent as usize));
    }
    let written = match (minor < 0, style) {
        (false, 0) => format!("+{body}"),
        (false, _) => body.clone(),
        (true, 0) => format!("-{body}"),
        (true, 1) => format!("{body}-"),
        (true, _) => format!("({body})"),
    };
    assert_eq!(
        parse_amount(&written, decimal, thousands, exponent),
        Some(Money::from_minor(minor)),
        "{written:?} with {decimal:?} / {thousands:?} / {exponent}"
    );
    // `Money`'s own display is the plain two-decimal form of the same number.
    if exponent == 2 {
        let shown = Money::from_minor(minor).to_string();
        assert_eq!(parse_amount(&shown, '.', ",", 2), Some(Money::from_minor(minor)));
    }
}

fuzz_target!(|data: &[u8]| {
    let mut inp = Input::new(data);
    match inp.byte() % 4 {
        0 => arbitrary_text(&mut inp),
        1 => real_date(&mut inp),
        2 => arbitrary_amount(&mut inp),
        _ => real_amount(&mut inp),
    }
});
