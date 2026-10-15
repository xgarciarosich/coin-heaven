//! What a market-data source can answer with, and what is built to ask it.
//!
//! The input is a symbol (or search text), a NUL, then the answer: a chart
//! response and a search response are the same bytes read twice, so one
//! document can exercise both readers. The first byte picks the day and the
//! staleness for the last-close lookup, and a script for the resolver.
//!
//! What is held to: a chart is a strictly dated, positive, finite series
//! whatever the source sent; a symbol is one path segment that cannot step out
//! of it; the resolver never asks twice, never asks more than it should and
//! never returns a series in the wrong currency.
#![no_main]
use chrono::NaiveDate;
use coin_heaven_fuzz::{Input, json, known_findings, percent_decode};
use coin_heaven_lib::market::{
    self, Candidate, Problem, Series, close_on_or_before, encode, first_guesses, parse_chart,
    parse_search, resolve,
};
use libfuzzer_sys::fuzz_target;
use serde_json::Value;

/// Whether reading this chart adds an offset to a timestamp that cannot hold it.
///
/// Known defect: `parse_chart` computes `timestamp + gmtoffset` unchecked, so
/// a bar stamped near `i64::MAX` overflows (a panic in a debug or test build,
/// a silent wrap in the shipped one). See `fuzz/regressions/market_json/`;
/// skipped unless `COIN_HEAVEN_FUZZ_KNOWN=1`.
fn offset_overflows(v: &Value) -> bool {
    let offset = v
        .pointer("/chart/result/0/meta/gmtoffset")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    v.pointer("/chart/result/0/timestamp")
        .and_then(Value::as_array)
        .is_some_and(|t| {
            t.iter()
                .filter_map(Value::as_i64)
                .any(|t| t.checked_add(offset).is_none())
        })
}

fn check_series(s: &Series) {
    assert!(
        s.closes.windows(2).all(|w| w[0].0 < w[1].0),
        "closes are ordered by day with no day twice"
    );
    for (_, price) in &s.closes {
        assert!(price.is_finite() && *price > 0.0, "price {price}");
    }
    assert_eq!(s.currency, s.currency.trim());
}

fn chart(body: &Value, day: NaiveDate, stale: i64) {
    match parse_chart(body) {
        Err(problem) => {
            // The only answers a reader may give: it is not there, or unreadable.
            assert!(matches!(problem, Problem::NotFound | Problem::Unreachable));
            // A result that is there but has no metadata is unreadable, and a
            // missing or null one is simply not found.
            let there = body.pointer("/chart/result/0").is_some_and(|r| !r.is_null());
            assert_eq!(problem == Problem::Unreachable, there);
        }
        Ok(s) => {
            check_series(&s);
            let stamps = body
                .pointer("/chart/result/0/timestamp")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            assert!(s.closes.len() <= stamps);

            // The last close on or before a day is the latest one, and fresh.
            match close_on_or_before(&s, day, stale) {
                Some((d, price)) => {
                    assert!(d <= day && (day - d).num_days() <= stale);
                    assert!(s.closes.contains(&(d, price)));
                    assert!(s.closes.iter().all(|(later, _)| *later <= d || *later > day));
                }
                None => {
                    let fresh = s
                        .closes
                        .iter()
                        .rev()
                        .find(|(d, _)| *d <= day)
                        .is_some_and(|(d, _)| (day - *d).num_days() <= stale);
                    assert!(!fresh, "a fresh close was not returned");
                }
            }
        }
    }
}

fn search(body: &Value) {
    for c in parse_search(body) {
        // Whitespace in a name is collapsed, so it can sit in a table cell.
        assert_eq!(c.name, c.name.split_whitespace().collect::<Vec<_>>().join(" "));
    }
}

/// A symbol is one path segment, and the person's text round-trips through it.
fn urls(symbol: &str, day: NaiveDate) {
    let e = encode(symbol);
    assert!(
        e.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b'%')),
        "{e:?} holds a character that is not safe in a path"
    );
    if symbol == "." || symbol == ".." {
        // "%2E" would still be read as a directory step; it is encoded twice.
        assert!(!e.contains('.') && e.contains("%25"));
    } else {
        assert_eq!(percent_decode(&e).as_deref(), Some(symbol.as_bytes()));
    }

    let chart = market::chart_url(symbol, day, day);
    let path = chart
        .strip_prefix("https://query1.finance.yahoo.com/v8/finance/chart/")
        .expect("the chart address is on its own host");
    let (segment, query) = path.split_once('?').expect("a query follows the symbol");
    assert_eq!(segment, e, "one segment, the encoded symbol");
    assert!(query.starts_with("period1=") && query.ends_with("&interval=1d"));

    let q = market::search_url(symbol);
    let (_, rest) = q.split_once("?q=").unwrap();
    assert_eq!(rest.split_once('&').unwrap().0, e);
    assert!(market::quote_page(symbol).ends_with(&e));
}

fn guesses(symbol: &str, crypto: bool, currency: &str) {
    let g = first_guesses(Some(symbol), crypto, currency);
    let typed = symbol.trim();
    if typed.is_empty() {
        assert!(g.is_empty());
    } else {
        // What the person typed is always tried, and last.
        assert_eq!(g.last().map(String::as_str), Some(typed));
        assert!(g.len() <= 2);
    }
}

/// The resolver, driven by a script of what each symbol answers.
fn resolving(inp: &mut Input, currency: &str) {
    let guesses: Vec<String> = (0..inp.byte() % 5)
        .map(|_| inp.text().chars().take(8).collect())
        .collect();
    let candidates: Vec<Candidate> = (0..inp.byte() % 14)
        .map(|_| Candidate {
            symbol: inp.text().chars().take(8).collect(),
            name: String::new(),
            exchange: String::new(),
        })
        .collect();
    let script: Vec<u8> = (0..16).map(|_| inp.byte()).collect();

    let want = currency.to_uppercase();
    let mut asked: Vec<String> = Vec::new();
    let result = resolve(
        &guesses,
        || candidates.clone(),
        |symbol| {
            asked.push(symbol.to_string());
            let n = script[asked.len() % script.len()];
            match n % 6 {
                0 => Err(Problem::NotFound),
                1 => Err(Problem::Unreachable),
                2 => Err(Problem::RateLimited),
                3 => Err(Problem::NoData),
                4 => Ok(Series {
                    symbol: symbol.to_string(),
                    currency: "ZZZ".into(),
                    closes: Vec::new(),
                }),
                _ => Ok(Series {
                    symbol: symbol.to_string(),
                    currency: want.clone(),
                    closes: Vec::new(),
                }),
            }
        },
        currency,
    );

    // Never the same symbol twice (the source is rate limited), and at most
    // the guesses plus the first eight of the search.
    let mut seen: Vec<String> = Vec::new();
    for a in &asked {
        let key = a.to_ascii_lowercase();
        assert!(!seen.contains(&key), "{a:?} asked twice");
        seen.push(key);
    }
    assert!(asked.len() <= guesses.len() + 8, "asked {} times", asked.len());
    if let Ok(s) = result {
        assert_eq!(s.currency, want, "a series in another currency was accepted");
        assert!(asked.contains(&s.symbol));
    }
}

fuzz_target!(|data: &[u8]| {
    let mut inp = Input::new(data);
    let knobs = inp.byte();
    let symbol = inp.text();
    let day = NaiveDate::from_num_days_from_ce_opt(738_000 + i32::from(inp.byte()) * 30)
        .expect("a day in the 2020s");
    let stale = i64::from(knobs % 16);

    let currency = ["EUR", "usd", "GBP", "eur"][usize::from(knobs >> 4) % 4];
    urls(&symbol, day);
    guesses(&symbol, knobs & 1 == 1, currency);
    {
        let mut script = Input::new(inp.rest());
        resolving(&mut script, currency);
    }

    if let Some(body) = json(inp.rest()) {
        if known_findings() || !offset_overflows(&body) {
            chart(&body, day, stale);
        }
        search(&body);
    }
});
