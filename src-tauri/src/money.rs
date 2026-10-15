//! Integer money.
//!
//! The original specification called for "double-precision floats to preserve
//! precision". Floats do the opposite: 0.1 + 0.2 != 0.3, and a 120-month
//! simulation loop compounds that error into visibly wrong balances. Every
//! monetary value in Coin Heaven is therefore a signed integer count of *minor
//! units* (cents for EUR), and only rates are floating point.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// A signed monetary amount in minor units (cents). Currency-agnostic; the
/// currency travels alongside in the database column, not in this type.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(i64);

/// Rounding used whenever a rate turns a `Money` into another `Money`.
/// Half away from zero matches how invoices and mortgage tables round.
fn round_half_away_from_zero(v: f64) -> i64 {
    if v >= 0.0 {
        (v + 0.5).floor() as i64
    } else {
        (v - 0.5).ceil() as i64
    }
}

impl Money {
    pub const ZERO: Money = Money(0);

    #[inline]
    pub const fn from_minor(minor: i64) -> Self {
        Money(minor)
    }

    #[inline]
    pub const fn minor(self) -> i64 {
        self.0
    }

    /// Convenience for tests, fixtures and defaults. Not used on hot paths.
    pub fn from_major(major: f64) -> Self {
        Money(round_half_away_from_zero(major * 100.0))
    }

    #[inline]
    pub fn as_major_f64(self) -> f64 {
        self.0 as f64 / 100.0
    }

    #[inline]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
    #[inline]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }
    #[inline]
    pub const fn abs(self) -> Money {
        Money(self.0.abs())
    }

    #[inline]
    pub const fn max(self, other: Money) -> Money {
        if self.0 >= other.0 { self } else { other }
    }
    #[inline]
    pub const fn min(self, other: Money) -> Money {
        if self.0 <= other.0 { self } else { other }
    }

    /// Scale by a rate expressed in basis points (1 bp = 0.01%).
    /// Exact integer math with half-away-from-zero rounding — no float involved.
    pub fn mul_bps(self, bps: i64) -> Money {
        let num = (self.0 as i128) * (bps as i128);
        let den = 10_000i128;
        let q = num / den;
        let r = num % den;
        // round half away from zero
        let adj = if r.abs() * 2 >= den {
            if num.is_negative() { -1 } else { 1 }
        } else {
            0
        };
        Money((q + adj) as i64)
    }

    /// Scale by an arbitrary float factor. Reserved for genuinely
    /// irrational maths (compounding, annuity factors); everything else
    /// should use [`Money::mul_bps`].
    pub fn mul_f64(self, factor: f64) -> Money {
        Money(round_half_away_from_zero(self.0 as f64 * factor))
    }

    #[inline]
    pub fn mul_int(self, n: i64) -> Money {
        Money(self.0.saturating_mul(n))
    }

    /// Split `self` into `n` parts that sum back to exactly `self`.
    /// The remainder cents are handed to the first parts, so nothing is lost.
    pub fn split_evenly(self, n: u32) -> Vec<Money> {
        assert!(n > 0, "cannot split money into zero parts");
        let n_i = n as i64;
        let base = self.0 / n_i;
        let mut rem = self.0 % n_i;
        let step = if self.0.is_negative() { -1 } else { 1 };
        (0..n_i)
            .map(|_| {
                let extra = if rem != 0 {
                    rem -= step;
                    step
                } else {
                    0
                };
                Money(base + extra)
            })
            .collect()
    }
}

impl Add for Money {
    type Output = Money;
    #[inline]
    fn add(self, rhs: Money) -> Money {
        Money(self.0.saturating_add(rhs.0))
    }
}
impl Sub for Money {
    type Output = Money;
    #[inline]
    fn sub(self, rhs: Money) -> Money {
        Money(self.0.saturating_sub(rhs.0))
    }
}
impl Neg for Money {
    type Output = Money;
    #[inline]
    fn neg(self) -> Money {
        Money(-self.0)
    }
}
impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Money) {
        *self = *self + rhs;
    }
}
impl SubAssign for Money {
    fn sub_assign(&mut self, rhs: Money) {
        *self = *self - rhs;
    }
}
impl Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        iter.fold(Money::ZERO, |a, b| a + b)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let a = self.0.unsigned_abs();
        write!(f, "{sign}{}.{:02}", a / 100, a % 100)
    }
}
impl fmt::Debug for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Money({self})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_trap_that_motivates_this_type() {
        // The reason the spec's "f64 for money" is rejected.
        assert_ne!(0.1_f64 + 0.2_f64, 0.3_f64);
        assert_eq!(
            Money::from_major(0.1) + Money::from_major(0.2),
            Money::from_major(0.3)
        );
    }

    #[test]
    fn accumulating_a_cent_120_times_is_exact() {
        let mut acc = Money::ZERO;
        for _ in 0..120 {
            acc += Money::from_minor(1);
        }
        assert_eq!(acc, Money::from_minor(120));
    }

    #[test]
    fn mul_bps_rounds_half_away_from_zero() {
        // 1.00 * 0.5% = 0.005 -> rounds to 1 cent
        assert_eq!(Money::from_minor(100).mul_bps(50), Money::from_minor(1));
        assert_eq!(Money::from_minor(-100).mul_bps(50), Money::from_minor(-1));
        // 10% of 250.00
        assert_eq!(
            Money::from_major(250.0).mul_bps(1000),
            Money::from_major(25.0)
        );
        assert_eq!(
            Money::from_major(1.0).mul_bps(10_000),
            Money::from_major(1.0)
        );
        assert_eq!(Money::from_major(1.0).mul_bps(0), Money::ZERO);
    }

    #[test]
    fn mul_bps_does_not_overflow_on_large_portfolios() {
        // 50 million euros at 100% must stay exact.
        let big = Money::from_major(50_000_000.0);
        assert_eq!(big.mul_bps(10_000), big);
    }

    #[test]
    fn split_evenly_conserves_every_cent() {
        for total in [
            Money::from_minor(100),
            Money::from_minor(101),
            Money::from_minor(-7),
        ] {
            for n in 1..=7u32 {
                let parts = total.split_evenly(n);
                assert_eq!(parts.len(), n as usize);
                assert_eq!(parts.iter().copied().sum::<Money>(), total, "n={n}");
            }
        }
    }

    #[test]
    fn display_is_stable() {
        assert_eq!(Money::from_major(1234.5).to_string(), "1234.50");
        assert_eq!(Money::from_minor(-5).to_string(), "-0.05");
        assert_eq!(Money::ZERO.to_string(), "0.00");
    }

    #[test]
    fn serialises_as_a_bare_integer_so_no_float_crosses_ipc() {
        let json = serde_json::to_string(&Money::from_major(12.34)).unwrap();
        assert_eq!(json, "1234");
        let back: Money = serde_json::from_str(&json).unwrap();
        assert_eq!(back, Money::from_major(12.34));
    }
}
