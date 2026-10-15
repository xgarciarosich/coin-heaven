use super::*;

#[test]
fn the_catalogue_is_well_formed() {
    let all = catalogue();
    assert!(all.len() >= 15);
    for f in &all {
        assert!(
            f.pattern.contains('%'),
            "{} is not a strftime pattern",
            f.pattern
        );
        assert!(!f.label.is_empty());
        assert!(!f.group.is_empty());
        // Every advertised example must actually parse under its own pattern,
        // or the dropdown is lying to the user.
        assert!(
            parse_strict(f.example, f.pattern).is_some(),
            "example {:?} does not parse as {}",
            f.example,
            f.pattern
        );
        assert_eq!(
            f.has_time,
            f.pattern.contains("%H") || f.pattern.contains("%I"),
            "has_time is wrong for {}",
            f.pattern
        );
    }
}

#[test]
fn patterns_are_unique() {
    let mut seen = std::collections::HashSet::new();
    for p in patterns() {
        assert!(seen.insert(p), "duplicate pattern {p}");
    }
}

#[test]
fn iso_is_tried_before_the_ambiguous_ones() {
    // 2026-03-04 must never be read as a day-first format.
    assert_eq!(detect("2026-03-04"), Some("%Y-%m-%d"));
}

#[test]
fn day_first_wins_over_month_first_when_both_parse() {
    // 04/03/2026 is valid under either; Europe is the default lean, and the
    // detector raises a warning separately when a file cannot settle it.
    assert_eq!(detect("04/03/2026"), Some("%d/%m/%Y"));
}

#[test]
fn a_day_above_twelve_only_parses_one_way() {
    assert_eq!(detect("25/03/2026"), Some("%d/%m/%Y"));
    assert_eq!(parse_strict("25/03/2026", "%m/%d/%Y"), None);
}

#[test]
fn values_carrying_a_time_are_recognised() {
    for (value, pattern) in [
        ("2026-03-04T14:30:00", "%Y-%m-%dT%H:%M:%S"),
        ("2026-03-04 14:30:00", "%Y-%m-%d %H:%M:%S"),
        ("04/03/2026 14:30", "%d/%m/%Y %H:%M"),
    ] {
        assert_eq!(detect(value), Some(pattern), "for {value}");
        assert_eq!(
            parse_strict(value, pattern).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 4).unwrap(),
            "the time should be dropped, not the date"
        );
    }
}

#[test]
fn a_twelve_hour_clock_parses() {
    let d = parse_strict("03/04/2026 02:30 PM", "%m/%d/%Y %I:%M %p").unwrap();
    assert_eq!(d, NaiveDate::from_ymd_opt(2026, 3, 4).unwrap());
}

#[test]
fn a_named_month_parses_in_both_conventions() {
    assert_eq!(
        parse_strict("04 Mar 2026", "%d %b %Y").unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()
    );
    assert_eq!(
        parse_strict("Mar 04, 2026", "%b %d, %Y").unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()
    );
}

#[test]
fn lenient_parsing_rescues_a_time_the_chosen_format_did_not_mention() {
    // The commonest hand-picked mismatch: the user chose a date format and the
    // column turns out to carry a timestamp.
    assert_eq!(parse_strict("2026-03-04 14:30:00", "%Y-%m-%d"), None);
    assert_eq!(
        parse_lenient("2026-03-04 14:30:00", "%Y-%m-%d").unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()
    );
    assert_eq!(
        parse_lenient("2026-03-04T14:30:00Z", "%Y-%m-%d").unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 4).unwrap()
    );
}

#[test]
fn lenient_parsing_does_not_invent_a_date_from_nonsense() {
    assert_eq!(parse_lenient("MERCADONA", "%d/%m/%Y"), None);
    assert_eq!(parse_lenient("", "%d/%m/%Y"), None);
    assert_eq!(parse_lenient("45,20", "%d/%m/%Y"), None);
    // A real date under the wrong convention must still fail rather than
    // silently swapping day and month.
    assert_eq!(parse_lenient("25/03/2026", "%m/%d/%Y"), None);
}

#[test]
fn short_junk_is_rejected_before_it_reaches_a_parser() {
    assert_eq!(detect("12"), None);
    assert_eq!(detect("abc"), None);
}

#[test]
fn every_catalogue_entry_is_reachable_by_detection() {
    // Guards against an entry that can never win because an earlier pattern
    // always swallows its examples — a dropdown option that does nothing.
    for f in catalogue() {
        let got = detect(f.example).expect("example should be detected");
        assert!(
            parse_strict(f.example, got).is_some(),
            "{} detected as {got}, which does not parse it",
            f.pattern
        );
    }
}
