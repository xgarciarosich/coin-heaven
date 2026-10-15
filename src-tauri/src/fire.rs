//! FIRE calculation — two metrics, never blended.
//!
//! The original spec computed real-estate equity and then also subtracted the
//! rent that equity produces from the spending target. Counting the same asset
//! twice — once as capital, once as the income it throws off — overstates
//! progress, potentially by years.
//!
//! The fix is to report two separate numbers and never add them together:
//!
//!   1. **Liquid FIRE target** — `(annual_spend − net_rental_income) / SWR`,
//!      compared strictly against liquid assets (cash, equities, pensions).
//!      Rental income covers part of the spending, so the portfolio only has to
//!      cover the remainder. Real-estate equity is *excluded* from the assets
//!      side, because its contribution is already counted as that income.
//!
//!   2. **Total wealth milestone** — net worth including real-estate equity.
//!      Context, not a finish line.
//!
//! ## The SWR is real, not nominal
//!
//! A 4% safe withdrawal rate is conventionally an inflation-adjusted rate. Two
//! consequences, both enforced by this module's contract:
//!
//!   * `annual_spend_target` is in **today's** money;
//!   * `annual_net_rental_income` must be **today's** rent, not a projected
//!     future figure. Feeding in a CPI-inflated rent from year 10 of the
//!     simulator while comparing against a real SWR silently shrinks the
//!     target. `fire::from_simulation` exists to deflate it correctly.

use crate::money::Money;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FireParams {
    /// Annual spending target, in today's money.
    pub annual_spend_target: Money,
    /// Safe withdrawal rate in basis points. 400 = 4%. Treated as a *real* rate.
    pub swr_bps: i64,
    /// Net rental income after costs, debt service and tax — in today's money.
    pub annual_net_rental_income: Money,
}

impl Default for FireParams {
    fn default() -> Self {
        FireParams {
            annual_spend_target: Money::from_minor(3_600_000), // 36,000.00
            swr_bps: 400,
            annual_net_rental_income: Money::ZERO,
        }
    }
}

/// Balances taken from a net-worth snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Balances {
    /// Cash + equities + pensions. Not real estate.
    pub liquid: Money,
    /// Market value minus outstanding mortgage principal.
    pub real_estate_equity: Money,
    pub other_assets: Money,
    pub other_debt: Money,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FireResult {
    // --- Metric 1: liquid FIRE -------------------------------------------
    /// Portfolio needed to cover spending the rent does not already cover.
    pub liquid_target: Money,
    pub liquid_assets: Money,
    pub liquid_shortfall: Money,
    /// Progress toward the liquid target, in basis points. 10000 = 100%.
    pub liquid_progress_bps: i64,
    pub liquid_covered: bool,

    // --- Metric 2: total wealth ------------------------------------------
    pub total_net_worth: Money,

    // --- Context ----------------------------------------------------------
    /// Share of the spending target already met by rent alone, in bps.
    pub rent_coverage_bps: i64,
    /// Rent alone meets or exceeds the spending target.
    pub covered_by_rent_alone: bool,
    /// Spending the portfolio must still fund each year.
    pub residual_annual_spend: Money,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FireError {
    pub field: &'static str,
    pub message: String,
}

pub fn calculate(params: &FireParams, balances: &Balances) -> Result<FireResult, Vec<FireError>> {
    let mut errs = Vec::new();
    if params.swr_bps <= 0 || params.swr_bps > 2_000 {
        errs.push(FireError {
            field: "swrBps",
            message: "safe withdrawal rate must be between 0% and 20%".into(),
        });
    }
    if params.annual_spend_target.is_negative() {
        errs.push(FireError {
            field: "annualSpendTarget",
            message: "spending target cannot be negative".into(),
        });
    }
    if !errs.is_empty() {
        return Err(errs);
    }

    let spend = params.annual_spend_target;
    let rent = params.annual_net_rental_income;

    // Rent covers part of the bill; the portfolio funds what is left.
    let residual = (spend - rent).max(Money::ZERO);
    let covered_by_rent_alone = rent >= spend && !spend.is_zero();

    // target = residual / SWR, i.e. residual * 10000 / swr_bps in integer maths.
    let liquid_target =
        Money::from_minor(((residual.minor() as i128 * 10_000) / params.swr_bps as i128) as i64);

    let liquid_assets = balances.liquid;
    let liquid_shortfall = (liquid_target - liquid_assets).max(Money::ZERO);
    let liquid_progress_bps = if liquid_target.minor() <= 0 {
        10_000 // nothing left to fund
    } else {
        ((liquid_assets.minor().max(0) as i128 * 10_000) / liquid_target.minor() as i128) as i64
    };

    let rent_coverage_bps = if spend.minor() <= 0 {
        10_000
    } else {
        ((rent.minor().max(0) as i128 * 10_000) / spend.minor() as i128) as i64
    };

    Ok(FireResult {
        liquid_target,
        liquid_assets,
        liquid_shortfall,
        liquid_progress_bps,
        liquid_covered: liquid_assets >= liquid_target,
        // Deliberately a separate figure. It is never compared to liquid_target.
        total_net_worth: balances.liquid + balances.real_estate_equity + balances.other_assets
            - balances.other_debt,
        rent_coverage_bps,
        covered_by_rent_alone,
        residual_annual_spend: residual,
    })
}

/// Deflate a nominal future rent back to today's money before it meets a real
/// SWR. `years_ahead` is how far into the projection the figure came from.
pub fn to_todays_money(nominal: Money, inflation_bps: i64, years_ahead: f64) -> Money {
    if years_ahead <= 0.0 {
        return nominal;
    }
    let factor = (1.0 + inflation_bps as f64 / 10_000.0).powf(years_ahead);
    nominal.mul_f64(1.0 / factor)
}

/* ------------------------------------------------------------ the path -- */

/// How the portfolio is expected to get from here to the target.
///
/// Everything here is in **today's money at a real rate of return**, for the
/// same reason the SWR is real: mixing a nominal return with a real withdrawal
/// rate quietly shortens the answer by years. `real_return_bps` is therefore
/// the return *above inflation* — roughly 5% for a global equity portfolio on
/// long-run history, not the 8% a nominal chart shows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanParams {
    /// Added to the portfolio each year, in today's money.
    pub annual_savings: Money,
    /// Real return on liquid assets, in basis points. 500 = 5% above inflation.
    pub real_return_bps: i64,
    pub current_age: Option<u32>,
    /// The age the coast number is measured to. Without it there is no
    /// "by when", and a coast number has no meaning.
    pub target_age: Option<u32>,
    pub horizon_years: u32,
    /// Gross up withdrawals for the tax on investment gains, so the target is
    /// what funds the *spending*, not what funds spending plus Hacienda.
    #[serde(default = "yes")]
    pub withdrawal_tax: bool,
    /// Share of each withdrawal that is gain rather than returned cost — the
    /// only part that is taxed. Half is a fair long-run guess for an index
    /// portfolio; the portfolio module can supply the real figure.
    #[serde(default = "half")]
    pub gain_share_bps: i64,
    /// Age the state pension starts, and what it pays a year in today's money.
    /// With both set the target falls in steps: the portfolio only has to
    /// carry the full bill until the pension arrives.
    #[serde(default)]
    pub pension_age: Option<u32>,
    #[serde(default)]
    pub annual_pension: Money,
    /// Width of the pessimistic/optimistic band, in basis points of real
    /// return either side of the base case.
    #[serde(default = "two_pct")]
    pub band_bps: i64,
    /// The tax table withdrawals are grossed up with. See [`default_brackets`].
    #[serde(default = "default_brackets")]
    pub tax_brackets: Vec<TaxBracket>,
}

fn yes() -> bool {
    true
}
fn half() -> i64 {
    5_000
}
fn two_pct() -> i64 {
    200
}

impl Default for PlanParams {
    fn default() -> Self {
        PlanParams {
            annual_savings: Money::from_minor(1_200_000), // 12,000.00
            real_return_bps: 500,
            current_age: None,
            target_age: Some(60),
            horizon_years: 40,
            withdrawal_tax: true,
            gain_share_bps: 5_000,
            pension_age: None,
            annual_pension: Money::ZERO,
            band_bps: 200,
            tax_brackets: default_brackets(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanPoint {
    /// 0 is today.
    pub year: u32,
    pub age: Option<u32>,
    pub liquid: Money,
    /// The pessimistic and optimistic paths either side of `liquid`.
    pub liquid_low: Money,
    pub liquid_high: Money,
    /// What the portfolio would have to be to stop *in this year*. Flat when
    /// there is no pension in the picture — both sides are in today's money,
    /// so a rising line would be inflation counted twice — and stepping down
    /// as the pension age approaches when there is one.
    pub target: Money,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sensitivity {
    pub swr_bps: i64,
    pub target: Money,
    pub years: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    /// 2500, 5000, 7500 or 10000.
    pub share_bps: i64,
    pub years: Option<f64>,
    pub age: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lever {
    /// "save" — invest this much more a month; "spend" — spend this much less.
    pub kind: String,
    pub monthly: Money,
    /// Months earlier than the base case. `None` when either path never
    /// arrives, because "infinitely earlier" is not a number.
    pub months_earlier: Option<i64>,
}

/// "I would like to stop at this age" — and whether the numbers agree.
///
/// The date is an output of the plan, so an age someone types cannot move
/// it. What it can do is be compared with it: on course or not, by how much,
/// and what change in the two things a person controls would close the gap.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub age: u32,
    pub years_available: u32,
    pub on_track: bool,
    /// How many years before the wished-for age the plan arrives.
    pub spare_years: Option<f64>,
    /// What the portfolio would be at that age, and what it would have to be.
    pub liquid_at_goal: Money,
    pub target_at_goal: Money,
    /// Extra invested each month that would arrive on time, to the nearest
    /// ten euros. `None` when on track, or when no plausible amount does it.
    pub extra_saving_monthly: Option<Money>,
    /// Or: spending this much less each month, the saving invested.
    pub spend_cut_monthly: Option<Money>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub path: Vec<PlanPoint>,
    /// Approximate: whole years are simulated and the crossing is interpolated
    /// linearly inside the year it happens in.
    pub years_to_target: Option<f64>,
    pub years_low: Option<f64>,
    pub years_high: Option<f64>,
    pub age_at_target: Option<u32>,
    /// The portfolio needed to stop today, with tax and pension applied.
    pub target_now: Money,
    /// What has to be withdrawn a year before tax to leave the spending
    /// target in hand, and the tax inside it.
    pub gross_withdrawal: Money,
    pub withdrawal_tax: Money,
    /// Capital which, left alone at the real return and with nothing further
    /// added, reaches the target by `target_age`.
    pub coast_number: Option<Money>,
    pub coasting: bool,
    pub goal: Option<Goal>,
    pub sensitivity: Vec<Sensitivity>,
    pub milestones: Vec<Milestone>,
    pub levers: Vec<Lever>,
}

/* ----------------------------------------------------------- tax -------- */

/// One band of the tax on savings income: everything up to `up_to` a year
/// (or without limit, for the last band) is taxed at `rate_bps`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaxBracket {
    pub up_to: Option<Money>,
    pub rate_bps: i64,
}

/// The tax table a plan starts with before a country has been chosen: one
/// band, no tax.
///
/// Tax law is a fact about a country and changes more often than software
/// does, so it does not live here. A region file (`regions/<id>.json`)
/// carries each country's bands; picking a country copies them into the plan,
/// and the plan keeps its own from then on. "No tax" is the honest neutral —
/// a made-up rate would be wrong for almost everyone and look right.
pub fn default_brackets() -> Vec<TaxBracket> {
    vec![TaxBracket {
        up_to: None,
        rate_bps: 0,
    }]
}

/// Tax on a year's realised gains.
pub fn savings_tax(gain: Money, brackets: &[TaxBracket]) -> Money {
    let mut left = gain.minor().max(0);
    let mut floor = 0i64;
    let mut tax = Money::ZERO;
    for b in brackets {
        let cap = b.up_to.map_or(i64::MAX, |m| m.minor());
        let slice = left.min(cap.saturating_sub(floor));
        if slice <= 0 {
            // A band at or below the one before it is empty, not an error.
            continue;
        }
        tax += Money::from_minor(slice).mul_bps(b.rate_bps);
        left -= slice;
        floor = cap;
        if left == 0 {
            break;
        }
    }
    // Anything above the last band someone left closed is taxed at its rate:
    // gains do not become free past the top of the table.
    if left > 0 {
        if let Some(last) = brackets.last() {
            tax += Money::from_minor(left).mul_bps(last.rate_bps);
        }
    }
    tax
}

/// The gross withdrawal that leaves `net` after tax on its gain share.
///
/// Solved by bisection because the brackets make the relationship piecewise;
/// net-of-tax is monotonic in gross, so the search cannot go wrong.
pub fn gross_for_net(net: Money, gain_share_bps: i64, brackets: &[TaxBracket]) -> Money {
    if net.minor() <= 0 {
        return Money::ZERO;
    }
    let after_tax = |gross: i64| {
        gross - savings_tax(Money::from_minor(gross).mul_bps(gain_share_bps), brackets).minor()
    };
    let (mut lo, mut hi) = (net.minor(), net.minor() * 2);
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if after_tax(mid) >= net.minor() {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Money::from_minor(hi)
}

/* --------------------------------------------------------- requirement -- */

/// Everything that decides how big the portfolio has to be, resolved once.
struct Need {
    swr_bps: i64,
    real_return_bps: i64,
    /// Gross withdrawal before and after the pension starts.
    before: Money,
    after: Money,
    /// Age now and pension age, when both are known and a pension is set.
    bridge: Option<(u32, u32)>,
}

impl Need {
    fn new(params: &FireParams, plan: &PlanParams) -> Need {
        let residual =
            (params.annual_spend_target - params.annual_net_rental_income).max(Money::ZERO);
        let gross = |net: Money| {
            if plan.withdrawal_tax {
                gross_for_net(net, plan.gain_share_bps, &plan.tax_brackets)
            } else {
                net
            }
        };
        let after_pension = (residual - plan.annual_pension).max(Money::ZERO);
        Need {
            swr_bps: params.swr_bps,
            real_return_bps: plan.real_return_bps,
            before: gross(residual),
            after: gross(after_pension),
            bridge: match (plan.current_age, plan.pension_age) {
                (Some(now), Some(p)) if !plan.annual_pension.is_zero() => Some((now, p)),
                _ => None,
            },
        }
    }

    fn perpetual(&self, withdrawal: Money) -> Money {
        Money::from_minor(
            ((withdrawal.minor() as i128 * 10_000) / self.swr_bps.max(1) as i128) as i64,
        )
    }

    /// Portfolio needed to stop at the start of year `t`.
    ///
    /// Without a pension it is the classic withdrawal ÷ SWR. With one, the
    /// years until the pension are a finite drawdown — the present value of
    /// the full withdrawal for those years — and only what remains afterwards
    /// has to last indefinitely, discounted back to the day you stop.
    fn at(&self, t: u32) -> Money {
        let Some((now, pension_age)) = self.bridge else {
            return self.perpetual(self.before);
        };
        let age = now + t;
        if age >= pension_age {
            return self.perpetual(self.after);
        }
        let n = (pension_age - age) as f64;
        let r = self.real_return_bps as f64 / 10_000.0;
        let annuity = if r.abs() < 1e-9 {
            n
        } else {
            (1.0 - (1.0 + r).powf(-n)) / r
        };
        let discount = (1.0 + r).powf(n);
        self.before.mul_f64(annuity) + self.perpetual(self.after).mul_f64(1.0 / discount)
    }
}

/* ------------------------------------------------------------- the path -- */

/// One year of growth, contributions made at the end of the year.
///
/// End-of-year is the conservative reading — money saved in December earned
/// nothing that year — and it is what "I save 12k a year" usually means.
fn grow(liquid: Money, real_return_bps: i64, savings: Money) -> Money {
    liquid + liquid.mul_bps(real_return_bps) + savings
}

/// Years until `liquid` first reaches `share` of the requirement, or `None`
/// within the horizon. Fractional, by linear interpolation of the gap inside
/// the crossing year — the requirement can move too, so it is the gap that is
/// interpolated, not the portfolio.
fn years_until(
    start: Money,
    need: &Need,
    share_bps: i64,
    real_return_bps: i64,
    savings: Money,
    horizon: u32,
) -> Option<f64> {
    let gap = |t: u32, liquid: Money| (liquid - need.at(t).mul_bps(share_bps)).minor() as f64;
    let mut liquid = start;
    let mut g0 = gap(0, liquid);
    if g0 >= 0.0 {
        return Some(0.0);
    }
    for year in 0..horizon {
        let next = grow(liquid, real_return_bps, savings);
        let g1 = gap(year + 1, next);
        if g1 >= 0.0 {
            let frac = if g1 - g0 > 0.0 { -g0 / (g1 - g0) } else { 1.0 };
            return Some(year as f64 + frac);
        }
        // Neither growing nor being helped along by a falling requirement:
        // forty identical years would only confirm it never arrives.
        if g1 <= g0 {
            return None;
        }
        liquid = next;
        g0 = g1;
    }
    None
}

/// Project the portfolio forward and answer "when?".
pub fn plan(
    params: &FireParams,
    balances: &Balances,
    plan: &PlanParams,
) -> Result<Plan, Vec<FireError>> {
    calculate(params, balances)?;

    let mut errs = Vec::new();
    if plan.annual_savings.is_negative() {
        errs.push(FireError {
            field: "annualSavings",
            message: "annual savings cannot be negative — spending more than you earn \
                      is a different question than this page answers"
                .into(),
        });
    }
    if !(-1_000..=2_000).contains(&plan.real_return_bps) {
        errs.push(FireError {
            field: "realReturnBps",
            message: "a real return outside −10% to 20% is not a plan".into(),
        });
    }
    if !(1..=60).contains(&plan.horizon_years) {
        errs.push(FireError {
            field: "horizonYears",
            message: "project between 1 and 60 years".into(),
        });
    }
    if !(0..=10_000).contains(&plan.gain_share_bps) {
        errs.push(FireError {
            field: "gainShareBps",
            message: "the gain share of a withdrawal is between 0% and 100%".into(),
        });
    }
    if plan.annual_pension.is_negative() {
        errs.push(FireError {
            field: "annualPension",
            message: "a pension cannot be negative".into(),
        });
    }
    if !(0..=1_000).contains(&plan.band_bps) {
        errs.push(FireError {
            field: "bandBps",
            message: "the band is between 0% and 10% either side".into(),
        });
    }
    if plan.tax_brackets.len() > 12
        || plan.tax_brackets.iter().any(|b| {
            !(0..=10_000).contains(&b.rate_bps) || b.up_to.is_some_and(|m| m.is_negative())
        })
    {
        errs.push(FireError {
            field: "taxBrackets",
            message: "a tax band has a rate between 0% and 100% and a limit that is not negative"
                .into(),
        });
    }
    if !errs.is_empty() {
        return Err(errs);
    }

    let need = Need::new(params, plan);
    let r = plan.real_return_bps;
    let lo_r = r - plan.band_bps;
    let hi_r = r + plan.band_bps;

    let mut path = Vec::with_capacity(plan.horizon_years as usize + 1);
    let (mut mid, mut low, mut high) = (balances.liquid, balances.liquid, balances.liquid);
    for year in 0..=plan.horizon_years {
        path.push(PlanPoint {
            year,
            age: plan.current_age.map(|a| a + year),
            liquid: mid,
            liquid_low: low,
            liquid_high: high,
            target: need.at(year),
        });
        mid = grow(mid, r, plan.annual_savings);
        low = grow(low, lo_r, plan.annual_savings);
        high = grow(high, hi_r, plan.annual_savings);
    }

    let when = |share: i64, rate: i64, savings: Money, need: &Need| {
        years_until(
            balances.liquid,
            need,
            share,
            rate,
            savings,
            plan.horizon_years,
        )
    };
    let years_to_target = when(10_000, r, plan.annual_savings, &need);
    let age_of = |y: Option<f64>| match (plan.current_age, y) {
        (Some(a), Some(y)) => Some(a + y.ceil() as u32),
        _ => None,
    };

    let target_now = need.at(0);
    let coast_number = match (plan.current_age, plan.target_age) {
        (Some(now), Some(then)) if then > now => {
            let n = then - now;
            let growth = (1.0 + r as f64 / 10_000.0).powf(n as f64);
            // A portfolio that shrinks in real terms never coasts to anything.
            (growth > 0.0).then(|| need.at(n).mul_f64(1.0 / growth))
        }
        _ => None,
    };

    let sensitivity = [300, 325, 350, 400, 450]
        .into_iter()
        .map(|swr_bps| {
            let alt = Need::new(
                &FireParams {
                    swr_bps,
                    ..params.clone()
                },
                plan,
            );
            Sensitivity {
                swr_bps,
                target: alt.at(0),
                years: when(10_000, r, plan.annual_savings, &alt),
            }
        })
        .collect();

    let milestones = [2_500, 5_000, 7_500, 10_000]
        .into_iter()
        .map(|share_bps| {
            let years = when(share_bps, r, plan.annual_savings, &need);
            Milestone {
                share_bps,
                years,
                age: age_of(years),
            }
        })
        .collect();

    // What small, concrete changes buy, in the only unit people feel: time.
    let months = |alt: Option<f64>| match (years_to_target, alt) {
        (Some(base), Some(alt)) => Some(((base - alt) * 12.0).round() as i64),
        _ => None,
    };
    let mut levers = Vec::new();
    for monthly in [10_000i64, 30_000] {
        let m = Money::from_minor(monthly);
        levers.push(Lever {
            kind: "save".into(),
            monthly: m,
            months_earlier: months(when(10_000, r, plan.annual_savings + m.mul_int(12), &need)),
        });
        // Spending less both lowers the target and, money not spent being
        // money invested, raises the saving by the same amount.
        let cheaper = FireParams {
            annual_spend_target: (params.annual_spend_target - m.mul_int(12)).max(Money::ZERO),
            ..params.clone()
        };
        levers.push(Lever {
            kind: "spend".into(),
            monthly: m,
            months_earlier: months(when(
                10_000,
                r,
                plan.annual_savings + m.mul_int(12),
                &Need::new(&cheaper, plan),
            )),
        });
    }

    // The wished-for age, held up against the plan. An age that is not in the
    // future is not an error — it is what the field holds halfway through
    // being typed — it simply has no answer yet.
    let goal = match (plan.current_age, plan.target_age) {
        (Some(now), Some(then)) if then > now && then - now <= 80 => {
            let n = then - now;
            let reach = plan.horizon_years.max(n);
            let arrives = |savings: Money, need: &Need| {
                years_until(balances.liquid, need, 10_000, r, savings, reach)
                    .is_some_and(|y| y <= n as f64)
            };
            let base = years_until(
                balances.liquid,
                &need,
                10_000,
                r,
                plan.annual_savings,
                reach,
            );
            let on_track = base.is_some_and(|y| y <= n as f64);
            // Smallest monthly change, in steps of ten euros, that arrives.
            let smallest = |max_steps: i64, works: &dyn Fn(Money) -> bool| -> Option<Money> {
                let step = |k: i64| Money::from_minor(k * 1_000);
                if max_steps <= 0 || !works(step(max_steps)) {
                    return None;
                }
                let (mut lo, mut hi) = (0, max_steps);
                while hi - lo > 1 {
                    let mid = (lo + hi) / 2;
                    if works(step(mid)) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                Some(step(hi))
            };
            let mut liquid_at_goal = balances.liquid;
            for _ in 0..n {
                liquid_at_goal = grow(liquid_at_goal, r, plan.annual_savings);
            }
            Some(Goal {
                age: then,
                years_available: n,
                on_track,
                spare_years: base.filter(|_| on_track).map(|y| n as f64 - y),
                liquid_at_goal,
                target_at_goal: need.at(n),
                extra_saving_monthly: if on_track {
                    None
                } else {
                    // Up to €20,000 a month: past that it is not a plan.
                    smallest(2_000, &|m| {
                        arrives(plan.annual_savings + m.mul_int(12), &need)
                    })
                },
                spend_cut_monthly: if on_track {
                    None
                } else {
                    smallest(params.annual_spend_target.minor() / 12 / 1_000, &|m| {
                        let cheaper = FireParams {
                            annual_spend_target: (params.annual_spend_target - m.mul_int(12))
                                .max(Money::ZERO),
                            ..params.clone()
                        };
                        arrives(
                            plan.annual_savings + m.mul_int(12),
                            &Need::new(&cheaper, plan),
                        )
                    })
                },
            })
        }
        _ => None,
    };

    Ok(Plan {
        goal,
        age_at_target: age_of(years_to_target),
        coasting: coast_number.is_some_and(|c| balances.liquid >= c),
        coast_number,
        years_low: when(10_000, lo_r, plan.annual_savings, &need),
        years_high: when(10_000, hi_r, plan.annual_savings, &need),
        years_to_target,
        target_now,
        gross_withdrawal: need.before,
        withdrawal_tax: need.before
            - (params.annual_spend_target - params.annual_net_rental_income).max(Money::ZERO),
        path,
        sensitivity,
        milestones,
        levers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(major: f64) -> Money {
        Money::from_major(major)
    }

    fn balances(liquid: f64, re_equity: f64) -> Balances {
        Balances {
            liquid: eur(liquid),
            real_estate_equity: eur(re_equity),
            other_assets: Money::ZERO,
            other_debt: Money::ZERO,
        }
    }

    fn plan_params(savings: f64, real_bps: i64) -> PlanParams {
        PlanParams {
            annual_savings: eur(savings),
            real_return_bps: real_bps,
            current_age: None,
            target_age: None,
            horizon_years: 40,
            withdrawal_tax: false,
            gain_share_bps: 5_000,
            pension_age: None,
            annual_pension: Money::ZERO,
            band_bps: 200,
            tax_brackets: progressive(),
        }
    }

    /// A progressive table to test the arithmetic with: five bands, 19% to
    /// 30%. It is a fixture, not a claim about any country's law.
    fn progressive() -> Vec<TaxBracket> {
        [
            (Some(6_000), 1_900),
            (Some(50_000), 2_100),
            (Some(200_000), 2_300),
            (Some(300_000), 2_700),
            (None, 3_000),
        ]
        .into_iter()
        .map(|(up_to, rate_bps)| TaxBracket {
            up_to: up_to.map(|e: i64| Money::from_minor(e * 100)),
            rate_bps,
        })
        .collect()
    }

    /// 40k spend at 4% is a 1M target.
    fn million() -> FireParams {
        FireParams {
            annual_spend_target: eur(40_000.0),
            swr_bps: 400,
            annual_net_rental_income: Money::ZERO,
        }
    }

    #[test]
    fn already_there_is_zero_years_not_a_projection() {
        let p = plan(
            &million(),
            &balances(1_200_000.0, 0.0),
            &plan_params(0.0, 500),
        )
        .unwrap();
        assert_eq!(p.years_to_target, Some(0.0));
    }

    #[test]
    fn compounding_alone_matches_the_closed_form() {
        // 100k doubling to 200k at 5% real is ln2/ln1.05 = 14.21 years.
        let params = FireParams {
            annual_spend_target: eur(8_000.0), // 8k / 4% = 200k target
            ..million()
        };
        let p = plan(&params, &balances(100_000.0, 0.0), &plan_params(0.0, 500)).unwrap();
        let years = p.years_to_target.unwrap();
        assert!(
            (years - 14.2067).abs() < 0.05,
            "expected about 14.21 years, got {years}"
        );
    }

    #[test]
    fn saving_with_no_return_is_plain_division() {
        let params = FireParams {
            annual_spend_target: eur(4_000.0), // 4k / 4% = 100k target
            ..million()
        };
        let p = plan(&params, &balances(0.0, 0.0), &plan_params(10_000.0, 0)).unwrap();
        assert_eq!(p.years_to_target, Some(10.0));
    }

    #[test]
    fn a_portfolio_that_cannot_grow_never_arrives() {
        let p = plan(&million(), &balances(50_000.0, 0.0), &plan_params(0.0, 0)).unwrap();
        assert_eq!(p.years_to_target, None);
        assert_eq!(p.age_at_target, None);
    }

    #[test]
    fn the_coast_number_is_the_target_discounted_back() {
        let pp = PlanParams {
            current_age: Some(40),
            target_age: Some(60),
            ..plan_params(0.0, 500)
        };
        let p = plan(&million(), &balances(100_000.0, 0.0), &pp).unwrap();
        // 1,000,000 / 1.05^20 = 376,889.48
        let coast = p.coast_number.unwrap();
        assert!(
            (coast.as_major_f64() - 376_889.48).abs() < 1.0,
            "got {coast:?}"
        );
        assert!(!p.coasting, "100k is well short of the coast number");

        let past = plan(&million(), &balances(400_000.0, 0.0), &pp).unwrap();
        assert!(past.coasting, "400k is past it");
    }

    #[test]
    fn a_coast_number_needs_a_by_when() {
        let p = plan(
            &million(),
            &balances(100_000.0, 0.0),
            &plan_params(0.0, 500),
        )
        .unwrap();
        assert_eq!(p.coast_number, None, "no ages given");
        assert!(!p.coasting);
    }

    #[test]
    fn a_lower_withdrawal_rate_is_a_bigger_target_and_a_longer_wait() {
        let p = plan(
            &million(),
            &balances(200_000.0, 0.0),
            &plan_params(20_000.0, 500),
        )
        .unwrap();
        let at = |bps: i64| p.sensitivity.iter().find(|s| s.swr_bps == bps).unwrap();
        assert!(at(300).target > at(450).target);
        assert!(at(300).years.unwrap() > at(450).years.unwrap());
    }

    #[test]
    fn the_target_line_is_flat_because_both_sides_are_in_todays_money() {
        let p = plan(&million(), &balances(0.0, 0.0), &plan_params(10_000.0, 500)).unwrap();
        assert_eq!(p.path.len(), 41, "today plus forty years");
        assert!(p.path.iter().all(|pt| pt.target == eur(1_000_000.0)));
        assert_eq!(p.path[0].liquid, Money::ZERO);
    }

    #[test]
    fn ages_run_alongside_the_years() {
        let pp = PlanParams {
            current_age: Some(35),
            target_age: Some(65),
            horizon_years: 10,
            ..plan_params(30_000.0, 500)
        };
        let p = plan(&million(), &balances(0.0, 0.0), &pp).unwrap();
        assert_eq!(p.path[0].age, Some(35));
        assert_eq!(p.path[10].age, Some(45));
    }

    #[test]
    fn nonsense_inputs_are_named_rather_than_silently_clamped() {
        let bad = PlanParams {
            real_return_bps: 9_000,
            ..plan_params(0.0, 500)
        };
        let e = plan(&million(), &balances(0.0, 0.0), &bad).unwrap_err();
        assert_eq!(e[0].field, "realReturnBps");

        let backwards = PlanParams {
            current_age: Some(60),
            target_age: Some(50),
            ..plan_params(0.0, 500)
        };
        // An age already behind you is a field half typed, not an error.
        let p = plan(&million(), &balances(0.0, 0.0), &backwards).unwrap();
        assert!(p.goal.is_none());
    }

    #[test]
    fn a_wished_for_age_is_checked_against_the_plan() {
        // 1M needed, 200k held, 20k a year at 5%: about 21 years.
        let at = |age: u32| {
            let pp = PlanParams {
                current_age: Some(40),
                target_age: Some(age),
                ..plan_params(20_000.0, 500)
            };
            plan(&million(), &balances(200_000.0, 0.0), &pp)
                .unwrap()
                .goal
                .unwrap()
        };
        let late = at(65);
        assert!(late.on_track);
        assert!(late.spare_years.unwrap() > 3.0);
        assert_eq!(late.extra_saving_monthly, None);

        let early = at(55);
        assert!(!early.on_track);
        assert!(early.liquid_at_goal < early.target_at_goal);
        // The suggested extra saving really does arrive on time, and ten
        // euros less does not.
        let extra = early.extra_saving_monthly.unwrap();
        let with = |m: Money| {
            let pp = PlanParams {
                current_age: Some(40),
                target_age: Some(55),
                annual_savings: eur(20_000.0) + m.mul_int(12),
                ..plan_params(0.0, 500)
            };
            plan(&million(), &balances(200_000.0, 0.0), &pp)
                .unwrap()
                .goal
                .unwrap()
                .on_track
        };
        assert!(with(extra));
        assert!(!with(extra - eur(10.0)));
        // Cutting spending works twice over, so it takes less of it.
        assert!(early.spend_cut_monthly.unwrap() < extra);
    }

    #[test]
    fn savings_tax_follows_the_brackets() {
        // 6k at 19% + 4k at 21% = 1,140 + 840.
        assert_eq!(savings_tax(eur(10_000.0), &progressive()), eur(1_980.0));
        assert_eq!(savings_tax(Money::ZERO, &progressive()), Money::ZERO);
        // 6k·19% + 44k·21% + 150k·23% + 100k·27% + 50k·30%
        assert_eq!(
            savings_tax(eur(350_000.0), &progressive()),
            eur(1_140.0 + 9_240.0 + 34_500.0 + 27_000.0 + 15_000.0)
        );
    }

    #[test]
    fn the_tax_table_is_the_plans_own_and_can_be_changed() {
        // A law that taxed everything at a flat 25%.
        let flat = vec![TaxBracket {
            up_to: None,
            rate_bps: 2_500,
        }];
        assert_eq!(savings_tax(eur(10_000.0), &flat), eur(2_500.0));
        // A table whose last band was left closed still taxes what is above it.
        let closed = vec![TaxBracket {
            up_to: Some(eur(1_000.0)),
            rate_bps: 1_000,
        }];
        assert_eq!(savings_tax(eur(3_000.0), &closed), eur(300.0));
        // A plan saved before the table was editable gets today's brackets.
        let old: PlanParams = serde_json::from_str(
            r#"{"annualSavings":1200000,"realReturnBps":500,"currentAge":null,
                "targetAge":null,"horizonYears":40}"#,
        )
        .unwrap();
        assert_eq!(old.tax_brackets, default_brackets());
        assert_eq!(default_brackets().len(), 1);
        assert_eq!(default_brackets()[0].rate_bps, 0, "no country, no tax");
    }

    #[test]
    fn the_gross_withdrawal_leaves_the_spending_after_tax() {
        let net = eur(30_000.0);
        let gross = gross_for_net(net, 5_000, &progressive());
        let after = gross - savings_tax(gross.mul_bps(5_000), &progressive());
        assert!(
            (after - net).minor().abs() <= 1,
            "{gross:?} leaves {after:?}"
        );
        assert!(gross > net);
    }

    #[test]
    fn tax_makes_the_target_bigger_not_smaller() {
        let with_tax = PlanParams {
            withdrawal_tax: true,
            ..plan_params(0.0, 500)
        };
        let p = plan(&million(), &balances(0.0, 0.0), &with_tax).unwrap();
        assert!(p.target_now > eur(1_000_000.0));
        assert!(p.withdrawal_tax > Money::ZERO);
        assert_eq!(p.gross_withdrawal - p.withdrawal_tax, eur(40_000.0));
    }

    #[test]
    fn a_pension_lowers_the_target_as_it_gets_closer() {
        let pp = PlanParams {
            current_age: Some(40),
            pension_age: Some(67),
            annual_pension: eur(15_000.0),
            ..plan_params(20_000.0, 500)
        };
        let p = plan(&million(), &balances(100_000.0, 0.0), &pp).unwrap();
        let t0 = p.path[0].target;
        let t20 = p.path[20].target;
        let t30 = p.path[30].target;
        assert!(t0 < eur(1_000_000.0), "a pension should help even now");
        assert!(t20 < t0, "closer to the pension, less to carry");
        // Past 67 the portfolio only covers the 25k the pension does not.
        assert_eq!(t30, eur(625_000.0));
    }

    #[test]
    fn the_band_brackets_the_base_case() {
        let p = plan(
            &million(),
            &balances(100_000.0, 0.0),
            &plan_params(20_000.0, 500),
        )
        .unwrap();
        let (lo, mid, hi) = (
            p.years_low.unwrap(),
            p.years_to_target.unwrap(),
            p.years_high.unwrap(),
        );
        assert!(hi < mid && mid < lo, "{hi} < {mid} < {lo}");
        assert!(p.path[10].liquid_low < p.path[10].liquid);
        assert!(p.path[10].liquid < p.path[10].liquid_high);
    }

    #[test]
    fn milestones_come_in_order_and_the_last_one_is_the_target() {
        let p = plan(&million(), &balances(0.0, 0.0), &plan_params(30_000.0, 500)).unwrap();
        let ys: Vec<f64> = p.milestones.iter().map(|m| m.years.unwrap()).collect();
        assert!(ys.windows(2).all(|w| w[0] < w[1]), "{ys:?}");
        assert_eq!(p.milestones[3].years, p.years_to_target);
    }

    #[test]
    fn spending_less_is_worth_more_than_saving_the_same_amount() {
        // Spending less cuts the target *and* raises the saving.
        let p = plan(
            &million(),
            &balances(50_000.0, 0.0),
            &plan_params(15_000.0, 500),
        )
        .unwrap();
        let get = |kind: &str| {
            p.levers
                .iter()
                .find(|l| l.kind == kind && l.monthly == eur(100.0))
                .unwrap()
                .months_earlier
                .unwrap()
        };
        assert!(get("save") > 0);
        assert!(get("spend") > get("save"));
    }

    #[test]
    fn the_classic_twenty_five_times_rule() {
        let p = FireParams {
            annual_spend_target: eur(40_000.0),
            swr_bps: 400,
            annual_net_rental_income: Money::ZERO,
        };
        let r = calculate(&p, &balances(0.0, 0.0)).unwrap();
        assert_eq!(r.liquid_target, eur(1_000_000.0));
    }

    #[test]
    fn rent_reduces_the_portfolio_needed() {
        let p = FireParams {
            annual_spend_target: eur(40_000.0),
            swr_bps: 400,
            annual_net_rental_income: eur(12_000.0),
        };
        let r = calculate(&p, &balances(0.0, 0.0)).unwrap();
        assert_eq!(r.residual_annual_spend, eur(28_000.0));
        assert_eq!(r.liquid_target, eur(700_000.0));
        assert_eq!(r.rent_coverage_bps, 3_000); // 30%
    }

    #[test]
    fn real_estate_equity_is_never_added_to_the_liquid_side() {
        // This is the double-count the spec had. A large property portfolio
        // must not move the liquid target or the liquid progress figure.
        let p = FireParams {
            annual_spend_target: eur(40_000.0),
            swr_bps: 400,
            annual_net_rental_income: eur(12_000.0),
        };
        let poor_in_property = calculate(&p, &balances(350_000.0, 0.0)).unwrap();
        let rich_in_property = calculate(&p, &balances(350_000.0, 500_000.0)).unwrap();

        assert_eq!(
            poor_in_property.liquid_target,
            rich_in_property.liquid_target
        );
        assert_eq!(
            poor_in_property.liquid_progress_bps,
            rich_in_property.liquid_progress_bps
        );
        assert_eq!(poor_in_property.liquid_progress_bps, 5_000); // 350k of 700k

        // The equity shows up only in the separate total-wealth figure.
        assert_eq!(poor_in_property.total_net_worth, eur(350_000.0));
        assert_eq!(rich_in_property.total_net_worth, eur(850_000.0));
    }

    #[test]
    fn rent_alone_can_cover_everything() {
        let p = FireParams {
            annual_spend_target: eur(30_000.0),
            swr_bps: 400,
            annual_net_rental_income: eur(36_000.0),
        };
        let r = calculate(&p, &balances(0.0, 400_000.0)).unwrap();
        assert!(r.covered_by_rent_alone);
        assert_eq!(r.liquid_target, Money::ZERO);
        assert_eq!(r.liquid_shortfall, Money::ZERO);
        assert!(r.liquid_covered);
        assert_eq!(r.liquid_progress_bps, 10_000);
        assert!(r.rent_coverage_bps > 10_000);
    }

    #[test]
    fn shortfall_and_progress_agree_with_each_other() {
        let p = FireParams {
            annual_spend_target: eur(40_000.0),
            swr_bps: 400,
            annual_net_rental_income: Money::ZERO,
        };
        let r = calculate(&p, &balances(250_000.0, 0.0)).unwrap();
        assert_eq!(r.liquid_target, eur(1_000_000.0));
        assert_eq!(r.liquid_shortfall, eur(750_000.0));
        assert_eq!(r.liquid_progress_bps, 2_500);
        assert!(!r.liquid_covered);
    }

    #[test]
    fn a_lower_withdrawal_rate_demands_a_bigger_portfolio() {
        let base = FireParams {
            annual_spend_target: eur(40_000.0),
            swr_bps: 400,
            annual_net_rental_income: Money::ZERO,
        };
        let cautious = FireParams {
            swr_bps: 325,
            ..base.clone()
        };
        let a = calculate(&base, &balances(0.0, 0.0)).unwrap();
        let b = calculate(&cautious, &balances(0.0, 0.0)).unwrap();
        assert!(b.liquid_target > a.liquid_target);
    }

    #[test]
    fn other_debt_reduces_total_wealth_but_not_the_liquid_target() {
        let p = FireParams::default();
        let b = Balances {
            liquid: eur(100_000.0),
            real_estate_equity: eur(200_000.0),
            other_assets: eur(10_000.0),
            other_debt: eur(50_000.0),
        };
        let r = calculate(&p, &b).unwrap();
        assert_eq!(r.total_net_worth, eur(260_000.0));
        assert_eq!(r.liquid_assets, eur(100_000.0));
    }

    #[test]
    fn nominal_rent_is_deflated_before_meeting_a_real_swr() {
        // 1,000/month in year 10 at 2% CPI is worth ~820 today.
        let today = to_todays_money(eur(1_000.0), 200, 10.0);
        assert!((today.as_major_f64() - 820.35).abs() < 1.0, "got {today}");
        assert_eq!(to_todays_money(eur(1_000.0), 200, 0.0), eur(1_000.0));
        assert_eq!(to_todays_money(eur(1_000.0), 0, 10.0), eur(1_000.0));
    }

    #[test]
    fn using_an_undeflated_rent_would_understate_the_target() {
        // Guards the documented contract: feeding a nominal year-10 rent into a
        // real SWR shrinks the target, which is exactly the trap being avoided.
        let nominal = eur(18_000.0);
        let real = to_todays_money(nominal, 200, 10.0);
        let p_wrong = FireParams {
            annual_spend_target: eur(40_000.0),
            swr_bps: 400,
            annual_net_rental_income: nominal,
        };
        let p_right = FireParams {
            annual_net_rental_income: real,
            ..p_wrong.clone()
        };
        let wrong = calculate(&p_wrong, &balances(0.0, 0.0)).unwrap();
        let right = calculate(&p_right, &balances(0.0, 0.0)).unwrap();
        assert!(
            right.liquid_target > wrong.liquid_target,
            "deflating rent must raise the target, not lower it"
        );
    }

    #[test]
    fn invalid_parameters_are_rejected() {
        let bad = FireParams {
            swr_bps: 0,
            ..Default::default()
        };
        let errs = calculate(&bad, &balances(0.0, 0.0)).unwrap_err();
        assert_eq!(errs[0].field, "swrBps");

        let negative = FireParams {
            annual_spend_target: eur(-1.0),
            ..Default::default()
        };
        assert!(calculate(&negative, &balances(0.0, 0.0)).is_err());
    }

    #[test]
    fn a_zero_spend_target_is_trivially_covered() {
        let p = FireParams {
            annual_spend_target: Money::ZERO,
            swr_bps: 400,
            annual_net_rental_income: Money::ZERO,
        };
        let r = calculate(&p, &balances(0.0, 0.0)).unwrap();
        assert_eq!(r.liquid_target, Money::ZERO);
        assert!(r.liquid_covered);
        assert!(
            !r.covered_by_rent_alone,
            "no rent, no spending: not rent-covered"
        );
    }
}
