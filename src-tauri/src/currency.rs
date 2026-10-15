//! ISO-4217 currencies for v1.
//!
//! `Money` is a count of *minor units*, which is currency-agnostic — but the
//! number of minor units per major unit is not. Most currencies use 2 decimal
//! places; JPY and KRW use 0; KWD, BHD and TND use 3. Formatting a JPY amount
//! by dividing by 100 is off by two orders of magnitude, so the exponent
//! travels with the currency rather than being hardcoded.

use crate::money::Money;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Currency {
    pub code: &'static str,
    pub name: &'static str,
    /// Decimal places. 2 for EUR, 0 for JPY, 3 for KWD.
    pub exponent: u8,
    pub symbol: &'static str,
}

/// The currencies offered in v1: the majors, plus the ones most likely to show
/// up in a European user's brokerage or crypto statement.
pub const CURRENCIES: &[Currency] = &[
    Currency {
        code: "EUR",
        name: "Euro",
        exponent: 2,
        symbol: "€",
    },
    Currency {
        code: "USD",
        name: "US Dollar",
        exponent: 2,
        symbol: "$",
    },
    Currency {
        code: "GBP",
        name: "Pound Sterling",
        exponent: 2,
        symbol: "£",
    },
    Currency {
        code: "CHF",
        name: "Swiss Franc",
        exponent: 2,
        symbol: "CHF",
    },
    Currency {
        code: "JPY",
        name: "Japanese Yen",
        exponent: 0,
        symbol: "¥",
    },
    Currency {
        code: "CAD",
        name: "Canadian Dollar",
        exponent: 2,
        symbol: "CA$",
    },
    Currency {
        code: "AUD",
        name: "Australian Dollar",
        exponent: 2,
        symbol: "A$",
    },
    Currency {
        code: "SEK",
        name: "Swedish Krona",
        exponent: 2,
        symbol: "kr",
    },
    Currency {
        code: "NOK",
        name: "Norwegian Krone",
        exponent: 2,
        symbol: "kr",
    },
    Currency {
        code: "DKK",
        name: "Danish Krone",
        exponent: 2,
        symbol: "kr",
    },
    Currency {
        code: "PLN",
        name: "Polish Zloty",
        exponent: 2,
        symbol: "zł",
    },
    Currency {
        code: "CZK",
        name: "Czech Koruna",
        exponent: 2,
        symbol: "Kč",
    },
    Currency {
        code: "MXN",
        name: "Mexican Peso",
        exponent: 2,
        symbol: "MX$",
    },
    Currency {
        code: "BRL",
        name: "Brazilian Real",
        exponent: 2,
        symbol: "R$",
    },
    Currency {
        code: "CNY",
        name: "Chinese Yuan",
        exponent: 2,
        symbol: "CN¥",
    },
    Currency {
        code: "HKD",
        name: "Hong Kong Dollar",
        exponent: 2,
        symbol: "HK$",
    },
    Currency {
        code: "SGD",
        name: "Singapore Dollar",
        exponent: 2,
        symbol: "S$",
    },
    Currency {
        code: "NZD",
        name: "New Zealand Dollar",
        exponent: 2,
        symbol: "NZ$",
    },
    Currency {
        code: "ZAR",
        name: "South African Rand",
        exponent: 2,
        symbol: "R",
    },
    Currency {
        code: "TRY",
        name: "Turkish Lira",
        exponent: 2,
        symbol: "₺",
    },
    Currency {
        code: "KRW",
        name: "South Korean Won",
        exponent: 0,
        symbol: "₩",
    },
    Currency {
        code: "INR",
        name: "Indian Rupee",
        exponent: 2,
        symbol: "₹",
    },
    Currency {
        code: "AED",
        name: "UAE Dirham",
        exponent: 2,
        symbol: "AED",
    },
    Currency {
        code: "KWD",
        name: "Kuwaiti Dinar",
        exponent: 3,
        symbol: "KD",
    },
];

/// What an amount is assumed to be in when nothing says otherwise. It is only
/// a fallback: the region chosen in Settings sets the real default, and an
/// account always carries its own currency.
pub const DEFAULT: &str = "USD";

pub fn lookup(code: &str) -> Option<&'static Currency> {
    CURRENCIES
        .iter()
        .find(|c| c.code.eq_ignore_ascii_case(code))
}

pub fn is_supported(code: &str) -> bool {
    lookup(code).is_some()
}

impl Currency {
    /// Minor units per major unit: 100 for EUR, 1 for JPY, 1000 for KWD.
    pub const fn scale(&self) -> i64 {
        match self.exponent {
            0 => 1,
            1 => 10,
            2 => 100,
            _ => 1000,
        }
    }

    pub fn from_major(&self, major: f64) -> Money {
        let v = major * self.scale() as f64;
        Money::from_minor(if v >= 0.0 {
            (v + 0.5).floor() as i64
        } else {
            (v - 0.5).ceil() as i64
        })
    }

    pub fn to_major(&self, m: Money) -> f64 {
        m.minor() as f64 / self.scale() as f64
    }

    /// Plain decimal string with the right number of places for this currency.
    pub fn format(&self, m: Money) -> String {
        let sign = if m.minor() < 0 { "-" } else { "" };
        let a = m.minor().unsigned_abs();
        let scale = self.scale() as u64;
        let whole = a / scale;
        if self.exponent == 0 {
            format!("{sign}{whole}")
        } else {
            let frac = a % scale;
            format!(
                "{sign}{whole}.{frac:0width$}",
                width = self.exponent as usize
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponents_are_not_all_two() {
        assert_eq!(lookup("JPY").unwrap().exponent, 0);
        assert_eq!(lookup("KWD").unwrap().exponent, 3);
        assert_eq!(lookup("EUR").unwrap().exponent, 2);
    }

    #[test]
    fn one_thousand_yen_is_one_thousand_minor_units_not_one_hundred_thousand() {
        let jpy = lookup("JPY").unwrap();
        let m = jpy.from_major(1000.0);
        assert_eq!(m.minor(), 1000);
        assert_eq!(jpy.format(m), "1000");
    }

    #[test]
    fn kuwaiti_dinar_keeps_three_places() {
        let kwd = lookup("KWD").unwrap();
        let m = kwd.from_major(1.234);
        assert_eq!(m.minor(), 1234);
        assert_eq!(kwd.format(m), "1.234");
    }

    #[test]
    fn euro_round_trips_through_major_and_back() {
        let eur = lookup("EUR").unwrap();
        for v in [0.0, 1.0, 12.34, -99.99, 1_000_000.05] {
            let m = eur.from_major(v);
            assert!((eur.to_major(m) - v).abs() < 1e-9, "{v}");
        }
        assert_eq!(eur.format(eur.from_major(-0.05)), "-0.05");
    }

    #[test]
    fn lookup_is_case_insensitive_and_rejects_unknown_codes() {
        assert!(lookup("eur").is_some());
        assert!(lookup("XYZ").is_none());
        assert!(!is_supported("BTC"), "crypto is not an ISO-4217 currency");
    }

    #[test]
    fn every_entry_is_well_formed_and_unique() {
        let mut seen = std::collections::HashSet::new();
        for c in CURRENCIES {
            assert_eq!(c.code.len(), 3, "{}", c.code);
            assert!(
                c.code.chars().all(|ch| ch.is_ascii_uppercase()),
                "{}",
                c.code
            );
            assert!(c.exponent <= 3, "{}", c.code);
            assert!(seen.insert(c.code), "duplicate {}", c.code);
        }
        assert!(is_supported(DEFAULT));
    }
}
