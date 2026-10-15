//! Reducing a bank description to the shop it came from.
//!
//! Statements wrap the useful word in boilerplate — "COMPRA TARJETA 5487
//! MERCADONA VALENCIA" — and that boilerplate is what stops two visits to the
//! same shop from looking alike. The key is one token from the description, so
//! it is always a literal substring of it: a rule written as `contains <key>`
//! is guaranteed to match everything the key grouped, which would not be true
//! of a synthesised phrase.

use std::collections::HashSet;
use std::sync::RwLock;

/// Words that appear on every second line of a statement and so identify
/// nothing — "compra", "purchase", "pos debit".
///
/// They are different for every country and bank, so they are not in the code:
/// each region file lists its own (`merchantNoise` in `regions/<id>.json`) and
/// the interface hands the active region's list over when it starts and
/// whenever the region changes. Until it has, nothing is treated as noise and
/// the longest word wins, which is right more often than not.
static NOISE: RwLock<Option<HashSet<String>>> = RwLock::new(None);

/// Replace the noise words. Each entry is normalised the way descriptions are,
/// so a region file can be written naturally ("Pago con tarjeta").
pub fn set_noise<I: IntoIterator<Item = String>>(words: I) {
    let set: HashSet<String> = words
        .into_iter()
        .flat_map(|w| {
            crate::security::pii::normalise_description(&w)
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    if let Ok(mut g) = NOISE.write() {
        *g = Some(set);
    }
}

/// The word this description is most likely to be recognised by.
///
/// Returns `None` when there is nothing but noise and digits to go on, which
/// is a real outcome — some transfers genuinely carry no merchant.
pub fn key(description_norm: &str) -> Option<String> {
    let guard = NOISE.read().ok();
    let empty = HashSet::new();
    let noise = guard.as_deref().and_then(|g| g.as_ref()).unwrap_or(&empty);
    key_with(description_norm, noise)
}

/// `key` with the noise words given, so a test (or a caller that knows its
/// region) does not depend on what was last set process-wide.
pub fn key_with(description_norm: &str, noise: &HashSet<String>) -> Option<String> {
    let tokens: Vec<&str> = description_norm
        .split_whitespace()
        .filter(|t| t.len() >= 4)
        .collect();
    if tokens.is_empty() {
        return None;
    }
    // Longest signal word wins; ties go to the earlier one, which on a
    // statement is usually the shop rather than the town it sits in.
    let pick = |pool: &[&str]| -> Option<String> {
        pool.iter()
            .enumerate()
            .max_by_key(|(i, t)| (t.len(), std::cmp::Reverse(*i)))
            .map(|(_, t)| (*t).to_string())
    };
    let signal: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| !noise.contains(*t))
        .collect();
    pick(&signal).or_else(|| pick(&tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Noise words as a Spanish region file would list them.
    fn es() -> HashSet<String> {
        [
            "compra",
            "tarjeta",
            "tarj",
            "pago",
            "recibo",
            "transferencia",
            "transf",
            "traspaso",
        ]
        .iter()
        .map(|w| w.to_string())
        .collect()
    }

    fn k(s: &str) -> Option<String> {
        key_with(&crate::security::pii::normalise_description(s), &es())
    }

    #[test]
    fn strips_card_boilerplate() {
        assert_eq!(
            k("COMPRA TARJETA 5487 MERCADONA VALENCIA").as_deref(),
            Some("mercadona")
        );
        assert_eq!(
            k("Pago en MERCADONA 0091 Valencia").as_deref(),
            Some("mercadona")
        );
    }

    #[test]
    fn noise_words_decide_which_word_is_the_shop() {
        // "transferencia" is the longest word; only the region's list says it
        // identifies nothing.
        let norm = crate::security::pii::normalise_description("TRANSFERENCIA PAGO CAFE");
        assert_eq!(key_with(&norm, &es()).as_deref(), Some("cafe"));
        assert_eq!(
            key_with(&norm, &HashSet::new()).as_deref(),
            Some("transferencia")
        );
    }

    #[test]
    fn a_region_list_is_normalised_like_descriptions() {
        set_noise(["Pago con TARJETA".to_string()]);
        let g = NOISE.read().unwrap();
        let set = g.as_ref().unwrap();
        assert!(set.contains("pago") && set.contains("tarjeta"));
        drop(g);
        set_noise(Vec::new());
    }

    #[test]
    fn two_visits_to_the_same_shop_share_a_key() {
        assert_eq!(
            k("COMPRA TARJ 1234 CARREFOUR MADRID"),
            k("Carrefour Express 77")
        );
    }

    #[test]
    fn the_key_is_always_a_substring_so_a_contains_rule_matches() {
        let norm = crate::security::pii::normalise_description("RECIBO IBERDROLA CLIENTES SAU");
        let key = key_with(&norm, &es()).unwrap();
        assert!(norm.contains(&key), "{norm} does not contain {key}");
    }

    #[test]
    fn nothing_to_go_on_is_not_an_error() {
        assert_eq!(key_with("", &es()), None);
        assert_eq!(key_with("a de la", &es()), None);
    }

    #[test]
    fn falls_back_when_every_word_is_boilerplate() {
        // Better to group these together than to give up on them entirely.
        assert_eq!(
            k("TRANSFERENCIA RECIBO 0912").as_deref(),
            Some("transferencia")
        );
    }
}
