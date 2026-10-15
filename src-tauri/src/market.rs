//! Closing prices from a market-data source, for the buys that have none.
//!
//! A recurring buy of "€50 of SAP" does not say how many shares €50 bought:
//! that takes the price on the day. A web search is a poor way to ask — a
//! snippet rarely carries last March's close — so this asks a source that
//! publishes daily closes, once per holding for the whole stretch, and fills
//! in a price for every buy date.
//!
//! The source is Yahoo Finance's public chart data. It is unofficial: it has
//! no contract and it can refuse or change. Everything here is therefore
//! built to fail plainly (the reason is reported, nothing is half-written)
//! and the parsing is pure, so it is tested against recorded responses.

use chrono::{Duration, NaiveDate};
use serde::Serialize;

/// One instrument's daily closes.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub symbol: String,
    pub currency: String,
    pub closes: Vec<(NaiveDate, f64)>,
}

/// A listing the source's search offers for a name or ticker.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub symbol: String,
    pub name: String,
    pub exchange: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Problem {
    /// The source has nothing under that symbol.
    NotFound,
    /// It has the instrument, but not priced in the currency Coin Heaven works in.
    OtherCurrency,
    /// Nothing in the dates that were needed.
    NoData,
    /// The source asked to be left alone for a while.
    RateLimited,
    /// Could not be reached, or answered with something unreadable.
    Unreachable,
}

/// Read a chart response.
pub fn parse_chart(body: &serde_json::Value) -> Result<Series, Problem> {
    let result = body
        .pointer("/chart/result/0")
        .filter(|r| !r.is_null())
        .ok_or(Problem::NotFound)?;
    let meta = result.get("meta").ok_or(Problem::Unreachable)?;
    let quoted = meta
        .get("currency")
        .and_then(|c| c.as_str())
        .ok_or(Problem::Unreachable)?;
    // London shares are quoted in pence ("GBp"/"GBX"), Johannesburg in cents
    // ("ZAc"), Tel Aviv in agorot ("ILA"). Reading those as pounds, rand or
    // shekels would store a price one hundred times too high.
    let (currency, divisor) = match quoted {
        "GBp" | "GBX" => ("GBP".to_string(), 100.0),
        "ZAc" | "ZAC" => ("ZAR".to_string(), 100.0),
        "ILA" => ("ILS".to_string(), 100.0),
        other => (other.to_uppercase(), 1.0),
    };
    let symbol = meta
        .get("symbol")
        .and_then(|s| s.as_str())
        .unwrap_or_default()
        .to_string();
    // Bars are stamped at the exchange's open; the offset puts each on the
    // calendar day of the exchange rather than of UTC.
    let offset = meta.get("gmtoffset").and_then(|g| g.as_i64()).unwrap_or(0);
    let stamps = result
        .get("timestamp")
        .and_then(|t| t.as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();
    let closes_raw = result
        .pointer("/indicators/quote/0/close")
        .and_then(|c| c.as_array())
        .map(Vec::as_slice)
        .unwrap_or_default();

    let mut closes: Vec<(NaiveDate, f64)> = stamps
        .iter()
        .zip(closes_raw)
        .filter_map(|(t, c)| {
            let day = chrono::DateTime::from_timestamp(t.as_i64()? + offset, 0)?.date_naive();
            let price = c.as_f64().filter(|p| p.is_finite() && *p > 0.0)? / divisor;
            Some((day, price))
        })
        .collect();
    closes.sort_by_key(|c| c.0);
    // A day can appear twice around a holiday adjustment; the later wins.
    closes.dedup_by(|b, a| {
        if a.0 == b.0 {
            a.1 = b.1;
            true
        } else {
            false
        }
    });
    Ok(Series {
        symbol,
        currency,
        closes,
    })
}

/// Read a search response, keeping what can have a price: shares, funds,
/// ETFs and coins.
pub fn parse_search(body: &serde_json::Value) -> Vec<Candidate> {
    body.get("quotes")
        .and_then(|q| q.as_array())
        .map(|qs| {
            qs.iter()
                .filter(|q| {
                    matches!(
                        q.get("quoteType").and_then(|t| t.as_str()),
                        Some("EQUITY" | "ETF" | "MUTUALFUND" | "CRYPTOCURRENCY")
                    )
                })
                .filter_map(|q| {
                    Some(Candidate {
                        symbol: q.get("symbol")?.as_str()?.to_string(),
                        name: q
                            .get("shortname")
                            .or_else(|| q.get("longname"))
                            .and_then(|n| n.as_str())
                            .unwrap_or_default()
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" "),
                        exchange: q
                            .get("exchDisp")
                            .and_then(|e| e.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The last close on or before `day`, if there is one within `stale_days`.
/// A holiday or weekend takes the Friday's close; a gap of weeks is not a
/// price for that day.
pub fn close_on_or_before(s: &Series, day: NaiveDate, stale_days: i64) -> Option<(NaiveDate, f64)> {
    s.closes
        .iter()
        .rev()
        .find(|(d, _)| *d <= day)
        .copied()
        .filter(|(d, _)| (day - *d).num_days() <= stale_days)
}

/// The symbols worth trying for a holding, best first: what the person
/// typed, then — for a coin — its pair in the working currency, then
/// whatever the source's search offers.
pub fn first_guesses(symbol: Option<&str>, is_crypto: bool, currency: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(s) = symbol.map(str::trim).filter(|s| !s.is_empty()) {
        if is_crypto && !s.contains('-') && s.chars().all(|c| c.is_ascii_alphanumeric()) {
            out.push(format!("{}-{}", s.to_uppercase(), currency));
        }
        out.push(s.to_string());
    }
    out
}

/// The bookkeeping of a search for a listing, so the same rules apply
/// whether the answers come from a closure (in tests) or from the network.
///
/// It remembers what was tried and the most useful reason there was no match:
/// finding the instrument only in another currency is a different problem
/// from not finding it at all, and the fix is different (a ticker for the
/// right listing).
#[derive(Debug)]
pub struct Resolver {
    tried: Vec<String>,
    currency: String,
    best: Problem,
}

impl Resolver {
    pub fn new(currency: &str) -> Self {
        Resolver {
            tried: Vec::new(),
            currency: currency.to_uppercase(),
            best: Problem::NotFound,
        }
    }

    /// Whether `symbol` still needs asking about.
    pub fn wants(&mut self, symbol: &str) -> bool {
        if self.tried.iter().any(|t| t.eq_ignore_ascii_case(symbol)) {
            return false;
        }
        self.tried.push(symbol.to_string());
        true
    }

    /// Take an answer. `Some` is the series to use.
    pub fn offer(&mut self, answer: Result<Series, Problem>) -> Option<Series> {
        match answer {
            Ok(s) if s.currency == self.currency => Some(s),
            Ok(_) => {
                self.best = Problem::OtherCurrency;
                None
            }
            Err(Problem::RateLimited) => {
                self.best = Problem::RateLimited;
                None
            }
            Err(Problem::Unreachable) if self.best == Problem::NotFound => {
                self.best = Problem::Unreachable;
                None
            }
            Err(_) => None,
        }
    }

    /// A refusal ends the search at once instead of hammering the source.
    pub fn must_stop(&self) -> bool {
        self.best == Problem::RateLimited
    }

    pub fn failure(&self) -> Problem {
        self.best.clone()
    }
}

/// Find a series priced in `currency`: the guesses first, then the search
/// results. `fetch` stands in for the network.
pub fn resolve(
    guesses: &[String],
    search: impl FnOnce() -> Vec<Candidate>,
    mut fetch: impl FnMut(&str) -> Result<Series, Problem>,
    currency: &str,
) -> Result<Series, Problem> {
    let mut r = Resolver::new(currency);
    for g in guesses {
        if r.wants(g) {
            if let Some(s) = r.offer(fetch(g)) {
                return Ok(s);
            }
        }
        if r.must_stop() {
            return Err(r.failure());
        }
    }
    for c in search().into_iter().take(8) {
        if r.wants(&c.symbol) {
            if let Some(s) = r.offer(fetch(&c.symbol)) {
                return Ok(s);
            }
        }
        if r.must_stop() {
            break;
        }
    }
    Err(r.failure())
}

/// Percent-encode a path or query component.
pub fn encode(s: &str) -> String {
    // A path segment of "." or ".." is a directory step, not a symbol — and
    // URL parsers read even "%2E%2E" as one. Double-encoded, it stays a name
    // the source will simply not find.
    if s == "." || s == ".." {
        return s.bytes().map(|_| "%252E").collect();
    }
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn chart_url(symbol: &str, from: NaiveDate, to: NaiveDate) -> String {
    let ts = |d: NaiveDate| {
        d.and_hms_opt(0, 0, 0)
            .map_or(0, |t| t.and_utc().timestamp())
    };
    format!(
        "https://query1.finance.yahoo.com/v8/finance/chart/{}?period1={}&period2={}&interval=1d",
        encode(symbol),
        ts(from - Duration::days(10)),
        ts(to + Duration::days(1))
    )
}

pub fn search_url(q: &str) -> String {
    format!(
        "https://query2.finance.yahoo.com/v1/finance/search?q={}&quotesCount=10&newsCount=0",
        encode(q)
    )
}

/// The page a person can open to check a price against.
pub fn quote_page(symbol: &str) -> String {
    format!("https://finance.yahoo.com/quote/{}", encode(symbol))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }
    fn fixture(name: &str) -> serde_json::Value {
        let p = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
    }

    #[test]
    fn a_recorded_chart_is_read_into_dated_closes() {
        let s = parse_chart(&fixture("yahoo_chart_sap_de.json")).unwrap();
        assert_eq!(s.symbol, "SAP.DE");
        assert_eq!(s.currency, "EUR");
        assert!(s.closes.len() >= 15, "a month of trading days");
        assert!(
            s.closes.windows(2).all(|w| w[0].0 < w[1].0),
            "ordered, no repeats"
        );
        assert!(s.closes.iter().all(|(_, p)| *p > 10.0 && *p < 1_000.0));
        // 2024-03-02 was a Saturday: the price for it is Friday the 1st's.
        let sat = close_on_or_before(&s, d("2024-03-02"), 5).unwrap();
        assert_eq!(sat.0, d("2024-03-01"));
    }

    #[test]
    fn a_missing_symbol_and_a_stale_gap_are_not_prices() {
        assert_eq!(
            parse_chart(&fixture("yahoo_chart_missing.json")),
            Err(Problem::NotFound)
        );
        let s = Series {
            symbol: "X".into(),
            currency: "EUR".into(),
            closes: vec![(d("2024-01-02"), 10.0)],
        };
        assert!(close_on_or_before(&s, d("2024-03-01"), 7).is_none());
        assert!(
            close_on_or_before(&s, d("2023-12-31"), 7).is_none(),
            "before the first"
        );
        assert_eq!(close_on_or_before(&s, d("2024-01-05"), 7).unwrap().1, 10.0);
    }

    #[test]
    fn a_search_keeps_only_things_that_can_have_a_price() {
        let c = parse_search(&fixture("yahoo_search_sap.json"));
        let syms: Vec<&str> = c.iter().map(|c| c.symbol.as_str()).collect();
        assert!(syms.contains(&"SAP.DE"), "{syms:?}");
        let de = c.iter().find(|c| c.symbol == "SAP.DE").unwrap();
        assert_eq!(de.exchange, "XETRA");
        assert_eq!(de.name, "SAP SE I", "whitespace collapsed");
    }

    #[test]
    fn the_right_listing_is_found_among_wrong_currencies() {
        let series = |sym: &str, cur: &str| Series {
            symbol: sym.into(),
            currency: cur.into(),
            closes: vec![(d("2024-03-01"), 150.0)],
        };
        // "SAP" is the New York listing, in dollars; the euro one is further on.
        let found = resolve(
            &["SAP".to_string()],
            || {
                vec![
                    Candidate {
                        symbol: "SAP".into(),
                        name: String::new(),
                        exchange: "NYSE".into(),
                    },
                    Candidate {
                        symbol: "SAP.TO".into(),
                        name: String::new(),
                        exchange: "TSX".into(),
                    },
                    Candidate {
                        symbol: "SAP.DE".into(),
                        name: String::new(),
                        exchange: "XETRA".into(),
                    },
                ]
            },
            |s| {
                Ok(match s {
                    "SAP" => series("SAP", "USD"),
                    "SAP.TO" => series("SAP.TO", "CAD"),
                    _ => series(s, "EUR"),
                })
            },
            "EUR",
        )
        .unwrap();
        assert_eq!(found.symbol, "SAP.DE");

        // Only a dollar listing exists: say so, rather than "not found".
        let only = resolve(
            &["SAP".to_string()],
            Vec::new,
            |s| Ok(series(s, "USD")),
            "EUR",
        );
        assert_eq!(only, Err(Problem::OtherCurrency));

        let none = resolve(&[], Vec::new, |_| Err(Problem::NotFound), "EUR");
        assert_eq!(none, Err(Problem::NotFound));

        // A refusal ends the search at once instead of hammering the source.
        let mut calls = 0;
        let limited = resolve(
            &["A".into(), "B".into()],
            Vec::new,
            |_| {
                calls += 1;
                Err(Problem::RateLimited)
            },
            "EUR",
        );
        assert_eq!(limited, Err(Problem::RateLimited));
        assert_eq!(calls, 1);
    }

    #[test]
    fn a_coin_is_tried_as_its_pair_in_the_working_currency_first() {
        assert_eq!(first_guesses(Some("btc"), true, "EUR"), ["BTC-EUR", "btc"]);
        assert_eq!(first_guesses(Some("SAP.DE"), false, "EUR"), ["SAP.DE"]);
        assert_eq!(first_guesses(Some("BTC-EUR"), true, "EUR"), ["BTC-EUR"]);
        assert!(first_guesses(None, false, "EUR").is_empty());
    }

    #[test]
    fn urls_are_encoded_and_dated() {
        assert_eq!(encode("^GSPC"), "%5EGSPC");
        let u = chart_url("SAP.DE", d("2024-03-01"), d("2024-03-31"));
        assert!(u.contains("/chart/SAP.DE?period1="));
        assert!(u.ends_with("&interval=1d"));
        assert_eq!(
            search_url("S&P 500"),
            "https://query2.finance.yahoo.com/v1/finance/search?q=S%26P%20500&quotesCount=10&newsCount=0"
        );
    }

    #[test]
    fn pence_are_read_as_pounds_not_as_a_hundred_times_the_price() {
        let body = serde_json::json!({"chart":{"result":[{
            "meta":{"currency":"GBp","symbol":"VOD.L","gmtoffset":0},
            "timestamp":[1735732800],
            "indicators":{"quote":[{"close":[72.5]}]}
        }]}});
        let s = parse_chart(&body).unwrap();
        assert_eq!(s.currency, "GBP");
        assert!((s.closes[0].1 - 0.725).abs() < 1e-9, "{:?}", s.closes);
    }
}
