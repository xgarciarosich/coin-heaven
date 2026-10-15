//! A model's answer to "what do these assets cost", read into prices.
//!
//! This is the one place text that nobody on this machine wrote becomes a
//! number that is stored against a holding. The reader's rules are held to
//! here: an id the request never named is not a price, a price is positive and
//! plausible, a date is never in the future, a source is an http(s) address,
//! and a price is believed only if words that contain it are in a result.
//!
//! The input is: a day offset, the addresses the search returned (one per
//! line, a NUL after them), the search results (title and snippet on
//! alternate lines, a NUL after them), the price to check as 8 bytes, a NUL,
//! then the model's reply.
#![no_main]
use chrono::{NaiveDate, TimeDelta};
use coin_heaven_fuzz::{Input, known_findings};
use coin_heaven_lib::ai::{self, SearchHit};
use coin_heaven_lib::commands::assistant::parse_prices;
use libfuzzer_sys::fuzz_target;
use serde_json::Value;
use std::collections::HashMap;

/// The ids a request could have named: a few ordinary ones and the extremes.
fn names() -> HashMap<i64, String> {
    [0, 1, 2, 3, 7, -1, i64::MAX, i64::MIN]
        .into_iter()
        .map(|id| (id, format!("asset {id}")))
        .collect()
}

fn hits(text: &str) -> Vec<SearchHit> {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .chunks(2)
        .enumerate()
        .map(|(i, pair)| SearchHit {
            title: pair[0].to_string(),
            url: format!("https://example.test/{i}"),
            snippet: pair.get(1).copied().unwrap_or("").to_string(),
        })
        .collect()
}

fn same_site(claimed: &str, seen: &str) -> bool {
    claimed == seen || seen.ends_with(&format!(".{claimed}")) || claimed.ends_with(&format!(".{seen}"))
}

fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let h = rest.split(['/', '?', '#']).next().unwrap_or("").to_ascii_lowercase();
    h.trim_start_matches("www.").to_string()
}

fn reader(reply: &str, seen: &[String], today: NaiveDate) {
    let names = names();
    let items = ai::answer_items(reply);

    // Every item is an object that carries an id, and writing the items out
    // again and reading that gives the same items: nothing is made up and
    // nothing is dropped by being read twice.
    for it in &items {
        assert!(it.as_object().is_some_and(|o| o.contains_key("id")));
    }
    let again = items.iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    assert_eq!(ai::answer_items(&again), items);

    let proposals = parse_prices(reply, &names, seen, today);
    assert!(proposals.len() <= items.len());
    let seen_hosts: Vec<String> = seen.iter().map(|u| host(u)).filter(|h| !h.is_empty()).collect();
    for p in &proposals {
        assert_eq!(names.get(&p.holding_id), Some(&p.name), "an id nobody asked about");

        if let Some(micro) = p.price_micro {
            // < 1e9 of a currency unit, in millionths.
            assert!((0..=1_000_000_000_000_000).contains(&micro), "price {micro}");
            // Known defect: a positive price below half a millionth rounds to
            // zero here, after the "positive" check has passed it, so a model
            // that answers 0.0000001 stores a price of nothing. See
            // `fuzz/regressions/ai_price_reply/`.
            if known_findings() {
                assert!(micro > 0, "a price of zero was proposed");
            }
        }
        if let Some(d) = &p.price_date {
            let day = NaiveDate::parse_from_str(d, "%Y-%m-%d").expect("a date, written as one");
            assert!(day <= today, "{d} is in the future of {today}");
            assert_eq!(p.historical, (today - day).num_days() > 7);
        } else {
            assert!(!p.historical);
        }
        if let Some(url) = &p.source_url {
            assert!(url.starts_with("https://") || url.starts_with("http://"));
        }
        if p.source_seen {
            // "Seen" means the claimed page is on a site the search returned.
            let url = p.source_url.as_deref().expect("a source seen without a source");
            let claimed = host(url);
            assert!(
                seen_hosts.iter().any(|s| same_site(&claimed, s)),
                "{url:?} is not on any of {seen_hosts:?}"
            );
            // Known defect: a bare top-level domain is a "site" that every
            // address under it is on, so `https://com/` counts as seen once any
            // .com page was. See `fuzz/regressions/ai_price_reply/`.
            if known_findings() {
                assert!(
                    claimed.contains('.') || seen_hosts.contains(&claimed),
                    "{url:?} was taken for a page that was read"
                );
            }
        }
        assert!(p.evidence.as_deref().is_none_or(|e| !e.trim().is_empty()));
        assert!(p.note.as_deref().is_none_or(|n| !n.is_empty()));
    }
}

fn evidence(price: f64, evidence: &str, found: &[SearchHit]) {
    let Some(hit) = ai::check_evidence(price, evidence, found) else {
        return;
    };
    assert!(found.iter().any(|h| std::ptr::eq(h, hit)), "a result nobody returned");

    // The words have to hold the number, give or take what the check allows.
    let close = |n: f64| (n - price).abs() <= price.abs() * 1e-6 + 0.0049;
    assert!(ai::numbers_in(evidence).into_iter().any(close), "no number in {evidence:?}");
    // And the result has to be one that says it: the words are in it, or the
    // number is.
    let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let text = format!("{} {}", hit.title, hit.snippet);
    let quoted = squash(evidence.trim().trim_matches(['"', '“', '”', '…', '.']));
    assert!(
        squash(&text).contains(&quoted) || ai::numbers_in(&text).into_iter().any(close),
        "{evidence:?} is not in {text:?}"
    );

    // Known defect: the allowance is half a cent whatever the price, so for an
    // asset worth less than that any number from nought upwards "contains the
    // price". See `fuzz/regressions/ai_price_reply/`.
    if known_findings() && price > 0.0 && price.is_finite() {
        let n = ai::numbers_in(evidence)
            .into_iter()
            .filter(|n| close(*n))
            .any(|n| n > 0.0 && (n - price).abs() <= price);
        assert!(n, "{evidence:?} was accepted for a price of {price}");
    }
}

fuzz_target!(|data: &[u8]| {
    let mut inp = Input::new(data);
    let today = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap()
        + TimeDelta::days(i64::from(inp.byte()) - 128);
    let seen: Vec<String> = inp
        .text()
        .lines()
        .map(str::to_string)
        .filter(|l| !l.is_empty())
        .collect();
    let found = hits(&inp.text());
    let mut bits = [0u8; 8];
    bits.iter_mut().for_each(|b| *b = inp.byte());
    let price = f64::from_bits(u64::from_le_bytes(bits));
    inp.byte();
    let reply = String::from_utf8_lossy(inp.rest());

    reader(&reply, &seen, today);
    evidence(price, &reply, &found);

    // Numbers are read out of any text without panicking, and are never
    // negative: a sign is not part of "a number written in a piece of text".
    for n in ai::numbers_in(&reply) {
        assert!(n >= 0.0, "{n} read out of text");
    }
});
