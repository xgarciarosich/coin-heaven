//! Holdings, recurring buys and returns — the arithmetic, without I/O.
//!
//! Everything a portfolio screen shows is derived from dated facts: buys and
//! sells with their cash amounts, occasional reconciliations of how many units
//! are held, and prices on given dates. Nothing here stores a running total,
//! because a running total is a number that can disagree with its own history.

use crate::money::Money;
use chrono::{Datelike, Duration, NaiveDate};

/// One unit, in the ×1e8 fixed point used for units.
pub const UNIT: i128 = 100_000_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Buy,
    Sell,
    /// "I held exactly this many units on this date."
    Units,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Txn {
    pub on: NaiveDate,
    pub kind: Kind,
    pub amount: Money,
    /// `None` when only the cash amount is known.
    pub units_e8: Option<i64>,
    pub skipped: bool,
}

/* -------------------------------------------------------- schedules -- */

/// Every date a recurring buy fell on, from its start up to `until`.
///
/// Monthly schedules on the 29th–31st land on the last day of shorter months
/// rather than skipping them: a direct debit set for the 31st still happens
/// in February.
pub fn schedule_dates(
    every_days: Option<u32>,
    day_of_month: Option<u32>,
    start: NaiveDate,
    end: Option<NaiveDate>,
    until: NaiveDate,
) -> Vec<NaiveDate> {
    let last = end.map_or(until, |e| e.min(until));
    let mut out = Vec::new();
    if last < start {
        return out;
    }
    match (every_days, day_of_month) {
        (Some(n), _) if n > 0 => {
            let mut d = start;
            while d <= last {
                out.push(d);
                d += Duration::days(n as i64);
            }
        }
        (_, Some(dom)) => {
            let (mut y, mut m) = (start.year(), start.month());
            loop {
                let day = dom.min(days_in_month(y, m));
                let Some(d) = NaiveDate::from_ymd_opt(y, m, day) else {
                    break;
                };
                if d > last {
                    break;
                }
                if d >= start {
                    out.push(d);
                }
                (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
            }
        }
        _ => {}
    }
    out
}

fn days_in_month(y: i32, m: u32) -> u32 {
    let next = if m == 12 {
        NaiveDate::from_ymd_opt(y + 1, 1, 1)
    } else {
        NaiveDate::from_ymd_opt(y, m + 1, 1)
    };
    next.and_then(|n| n.pred_opt()).map_or(28, |d| d.day())
}

/* ------------------------------------------------------------ units -- */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Units {
    pub e8: i64,
    /// False when some buy or sell counted here has no unit figure, so the
    /// total is a floor rather than the truth.
    pub complete: bool,
}

/// Units held at the end of `d`.
///
/// Starts from the last reconciliation on or before `d` (or zero), then adds
/// buys and removes sells after it. A skipped scheduled buy counts for nothing.
pub fn units_at(txns: &[Txn], d: NaiveDate) -> Units {
    let upto: Vec<&Txn> = txns.iter().filter(|t| t.on <= d && !t.skipped).collect();
    let base = upto
        .iter()
        .filter(|t| t.kind == Kind::Units)
        .max_by_key(|t| t.on)
        .copied();
    let (mut e8, since) = match base {
        Some(b) => (b.units_e8.unwrap_or(0) as i128, Some(b.on)),
        None => (0, None),
    };
    let mut complete = true;
    for t in upto.iter().filter(|t| since.is_none_or(|s| t.on > s)) {
        match (&t.kind, t.units_e8) {
            (Kind::Buy, Some(u)) => e8 += u as i128,
            (Kind::Sell, Some(u)) => e8 -= u as i128,
            (Kind::Units, _) => {}
            (_, None) => complete = false,
        }
    }
    Units {
        e8: e8.max(0) as i64,
        complete,
    }
}

/// Net cash put in up to the end of `d`: buys less sells.
pub fn contributions_at(txns: &[Txn], d: NaiveDate) -> Money {
    txns.iter()
        .filter(|t| t.on <= d && !t.skipped)
        .map(|t| match t.kind {
            Kind::Buy => t.amount,
            Kind::Sell => -t.amount,
            Kind::Units => Money::ZERO,
        })
        .sum()
}

/// Work out unit counts nobody typed, where the other facts pin them down.
///
/// A recurring buy says what it cost and not what it bought, and asking for
/// the units of every one of them is asking for the spreadsheet this app is
/// meant to replace. Two things can stand in:
///
/// * a later reconciliation — "I hold 0.02 BTC" — fixes the total, so the
///   buys before it share out what the known buys leave over, in proportion
///   to what each cost;
/// * otherwise a price: the buy's amount at the price recorded closest to
///   its date. A price check fetches month-end prices for exactly this, so a
///   buy is never more than about two weeks from the price it is valued at.
///
/// Returns the movements with those counts filled in, and which ones were.
/// A buy with neither a reconciliation after it nor any price stays unknown.
pub fn fill_units(txns: &[Txn], prices: &[(NaiveDate, i64)]) -> (Vec<Txn>, Vec<bool>) {
    let mut out = txns.to_vec();
    let mut estimated = vec![false; txns.len()];
    let live = |t: &Txn| !t.skipped;
    let unknown = |t: &Txn| live(t) && t.kind != Kind::Units && t.units_e8.is_none();

    // Reconciliations, oldest first.
    let mut recs: Vec<(NaiveDate, i128)> = txns
        .iter()
        .filter(|t| live(t) && t.kind == Kind::Units)
        .filter_map(|t| t.units_e8.map(|u| (t.on, u as i128)))
        .collect();
    recs.sort_by_key(|r| r.0);

    let mut prev: Option<(NaiveDate, i128)> = None;
    for &(on, total) in &recs {
        let inside = |t: &Txn| live(t) && t.on <= on && prev.is_none_or(|p| t.on > p.0);
        let known: i128 = txns
            .iter()
            .filter(|t| inside(t))
            .map(|t| match (&t.kind, t.units_e8) {
                (Kind::Buy, Some(u)) => u as i128,
                (Kind::Sell, Some(u)) => -(u as i128),
                _ => 0,
            })
            .sum();
        let left = (total - prev.map_or(0, |p| p.1) - known).max(0);
        let cost: i128 = txns
            .iter()
            .filter(|t| inside(t) && unknown(t) && t.kind == Kind::Buy)
            .map(|t| t.amount.minor() as i128)
            .sum();
        for (i, t) in txns.iter().enumerate() {
            if inside(t) && unknown(t) && t.kind == Kind::Buy && cost > 0 {
                out[i].units_e8 = Some((left * t.amount.minor() as i128 / cost) as i64);
                estimated[i] = true;
            }
        }
        prev = Some((on, total));
    }

    // Whatever no reconciliation covers falls back on a price.
    for (i, t) in txns.iter().enumerate() {
        if !unknown(t) || estimated[i] {
            continue;
        }
        // The price recorded closest to the day of the buy, before or after;
        // of two equally close, the earlier.
        let price = prices
            .iter()
            .min_by_key(|p| ((p.0 - t.on).num_days().abs(), p.0))
            .map(|p| p.1)
            .filter(|p| *p > 0);
        if let Some(p) = price {
            // cents ÷ µ€ per unit, into units ×1e8: ×1e12.
            out[i].units_e8 =
                Some((t.amount.minor() as i128 * 1_000_000_000_000 / p as i128) as i64);
            estimated[i] = true;
        }
    }
    (out, estimated)
}

/// Whether the count on `d` leans on any worked-out figure from
/// [`fill_units`]: one made since the reconciliation the count starts from.
pub fn estimated_at(txns: &[Txn], estimated: &[bool], d: NaiveDate) -> bool {
    let since = txns
        .iter()
        .filter(|t| t.on <= d && !t.skipped && t.kind == Kind::Units)
        .map(|t| t.on)
        .max();
    txns.iter()
        .zip(estimated)
        .any(|(t, e)| *e && t.on <= d && since.is_none_or(|s| t.on > s))
}

/// Units × price, in cents. Price is in millionths of a euro per unit.
pub fn value(units_e8: i64, price_micro: i64) -> Money {
    // e8 · µ€ = 1e-14 €; one cent is 1e-2 €, so divide by 1e12, rounding.
    let raw = units_e8 as i128 * price_micro as i128;
    Money::from_minor(((raw + 500_000_000_000) / 1_000_000_000_000) as i64)
}

/* ----------------------------------------------------------- return -- */

/// Money-weighted annual return (XIRR).
///
/// `flows` are cash from the investor's side: negative going in, positive
/// coming out, with the final value as the last positive flow. This is the
/// honest return for someone who buys on a schedule — a time-weighted return
/// needs a valuation on every date money moved, and with half-yearly prices
/// that would be a guess presented as a measurement.
///
/// Solved by bisection: slower than Newton's method, but it cannot diverge.
pub fn xirr(flows: &[(NaiveDate, f64)]) -> Option<f64> {
    let first = flows.iter().map(|f| f.0).min()?;
    let has_in = flows.iter().any(|f| f.1 < 0.0);
    let has_out = flows.iter().any(|f| f.1 > 0.0);
    if !has_in || !has_out {
        return None;
    }
    let npv = |r: f64| -> f64 {
        flows
            .iter()
            .map(|(d, v)| v / (1.0 + r).powf((*d - first).num_days() as f64 / 365.0))
            .sum()
    };
    let (mut lo, mut hi) = (-0.9999, 10.0);
    let (flo, fhi) = (npv(lo), npv(hi));
    if flo.signum() == fhi.signum() {
        return None;
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if npv(mid).signum() == flo.signum() {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    // Under a few months of history an annualised figure is noise dressed as
    // a result: 2% in six weeks is not a 17% year.
    let span = (flows.iter().map(|f| f.0).max()? - first).num_days();
    (span >= 90).then_some((lo + hi) / 2.0)
}

/* ------------------------------------------------------ price dates -- */

/// The month-end dates worth asking a price for, so that blind buys can be
/// turned into units and the history chart has something to stand on.
///
/// One per month from the first movement up to the month before `today`,
/// leaving out months that already have a price. When that is more than
/// `cap`, they are thinned evenly — the most recent always kept — because a
/// lookup of sixty dates is a lookup nobody waits for.
pub fn month_ends_to_price(
    first: NaiveDate,
    priced: &[NaiveDate],
    today: NaiveDate,
    cap: usize,
) -> Vec<NaiveDate> {
    let mut out = Vec::new();
    let (mut y, mut m) = (first.year(), first.month());
    while (y, m) < (today.year(), today.month()) {
        let end = NaiveDate::from_ymd_opt(y, m, days_in_month(y, m)).unwrap_or(first);
        if !priced.iter().any(|p| p.year() == y && p.month() == m) {
            out.push(end);
        }
        (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    }
    if cap == 0 {
        return Vec::new();
    }
    if out.len() > cap {
        let n = out.len();
        out = (0..cap)
            .map(|i| out[n - 1 - (i * (n - 1)) / (cap - 1).max(1)])
            .rev()
            .collect();
        out.dedup();
    }
    out
}

/* --------------------------------------------------------- semesters -- */

/// The most recent half-year checkpoint on or before `today`: 30 June or
/// 31 December. The year-end is kept on the 31st — the same moment as
/// 1 January, and the day that actually has a closing price.
pub fn last_checkpoint(today: NaiveDate) -> NaiveDate {
    let june = NaiveDate::from_ymd_opt(today.year(), 6, 30).unwrap_or(today);
    let dec = NaiveDate::from_ymd_opt(today.year(), 12, 31).unwrap_or(today);
    if today >= dec {
        dec
    } else if today >= june {
        june
    } else {
        NaiveDate::from_ymd_opt(today.year() - 1, 12, 31).unwrap_or(today)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn buy(on: &str, eur: i64, units: Option<f64>) -> Txn {
        Txn {
            on: d(on),
            kind: Kind::Buy,
            amount: Money::from_minor(eur * 100),
            units_e8: units.map(|u| (u * 1e8).round() as i64),
            skipped: false,
        }
    }

    #[test]
    fn every_fifteen_days_since_january_2025() {
        let dates = schedule_dates(Some(15), None, d("2025-01-01"), None, d("2026-09-27"));
        assert_eq!(dates[0], d("2025-01-01"));
        assert_eq!(dates[1], d("2025-01-16"));
        // 634 days from 1 Jan 2025 to 27 Sep 2026: 42 full steps plus the first.
        assert_eq!(dates.len(), 43);
        assert!(*dates.last().unwrap() <= d("2026-09-27"));
    }

    #[test]
    fn a_monthly_buy_on_the_31st_still_happens_in_february() {
        let dates = schedule_dates(None, Some(31), d("2026-01-31"), None, d("2026-04-30"));
        assert_eq!(
            dates,
            vec![
                d("2026-01-31"),
                d("2026-02-28"),
                d("2026-03-31"),
                d("2026-04-30")
            ]
        );
    }

    #[test]
    fn a_schedule_stops_at_its_end_date() {
        let dates = schedule_dates(
            Some(7),
            None,
            d("2026-01-01"),
            Some(d("2026-01-20")),
            d("2026-12-31"),
        );
        assert_eq!(
            dates,
            vec![d("2026-01-01"), d("2026-01-08"), d("2026-01-15")]
        );
    }

    #[test]
    fn units_add_up_when_every_buy_says_how_many() {
        let t = vec![
            buy("2026-01-01", 100, Some(0.001)),
            buy("2026-02-01", 100, Some(0.0012)),
        ];
        let u = units_at(&t, d("2026-06-30"));
        assert_eq!(u.e8, 220_000);
        assert!(u.complete);
    }

    #[test]
    fn a_buy_known_only_by_its_amount_makes_the_count_incomplete() {
        let t = vec![
            buy("2026-01-01", 100, Some(0.001)),
            buy("2026-02-01", 100, None),
        ];
        let u = units_at(&t, d("2026-06-30"));
        assert_eq!(u.e8, 100_000);
        assert!(!u.complete);
    }

    #[test]
    fn a_reconciliation_resets_the_count_and_later_buys_add_to_it() {
        let mut t = vec![
            buy("2025-01-01", 50, None),
            buy("2025-01-16", 50, None),
            Txn {
                on: d("2026-06-30"),
                kind: Kind::Units,
                amount: Money::ZERO,
                units_e8: Some(1_500_000),
                skipped: false,
            },
        ];
        let at_june = units_at(&t, d("2026-06-30"));
        assert_eq!(
            at_june,
            Units {
                e8: 1_500_000,
                complete: true
            },
            "unknown buys before it no longer matter"
        );
        t.push(buy("2026-07-15", 50, Some(0.0005)));
        assert_eq!(units_at(&t, d("2026-12-31")).e8, 1_550_000);
    }

    #[test]
    fn a_reconciliation_shares_its_total_over_the_buys_before_it() {
        // Two blind buys, €100 and €300, then "I hold 0.04".
        let mut t = vec![buy("2025-01-01", 100, None), buy("2025-02-01", 300, None)];
        t.push(Txn {
            on: d("2025-03-01"),
            kind: Kind::Units,
            amount: Money::ZERO,
            units_e8: Some(4_000_000),
            skipped: false,
        });
        let (filled, est) = fill_units(&t, &[]);
        assert_eq!(filled[0].units_e8, Some(1_000_000), "a quarter of the cost");
        assert_eq!(filled[1].units_e8, Some(3_000_000));
        assert_eq!(est, vec![true, true, false]);
        // Before the reconciliation the count is worked out; on it, it is read.
        let jan = units_at(&filled, d("2025-01-15"));
        assert_eq!((jan.e8, jan.complete), (1_000_000, true));
        assert!(estimated_at(&filled, &est, d("2025-01-15")));
        assert!(!estimated_at(&filled, &est, d("2025-03-01")));
    }

    #[test]
    fn known_buys_keep_their_units_and_the_rest_share_what_is_left() {
        let mut t = vec![
            buy("2025-01-01", 100, Some(0.01)),
            buy("2025-02-01", 100, None),
        ];
        t.push(Txn {
            on: d("2025-03-01"),
            kind: Kind::Units,
            amount: Money::ZERO,
            units_e8: Some(2_500_000),
            skipped: false,
        });
        let (filled, est) = fill_units(&t, &[]);
        assert_eq!(filled[0].units_e8, Some(1_000_000));
        assert_eq!(filled[1].units_e8, Some(1_500_000));
        assert_eq!(est, vec![false, true, false]);
    }

    #[test]
    fn a_blind_buy_no_reconciliation_covers_is_valued_at_the_nearest_price() {
        // €500 at €50,000 a unit is 0.01.
        let t = vec![buy("2025-06-10", 500, None)];
        let prices = [
            (d("2025-06-01"), 50_000_000_000),
            (d("2025-07-01"), 99_000_000_000),
        ];
        let (filled, est) = fill_units(&t, &prices);
        assert_eq!(filled[0].units_e8, Some(1_000_000));
        assert!(est[0]);
        // The closest price wins, whichever side of the buy it falls on.
        let (filled, _) = fill_units(&t, &prices[1..]);
        assert_eq!(filled[0].units_e8, Some(505_050));
        let near_after = [
            (d("2025-05-01"), 50_000_000_000),
            (d("2025-06-12"), 100_000_000_000),
        ];
        assert_eq!(fill_units(&t, &near_after).0[0].units_e8, Some(500_000));
        // No price at all: still unknown.
        let (filled, est) = fill_units(&t, &[]);
        assert_eq!((filled[0].units_e8, est[0]), (None, false));
    }

    #[test]
    fn a_skipped_buy_neither_costs_nor_counts() {
        let mut skipped = buy("2026-01-16", 50, None);
        skipped.skipped = true;
        let t = vec![buy("2026-01-01", 50, Some(0.001)), skipped];
        assert_eq!(contributions_at(&t, d("2026-12-31")).minor(), 5_000);
        assert!(units_at(&t, d("2026-12-31")).complete);
    }

    #[test]
    fn value_is_units_times_price_to_the_cent() {
        // 0.015 BTC at €90,000.00 = €1,350.00
        assert_eq!(value(1_500_000, 90_000_000_000).minor(), 135_000);
        // 12.5 units at €0.000123 each = €0.0015375 → €0.00
        assert_eq!(value(1_250_000_000, 123).minor(), 0);
    }

    #[test]
    fn xirr_of_a_simple_doubling_in_one_year_is_one_hundred_percent() {
        let r = xirr(&[(d("2025-01-01"), -1000.0), (d("2026-01-01"), 2000.0)]).unwrap();
        assert!((r - 1.0).abs() < 1e-4, "{r}");
    }

    #[test]
    fn xirr_of_regular_buys_matches_a_known_answer() {
        // 12 monthly buys of 100 then worth 1,300. Checked independently
        // (bisection on the same flows, /365 day count): 0.15670.
        let mut flows: Vec<(NaiveDate, f64)> = (1..=12)
            .map(|m| (NaiveDate::from_ymd_opt(2025, m, 1).unwrap(), -100.0))
            .collect();
        flows.push((d("2026-01-01"), 1300.0));
        let r = xirr(&flows).unwrap();
        assert!((r - 0.1567).abs() < 0.0005, "{r}");
    }

    #[test]
    fn a_few_weeks_of_history_does_not_get_an_annual_return() {
        assert_eq!(
            xirr(&[(d("2026-09-01"), -100.0), (d("2026-09-20"), 102.0)]),
            None
        );
    }

    #[test]
    fn month_ends_are_asked_for_once_and_thinned_when_there_are_many() {
        let ends = month_ends_to_price(d("2026-01-15"), &[d("2026-02-10")], d("2026-05-20"), 24);
        assert_eq!(
            ends,
            vec![d("2026-01-31"), d("2026-03-31"), d("2026-04-30")],
            "February already has a price; May is not over"
        );
        let many = month_ends_to_price(d("2020-01-01"), &[], d("2026-05-20"), 12);
        assert_eq!(many.len(), 12);
        assert_eq!(many.first(), Some(&d("2020-01-31")));
        assert_eq!(
            many.last(),
            Some(&d("2026-04-30")),
            "the latest is always kept"
        );
        assert!(many.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn checkpoints_are_the_end_of_june_and_the_end_of_december() {
        assert_eq!(last_checkpoint(d("2026-09-27")), d("2026-06-30"));
        assert_eq!(last_checkpoint(d("2026-06-30")), d("2026-06-30"));
        assert_eq!(last_checkpoint(d("2026-03-10")), d("2025-12-31"));
        assert_eq!(last_checkpoint(d("2026-12-31")), d("2026-12-31"));
    }
}
