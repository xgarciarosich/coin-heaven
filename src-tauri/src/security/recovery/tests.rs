use super::*;

const PASS: &str = "seven purple lanterns";

fn dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

/* --------------------------------------------------------------- setup -- */

#[test]
fn creating_without_a_passphrase_leaves_only_a_recovery_wrap() {
    let d = dir();
    let created = initialise(d.path(), None).unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();

    assert_eq!(info.version, 2);
    assert!(!info.has_passphrase());
    assert_eq!(info.wraps.len(), 1);
    assert!(info.matches(&created.dek));
}

#[test]
fn both_secrets_unwrap_the_very_same_database_key() {
    // The whole point of the envelope: two doors, one key behind them.
    let d = dir();
    let created = initialise(d.path(), Some(PASS)).unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();

    let by_pass = info.unlock(PASS, WrapKind::Passphrase).unwrap();
    let by_phrase = info
        .unlock(created.phrase.expose_secret(), WrapKind::Recovery)
        .unwrap();

    assert_eq!(by_pass.as_slice(), created.dek.as_slice());
    assert_eq!(by_phrase.as_slice(), created.dek.as_slice());
}

#[test]
fn a_short_passphrase_is_refused_at_creation() {
    let d = dir();
    let err = initialise(d.path(), Some("abc")).unwrap_err();
    assert!(err.to_string().contains("at least"), "got: {err}");
}

/* -------------------------------------------------------------- unlock -- */

#[test]
fn the_wrong_passphrase_is_rejected_with_a_useful_message() {
    let d = dir();
    initialise(d.path(), Some(PASS)).unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();

    let err = info
        .unlock("not the passphrase", WrapKind::Passphrase)
        .unwrap_err();
    assert!(
        err.to_string().contains("passphrase is not correct"),
        "got: {err}"
    );
}

#[test]
fn a_different_valid_recovery_phrase_is_rejected() {
    let d = dir();
    initialise(d.path(), None).unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();
    let other = generate_phrase().unwrap();

    let err = info
        .unlock(other.expose_secret(), WrapKind::Recovery)
        .unwrap_err();
    assert!(err.to_string().contains("does not match"), "got: {err}");
}

#[test]
fn asking_for_a_passphrase_that_was_never_set_says_so() {
    let d = dir();
    initialise(d.path(), None).unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();

    let err = info.unlock(PASS, WrapKind::Passphrase).unwrap_err();
    assert!(
        err.to_string().contains("does not use a passphrase"),
        "got: {err}"
    );
}

#[test]
fn transcription_noise_in_the_recovery_phrase_is_tolerated() {
    let d = dir();
    let created = initialise(d.path(), None).unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();

    let messy = format!(
        "  {}  ",
        created
            .phrase
            .expose_secret()
            .to_uppercase()
            .replace(' ', "   ")
    );
    let dek = info.unlock(&messy, WrapKind::Recovery).unwrap();
    assert_eq!(dek.as_slice(), created.dek.as_slice());
}

#[test]
fn passphrase_case_is_significant() {
    // Unlike the recovery phrase, a passphrase is not folded — doing so would
    // throw away entropy the user deliberately chose.
    let d = dir();
    initialise(d.path(), Some("Correct Horse Battery")).unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();

    assert!(
        info.unlock("correct horse battery", WrapKind::Passphrase)
            .is_err()
    );
    assert!(
        info.unlock("Correct Horse Battery", WrapKind::Passphrase)
            .is_ok()
    );
}

/* ------------------------------------------------------- rewrapping ---- */

#[test]
fn adding_a_passphrase_later_keeps_the_same_database_key() {
    // If the DEK changed, every byte already written would become unreadable.
    let d = dir();
    let created = initialise(d.path(), None).unwrap();

    set_passphrase(d.path(), &created.dek, PASS).unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();

    assert!(info.has_passphrase());
    assert!(info.matches(&created.dek));
    let dek = info.unlock(PASS, WrapKind::Passphrase).unwrap();
    assert_eq!(dek.as_slice(), created.dek.as_slice());
}

#[test]
fn changing_the_passphrase_replaces_the_wrap_rather_than_adding_one() {
    let d = dir();
    let created = initialise(d.path(), Some(PASS)).unwrap();

    set_passphrase(d.path(), &created.dek, "a different passphrase").unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();

    assert_eq!(
        info.wraps
            .iter()
            .filter(|w| w.kind == WrapKind::Passphrase)
            .count(),
        1
    );
    assert!(info.unlock(PASS, WrapKind::Passphrase).is_err());
    assert!(
        info.unlock("a different passphrase", WrapKind::Passphrase)
            .is_ok()
    );
}

#[test]
fn removing_the_passphrase_leaves_the_recovery_path_intact() {
    let d = dir();
    let created = initialise(d.path(), Some(PASS)).unwrap();

    remove_passphrase(d.path()).unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();

    assert!(!info.has_passphrase());
    let dek = info
        .unlock(created.phrase.expose_secret(), WrapKind::Recovery)
        .unwrap();
    assert_eq!(dek.as_slice(), created.dek.as_slice());
}

#[test]
fn rewrapping_with_a_foreign_key_is_refused() {
    // Guards against a bug that would silently orphan the database.
    let d = dir();
    initialise(d.path(), None).unwrap();
    let foreign = generate_dek();

    let err = set_passphrase(d.path(), &foreign, PASS).unwrap_err();
    assert!(err.to_string().contains("does not belong"), "got: {err}");
}

/* ------------------------------------------------------------ secrecy -- */

#[test]
fn the_sidecar_leaks_neither_the_key_nor_either_secret() {
    let d = dir();
    let created = initialise(d.path(), Some(PASS)).unwrap();
    let raw = std::fs::read_to_string(d.path().join(KEYINFO_FILENAME)).unwrap();

    assert!(!raw.contains(PASS), "the passphrase leaked");
    assert!(
        !raw.contains(dek_to_hex(&created.dek).expose_secret()),
        "the database key leaked"
    );
    assert!(
        !raw.contains(created.phrase.expose_secret()),
        "the recovery phrase leaked"
    );

    // No phrase word may be stored as a *value*. Field names are not values:
    // two words of the wordlist ("version", "kind") are also names in this
    // file, and a phrase that happened to contain one used to fail this test
    // about one run in seventy without anything having leaked.
    fn values(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Array(a) => a.iter().for_each(|x| values(x, out)),
            serde_json::Value::Object(m) => m.values().for_each(|x| values(x, out)),
            _ => {}
        }
    }
    let mut stored = Vec::new();
    values(&serde_json::from_str(&raw).unwrap(), &mut stored);
    assert!(!stored.is_empty(), "the file holds values to check");
    for word in created.phrase.expose_secret().split_whitespace() {
        assert!(
            !stored.iter().any(|v| v == word),
            "a phrase word leaked verbatim"
        );
    }
}

/// The failure this test used to have, shown directly rather than waited for:
/// the file's own field names are wordlist words, and that is not a leak.
#[test]
fn a_wordlist_word_that_is_also_a_field_name_is_not_a_leak() {
    let d = dir();
    initialise(d.path(), Some(PASS)).unwrap();
    let raw = std::fs::read_to_string(d.path().join(KEYINFO_FILENAME)).unwrap();
    let words: std::collections::HashSet<&str> = bip39::Language::English
        .word_list()
        .iter()
        .copied()
        .collect();
    let names: Vec<&str> = ["version", "kind"]
        .into_iter()
        .filter(|n| raw.contains(&format!("\"{n}\"")))
        .collect();
    assert_eq!(names.len(), 2, "the file has both as field names");
    assert!(
        names.iter().all(|n| words.contains(n)),
        "and both are words a recovery phrase can contain"
    );
}

#[test]
fn tampering_with_a_wrap_is_detected_rather_than_yielding_a_wrong_key() {
    let d = dir();
    initialise(d.path(), Some(PASS)).unwrap();
    let mut info = KeyInfo::load(d.path()).unwrap().unwrap();

    // Flip a byte of the ciphertext. AEAD authentication must catch it.
    let wrap = info
        .wraps
        .iter_mut()
        .find(|w| w.kind == WrapKind::Passphrase)
        .unwrap();
    let mut ct = B64.decode(&wrap.ciphertext_b64).unwrap();
    ct[0] ^= 0xff;
    wrap.ciphertext_b64 = B64.encode(&ct);

    assert!(
        unwrap_dek(wrap, PASS).unwrap().is_none(),
        "tampering went undetected"
    );
}

#[test]
fn every_wrap_uses_a_fresh_salt_and_nonce() {
    let dek = generate_dek();
    let a = wrap_dek(&dek, PASS, WrapKind::Passphrase).unwrap();
    let b = wrap_dek(&dek, PASS, WrapKind::Passphrase).unwrap();

    assert_ne!(a.salt_b64, b.salt_b64);
    assert_ne!(a.nonce_b64, b.nonce_b64);
    assert_ne!(a.ciphertext_b64, b.ciphertext_b64, "nonce reuse");
}

#[test]
fn generated_keys_are_thirty_two_bytes_and_unique() {
    let a = generate_dek();
    let b = generate_dek();
    assert_eq!(a.len(), 32);
    assert_ne!(a.as_slice(), b.as_slice());
    assert_eq!(dek_to_hex(&a).expose_secret().len(), 64);
}

/* ------------------------------------------------------------ phrases -- */

#[test]
fn generates_twelve_valid_words() {
    let p = generate_phrase().unwrap();
    assert_eq!(p.expose_secret().split_whitespace().count(), 12);
    validate_phrase(p.expose_secret()).unwrap();
}

#[test]
fn rejects_wrong_length_and_bad_checksums() {
    assert!(validate_phrase("abandon abandon abandon").is_err());
    // In the wordlist, but fails the BIP-39 checksum.
    let twelve = "abandon ".repeat(11) + "abandon";
    assert!(validate_phrase(&twelve).is_err());
}

/* ----------------------------------------------------------- strength -- */

#[test]
fn passphrase_strength_blocks_only_on_length() {
    let short = rate_passphrase("abc");
    assert!(!short.acceptable);
    assert!(short.message.contains("8"));

    // Weak but long enough: advised against, not blocked. A passphrase the
    // user cannot remember is not a security win.
    let weak = rate_passphrase("password123");
    assert!(weak.acceptable);
    assert!(weak.score <= 2, "a weak passphrase rated too high");

    let strong = rate_passphrase("seven purple lanterns drift");
    assert!(strong.acceptable);
    assert!(strong.score >= 3, "a strong passphrase rated too low");
}

#[test]
fn key_files_are_private_from_the_first_byte_and_replaced_whole() {
    let d = dir();
    let path = d.path().join("k");
    write_private(&path, b"first").unwrap();
    write_private(&path, b"second").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"second");
    assert!(!d.path().join("k.tmp").exists(), "no temporary left behind");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn a_tampered_key_file_cannot_ask_for_unreasonable_work() {
    let d = dir();
    initialise(d.path(), Some(PASS)).unwrap();
    let mut info = KeyInfo::load(d.path()).unwrap().unwrap();
    for w in &mut info.wraps {
        w.m_cost_kib = u32::MAX;
    }
    info.save(d.path()).unwrap();
    let info = KeyInfo::load(d.path()).unwrap().unwrap();
    assert!(info.unlock(PASS, WrapKind::Passphrase).is_err());
}
