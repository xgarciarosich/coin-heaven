use super::*;

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
}

fn txn(id: i64, account: i64, date: &str, minor: i64) -> TxnRef {
    TxnRef {
        id,
        account_id: account,
        booked_on: d(date),
        amount_minor: Money::from_minor(minor),
        currency: "EUR".into(),
        description: format!("txn {id}"),
        counterparty: None,
        is_neutral_transfer: false,
        has_parent: false,
    }
}

fn none() -> HashSet<(i64, i64)> {
    HashSet::new()
}

// ------------------------------------------------------------------ transfers

#[test]
fn matches_a_clean_transfer_between_two_accounts() {
    let txns = vec![
        txn(1, 10, "2026-03-01", -50_000),
        txn(2, 20, "2026-03-02", 50_000),
    ];
    let got = find_transfers(&txns, TransferParams::default(), &none());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].out_id, 1);
    assert_eq!(got[0].in_id, 2);
    assert_eq!(got[0].residual_minor, Money::ZERO);
    assert_eq!(got[0].day_gap, 1);
}

#[test]
fn a_wire_fee_is_absorbed_by_the_tolerance() {
    // 500.00 leaves, 498.50 arrives: a 1.50 fee.
    let txns = vec![
        txn(1, 10, "2026-03-01", -50_000),
        txn(2, 20, "2026-03-01", 49_850),
    ];
    let got = find_transfers(&txns, TransferParams::default(), &none());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].residual_minor, Money::from_minor(-150));
}

#[test]
fn a_fee_larger_than_tolerance_is_not_proposed() {
    let txns = vec![
        txn(1, 10, "2026-03-01", -50_000),
        txn(2, 20, "2026-03-01", 45_000),
    ];
    assert!(find_transfers(&txns, TransferParams::default(), &none()).is_empty());
}

#[test]
fn same_account_pairs_never_match() {
    let txns = vec![
        txn(1, 10, "2026-03-01", -50_000),
        txn(2, 10, "2026-03-01", 50_000),
    ];
    assert!(
        find_transfers(&txns, TransferParams::default(), &none()).is_empty(),
        "a debit and credit in one account is not an internal transfer"
    );
}

#[test]
fn same_sign_pairs_never_match() {
    let txns = vec![
        txn(1, 10, "2026-03-01", -50_000),
        txn(2, 20, "2026-03-01", -50_000),
    ];
    assert!(find_transfers(&txns, TransferParams::default(), &none()).is_empty());
}

#[test]
fn cross_currency_pairs_are_left_alone() {
    let mut b = txn(2, 20, "2026-03-01", 50_000);
    b.currency = "USD".into();
    let txns = vec![txn(1, 10, "2026-03-01", -50_000), b];
    assert!(
        find_transfers(&txns, TransferParams::default(), &none()).is_empty(),
        "matching across currencies needs an FX rate, not an amount tolerance"
    );
}

#[test]
fn the_date_window_is_enforced() {
    let txns = vec![
        txn(1, 10, "2026-03-01", -50_000),
        txn(2, 20, "2026-03-05", 50_000),
    ];
    assert!(find_transfers(&txns, TransferParams::default(), &none()).is_empty());
    let wide = TransferParams {
        window_days: 5,
        ..Default::default()
    };
    assert_eq!(find_transfers(&txns, wide, &none()).len(), 1);
}

#[test]
fn matching_is_one_to_one_so_one_credit_cannot_neutralise_two_debits() {
    let txns = vec![
        txn(1, 10, "2026-03-01", -50_000),
        txn(2, 11, "2026-03-01", -50_000),
        txn(3, 20, "2026-03-01", 50_000),
    ];
    let got = find_transfers(&txns, TransferParams::default(), &none());
    assert_eq!(got.len(), 1, "only one debit may be neutralised");
    let used: Vec<i64> = got.iter().flat_map(|c| [c.out_id, c.in_id]).collect();
    assert!(used.contains(&3));
}

#[test]
fn the_tightest_residual_wins_when_several_candidates_compete() {
    let txns = vec![
        txn(1, 10, "2026-03-01", -50_000),
        txn(2, 20, "2026-03-01", 49_900), // 1.00 off
        txn(3, 21, "2026-03-01", 50_000), // exact
    ];
    let got = find_transfers(&txns, TransferParams::default(), &none());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].in_id, 3, "the exact match should be preferred");
}

#[test]
fn a_rejected_pair_is_never_proposed_again() {
    let txns = vec![
        txn(1, 10, "2026-03-01", -50_000),
        txn(2, 20, "2026-03-01", 50_000),
    ];
    let mut decided = HashSet::new();
    decided.insert(pair_key(1, 2));
    assert!(find_transfers(&txns, TransferParams::default(), &decided).is_empty());
}

#[test]
fn already_neutralised_transactions_are_skipped() {
    let mut a = txn(1, 10, "2026-03-01", -50_000);
    a.is_neutral_transfer = true;
    let txns = vec![a, txn(2, 20, "2026-03-01", 50_000)];
    assert!(find_transfers(&txns, TransferParams::default(), &none()).is_empty());
}

#[test]
fn transfer_output_is_deterministic_regardless_of_input_order() {
    let mut txns = vec![
        txn(1, 10, "2026-03-01", -50_000),
        txn(2, 20, "2026-03-01", 50_000),
        txn(3, 11, "2026-03-02", -20_000),
        txn(4, 21, "2026-03-02", 20_000),
    ];
    let first = find_transfers(&txns, TransferParams::default(), &none());
    txns.reverse();
    let second = find_transfers(&txns, TransferParams::default(), &none());
    assert_eq!(first, second);
    assert_eq!(first.len(), 2);
}

// ---------------------------------------------------------------- split bills

#[test]
fn matches_a_half_share_of_a_dinner() {
    let txns = vec![
        txn(1, 10, "2026-04-10", -6_000), // 60.00 dinner
        txn(2, 10, "2026-04-11", 3_000),  // 30.00 back
    ];
    let got = find_split_reimbursements(&txns, SplitParams::default(), &none());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].parent_id, 1);
    assert_eq!(got[0].reimbursement_id, 2);
    assert_eq!(got[0].fraction_denominator, 2);
}

#[test]
fn matches_thirds_quarters_and_fifths() {
    for (denom, share) in [(3u32, 4_000i64), (4, 3_000), (5, 2_400)] {
        let txns = vec![
            txn(1, 10, "2026-04-10", -12_000),
            txn(2, 10, "2026-04-10", share),
        ];
        let got = find_split_reimbursements(&txns, SplitParams::default(), &none());
        assert_eq!(got.len(), 1, "denominator {denom}");
        assert_eq!(got[0].fraction_denominator, denom);
    }
}

#[test]
fn small_bills_are_below_the_guardrail() {
    // A 20.00 bill and a 10.00 credit is a 1/2 match on shape alone — exactly
    // the coincidence the 30.00 floor exists to suppress.
    let txns = vec![
        txn(1, 10, "2026-04-10", -2_000),
        txn(2, 10, "2026-04-11", 1_000),
    ];
    assert!(find_split_reimbursements(&txns, SplitParams::default(), &none()).is_empty());
}

#[test]
fn a_credit_before_the_bill_is_not_a_reimbursement() {
    let txns = vec![
        txn(1, 10, "2026-04-10", -6_000),
        txn(2, 10, "2026-04-09", 3_000),
    ];
    assert!(find_split_reimbursements(&txns, SplitParams::default(), &none()).is_empty());
}

#[test]
fn the_seventy_two_hour_window_is_three_days() {
    let inside = vec![
        txn(1, 10, "2026-04-10", -6_000),
        txn(2, 10, "2026-04-13", 3_000),
    ];
    assert_eq!(
        find_split_reimbursements(&inside, SplitParams::default(), &none()).len(),
        1
    );
    let outside = vec![
        txn(1, 10, "2026-04-10", -6_000),
        txn(2, 10, "2026-04-14", 3_000),
    ];
    assert!(find_split_reimbursements(&outside, SplitParams::default(), &none()).is_empty());
}

#[test]
fn one_bill_can_collect_several_reimbursements() {
    // 120.00 dinner for four; three friends each send 30.00.
    let txns = vec![
        txn(1, 10, "2026-04-10", -12_000),
        txn(2, 10, "2026-04-10", 3_000),
        txn(3, 10, "2026-04-11", 3_000),
        txn(4, 10, "2026-04-11", 3_000),
    ];
    let got = find_split_reimbursements(&txns, SplitParams::default(), &none());
    assert_eq!(got.len(), 3);
    assert!(got.iter().all(|c| c.parent_id == 1));
    assert!(got.iter().all(|c| c.fraction_denominator == 4));
}

#[test]
fn a_credit_already_linked_to_a_parent_is_not_re_proposed() {
    let mut c = txn(2, 10, "2026-04-11", 3_000);
    c.has_parent = true;
    let txns = vec![txn(1, 10, "2026-04-10", -6_000), c];
    assert!(find_split_reimbursements(&txns, SplitParams::default(), &none()).is_empty());
}

#[test]
fn an_amount_off_by_more_than_tolerance_is_not_proposed() {
    let txns = vec![
        txn(1, 10, "2026-04-10", -6_000),
        txn(2, 10, "2026-04-11", 2_500), // 25.00, not 30.00
    ];
    assert!(find_split_reimbursements(&txns, SplitParams::default(), &none()).is_empty());
}

#[test]
fn rounding_within_tolerance_still_matches() {
    // 100.00 split three ways is 33.33 each; someone sends a round 33.00.
    let txns = vec![
        txn(1, 10, "2026-04-10", -10_000),
        txn(2, 10, "2026-04-10", 3_300),
    ];
    let got = find_split_reimbursements(&txns, SplitParams::default(), &none());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].fraction_denominator, 3);
    assert_eq!(got[0].residual_minor, Money::from_minor(33));
}

#[test]
fn a_rejected_split_pair_is_never_proposed_again() {
    let txns = vec![
        txn(1, 10, "2026-04-10", -6_000),
        txn(2, 10, "2026-04-11", 3_000),
    ];
    let mut decided = HashSet::new();
    decided.insert(pair_key(1, 2));
    assert!(find_split_reimbursements(&txns, SplitParams::default(), &decided).is_empty());
}

#[test]
fn pair_key_is_order_independent() {
    assert_eq!(pair_key(9, 3), pair_key(3, 9));
    assert_eq!(pair_key(3, 9), (3, 9));
}
