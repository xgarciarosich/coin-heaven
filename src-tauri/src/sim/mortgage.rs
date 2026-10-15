//! French-system (constant annuity) mortgage maths.
//!
//! The original spec listed "Mortgage Payment" as an input to net rent but
//! never defined it, and omitted the loan term from the slider list entirely —
//! even though term is one of the two strongest drivers of cash flow. Both are
//! made explicit here.

use crate::money::Money;

/// A single amortising loan.
#[derive(Debug, Clone, PartialEq)]
pub struct Loan {
    pub balance: Money,
    pub monthly_payment: Money,
    /// Nominal annual rate / 12. Zero is legal and handled separately.
    pub monthly_rate: f64,
    pub remaining_months: u32,
}

/// One month of amortisation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Period {
    pub interest: Money,
    pub principal: Money,
    pub payment: Money,
}

/// Constant monthly payment for a fully amortising loan:
///
/// ```text
///          i
/// P = L · ─────────────      (i = annual_rate / 12,  n = term in months)
///         1 − (1+i)^(−n)
/// ```
pub fn annuity_payment(principal: Money, annual_rate_bps: i64, term_months: u32) -> Money {
    assert!(term_months > 0, "loan term must be at least one month");
    if principal.is_zero() {
        return Money::ZERO;
    }
    let i = (annual_rate_bps as f64 / 10_000.0) / 12.0;
    if i.abs() < f64::EPSILON {
        // Interest-free: straight-line principal repayment.
        return Money::from_minor((principal.minor() as f64 / term_months as f64).ceil() as i64);
    }
    let factor = i / (1.0 - (1.0 + i).powi(-(term_months as i32)));
    principal.mul_f64(factor)
}

impl Loan {
    pub fn new(principal: Money, annual_rate_bps: i64, term_months: u32) -> Self {
        Loan {
            balance: principal,
            monthly_payment: annuity_payment(principal, annual_rate_bps, term_months),
            monthly_rate: (annual_rate_bps as f64 / 10_000.0) / 12.0,
            remaining_months: term_months,
        }
    }

    pub fn is_settled(&self) -> bool {
        self.balance.is_zero() || self.remaining_months == 0
    }

    /// Advance one month, returning the interest/principal split.
    ///
    /// The final instalment absorbs any rounding residue so the balance lands
    /// on exactly zero rather than a few stray cents.
    pub fn advance_one_month(&mut self) -> Period {
        if self.is_settled() {
            self.balance = Money::ZERO;
            return Period {
                interest: Money::ZERO,
                principal: Money::ZERO,
                payment: Money::ZERO,
            };
        }
        let interest = self.balance.mul_f64(self.monthly_rate);
        let mut principal = self.monthly_payment - interest;
        let mut payment = self.monthly_payment;

        if principal >= self.balance || self.remaining_months == 1 {
            principal = self.balance;
            payment = principal + interest;
        }
        self.balance -= principal;
        self.remaining_months -= 1;
        Period {
            interest,
            principal,
            payment,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_a_known_annuity_figure() {
        // 200,000 EUR at 2.80% nominal over 30 years.
        //   i = 0.028/12,  P = L·i / (1 − (1+i)^−360) = 821.7889…
        let p = annuity_payment(Money::from_major(200_000.0), 280, 360);
        assert!(
            (p.as_major_f64() - 821.79).abs() < 0.01,
            "got {p}, expected 821.79"
        );

        // A second independent point: 96,000 EUR at 2.80% over 25 years.
        let q = annuity_payment(Money::from_major(96_000.0), 280, 300);
        assert!((q.as_major_f64() - 445.32).abs() < 0.01, "got {q}");
    }

    #[test]
    fn zero_interest_is_straight_line() {
        let p = annuity_payment(Money::from_major(1_200.0), 0, 12);
        assert_eq!(p, Money::from_major(100.0));
    }

    #[test]
    fn amortisation_fully_repays_the_principal_and_lands_on_zero() {
        for (amount, bps, months) in [
            (150_000.0, 280, 300),
            (80_000.0, 450, 180),
            (250_000.0, 100, 360),
            (10_000.0, 0, 24),
        ] {
            let principal = Money::from_major(amount);
            let mut loan = Loan::new(principal, bps, months);
            let mut repaid = Money::ZERO;
            for _ in 0..months {
                repaid += loan.advance_one_month().principal;
            }
            assert_eq!(loan.balance, Money::ZERO, "{amount}@{bps}bps/{months}m");
            assert_eq!(repaid, principal, "principal must reconcile exactly");
        }
    }

    #[test]
    fn interest_share_falls_as_the_loan_ages() {
        let mut loan = Loan::new(Money::from_major(200_000.0), 280, 360);
        let first = loan.advance_one_month();
        for _ in 0..200 {
            loan.advance_one_month();
        }
        let later = loan.advance_one_month();
        assert!(later.interest < first.interest);
        assert!(later.principal > first.principal);
    }

    #[test]
    fn settled_loan_stops_charging() {
        let mut loan = Loan::new(Money::from_major(1_000.0), 300, 12);
        for _ in 0..12 {
            loan.advance_one_month();
        }
        let after = loan.advance_one_month();
        assert_eq!(after.payment, Money::ZERO);
        assert!(loan.is_settled());
    }
}
