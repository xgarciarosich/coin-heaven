//! Import idempotency.
//!
//! Hash composition, as decided:
//!
//! ```text
//! account_id · booked_on · amount_minor · normalise_description(desc) · intra_day_sequence
//! ```
//!
//! The sequence number is what keeps two genuine €2.00 coffees bought at the
//! same shop on the same day from collapsing into one row. It is assigned per
//! `(account, date, amount, normalised description)` group in file order,
//! starting at zero.
//!
//! That ordinal is computed **within the file being imported**, not against the
//! database — which is what makes an overlapping re-import correct. A statement
//! listing coffee #0 and coffee #1 always produces the same two ordinals, so
//! the first is rejected by the unique index as a duplicate and the second is
//! inserted. This relies on bank exports listing the same transactions in the
//! same order, which they do; a bank that reorders rows between exports would
//! need a provider-supplied transaction id instead.

use crate::money::Money;
use crate::security::pii::normalise_description;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const DOMAIN: &str = "atlas-txn-dedupe-v1";

/// A parsed CSV row, before it becomes a transaction.
#[derive(Debug, Clone, PartialEq)]
pub struct RawRow {
    pub booked_on: String, // ISO-8601 YYYY-MM-DD
    pub amount_minor: Money,
    pub description: String,
}

/// Fields are length-prefixed before hashing. Without that, `("ab", 1)` and
/// `("a", "b1")` would hash identically.
fn field(h: &mut Sha256, s: &str) {
    h.update((s.len() as u64).to_le_bytes());
    h.update(s.as_bytes());
}

pub fn dedupe_hash(
    account_id: i64,
    booked_on: &str,
    amount_minor: Money,
    description: &str,
    intra_day_sequence: u32,
) -> String {
    let mut h = Sha256::new();
    field(&mut h, DOMAIN);
    field(&mut h, &account_id.to_string());
    field(&mut h, booked_on);
    field(&mut h, &amount_minor.minor().to_string());
    field(&mut h, &normalise_description(description));
    field(&mut h, &intra_day_sequence.to_string());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// A row ready to insert, with its ordinal resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedRow {
    pub booked_on: String,
    pub amount_minor: Money,
    pub description_raw: String,
    pub description_norm: String,
    pub intra_day_sequence: u32,
    pub dedupe_hash: String,
}

/// Assign ordinals in file order and compute each row's hash.
pub fn prepare(account_id: i64, rows: &[RawRow]) -> Vec<PreparedRow> {
    let mut seen: HashMap<(String, i64, String), u32> = HashMap::new();
    rows.iter()
        .map(|r| {
            let norm = normalise_description(&r.description);
            let key = (r.booked_on.clone(), r.amount_minor.minor(), norm.clone());
            let seq = seen.entry(key).or_insert(0);
            let n = *seq;
            *seq += 1;
            PreparedRow {
                booked_on: r.booked_on.clone(),
                amount_minor: r.amount_minor,
                description_raw: r.description.clone(),
                description_norm: norm,
                intra_day_sequence: n,
                dedupe_hash: dedupe_hash(
                    account_id,
                    &r.booked_on,
                    r.amount_minor,
                    &r.description,
                    n,
                ),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(date: &str, minor: i64, desc: &str) -> RawRow {
        RawRow {
            booked_on: date.into(),
            amount_minor: Money::from_minor(minor),
            description: desc.into(),
        }
    }

    #[test]
    fn identical_input_hashes_identically() {
        let a = dedupe_hash(1, "2026-01-05", Money::from_minor(-200), "CAFE X", 0);
        let b = dedupe_hash(1, "2026-01-05", Money::from_minor(-200), "CAFE X", 0);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[test]
    fn every_component_changes_the_hash() {
        let base = dedupe_hash(1, "2026-01-05", Money::from_minor(-200), "CAFE X", 0);
        assert_ne!(
            base,
            dedupe_hash(2, "2026-01-05", Money::from_minor(-200), "CAFE X", 0)
        );
        assert_ne!(
            base,
            dedupe_hash(1, "2026-01-06", Money::from_minor(-200), "CAFE X", 0)
        );
        assert_ne!(
            base,
            dedupe_hash(1, "2026-01-05", Money::from_minor(-201), "CAFE X", 0)
        );
        assert_ne!(
            base,
            dedupe_hash(1, "2026-01-05", Money::from_minor(-200), "CAFE Y", 0)
        );
        assert_ne!(
            base,
            dedupe_hash(1, "2026-01-05", Money::from_minor(-200), "CAFE X", 1)
        );
    }

    #[test]
    fn two_identical_coffees_on_one_day_stay_distinct() {
        let rows = vec![
            row("2026-01-05", -200, "CAFE CENTRAL"),
            row("2026-01-05", -200, "CAFE CENTRAL"),
        ];
        let prepared = prepare(1, &rows);
        assert_eq!(prepared[0].intra_day_sequence, 0);
        assert_eq!(prepared[1].intra_day_sequence, 1);
        assert_ne!(prepared[0].dedupe_hash, prepared[1].dedupe_hash);
    }

    #[test]
    fn an_overlapping_reimport_reproduces_the_same_hashes() {
        // January export.
        let jan = vec![
            row("2026-01-05", -200, "CAFE CENTRAL"),
            row("2026-01-05", -200, "CAFE CENTRAL"),
            row("2026-01-20", -4500, "TRATTORIA"),
        ];
        // January–February export: same January rows, plus February.
        let jan_feb = vec![
            row("2026-01-05", -200, "CAFE CENTRAL"),
            row("2026-01-05", -200, "CAFE CENTRAL"),
            row("2026-01-20", -4500, "TRATTORIA"),
            row("2026-02-02", -1200, "MERCADONA"),
        ];

        let a = prepare(7, &jan);
        let b = prepare(7, &jan_feb);

        for (x, y) in a.iter().zip(b.iter()) {
            assert_eq!(
                x.dedupe_hash, y.dedupe_hash,
                "overlapping rows must collide"
            );
        }
        // Only the February row is new.
        let old: std::collections::HashSet<_> = a.iter().map(|r| &r.dedupe_hash).collect();
        let fresh = b.iter().filter(|r| !old.contains(&r.dedupe_hash)).count();
        assert_eq!(fresh, 1);
    }

    #[test]
    fn cosmetic_description_noise_does_not_create_a_new_row() {
        // The same purchase exported twice with different reference numbers.
        let a = dedupe_hash(1, "2026-01-05", Money::from_minor(-200), "CAFE X 4471", 0);
        let b = dedupe_hash(1, "2026-01-05", Money::from_minor(-200), "cafe  x  9982", 0);
        assert_eq!(
            a, b,
            "normalisation should absorb reference digits and case"
        );
    }

    #[test]
    fn ordinals_are_scoped_per_group_not_per_file() {
        let rows = vec![
            row("2026-01-05", -200, "CAFE"),
            row("2026-01-05", -900, "TAXI"),
            row("2026-01-05", -200, "CAFE"),
        ];
        let p = prepare(1, &rows);
        assert_eq!(p[0].intra_day_sequence, 0);
        assert_eq!(
            p[1].intra_day_sequence, 0,
            "a different group restarts at 0"
        );
        assert_eq!(p[2].intra_day_sequence, 1);
    }

    #[test]
    fn length_prefixing_prevents_field_boundary_collisions() {
        // "ab" + seq 1  vs  "a" + seq ... would collide under naive concatenation.
        let a = dedupe_hash(1, "2026-01-05", Money::from_minor(-1), "ab", 1);
        let b = dedupe_hash(1, "2026-01-05", Money::from_minor(-1), "a", 11);
        assert_ne!(a, b);
    }
}
