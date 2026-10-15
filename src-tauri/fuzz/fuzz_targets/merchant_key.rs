//! Merchant keys: description to normalised text to the one word a rule can be
//! written against.
//!
//! The input is the region's noise words, a NUL, then a bank description. The
//! contract being held to is the one in the module notes: the key is a literal
//! piece of the description, so a rule written as `contains <key>` is
//! guaranteed to match everything the key grouped.
#![no_main]
use coin_heaven_fuzz::Input;
use coin_heaven_lib::import::merchant::{key, key_with, set_noise};
use coin_heaven_lib::import::rules::{Candidate, Rule, matches};
use coin_heaven_lib::money::Money;
use coin_heaven_lib::security::pii::normalise_description;
use libfuzzer_sys::fuzz_target;
use std::collections::HashSet;

fuzz_target!(|data: &[u8]| {
    let mut inp = Input::new(data);
    let words: Vec<String> = inp.text().split_whitespace().map(str::to_string).collect();
    let description = inp.text();

    let norm = normalise_description(&description);
    assert_eq!(norm, norm.trim());
    assert!(!norm.contains("  "), "{norm:?}");
    assert!(norm.chars().all(|c| c == ' ' || c.is_alphabetic()), "{norm:?}");
    assert_eq!(normalise_description(&norm), norm, "normalising is idempotent");

    // What `set_noise` does to a region's list, written out.
    let noise: HashSet<String> = words
        .iter()
        .flat_map(|w| {
            normalise_description(w)
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();

    let tokens: Vec<&str> = norm.split_whitespace().filter(|t| t.len() >= 4).collect();
    let got = key_with(&norm, &noise);
    assert_eq!(got.is_none(), tokens.is_empty());

    if let Some(k) = &got {
        assert!(tokens.contains(&k.as_str()), "{k:?} is not a word of {norm:?}");
        assert!(norm.contains(k.as_str()));

        // The best word: signal words if there are any, else all of them; the
        // longest, the earliest among equals.
        let signal: Vec<&str> = tokens.iter().copied().filter(|t| !noise.contains(*t)).collect();
        let pool = if signal.is_empty() { &tokens } else { &signal };
        let longest = pool.iter().map(|t| t.len()).max().unwrap();
        let first = pool.iter().find(|t| t.len() == longest).unwrap();
        assert_eq!(k, first);

        // The promise a rule depends on.
        let rule = Rule {
            id: Some(1),
            name: "key".into(),
            priority: 0,
            match_kind: "contains".into(),
            pattern: k.clone(),
            account_id: None,
            min_amount_minor: None,
            max_amount_minor: None,
            set_category_id: None,
            set_project_id: None,
            set_neutral_transfer: false,
            is_enabled: true,
        };
        let cand = Candidate {
            id: 1,
            account_id: 1,
            description_norm: norm.clone(),
            amount_minor: Money::from_minor(-100),
        };
        assert!(matches(&rule, &cand), "a contains-rule on {k:?} misses {norm:?}");
    }

    // The process-wide list does the same thing as the one passed in.
    set_noise(words);
    assert_eq!(key(&norm), got);
    set_noise(Vec::new());
});
