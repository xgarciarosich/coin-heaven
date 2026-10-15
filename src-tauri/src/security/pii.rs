//! Best-effort PII redaction for anything about to leave the machine.
//!
//! IMPORTANT — read `docs/adr/0005` before trusting this module. Regex
//! redaction catches *structured* identifiers (IBANs, cards, emails, phones).
//! It cannot reliably catch names, and a merchant string is itself disclosive:
//! "Clínica Dermatológica X" reveals health information no masker will strip.
//!
//! Redaction is therefore the second line of defence, not the first. The first
//! is the egress policy: aggregates by default, raw rows only on an explicit,
//! previewed, per-request opt-in.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

static IBAN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b[A-Z]{2}\d{2}[ ]?(?:[A-Z0-9]{4}[ ]?){2,7}[A-Z0-9]{1,4}\b").unwrap()
});
static CARD: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b(?:\d[ -]?){13,19}\b").unwrap());
static EMAIL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b").unwrap());
static PHONE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?:\+\d{1,3}[ .-]?)?(?:\(?\d{2,4}\)?[ .-]?){2,4}\d{2,4}").unwrap());
static LONG_DIGITS: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b\d{8,}\b").unwrap());

/// What was removed, so the UI can show the user before anything is sent.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Redactions {
    pub ibans: usize,
    pub cards: usize,
    pub emails: usize,
    pub phones: usize,
    pub account_numbers: usize,
}

impl Redactions {
    pub fn total(&self) -> usize {
        self.ibans + self.cards + self.emails + self.phones + self.account_numbers
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Masked {
    pub text: String,
    pub redactions: Redactions,
}

/// Luhn check, so order references and long invoice numbers are not mistaken
/// for card numbers.
fn passes_luhn(s: &str) -> bool {
    let digits: Vec<u32> = s.chars().filter_map(|c| c.to_digit(10)).collect();
    if !(13..=19).contains(&digits.len()) {
        return false;
    }
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, &d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// Redact structured identifiers from a free-text string.
pub fn mask(input: &str) -> Masked {
    let mut r = Redactions::default();

    let out = IBAN.replace_all(input, |_: &regex::Captures| {
        r.ibans += 1;
        "[IBAN]".to_string()
    });
    let out = CARD.replace_all(&out, |c: &regex::Captures| {
        if passes_luhn(&c[0]) {
            r.cards += 1;
            "[CARD]".to_string()
        } else {
            c[0].to_string()
        }
    });
    let out = EMAIL.replace_all(&out, |_: &regex::Captures| {
        r.emails += 1;
        "[EMAIL]".to_string()
    });
    let out = PHONE.replace_all(&out, |c: &regex::Captures| {
        // Require enough digits to actually be a phone number.
        if c[0].chars().filter(|ch| ch.is_ascii_digit()).count() >= 9 {
            r.phones += 1;
            "[PHONE]".to_string()
        } else {
            c[0].to_string()
        }
    });
    let out = LONG_DIGITS.replace_all(&out, |_: &regex::Captures| {
        r.account_numbers += 1;
        "[NUM]".to_string()
    });

    Masked {
        text: out.into_owned(),
        redactions: r,
    }
}

/// Collapse a vendor string to a stable, low-entropy token suitable for
/// categorisation: casefolded, punctuation and digits stripped, whitespace
/// squeezed. Also used as the dedupe/rule-matching key on import.
pub fn normalise_description(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_space = true;
    for ch in input.chars() {
        let c = ch.to_lowercase().next().unwrap_or(ch);
        if c.is_alphabetic() {
            out.push(c);
            last_space = false;
        } else if !last_space {
            out.push(' ');
            last_space = true;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_a_spanish_iban() {
        let m = mask("Transferencia a ES9121000418450200051332 de Ana");
        assert!(!m.text.contains("ES9121000418450200051332"));
        assert!(m.text.contains("[IBAN]"));
        assert_eq!(m.redactions.ibans, 1);
    }

    #[test]
    fn masks_a_valid_card_but_leaves_a_random_long_number_alone() {
        let m = mask("PAGO 4539578763621486");
        assert!(m.text.contains("[CARD]"), "got {}", m.text);

        // Not a valid card: falls through to the generic account-number rule.
        let m2 = mask("REF 1234567890123456");
        assert_eq!(m2.redactions.cards, 0);
        assert!(m2.text.contains("[NUM]") || m2.text.contains("[PHONE]"));
    }

    #[test]
    fn masks_emails_and_phone_numbers() {
        let m = mask("Contacto ana.lopez@example.com o +34 612 345 678");
        assert!(!m.text.contains("example.com"));
        assert!(!m.text.contains("612 345 678"));
        assert_eq!(m.redactions.emails, 1);
    }

    #[test]
    fn leaves_ordinary_merchant_text_untouched() {
        let m = mask("MERCADONA VALENCIA");
        assert_eq!(m.text, "MERCADONA VALENCIA");
        assert_eq!(m.redactions.total(), 0);
    }

    #[test]
    fn documents_its_own_blind_spot() {
        // A personal name is NOT redacted. This test exists so the limitation
        // is asserted rather than assumed — the egress policy, not this
        // module, is what keeps names off the wire.
        let m = mask("Bizum de Ana Lopez Ruiz");
        assert!(m.text.contains("Ana Lopez Ruiz"));
    }

    #[test]
    fn normalisation_is_stable_and_strips_noise() {
        assert_eq!(normalise_description("  MERCADONA  4512 "), "mercadona");
        assert_eq!(
            normalise_description("Café-Bar «El Túnel» 12/03"),
            "café bar el túnel"
        );
        assert_eq!(
            normalise_description("PAGO TARJ. 1234 REST"),
            normalise_description("pago tarj 9999 rest")
        );
    }
}
