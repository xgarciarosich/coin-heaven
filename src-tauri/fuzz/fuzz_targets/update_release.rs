//! The update check: a release page that may say anything, compared with a
//! version the person is running.
//!
//! The input is two version strings, each ending in a NUL, then the release
//! JSON. The page can only ever be read, never obeyed: what comes out is a
//! version to display, a yes or no, and an address that is empty or https.
//! The comparison has to be an order (a version is never newer than itself,
//! two versions are never each newer than the other), or the "update
//! available" banner can be made to flicker or never clear.
#![no_main]
use coin_heaven_fuzz::{Input, json};
use coin_heaven_lib::commands::update::{is_newer, parse_release};
use coin_heaven_lib::error::AppError;
use libfuzzer_sys::fuzz_target;

/// Version strings that mean the same thing under the rules in the module
/// notes: a leading `v`, a pre-release suffix and trailing zeros do not count.
fn order(a: &str, b: &str, c: &str) {
    assert!(!is_newer(a, a), "{a:?} is newer than itself");
    assert!(
        !(is_newer(a, b) && is_newer(b, a)),
        "{a:?} and {b:?} are each newer than the other"
    );
    if is_newer(a, b) && is_newer(b, c) {
        assert!(is_newer(a, c), "{a:?} > {b:?} > {c:?} but not {a:?} > {c:?}");
    }
    // Neither is newer: they are the same version, so a third cannot tell
    // them apart.
    if !is_newer(a, b) && !is_newer(b, a) {
        assert_eq!(is_newer(a, c), is_newer(b, c), "{a:?} and {b:?} differ against {c:?}");
        assert_eq!(is_newer(c, a), is_newer(c, b), "{c:?} differs against {a:?} and {b:?}");
    }
    // A trailing `.0` is no new version. (Only for a string with nothing to
    // trim: after trailing space the `.0` is a part of its own.)
    if a == a.trim() {
        assert_eq!(is_newer(&format!("{a}.0"), b), is_newer(a, b));
    }
    if b == b.trim() {
        assert_eq!(is_newer(a, &format!("{b}.0")), is_newer(a, b));
    }
}

fuzz_target!(|data: &[u8]| {
    let mut inp = Input::new(data);
    let current = inp.text();
    let other = inp.text();
    let third = inp.text();
    order(&current, &other, &third);

    let Some(body) = json(inp.rest()) else { return };
    match parse_release(&body, &current) {
        Err(e) => {
            // The page did not say which version is latest: a network answer
            // that could not be used, and that only.
            assert!(matches!(e, AppError::Network(_)), "{e:?}");
            assert!(body.get("tag_name").and_then(|t| t.as_str()).is_none());
        }
        Ok(info) => {
            assert_eq!(body["tag_name"].as_str(), Some(info.latest.as_str()));
            assert_eq!(info.current, current);
            assert_eq!(info.newer, is_newer(&info.latest, &current));
            assert!(
                info.url.is_empty() || info.url.starts_with("https://"),
                "the page offered {:?} to open",
                info.url
            );
            if !info.url.is_empty() {
                assert_eq!(body["html_url"].as_str(), Some(info.url.as_str()));
            }
        }
    }
});
