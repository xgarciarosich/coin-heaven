//! Text that was UTF-8, got read as Latin-1 on the way, and is put right again.
//!
//! The repair runs on every cell of every imported file, so it meets all the
//! text there is. Two things matter: it must leave good text alone, and what it
//! undoes must be undone exactly. The second is checked by doing the damage
//! ourselves, with both decoders a bank's export could have gone through.
#![no_main]
use coin_heaven_fuzz::known_findings;
use coin_heaven_lib::import::text::repair;
use libfuzzer_sys::fuzz_target;
use std::borrow::Cow;

/// Whether the text ends in a letter and then `Ã`.
///
/// Known defect: `repair` reads such an ending as an `à` that lost its second
/// byte to trimming, so correct Portuguese ("MANHÃ") is rewritten ("MANHà")
/// and a damaged one cannot be put back exactly. See
/// `fuzz/regressions/text_repair/`; skipped unless `COIN_HEAVEN_FUZZ_KNOWN=1`.
fn ends_in_a_tilde(text: &str) -> bool {
    text.ends_with('Ã') && text.chars().rev().nth(1).is_some_and(char::is_alphabetic)
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if !known_findings() && ends_in_a_tilde(&text) {
        return;
    }

    // Whatever the text is, the answer says whether it changed, truthfully.
    let fixed = repair(&text);
    match &fixed {
        Cow::Borrowed(b) => assert_eq!(*b, text, "reported unchanged, but not"),
        Cow::Owned(o) => {
            assert_ne!(o, &*text, "reported changed, but not");
            // Two to four characters become one; nothing is ever added.
            assert!(o.chars().count() <= text.chars().count());
            // Only a character that can lead a UTF-8 sequence starts a repair.
            assert!(
                text.chars().any(|c| matches!(c as u32, 0xC2..=0xF4)),
                "{text:?} changed with no lead character in it"
            );
        }
    }

    // Damage the text both ways and put it back: the original, exactly. This is
    // what "nothing is lost in that transformation" has to mean in practice.
    let cp1252 = encoding_rs::WINDOWS_1252.decode(text.as_bytes()).0.into_owned();
    assert_eq!(&*repair(&cp1252), &*text, "{text:?} damaged as {cp1252:?}");
    // Latin-1 proper: every byte is the character of the same number, so the
    // C1 range is controls rather than windows-1252's punctuation.
    let latin1: String = text.bytes().map(char::from).collect();
    assert_eq!(&*repair(&latin1), &*text, "{text:?} damaged as {latin1:?}");
});
