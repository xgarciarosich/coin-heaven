//! Transfer neutralisation and split-bill matching.
//!
//! Both engines **propose**; neither applies anything. Results go to a pending
//! drawer where the user confirms or rejects, and a rejection is remembered in
//! `match_decision` so the same pair is not offered again after every import.
//! An automatic, silent matcher would eventually neutralise a real expense and
//! quietly understate spending — the failure mode nobody notices.
//!
//! ## A note on the 72-hour window
//!
//! Bank CSV exports almost never carry a time of day; `txn.booked_on` is a
//! date. An hours-based window therefore degrades to whole days, and 72 hours
//! is implemented as 3 calendar days. If a provider ever supplies timestamps,
//! this is the place to tighten it.

use crate::money::Money;
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// The minimum a matcher needs to know about a transaction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TxnRef {
    pub id: i64,
    pub account_id: i64,
    pub booked_on: NaiveDate,
    pub amount_minor: Money,
    pub currency: String,
    pub description: String,
    /// Often the most identifying thing on a row — shown when reviewing a match.
    pub counterparty: Option<String>,
    pub is_neutral_transfer: bool,
    pub has_parent: bool,
}

/// Canonical pair key: lower id first, matching the `match_decision` table.
pub fn pair_key(a: i64, b: i64) -> (i64, i64) {
    if a < b { (a, b) } else { (b, a) }
}

// ---------------------------------------------------------------- transfers

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferParams {
    pub window_days: i64,
    /// Absorbs wire fees and rounding: the pair need not sum to exactly zero.
    pub tolerance_minor: i64,
}

impl Default for TransferParams {
    fn default() -> Self {
        TransferParams {
            window_days: 3,
            tolerance_minor: 200, // €2.00
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferCandidate {
    pub out_id: i64,
    pub in_id: i64,
    /// `out + in`. Zero for a clean pair; non-zero is the fee.
    pub residual_minor: Money,
    pub day_gap: i64,
}

/// Propose internal-transfer pairs.
///
/// A pair qualifies when it has opposite signs, sits in **different accounts**,
/// shares a currency, falls inside the date window, and sums to within
/// tolerance of zero. Matching is one-to-one: the best pair wins and both legs
/// are then unavailable, so one credit can never neutralise two debits.
pub fn find_transfers(
    txns: &[TxnRef],
    params: TransferParams,
    decided: &HashSet<(i64, i64)>,
) -> Vec<TransferCandidate> {
    let eligible: Vec<&TxnRef> = txns
        .iter()
        .filter(|t| !t.is_neutral_transfer && !t.amount_minor.is_zero())
        .collect();

    let mut scored: Vec<(i64, i64, TransferCandidate)> = Vec::new();
    for (i, a) in eligible.iter().enumerate() {
        for b in eligible.iter().skip(i + 1) {
            if a.account_id == b.account_id {
                continue;
            }
            if a.currency != b.currency {
                continue; // cross-currency needs an FX rate, not a tolerance
            }
            if a.amount_minor.is_negative() == b.amount_minor.is_negative() {
                continue; // same direction: not a transfer
            }
            if decided.contains(&pair_key(a.id, b.id)) {
                continue;
            }
            let gap = (a.booked_on - b.booked_on).num_days().abs();
            if gap > params.window_days {
                continue;
            }
            let residual = a.amount_minor + b.amount_minor;
            if residual.abs().minor() > params.tolerance_minor {
                continue;
            }
            let (out, inn) = if a.amount_minor.is_negative() {
                (a, b)
            } else {
                (b, a)
            };
            scored.push((
                residual.abs().minor(),
                gap,
                TransferCandidate {
                    out_id: out.id,
                    in_id: inn.id,
                    residual_minor: residual,
                    day_gap: gap,
                },
            ));
        }
    }

    // Tightest residual first, then closest in time, then by id so the output
    // is deterministic regardless of input ordering.
    scored.sort_by_key(|(res, gap, c)| (*res, *gap, c.out_id, c.in_id));

    let mut used: HashSet<i64> = HashSet::new();
    let mut out = Vec::new();
    for (_, _, c) in scored {
        if used.contains(&c.out_id) || used.contains(&c.in_id) {
            continue;
        }
        used.insert(c.out_id);
        used.insert(c.in_id);
        out.push(c);
    }
    out
}

// -------------------------------------------------------------- split bills

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitParams {
    /// 72 hours, expressed in whole days — see the module note.
    pub window_days: i64,
    /// Below this, a "1/2 match" is coincidence rather than a split bill.
    pub min_parent_minor: i64,
    pub tolerance_minor: i64,
}

impl Default for SplitParams {
    fn default() -> Self {
        SplitParams {
            window_days: 3,
            min_parent_minor: 3_000, // €30.00
            tolerance_minor: 100,    // €1.00
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitCandidate {
    pub parent_id: i64,
    pub reimbursement_id: i64,
    /// The denominator matched: 2 means the incoming payment is half the bill.
    pub fraction_denominator: u32,
    pub residual_minor: Money,
    pub day_gap: i64,
}

/// Denominators considered: a half, third, quarter or fifth of the bill.
const DENOMINATORS: [u32; 4] = [2, 3, 4, 5];

/// Propose reimbursements against a parent expense.
///
/// Guardrails: the parent must exceed `min_parent_minor`, the credit must land
/// on or after the parent and inside the window, and the amount must sit within
/// tolerance of an exact fraction. Everything proposed still requires
/// confirmation.
pub fn find_split_reimbursements(
    txns: &[TxnRef],
    params: SplitParams,
    decided: &HashSet<(i64, i64)>,
) -> Vec<SplitCandidate> {
    let parents: Vec<&TxnRef> = txns
        .iter()
        .filter(|t| {
            t.amount_minor.is_negative()
                && t.amount_minor.abs().minor() >= params.min_parent_minor
                && !t.is_neutral_transfer
        })
        .collect();

    let credits: Vec<&TxnRef> = txns
        .iter()
        .filter(|t| !t.amount_minor.is_negative() && !t.amount_minor.is_zero())
        .filter(|t| !t.is_neutral_transfer && !t.has_parent)
        .collect();

    let mut scored: Vec<(i64, i64, SplitCandidate)> = Vec::new();
    for p in &parents {
        for c in &credits {
            if p.id == c.id || p.currency != c.currency {
                continue;
            }
            if decided.contains(&pair_key(p.id, c.id)) {
                continue;
            }
            // A reimbursement follows its bill; it does not precede it.
            let gap = (c.booked_on - p.booked_on).num_days();
            if gap < 0 || gap > params.window_days {
                continue;
            }
            let total = p.amount_minor.abs().minor();
            let best = DENOMINATORS
                .iter()
                .map(|&d| {
                    let share = (total as f64 / d as f64).round() as i64;
                    ((c.amount_minor.minor() - share).abs(), d)
                })
                .min_by_key(|(residual, d)| (*residual, *d));

            if let Some((residual, d)) = best {
                if residual <= params.tolerance_minor {
                    scored.push((
                        residual,
                        gap,
                        SplitCandidate {
                            parent_id: p.id,
                            reimbursement_id: c.id,
                            fraction_denominator: d,
                            residual_minor: Money::from_minor(residual),
                            day_gap: gap,
                        },
                    ));
                }
            }
        }
    }

    scored.sort_by_key(|(res, gap, c)| (*res, *gap, c.parent_id, c.reimbursement_id));

    // A credit reimburses at most one bill; a bill may collect several credits.
    let mut used_credits: HashSet<i64> = HashSet::new();
    let mut out = Vec::new();
    for (_, _, c) in scored {
        if used_credits.insert(c.reimbursement_id) {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests;
