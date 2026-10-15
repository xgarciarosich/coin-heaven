use super::*;

fn rule(kind: &str, pattern: &str) -> Rule {
    Rule {
        id: Some(1),
        name: "test".into(),
        priority: 100,
        match_kind: kind.into(),
        pattern: pattern.into(),
        account_id: None,
        min_amount_minor: None,
        max_amount_minor: None,
        set_category_id: Some(5),
        set_project_id: None,
        set_neutral_transfer: false,
        is_enabled: true,
    }
}

fn txn(desc: &str, minor: i64) -> Candidate {
    Candidate {
        id: 1,
        account_id: 1,
        description_norm: crate::security::pii::normalise_description(desc),
        amount_minor: Money::from_minor(minor),
    }
}

#[test]
fn a_contains_rule_ignores_case_and_reference_numbers() {
    let r = rule("contains", "mercadona");
    assert!(matches(&r, &txn("MERCADONA VALENCIA 4471", -4520)));
    assert!(matches(&r, &txn("compra mercadona 9982", -1200)));
    assert!(!matches(&r, &txn("CARREFOUR", -4520)));
}

#[test]
fn a_pattern_is_normalised_the_same_way_the_description_is() {
    // Written with punctuation and capitals, as a person would.
    let r = rule("contains", "Café-Bar!");
    assert!(matches(&r, &txn("CAFE BAR EL TUNEL", -350)) || matches(&r, &txn("café bar", -350)));
}

#[test]
fn starts_with_and_exact_do_what_they_say() {
    assert!(matches(
        &rule("starts_with", "bizum"),
        &txn("BIZUM DE ANA", 3000)
    ));
    assert!(!matches(
        &rule("starts_with", "bizum"),
        &txn("PAGO BIZUM", -3000)
    ));

    assert!(matches(
        &rule("exact", "netflix com"),
        &txn("NETFLIX.COM", -1299)
    ));
    assert!(!matches(
        &rule("exact", "netflix"),
        &txn("NETFLIX.COM", -1299)
    ));
}

#[test]
fn a_broken_regex_matches_nothing_instead_of_failing_the_import() {
    let r = rule("regex", "([unclosed");
    assert!(!matches(&r, &txn("ANYTHING", -100)));
}

#[test]
fn a_working_regex_matches() {
    assert!(matches(
        &rule("regex", "^(aldi|lidl)"),
        &txn("LIDL SUPERMERCADO", -2200)
    ));
}

#[test]
fn an_amount_range_compares_the_signed_value() {
    // "big outgoings only": at most -500.00.
    let mut r = rule("contains", "alquiler");
    r.max_amount_minor = Some(-50_000);
    assert!(matches(&r, &txn("ALQUILER PISO", -90_000)));
    assert!(!matches(&r, &txn("ALQUILER TRASTERO", -4_000)));
}

#[test]
fn a_rule_can_be_scoped_to_one_account() {
    let mut r = rule("contains", "traspaso");
    r.account_id = Some(2);
    assert!(!matches(&r, &txn("TRASPASO", -50_000)));

    let other = Candidate {
        account_id: 2,
        ..txn("TRASPASO", -50_000)
    };
    assert!(matches(&r, &other));
}

#[test]
fn a_disabled_rule_never_matches() {
    let mut r = rule("contains", "mercadona");
    r.is_enabled = false;
    assert!(!matches(&r, &txn("MERCADONA", -100)));
}

#[test]
fn priority_decides_between_overlapping_rules() {
    let general = Rule {
        id: Some(1),
        priority: 100,
        set_category_id: Some(5),
        ..rule("contains", "cafe")
    };
    let specific = Rule {
        id: Some(2),
        priority: 10,
        set_category_id: Some(9),
        ..rule("contains", "cafe central")
    };

    let both = vec![general, specific];
    let hit = first_match(&both, &txn("CAFE CENTRAL", -240)).unwrap();
    assert_eq!(hit.set_category_id, Some(9), "the tighter rule should win");

    // Where only the general one matches, it still applies.
    let hit2 = first_match(&both, &txn("CAFE BILBAO", -350)).unwrap();
    assert_eq!(hit2.set_category_id, Some(5));
}

#[test]
fn equal_priorities_break_on_id_so_the_result_is_stable() {
    let a = Rule {
        id: Some(7),
        ..rule("contains", "shop")
    };
    let b = Rule {
        id: Some(3),
        ..rule("contains", "shop")
    };
    let forwards = vec![a.clone(), b.clone()];
    assert_eq!(
        first_match(&forwards, &txn("SHOP", -100)).unwrap().id,
        Some(3)
    );
    // Reversing the input must not change the answer.
    let backwards = vec![b, a];
    assert_eq!(
        first_match(&backwards, &txn("SHOP", -100)).unwrap().id,
        Some(3)
    );
}

#[test]
fn nothing_matching_yields_nothing() {
    assert!(first_match(&[rule("contains", "zzz")], &txn("MERCADONA", -100)).is_none());
}
