//! A buy-to-let plan, projected month by month.
//!
//! The person decides what to buy and when. Each planned property carries its
//! own price, renovation, rent and purchase tax — the things that differ from
//! one flat to the next and from one region to the next — and may carry its
//! own mortgage terms. Around them sit two smaller sets of numbers: the
//! person's own (income, how it grows, what they put aside, the cash they
//! start with) and the general ones that hold for every purchase unless a
//! property says otherwise (mortgage terms, the bank's debt limit, how rents
//! and prices rise, running costs).
//!
//! The engine does not choose purchases. It checks each one against the two
//! things that can stop it — the cash at the notary and the bank's
//! affordability test — and if the planned month fails, it tries every month
//! after and reports the delay and its cause. An earlier version bought
//! automatically whenever cash allowed; that answered "how many could I
//! own?", which is not a question anyone plans a purchase around.
//!
//! Conventions kept from that version, all deliberate — see
//! `docs/SIMULATOR-MATH.md`:
//!
//!  * Closing costs are transfer tax **plus** notary/registry/gestoría/AJD.
//!  * Rent is reduced by occupancy, management, a maintenance reserve and
//!    fixed annual costs before debt service, and taxed on profit after
//!    interest.
//!  * The bank's limit is a debt-to-income ratio in which only a share of
//!    rental income counts.
//!  * Values and rents are a fixed base times an index factor, never a rounded
//!    integer compounded month over month, so no rounding drift accumulates.

pub mod mortgage;

use crate::money::Money;
use mortgage::Loan;
use serde::{Deserialize, Serialize};

/// One purchase the person intends to make.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedProperty {
    pub name: String,
    /// Month of the projection the purchase is planned for; 1 is the first.
    pub buy_month: u32,
    /// What is paid to the seller.
    pub price: Money,
    pub renovation: Money,
    /// Gross rent a month when first let. Indexed from then on.
    pub monthly_rent: Money,
    /// Transfer tax (ITP) or its equivalent. It depends on the region and on
    /// the buyer, which is why it lives with the property.
    pub transfer_tax_bps: i64,
    /// Mortgage terms for this purchase; `None` takes the general ones.
    #[serde(default)]
    pub financing_bps: Option<i64>,
    #[serde(default)]
    pub mortgage_rate_bps: Option<i64>,
    #[serde(default)]
    pub mortgage_term_months: Option<u32>,
    /// IBI, insurance and community fees a year; `None` takes the general one.
    #[serde(default)]
    pub fixed_costs_annual: Option<Money>,
}

/// Everything the projection is run from.
/// All rates are basis points (1 bp = 0.01%): 2.8% -> 280, 80% -> 8000.
///
/// Fields added after the first release default when missing, and fields that
/// no longer exist are ignored, so a scenario saved by an older version still
/// loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimParams {
    pub horizon_years: u32,
    /// The calendar month the plan starts in, "YYYY-MM". The engine counts
    /// months from 1 and never reads this; it is carried so that "month 18"
    /// keeps meaning the same real month each time the plan is reopened,
    /// rather than sliding forward with the clock.
    #[serde(default)]
    pub start_month: Option<String>,

    // --- The person --------------------------------------------------------
    pub start_cash: Money,
    /// What is put aside each month in year one, before any rent.
    pub monthly_savings: Money,
    /// Net take-home pay in year one, used for the affordability check.
    pub net_monthly_salary: Money,
    /// Yearly rise in pay. What is put aside rises with it.
    #[serde(default)]
    pub income_growth_bps: i64,
    /// Existing monthly debt service already on the credit register.
    pub existing_monthly_debt: Money,
    /// Cash floor never spent at the notary.
    pub cash_buffer: Money,
    /// Net rent joins the cash that funds the next purchase.
    pub reinvest_net_rent: bool,

    // --- General: every purchase, unless the property says otherwise -------
    /// Loan-to-value.
    pub financing_bps: i64,
    pub mortgage_rate_bps: i64,
    pub mortgage_term_months: u32,
    /// Notary + land registry + gestoría + AJD, as a share of price.
    pub other_closing_bps: i64,
    /// Maximum share of computable income that may go to debt service.
    pub dti_max_bps: i64,
    /// Lender haircut applied to rental income when assessing capacity.
    pub computable_rent_bps: i64,

    // --- General: the market and the running of it --------------------------
    pub occupancy_bps: i64,
    /// Annual rent indexation.
    pub rent_cpi_bps: i64,
    /// Annual property value growth.
    pub appreciation_bps: i64,
    /// Letting agent fee, as a share of collected rent.
    pub mgmt_fee_bps: i64,
    /// Maintenance / capex reserve, as a share of collected rent.
    pub maintenance_bps: i64,
    /// Local property tax, insurance and building fees, per property per year.
    pub fixed_costs_annual: Money,
    /// Effective tax on taxable rental profit (rent − deductible costs − interest).
    pub rental_tax_bps: i64,

    /// The plan itself. Order does not matter; purchases happen in order of
    /// their planned month.
    #[serde(default)]
    pub properties: Vec<PlannedProperty>,
}

impl Default for SimParams {
    /// Mid-case numbers and an empty plan: what to buy is the person's to
    /// decide, so nothing is decided for them.
    fn default() -> Self {
        SimParams {
            horizon_years: 10,
            start_month: None,
            start_cash: Money::from_major(60_000.0),
            monthly_savings: Money::from_major(1_500.0),
            net_monthly_salary: Money::from_major(3_200.0),
            income_growth_bps: 200,
            existing_monthly_debt: Money::ZERO,
            cash_buffer: Money::from_major(6_000.0),
            reinvest_net_rent: true,

            // Round, unremarkable numbers. What a mortgage, a closing cost or
            // a lender's limit looks like depends on the country, so a region
            // file (`regions/<id>.json`) supplies the real ones when a new
            // plan is made; these only stand in until it has.
            financing_bps: 8_000,       // 80% of the price
            mortgage_rate_bps: 350,     // 3.50%
            mortgage_term_months: 300,  // 25 years
            other_closing_bps: 150,     // 1.5% in fees
            dti_max_bps: 3_500,         // 35% of income
            computable_rent_bps: 7_000, // lenders count 70% of rent

            occupancy_bps: 9_000,  // 90%
            rent_cpi_bps: 200,     // 2%
            appreciation_bps: 200, // 2%
            mgmt_fee_bps: 800,     // 8% of collected rent
            maintenance_bps: 500,  // 5% capex reserve
            fixed_costs_annual: Money::from_major(1_000.0),
            rental_tax_bps: 2_000, // tax on rental profit

            properties: Vec::new(),
        }
    }
}

/// Validation failures that would otherwise produce silently nonsensical output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParamError {
    pub field: &'static str,
    pub message: String,
}

impl SimParams {
    pub fn validate(&self) -> Result<(), Vec<ParamError>> {
        let mut errs = Vec::new();
        let mut bad =
            |field: &'static str, message: String| errs.push(ParamError { field, message });

        if !(1..=50).contains(&self.horizon_years) {
            bad("horizonYears", "must be between 1 and 50".into());
        }
        if self.mortgage_term_months == 0 || self.mortgage_term_months > 600 {
            bad("mortgageTermMonths", "must be between 1 and 600".into());
        }
        if !(0..=10_000).contains(&self.financing_bps) {
            bad(
                "financingBps",
                "loan-to-value must be between 0% and 100%".into(),
            );
        }
        if !(0..=10_000).contains(&self.occupancy_bps) {
            bad(
                "occupancyBps",
                "occupancy must be between 0% and 100%".into(),
            );
        }
        if !(0..=10_000).contains(&self.computable_rent_bps) {
            bad("computableRentBps", "must be between 0% and 100%".into());
        }
        if self.mortgage_rate_bps < 0 || self.mortgage_rate_bps > 5_000 {
            bad("mortgageRateBps", "rate must be between 0% and 50%".into());
        }
        if self.dti_max_bps <= 0 || self.dti_max_bps > 10_000 {
            bad("dtiMaxBps", "must be between 0% and 100%".into());
        }
        if !(-2_000..=5_000).contains(&self.income_growth_bps) {
            bad(
                "incomeGrowthBps",
                "must be between −20% and 50% a year".into(),
            );
        }
        if self.monthly_savings.is_negative() {
            bad("monthlySavings", "cannot be negative".into());
        }
        if self.cash_buffer.is_negative() {
            bad("cashBuffer", "cannot be negative".into());
        }
        if self.properties.len() > 60 {
            bad("properties", "a plan holds at most 60 purchases".into());
        }
        for p in &self.properties {
            if p.price.is_negative() || p.price.is_zero() {
                bad("propertyPrice", "must be greater than zero".into());
            }
            if p.renovation.is_negative() {
                bad("renovation", "cannot be negative".into());
            }
            if p.monthly_rent.is_negative() {
                bad("monthlyRent", "cannot be negative".into());
            }
            if p.buy_month == 0 {
                bad("buyMonth", "a purchase is planned from month 1 on".into());
            }
            if !(0..=3_000).contains(&p.transfer_tax_bps) {
                bad("transferTaxBps", "must be between 0% and 30%".into());
            }
            if p.financing_bps.is_some_and(|f| !(0..=10_000).contains(&f)) {
                bad(
                    "financingBps",
                    "loan-to-value must be between 0% and 100%".into(),
                );
            }
            if p.mortgage_rate_bps
                .is_some_and(|r| !(0..=5_000).contains(&r))
            {
                bad("mortgageRateBps", "rate must be between 0% and 50%".into());
            }
            if p.mortgage_term_months.is_some_and(|t| t == 0 || t > 600) {
                bad("mortgageTermMonths", "must be between 1 and 600".into());
            }
        }
        // One message per field is enough; ten flats with the same mistake
        // should not say it ten times.
        errs.dedup_by(|a, b| a.field == b.field);
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }

    fn financing(&self, p: &PlannedProperty) -> i64 {
        p.financing_bps.unwrap_or(self.financing_bps)
    }

    /// The loan a purchase is made with.
    pub fn loan_for(&self, p: &PlannedProperty) -> Loan {
        Loan::new(
            p.price.mul_bps(self.financing(p)),
            p.mortgage_rate_bps.unwrap_or(self.mortgage_rate_bps),
            p.mortgage_term_months.unwrap_or(self.mortgage_term_months),
        )
    }

    /// Cash needed at the notary for one purchase:
    /// deposit + transfer tax + other closing costs + renovation.
    pub fn cash_required(&self, p: &PlannedProperty) -> Money {
        let deposit = p.price.mul_bps(10_000 - self.financing(p));
        let itp = p.price.mul_bps(p.transfer_tax_bps);
        let other = p.price.mul_bps(self.other_closing_bps);
        deposit + itp + other + p.renovation
    }
}

/// One owned unit.
///
/// `base_*` are fixed at purchase and current figures are read off a cumulative
/// index, so a unit bought in year 4 is only ever grown by years 4 onward.
#[derive(Debug, Clone)]
struct Holding {
    /// Position in `SimParams::properties`.
    plan: usize,
    purchased_month: u32,
    base_value: Money,
    base_monthly_rent: Money,
    fixed_costs_annual: Money,
    loan: Loan,
}

impl Holding {
    fn value(&self, month: u32, idx: &[f64]) -> Money {
        self.base_value
            .mul_f64(idx[month as usize] / idx[self.purchased_month as usize])
    }
    fn rent(&self, month: u32, idx: &[f64]) -> Money {
        self.base_monthly_rent
            .mul_f64(idx[month as usize] / idx[self.purchased_month as usize])
    }
}

/// A single projected year.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct YearRow {
    pub year: u32,
    /// Put aside from pay this year, rent not included.
    pub savings_contributed: Money,
    /// Rent kept this year, after every cost, the mortgage and tax.
    pub net_rent_received: Money,
    pub purchases: u32,
    pub properties: u32,
    pub cash: Money,
    pub gross_rent_monthly: Money,
    pub net_rent_monthly: Money,
    pub portfolio_value: Money,
    pub debt: Money,
    pub equity: Money,
    /// What is yours in each planned property at the end of this year — its
    /// value less what is still owed on it — in the order of
    /// `SimParams::properties`. Zero for one not yet bought, or never bought.
    /// These add up to `equity`.
    pub property_equity: Vec<Money>,
    pub net_worth: Money,
    /// Take-home pay a month this year.
    pub salary_monthly: Money,
    /// Debt service ÷ computable income, in basis points.
    pub dti_bps: i64,
    pub dti_ok: bool,
}

/// What became of one planned purchase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PropertyOutcome {
    /// Position in `SimParams::properties`.
    pub index: usize,
    pub planned_month: u32,
    /// `None` when it could not be bought within the horizon.
    pub bought_month: Option<u32>,
    /// "cash" or "bank": what stood in the way in the planned month, whether
    /// it only delayed the purchase or prevented it.
    pub blocked_by: Option<String>,
    /// Deposit, taxes, fees and renovation: the cash the purchase takes.
    pub cash_needed: Money,
    /// How far short the available cash was in the planned month.
    pub cash_short: Money,
    pub loan: Money,
    pub monthly_payment: Money,
    /// Debt service ÷ computable income with this mortgage added, as the bank
    /// would see it in the planned month.
    pub dti_bps_planned: i64,
    /// Rent kept from this property in its first month, after every cost,
    /// the mortgage and tax.
    pub net_rent_first: Money,
    /// At the end of the projection. Zero when never bought.
    pub value_end: Money,
    pub debt_end: Money,
    pub net_rent_end: Money,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimResult {
    pub years: Vec<YearRow>,
    /// Net worth at the end of every month, for the chart.
    pub monthly_net_worth: Vec<Money>,
    /// One per planned property, in the order they were given.
    pub properties: Vec<PropertyOutcome>,
    pub final_net_worth: Money,
    pub final_properties: u32,
    pub final_net_rent_monthly: Money,
    /// What can be put aside a month at the end: from pay, plus rent kept.
    pub final_saving_capacity: Money,
}

fn year_of(month: u32) -> u32 {
    (month - 1) / 12 + 1
}

/// Pay rises once a year, on the anniversary — not a little every month.
fn income_factor(params: &SimParams, month: u32) -> f64 {
    (1.0 + params.income_growth_bps as f64 / 10_000.0).powi(year_of(month) as i32 - 1)
}

/// Cumulative monthly growth factor for an annual rate.
fn index(annual_bps: i64, total_months: u32) -> Vec<f64> {
    let step = (1.0 + annual_bps as f64 / 10_000.0).powf(1.0 / 12.0);
    let mut out = vec![1.0f64; total_months as usize + 1];
    for m in 1..=total_months as usize {
        out[m] = out[m - 1] * step;
    }
    out
}

/// One month of one holding's income. `advance` moves its loan on a month;
/// without it the figures are read off without touching anything, which is
/// what reporting uses.
fn month_of(
    params: &SimParams,
    h: &mut Holding,
    month: u32,
    rent_idx: &[f64],
    advance: bool,
) -> (Money, Money) {
    let rent = h.rent(month, rent_idx);
    let got = rent.mul_bps(params.occupancy_bps);
    let (interest, payment) = if advance {
        let period = h.loan.advance_one_month();
        (period.interest, period.payment)
    } else if h.loan.is_settled() {
        (Money::ZERO, Money::ZERO)
    } else {
        (
            h.loan.balance.mul_f64(h.loan.monthly_rate),
            h.loan.monthly_payment,
        )
    };
    let opex = got.mul_bps(params.mgmt_fee_bps + params.maintenance_bps)
        + h.fixed_costs_annual.mul_f64(1.0 / 12.0);
    let noi = got - opex;
    // Mortgage interest is deductible; principal repayment is not.
    let tax = (noi - interest)
        .max(Money::ZERO)
        .mul_bps(params.rental_tax_bps);
    (rent, noi - payment - tax)
}

/// Debt service ÷ computable income. Lenders count only a haircut share of
/// rental income (`computable_rent_bps`) toward capacity.
fn dti(params: &SimParams, salary: Money, service: Money, gross_rent: Money) -> (i64, bool) {
    let income = salary + gross_rent.mul_bps(params.computable_rent_bps);
    let service = params.existing_monthly_debt + service;
    if income.minor() <= 0 {
        return (
            if service.is_zero() { 0 } else { i64::MAX },
            service.is_zero(),
        );
    }
    let ratio_bps = (service.minor() as i128 * 10_000 / income.minor() as i128) as i64;
    (ratio_bps, ratio_bps <= params.dti_max_bps)
}

fn service_of(holdings: &[Holding]) -> Money {
    holdings
        .iter()
        .filter(|h| !h.loan.is_settled())
        .map(|h| h.loan.monthly_payment)
        .sum()
}

/// Run the projection. Pure: no I/O, no clock, no randomness — the same params
/// always produce the same result, which is what makes it testable and what
/// lets the UI call it on every edit.
pub fn run(params: &SimParams) -> Result<SimResult, Vec<ParamError>> {
    params.validate()?;

    let total_months = params.horizon_years * 12;
    let value_idx = index(params.appreciation_bps, total_months);
    let rent_idx = index(params.rent_cpi_bps, total_months);

    // Purchases are attempted in the order they were planned for.
    let mut queue: Vec<usize> = (0..params.properties.len()).collect();
    queue.sort_by_key(|&i| (params.properties[i].buy_month, i));
    let mut next = 0;

    let mut outcomes: Vec<PropertyOutcome> = params
        .properties
        .iter()
        .enumerate()
        .map(|(index, p)| {
            let loan = params.loan_for(p);
            PropertyOutcome {
                index,
                planned_month: p.buy_month,
                bought_month: None,
                blocked_by: None,
                cash_needed: params.cash_required(p),
                cash_short: Money::ZERO,
                loan: loan.balance,
                monthly_payment: loan.monthly_payment,
                dti_bps_planned: 0,
                net_rent_first: Money::ZERO,
                value_end: Money::ZERO,
                debt_end: Money::ZERO,
                net_rent_end: Money::ZERO,
            }
        })
        .collect();

    let mut cash = params.start_cash;
    let mut holdings: Vec<Holding> = Vec::new();
    let mut years = Vec::with_capacity(params.horizon_years as usize);
    let mut monthly_net_worth = Vec::with_capacity(total_months as usize);
    let (mut year_savings, mut year_rent, mut year_purchases) = (Money::ZERO, Money::ZERO, 0u32);

    for month in 1..=total_months {
        let factor = income_factor(params, month);
        let salary = params.net_monthly_salary.mul_f64(factor);

        // 1. What is put aside from pay arrives.
        let saving = params.monthly_savings.mul_f64(factor);
        cash += saving;
        year_savings += saving;

        // 2. Service the debt and collect the rent.
        let (mut gross_rent, mut net_rent) = (Money::ZERO, Money::ZERO);
        for h in holdings.iter_mut() {
            let (gross, net) = month_of(params, h, month, &rent_idx, true);
            gross_rent += gross;
            net_rent += net;
        }
        if params.reinvest_net_rent {
            cash += net_rent;
        }
        year_rent += net_rent;

        // 3. Make the purchases that are due, in order, for as long as each
        //    one passes. The first that fails holds the ones behind it: a
        //    plan is a sequence, and buying the third flat before the second
        //    is not the plan the person drew.
        while let Some(&i) = queue.get(next) {
            let plan = &params.properties[i];
            if plan.buy_month > month {
                break;
            }
            let need = outcomes[i].cash_needed;
            let loan = params.loan_for(plan);
            let (after_bps, bank_ok) = dti(
                params,
                salary,
                service_of(&holdings) + loan.monthly_payment,
                // The new unit's rent counts toward capacity, as a lender
                // would assess it.
                gross_rent + plan.monthly_rent,
            );
            let short = (need - (cash - params.cash_buffer)).max(Money::ZERO);
            if month == plan.buy_month {
                outcomes[i].dti_bps_planned = after_bps;
                outcomes[i].cash_short = short;
                outcomes[i].blocked_by = if !short.is_zero() {
                    Some("cash".into())
                } else if !bank_ok {
                    Some("bank".into())
                } else {
                    None
                };
            }
            if !short.is_zero() || !bank_ok {
                break;
            }
            cash -= need;
            let mut holding = Holding {
                plan: i,
                purchased_month: month,
                base_value: plan.price + plan.renovation,
                base_monthly_rent: plan.monthly_rent,
                fixed_costs_annual: plan.fixed_costs_annual.unwrap_or(params.fixed_costs_annual),
                loan,
            };
            outcomes[i].bought_month = Some(month);
            outcomes[i].net_rent_first = month_of(params, &mut holding, month, &rent_idx, false).1;
            holdings.push(holding);
            year_purchases += 1;
            next += 1;
        }

        let value: Money = holdings.iter().map(|h| h.value(month, &value_idx)).sum();
        let debt: Money = holdings.iter().map(|h| h.loan.balance).sum();
        monthly_net_worth.push(cash + value - debt);

        // 4. Close the year.
        if month % 12 == 0 {
            let (mut gross_end, mut net_end) = (Money::ZERO, Money::ZERO);
            for h in holdings.iter_mut() {
                let (gross, net) = month_of(params, h, month, &rent_idx, false);
                gross_end += gross;
                net_end += net;
            }
            let (dti_bps, dti_ok) = dti(params, salary, service_of(&holdings), gross_end);
            let mut property_equity = vec![Money::ZERO; params.properties.len()];
            for h in &holdings {
                property_equity[h.plan] = h.value(month, &value_idx) - h.loan.balance;
            }
            years.push(YearRow {
                year: year_of(month),
                savings_contributed: year_savings,
                net_rent_received: year_rent,
                purchases: year_purchases,
                properties: holdings.len() as u32,
                cash,
                gross_rent_monthly: gross_end,
                net_rent_monthly: net_end,
                portfolio_value: value,
                debt,
                equity: value - debt,
                property_equity,
                net_worth: cash + value - debt,
                salary_monthly: salary,
                dti_bps,
                dti_ok,
            });
            (year_savings, year_rent, year_purchases) = (Money::ZERO, Money::ZERO, 0);
        }
    }

    for h in holdings.iter_mut() {
        let o = &mut outcomes[h.plan];
        o.value_end = h.value(total_months, &value_idx);
        o.debt_end = h.loan.balance;
        o.net_rent_end = month_of(params, h, total_months, &rent_idx, false).1;
    }

    let last = years.last();
    let final_rent = last.map(|y| y.net_rent_monthly).unwrap_or(Money::ZERO);
    Ok(SimResult {
        final_net_worth: last.map(|y| y.net_worth).unwrap_or(cash),
        final_properties: holdings.len() as u32,
        final_net_rent_monthly: final_rent,
        final_saving_capacity: params
            .monthly_savings
            .mul_f64(income_factor(params, total_months.max(1)))
            + final_rent,
        properties: outcomes,
        years,
        monthly_net_worth,
    })
}

#[cfg(test)]
mod tests;
