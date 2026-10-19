use super::*;

fn eur(major: f64) -> Money {
    Money::from_major(major)
}

fn flat(buy_month: u32, price: f64, rent: f64) -> PlannedProperty {
    PlannedProperty {
        name: "Flat".into(),
        buy_month,
        price: eur(price),
        renovation: eur(15_000.0),
        monthly_rent: eur(rent),
        transfer_tax_bps: 1_000,
        financing_bps: None,
        mortgage_rate_bps: None,
        mortgage_term_months: None,
        fixed_costs_annual: None,
        owned: None,
    }
}

/// A flat bought before the plan, worth `value` today, with `owed` left on its
/// loan over `months` more months.
fn owned(value: f64, rent: f64, owed: f64, months: u32) -> PlannedProperty {
    PlannedProperty {
        name: "Home I let".into(),
        buy_month: 1,
        price: eur(value),
        renovation: Money::ZERO,
        monthly_rent: eur(rent),
        transfer_tax_bps: 0,
        financing_bps: None,
        mortgage_rate_bps: Some(300),
        mortgage_term_months: Some(months),
        fixed_costs_annual: None,
        owned: Some(OwnedSince {
            bought_on: Some("2019-05".into()),
            paid: Some(eur(150_000.0)),
            loan_remaining: eur(owed),
        }),
    }
}

/// The defaults, with the figures the hand-checked expectations below were
/// worked out with written out, so they do not move when a default does.
fn base() -> SimParams {
    SimParams {
        mortgage_rate_bps: 280,
        fixed_costs_annual: eur(900.0),
        rental_tax_bps: 1_900,
        ..SimParams::default()
    }
}

/// `base`, with one 120k flat planned for the first month.
fn params() -> SimParams {
    SimParams {
        income_growth_bps: 0,
        properties: vec![flat(1, 120_000.0, 700.0)],
        ..base()
    }
}

#[test]
fn default_scenario_produces_a_full_ten_year_projection() {
    let r = run(&base()).expect("defaults must validate");
    assert_eq!(r.years.len(), 10);
    assert_eq!(r.monthly_net_worth.len(), 120);
    assert_eq!(r.years[0].year, 1);
    assert_eq!(r.years[9].year, 10);
    assert!(
        r.properties.is_empty() && r.final_properties == 0,
        "a new plan starts empty: what to buy is the person's decision"
    );
}

#[test]
fn upfront_cash_matches_the_documented_formula() {
    let p = params();
    // 120k: 20% deposit 24,000 + 10% ITP 12,000 + 1.5% closing 1,800 + 15,000 reno
    assert_eq!(p.cash_required(&p.properties[0]), eur(52_800.0));
    assert_eq!(p.loan_for(&p.properties[0]).balance, eur(96_000.0));
}

#[test]
fn the_purchase_tax_belongs_to_the_property() {
    // The same flat in a region that charges 6% instead of 10% needs 4% of
    // the price less at the notary — and nothing else about the plan moves.
    let mut p = params();
    p.properties[0].transfer_tax_bps = 600;
    assert_eq!(p.cash_required(&p.properties[0]), eur(48_000.0));
}

#[test]
fn a_property_can_carry_its_own_mortgage_terms() {
    let mut p = params();
    p.properties[0].financing_bps = Some(6_000);
    p.properties[0].mortgage_term_months = Some(180);
    let own = p.loan_for(&p.properties[0]);
    assert_eq!(own.balance, eur(72_000.0));
    assert_eq!(own.remaining_months, 180);
    // A second one that says nothing takes the general terms.
    let general = p.loan_for(&flat(1, 120_000.0, 700.0));
    assert_eq!(general.balance, eur(96_000.0));
    assert_eq!(general.remaining_months, 300);
}

#[test]
fn nothing_is_bought_that_was_not_planned() {
    let p = SimParams {
        properties: Vec::new(),
        start_cash: eur(900_000.0),
        monthly_savings: eur(50.0),
        income_growth_bps: 0,
        ..base()
    };
    let r = run(&p).unwrap();
    assert_eq!(r.final_properties, 0, "cash alone buys nothing");
    // With no property, net worth is exactly cash plus the savings stream.
    assert_eq!(r.final_net_worth, eur(900_000.0) + eur(50.0).mul_int(120));
    for y in &r.years {
        assert_eq!(y.savings_contributed, eur(50.0).mul_int(12));
        assert_eq!(y.net_rent_monthly, Money::ZERO);
        assert_eq!(y.debt, Money::ZERO);
    }
}

#[test]
fn a_purchase_happens_in_the_month_it_was_planned_for() {
    let mut p = params();
    p.properties[0].buy_month = 30;
    let r = run(&p).unwrap();
    let o = &r.properties[0];
    assert_eq!(o.bought_month, Some(30));
    assert_eq!(o.blocked_by, None);
    assert_eq!(r.years[1].properties, 0, "nothing in year two");
    assert_eq!(r.years[2].purchases, 1, "month 30 is in year three");
    assert_eq!(r.years[2].properties, 1);
}

#[test]
fn a_purchase_the_cash_cannot_cover_waits_and_says_why() {
    // 52,800 needed, 20,000 in hand, 1,500 a month coming in.
    let p = SimParams {
        start_cash: eur(20_000.0),
        cash_buffer: Money::ZERO,
        ..params()
    };
    let r = run(&p).unwrap();
    let o = &r.properties[0];
    assert_eq!(o.blocked_by.as_deref(), Some("cash"));
    // Month 1 has 21,500 after that month's saving.
    assert_eq!(o.cash_short, eur(52_800.0 - 21_500.0));
    // 20,000 + 1,500·m ≥ 52,800 first holds at m = 22.
    assert_eq!(o.bought_month, Some(22));
}

#[test]
fn a_purchase_that_never_becomes_affordable_is_not_bought() {
    let p = SimParams {
        start_cash: Money::ZERO,
        monthly_savings: eur(50.0),
        ..params()
    };
    let r = run(&p).unwrap();
    assert_eq!(r.properties[0].bought_month, None);
    assert_eq!(r.properties[0].blocked_by.as_deref(), Some("cash"));
    assert_eq!(r.final_properties, 0);
}

#[test]
fn the_bank_can_refuse_what_the_cash_would_allow() {
    let rich = SimParams {
        start_cash: eur(500_000.0),
        ..params()
    };
    let strict = SimParams {
        dti_max_bps: 500, // 5% — nothing qualifies
        ..rich.clone()
    };
    assert_eq!(run(&rich).unwrap().properties[0].bought_month, Some(1));

    let r = run(&strict).unwrap();
    let o = &r.properties[0];
    assert_eq!(o.bought_month, None);
    assert_eq!(o.blocked_by.as_deref(), Some("bank"));
    assert!(o.cash_short.is_zero(), "the cash was there");
    assert!(o.dti_bps_planned > 500);
}

#[test]
fn the_banks_test_counts_the_new_mortgage_and_part_of_the_new_rent() {
    let p = params();
    let o = &run(&p).unwrap().properties[0];
    // 96,000 at 2.8% over 25 years is 445.32 a month. Income the bank counts:
    // 3,200 of pay plus 70% of 700 rent = 3,690.
    assert_eq!(o.monthly_payment, eur(445.32));
    assert_eq!(o.dti_bps_planned, 44_532 * 10_000 / 369_000);
}

#[test]
fn a_blocked_purchase_holds_the_ones_planned_after_it() {
    // The first flat cannot be afforded until month 22; a cheap second one
    // planned for month 2 does not jump the queue.
    let mut p = SimParams {
        start_cash: eur(20_000.0),
        cash_buffer: Money::ZERO,
        ..params()
    };
    let mut cheap = flat(2, 30_000.0, 300.0);
    cheap.renovation = Money::ZERO;
    p.properties.push(cheap);
    let r = run(&p).unwrap();
    assert_eq!(r.properties[0].bought_month, Some(22));
    assert!(r.properties[1].bought_month.unwrap() >= 22);
}

#[test]
fn outcomes_come_back_in_the_order_the_plan_was_given() {
    let mut p = params();
    p.start_cash = eur(400_000.0);
    p.properties = vec![flat(40, 100_000.0, 600.0), flat(3, 90_000.0, 550.0)];
    let r = run(&p).unwrap();
    assert_eq!(r.properties[0].index, 0);
    assert_eq!(r.properties[0].bought_month, Some(40));
    assert_eq!(r.properties[1].bought_month, Some(3));
}

#[test]
fn pay_and_what_is_put_aside_rise_once_a_year() {
    let p = SimParams {
        income_growth_bps: 1_000,
        properties: Vec::new(),
        ..base()
    };
    let r = run(&p).unwrap();
    assert_eq!(r.years[0].savings_contributed, eur(1_500.0).mul_int(12));
    assert_eq!(r.years[1].savings_contributed, eur(1_650.0).mul_int(12));
    assert_eq!(r.years[0].salary_monthly, eur(3_200.0));
    assert_eq!(r.years[1].salary_monthly, eur(3_520.0));
}

#[test]
fn rent_kept_adds_to_what_can_be_put_aside() {
    let p = params();
    let r = run(&p).unwrap();
    assert!(r.final_net_rent_monthly > Money::ZERO);
    assert_eq!(
        r.final_saving_capacity,
        p.monthly_savings + r.final_net_rent_monthly
    );
    // And it really does reach the cash: switch it off and less accumulates.
    let kept_apart = run(&SimParams {
        reinvest_net_rent: false,
        ..p
    })
    .unwrap();
    assert!(r.years[9].cash > kept_apart.years[9].cash);
}

#[test]
fn debt_matches_the_loan_taken_and_then_amortises_downward() {
    let r = run(&params()).unwrap();
    assert_eq!(r.final_properties, 1);
    // Bought in month 1 at 80% LTV, so debt starts at 96,000 and only falls.
    assert!(r.years[0].debt < eur(96_000.0));
    assert!(r.years[9].debt < r.years[0].debt);
    assert!(!r.years[9].debt.is_negative());
    assert_eq!(r.properties[0].debt_end, r.years[9].debt);
}

#[test]
fn cash_never_goes_below_the_buffer_because_of_a_purchase() {
    let mut p = SimParams {
        cash_buffer: eur(10_000.0),
        start_cash: eur(70_000.0),
        ..params()
    };
    p.properties.push(flat(20, 110_000.0, 650.0));
    p.properties.push(flat(60, 130_000.0, 760.0));
    let r = run(&p).unwrap();
    for y in &r.years {
        assert!(
            y.cash >= p.cash_buffer,
            "year {} broke the buffer: {}",
            y.year,
            y.cash
        );
    }
}

#[test]
fn a_zero_appreciation_zero_cpi_run_holds_value_flat() {
    let p = SimParams {
        appreciation_bps: 0,
        rent_cpi_bps: 0,
        ..params()
    };
    let r = run(&p).unwrap();
    let expected = eur(135_000.0); // price + renovation
    for y in &r.years {
        assert_eq!(y.portfolio_value, expected, "year {}", y.year);
        assert_eq!(y.gross_rent_monthly, eur(700.0));
    }
    assert_eq!(r.properties[0].value_end, expected);
}

#[test]
fn equity_always_reconciles_to_value_minus_debt() {
    let mut p = params();
    p.properties.push(flat(54, 140_000.0, 820.0));
    let r = run(&p).unwrap();
    for y in &r.years {
        assert_eq!(y.equity, y.portfolio_value - y.debt);
        assert_eq!(y.net_worth, y.cash + y.equity);
    }
    let value: Money = r.properties.iter().map(|p| p.value_end).sum();
    assert_eq!(value, r.years[9].portfolio_value);
}

#[test]
fn the_engine_is_deterministic() {
    let p = params();
    assert_eq!(run(&p).unwrap(), run(&p).unwrap());
}

#[test]
fn reporting_does_not_advance_the_simulation() {
    // A unit bought in month 1 is bought *after* that month's debt service, so
    // its first instalment falls in month 2 — as it would in reality, where
    // completion and the first payment are a month apart. Over a 120-month run
    // the loan therefore sees exactly 119 periods.
    //
    // The test matters because year-end reporting reads the loans to build the
    // annual table. If that read stepped them, debt would fall faster than the
    // amortisation schedule allows and the 119-period figure would not match.
    let p = params();
    let r = run(&p).unwrap();

    let build_reference = |periods: usize| {
        let mut l = p.loan_for(&p.properties[0]);
        for _ in 0..periods {
            l.advance_one_month();
        }
        l.balance
    };

    assert_eq!(r.years[9].debt, build_reference(119));
    assert_ne!(
        r.years[9].debt,
        build_reference(120),
        "reporting appears to have stepped the loans"
    );
}

#[test]
fn validation_rejects_impossible_inputs() {
    let mut bad = SimParams {
        horizon_years: 0,
        financing_bps: 12_000,
        occupancy_bps: -1,
        mortgage_term_months: 0,
        ..params()
    };
    bad.properties[0].price = Money::ZERO;
    bad.properties[0].buy_month = 0;
    let errs = run(&bad).unwrap_err();
    let fields: Vec<_> = errs.iter().map(|e| e.field).collect();
    for expected in [
        "horizonYears",
        "financingBps",
        "occupancyBps",
        "propertyPrice",
        "buyMonth",
        "mortgageTermMonths",
    ] {
        assert!(fields.contains(&expected), "missing error for {expected}");
    }
}

#[test]
fn params_round_trip_through_json_as_camel_case() {
    let p = params();
    let json = serde_json::to_string(&p).unwrap();
    assert!(json.contains("\"monthlyRent\""), "expected camelCase keys");
    assert!(json.contains("\"mortgageTermMonths\""));
    // Money must cross as integers, never as floats.
    assert!(
        !json.contains("."),
        "a float leaked into the IPC payload: {json}"
    );
    let back: SimParams = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn a_scenario_saved_by_the_automatic_simulator_still_loads() {
    // What 0.1 wrote: fields that no longer exist, none of the new ones.
    let old = r#"{"horizonYears":10,"startCash":6000000,"monthlySavings":150000,
        "netMonthlySalary":320000,"existingMonthlyDebt":0,"propertyPrice":12000000,
        "renovation":1500000,"grossYieldBps":700,"financingBps":8000,"transferTaxBps":1000,
        "otherClosingBps":150,"mortgageRateBps":280,"mortgageTermMonths":300,
        "occupancyBps":9000,"rentCpiBps":200,"appreciationBps":200,"mgmtFeeBps":800,
        "maintenanceBps":500,"fixedCostsAnnual":90000,"rentalTaxBps":1900,"dtiMaxBps":3500,
        "computableRentBps":7000,"cashBuffer":600000,"reinvestNetRent":true,
        "maxProperties":null,"overrides":[{"year":2,"annualSavings":null,"purchases":0,
        "propertyPrice":null,"renovation":null,"grossYieldBps":null,"mortgageRateBps":null,
        "mortgageTermMonths":null,"rentCpiBps":null,"appreciationBps":null,
        "occupancyBps":null,"note":null}]}"#;
    let p: SimParams = serde_json::from_str(old).unwrap();
    assert!(p.properties.is_empty(), "no plan was ever written down");
    assert_eq!(p.income_growth_bps, 0);
    assert_eq!(run(&p).unwrap().final_properties, 0);
}

#[test]
fn a_longer_mortgage_term_means_a_smaller_payment_and_more_rent_kept() {
    let mut short = params();
    short.properties[0].mortgage_term_months = Some(180);
    let mut long = params();
    long.properties[0].mortgage_term_months = Some(360);
    let a = &run(&short).unwrap().properties[0];
    let b = &run(&long).unwrap().properties[0];
    assert!(b.monthly_payment < a.monthly_payment);
    assert!(b.net_rent_first > a.net_rent_first);
    assert!(b.dti_bps_planned < a.dti_bps_planned);
}

#[test]
fn each_year_says_what_is_yours_in_each_property_and_it_adds_up() {
    // Two flats, the second well after the first, and a third far too big to
    // ever be bought: it must be present, as nothing.
    let p = SimParams {
        income_growth_bps: 0,
        // Enough pay and saving that the second flat is bought when planned.
        net_monthly_salary: eur(6_000.0),
        monthly_savings: eur(3_000.0),
        properties: vec![
            flat(1, 120_000.0, 700.0),
            flat(61, 130_000.0, 750.0),
            flat(100, 9_000_000.0, 5_000.0),
        ],
        ..base()
    };
    let r = run(&p).unwrap();
    assert_eq!(
        r.properties[1].bought_month,
        Some(61),
        "{:?}",
        r.properties[1]
    );
    assert!(r.properties[2].bought_month.is_none());

    for y in &r.years {
        assert_eq!(y.property_equity.len(), 3, "one slot per planned property");
        let sum: Money = y.property_equity.iter().copied().sum();
        assert_eq!(sum, y.equity, "year {}: the parts are the whole", y.year);
        assert_eq!(y.property_equity[2], Money::ZERO, "never bought");
    }
    // Year 1: only the first flat exists; the second appears in year 6.
    assert!(r.years[0].property_equity[0] > Money::ZERO);
    assert_eq!(r.years[0].property_equity[1], Money::ZERO);
    assert_eq!(r.years[4].property_equity[1], Money::ZERO);
    assert!(r.years[5].property_equity[1] > Money::ZERO);
    // Paying the mortgage down and a rising market only add to what is yours.
    assert!(r.years[9].property_equity[0] > r.years[0].property_equity[0]);
}

/* ------------------------------------------------- already owned -- */

#[test]
fn a_property_already_owned_is_held_from_the_start_and_costs_nothing() {
    let p = SimParams {
        income_growth_bps: 0,
        properties: vec![owned(200_000.0, 900.0, 80_000.0, 180)],
        ..base()
    };
    let r = run(&p).unwrap();
    let o = &r.properties[0];
    assert_eq!(o.bought_month, Some(0), "owned before the plan begins");
    assert!(o.blocked_by.is_none());
    assert_eq!(o.cash_needed, Money::ZERO);

    // Month one: the flat is there at today's value, its loan is owed, and no
    // cash went on buying it. Cash = start + saving + the month's net rent.
    let loan = Loan::new(eur(80_000.0), 300, 180);
    let first_net_rent = r.properties[0].net_rent_first;
    let expected_cash = p.start_cash + p.monthly_savings + first_net_rent;
    let value_m1 = eur(200_000.0).mul_f64(index(p.appreciation_bps, 1)[1]);
    let mut l = loan.clone();
    l.advance_one_month();
    let nw_m1 = expected_cash + value_m1 - l.balance;
    assert_eq!(r.monthly_net_worth[0], nw_m1);
    assert_eq!(r.years[0].properties, 1);
    assert_eq!(r.years[0].purchases, 0, "owning is not buying");
}

#[test]
fn a_paid_off_property_brings_rent_and_no_payment() {
    let p = SimParams {
        income_growth_bps: 0,
        properties: vec![owned(150_000.0, 800.0, 0.0, 1)],
        ..base()
    };
    let r = run(&p).unwrap();
    assert_eq!(r.properties[0].debt_end, Money::ZERO);
    assert!(r.properties[0].net_rent_first > Money::ZERO);
    assert_eq!(r.years.last().unwrap().debt, Money::ZERO);
}

#[test]
fn the_loan_on_a_property_already_owned_counts_when_the_bank_looks_at_the_next() {
    // Same planned purchase, with and without a heavily mortgaged flat already
    // held: the bank sees the existing payment, so capacity is lower.
    let planned = flat(1, 120_000.0, 700.0);
    let alone = run(&SimParams {
        income_growth_bps: 0,
        properties: vec![planned.clone()],
        ..base()
    })
    .unwrap();
    let with_owned = run(&SimParams {
        income_growth_bps: 0,
        properties: vec![owned(250_000.0, 0.0, 220_000.0, 240), planned],
        ..base()
    })
    .unwrap();
    assert!(with_owned.properties[1].dti_bps_planned > alone.properties[0].dti_bps_planned);
}

#[test]
fn an_owned_property_needs_no_purchase_month_and_rejects_a_negative_loan() {
    let mut p = owned(100_000.0, 500.0, 10_000.0, 60);
    p.buy_month = 0;
    assert!(
        run(&SimParams {
            properties: vec![p.clone()],
            ..base()
        })
        .is_ok()
    );
    p.owned.as_mut().unwrap().loan_remaining = eur(-1.0);
    assert!(
        run(&SimParams {
            properties: vec![p],
            ..base()
        })
        .is_err()
    );
}

#[test]
fn a_scenario_saved_before_owned_properties_still_loads() {
    let json = r#"{"name":"Old","buyMonth":3,"price":10000000,"renovation":0,
        "monthlyRent":60000,"transferTaxBps":600}"#;
    let p: PlannedProperty = serde_json::from_str(json).unwrap();
    assert!(p.owned.is_none());
}
