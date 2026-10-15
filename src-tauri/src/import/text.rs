//! Repairing text that was UTF-8 once and got read as Latin-1 on the way.
//!
//! Several banks — and most fintech exports touched by a spreadsheet on the
//! way out — ship descriptions whose UTF-8 bytes were already decoded as
//! Latin-1 or Windows-1252 and then re-encoded. The file is perfectly valid
//! UTF-8; it just says `inversiÃ³n` where it means `inversión`, and
//! `Ã\u{8e}le-de-France` where it means `Île-de-France`.
//!
//! Nothing is lost in that transformation, so it can be undone exactly: map
//! each character back to the byte it was mistaken for, and decode those bytes
//! as UTF-8. The repair is *local* — it only rewrites a run of characters that
//! forms a complete, valid UTF-8 sequence once turned back into bytes — so
//! correct text next to damaged text is left alone. `TRASPÀS` survives because
//! `À` followed by `S` is not the start of any valid sequence.

use std::borrow::Cow;

/// Windows-1252's 0x80–0x9F block, which Latin-1 leaves as C1 controls.
/// A damaged string can carry either form depending on which decoder did it.
const CP1252_HIGH: [(char, u8); 27] = [
    ('€', 0x80),
    ('‚', 0x82),
    ('ƒ', 0x83),
    ('„', 0x84),
    ('…', 0x85),
    ('†', 0x86),
    ('‡', 0x87),
    ('ˆ', 0x88),
    ('‰', 0x89),
    ('Š', 0x8A),
    ('‹', 0x8B),
    ('Œ', 0x8C),
    ('Ž', 0x8E),
    ('‘', 0x91),
    ('’', 0x92),
    ('“', 0x93),
    ('”', 0x94),
    ('•', 0x95),
    ('–', 0x96),
    ('—', 0x97),
    ('˜', 0x98),
    ('™', 0x99),
    ('š', 0x9A),
    ('›', 0x9B),
    ('œ', 0x9C),
    ('ž', 0x9E),
    ('Ÿ', 0x9F),
];

/// The byte a character would have been, had it come from a single-byte
/// decode of UTF-8. `None` for anything no such decode can produce.
fn as_byte(c: char) -> Option<u8> {
    let n = c as u32;
    if n <= 0xFF {
        return Some(n as u8);
    }
    CP1252_HIGH.iter().find(|(ch, _)| *ch == c).map(|(_, b)| *b)
}

/// How many continuation bytes a UTF-8 lead byte announces.
fn continuation_len(lead: u8) -> Option<usize> {
    match lead {
        0xC2..=0xDF => Some(1),
        0xE0..=0xEF => Some(2),
        0xF0..=0xF4 => Some(3),
        _ => None,
    }
}

/// Undo UTF-8-read-as-Latin-1 damage, wherever it appears in the string.
///
/// Returns the input unchanged — and unallocated — when there is nothing to
/// repair, which is the overwhelmingly common case.
pub fn repair(input: &str) -> Cow<'_, str> {
    // Cheap pre-check: every damaged sequence starts with one of these.
    if !input.chars().any(|c| matches!(c as u32, 0xC2..=0xF4)) {
        return Cow::Borrowed(input);
    }

    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut changed = false;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        let fixed = as_byte(c)
            .and_then(|lead| continuation_len(lead).map(|n| (lead, n)))
            .and_then(|(lead, n)| {
                // The sequence has to fit: characters i+1 ..= i+n must exist.
                if i + n >= chars.len() {
                    return None;
                }
                let mut bytes = vec![lead];
                for k in 1..=n {
                    let b = as_byte(chars[i + k])?;
                    if !(0x80..=0xBF).contains(&b) {
                        return None;
                    }
                    bytes.push(b);
                }
                std::str::from_utf8(&bytes)
                    .ok()
                    .and_then(|s| s.chars().next())
                    .map(|ch| (ch, n))
            });

        match fixed {
            Some((ch, n)) => {
                out.push(ch);
                i += n + 1;
                changed = true;
            }
            None => {
                out.push(c);
                i += 1;
            }
        }
    }

    // A lone `Ã` at the very end is `à` whose second byte, 0xA0, decoded to a
    // non-breaking space and was then trimmed off as whitespace. Only at the
    // end: anywhere else the trim cannot have happened, and a real `Ã` is left.
    if out.ends_with('Ã') && follows_a_letter(&out) {
        out.pop();
        out.push('à');
        changed = true;
    }

    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(input)
    }
}

/// A trailing `Ã` is only reinterpreted when it is preceded by a letter — a
/// word cut off mid-character, not a standalone initial.
fn follows_a_letter(s: &str) -> bool {
    let mut it = s.chars().rev();
    it.next();
    it.next().is_some_and(|c| c.is_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanish_accents_come_back() {
        assert_eq!(repair("A cuenta de inversiÃ³n"), "A cuenta de inversión");
        assert_eq!(
            repair("Rembolso por la cancelaciÃ³n del plan"),
            "Rembolso por la cancelación del plan"
        );
    }

    #[test]
    fn a_c1_control_from_a_latin1_decode_is_understood() {
        // What was actually found in a real ledger: 0x8E kept as U+008E.
        assert_eq!(
            repair("Ã\u{8e}le-de-France MobilitÃ©s"),
            "Île-de-France Mobilités"
        );
    }

    #[test]
    fn the_windows_1252_form_of_the_same_damage_is_understood_too() {
        // The same byte through a 1252 decoder comes out as Ž, not U+008E.
        assert_eq!(repair("ÃŽle"), "Île");
    }

    #[test]
    fn a_non_breaking_space_is_restored_rather_than_left_as_debris() {
        assert_eq!(
            repair("From CuentaÂ\u{a0}Remunerada"),
            "From Cuenta\u{a0}Remunerada"
        );
    }

    #[test]
    fn several_repairs_in_one_string() {
        assert_eq!(repair("CafÃ© du MÃ©tro"), "Café du Métro");
        assert_eq!(repair("BaÃ¯to"), "Baïto");
    }

    #[test]
    fn a_trailing_a_grave_cut_off_by_trimming_is_recovered() {
        assert_eq!(repair("La FelicitÃ"), "La Felicità");
    }

    #[test]
    fn correct_text_is_never_touched() {
        for s in [
            "TRASPÀS PROPI",
            "Mercadona Alborá",
            "Pago en CAFÉ CENTRAL",
            "Ñandú",
            "plain ascii",
            "",
            "€ 12",
        ] {
            assert!(matches!(repair(s), Cow::Borrowed(_)), "{s:?} was rewritten");
        }
    }

    #[test]
    fn a_standalone_capital_a_tilde_is_left_alone() {
        assert_eq!(repair("Ã"), "Ã");
        assert_eq!(repair("Nota Ã"), "Nota Ã");
    }

    #[test]
    fn four_byte_sequences_survive_the_round_trip() {
        // An emoji through the same damage, as a Revolut note can carry.
        let damaged: String = "💶".bytes().map(|b| b as char).collect();
        assert_eq!(repair(&damaged), "💶");
    }

    #[test]
    fn repair_is_idempotent() {
        let once = repair("inversiÃ³n").into_owned();
        assert_eq!(repair(&once), once);
    }
}

/* ------------------------------------------------ repairing stored rows -- */

#[derive(Debug, Default, PartialEq, Eq)]
pub struct StoredRepair {
    pub repaired: usize,
    /// Rows whose text was fixed but whose fingerprint could not be, because
    /// they were edited by hand after import (so the fingerprint no longer
    /// describes their text) or because a correctly-encoded copy of the same
    /// transaction already exists.
    pub fingerprint_kept: usize,
}

/// Undo the damage in transactions that were imported before `repair` existed.
///
/// The text alone is not enough. The dedupe fingerprint is computed from the
/// description, so a row repaired without it would stop matching its own
/// source: re-importing the same statement — now repaired on the way in —
/// would add every accented transaction a second time. So the fingerprint is
/// recomputed too, with the intra-day sequence recovered by finding the one
/// that reproduces the stored hash.
/// id, account, booked_on, amount, description, counterparty, fingerprint.
type StoredRow = (i64, i64, String, i64, String, Option<String>, String);

pub fn repair_stored(c: &rusqlite::Connection) -> rusqlite::Result<StoredRepair> {
    use crate::import::dedupe::dedupe_hash;
    use crate::money::Money;

    let rows: Vec<StoredRow> = {
        let mut stmt = c.prepare(
            "SELECT id, account_id, booked_on, amount_minor, description_raw,
                    counterparty, dedupe_hash
             FROM txn",
        )?;
        stmt.query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })?
        .collect::<Result<_, _>>()?
    };

    let tx = c.unchecked_transaction()?;
    let mut report = StoredRepair::default();

    for (id, account, on, amount, raw, counterparty, stored_hash) in rows {
        let fixed = repair(&raw);
        let fixed_cp = counterparty.as_deref().map(repair);
        let text_changed = matches!(fixed, Cow::Owned(_));
        let cp_changed = matches!(fixed_cp, Some(Cow::Owned(_)));
        if !text_changed && !cp_changed {
            continue;
        }

        let amount = Money::from_minor(amount);
        let sequence =
            (0..256u32).find(|s| dedupe_hash(account, &on, amount, &raw, *s) == stored_hash);
        let new_hash = sequence
            .map(|s| dedupe_hash(account, &on, amount, &fixed, s))
            .filter(|h| {
                // A correctly-encoded twin already holds this fingerprint; the
                // unique index would refuse it, and the row is a duplicate the
                // user should see rather than one to be silently merged.
                !tx.query_row(
                    "SELECT count(*) FROM txn WHERE dedupe_hash = ?1",
                    rusqlite::params![h],
                    |r| r.get::<_, i64>(0),
                )
                .map(|n| n > 0)
                .unwrap_or(true)
            });

        if new_hash.is_none() && text_changed {
            report.fingerprint_kept += 1;
        }
        tx.execute(
            "UPDATE txn SET description_raw = ?1, description_norm = ?2,
                            counterparty = ?3, dedupe_hash = COALESCE(?4, dedupe_hash)
             WHERE id = ?5",
            rusqlite::params![
                fixed.as_ref(),
                crate::security::pii::normalise_description(&fixed),
                fixed_cp.as_ref().map(|c| c.as_ref()),
                new_hash,
                id
            ],
        )?;
        report.repaired += 1;
    }
    tx.commit()?;
    Ok(report)
}

#[cfg(test)]
mod stored {
    use super::*;
    use crate::db::Db;
    use crate::import::dedupe::dedupe_hash;
    use crate::money::Money;
    use secrecy::SecretString;

    fn db() -> (tempfile::TempDir, Db) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(
            &dir.path().join("t.db"),
            &SecretString::from("0".repeat(64)),
        )
        .unwrap();
        db.with(|c| {
            c.execute(
                "INSERT INTO account (id, name, kind, currency) VALUES (1, 'A', 'checking', 'EUR')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        (dir, db)
    }

    fn insert(db: &Db, id: i64, desc: &str, seq: u32) {
        let hash = dedupe_hash(1, "2026-03-01", Money::from_minor(-500), desc, seq);
        db.with(|c| {
            c.execute(
                "INSERT INTO txn (id, account_id, booked_on, description_raw, description_norm,
                                  amount_minor, currency, dedupe_hash)
                 VALUES (?1, 1, '2026-03-01', ?2, ?3, -500, 'EUR', ?4)",
                rusqlite::params![
                    id,
                    desc,
                    crate::security::pii::normalise_description(desc),
                    hash
                ],
            )?;
            Ok(())
        })
        .unwrap();
    }

    fn row(db: &Db, id: i64) -> (String, String, String) {
        db.with(|c| {
            Ok(c.query_row(
                "SELECT description_raw, description_norm, dedupe_hash FROM txn WHERE id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?)
        })
        .unwrap()
    }

    #[test]
    fn a_repaired_row_matches_a_reimport_of_its_own_statement() {
        let (_d, db) = db();
        insert(&db, 1, "CafÃ© du MÃ©tro", 0);
        insert(&db, 2, "CafÃ© du MÃ©tro", 1); // the second coffee that day

        let report = db.with(|c| Ok(repair_stored(c)?)).unwrap();
        assert_eq!(
            report,
            StoredRepair {
                repaired: 2,
                fingerprint_kept: 0
            }
        );

        let (raw, norm, hash) = row(&db, 1);
        assert_eq!(raw, "Café du Métro");
        assert_eq!(norm, "café du métro");
        // Exactly what the importer will now compute for this row.
        assert_eq!(
            hash,
            dedupe_hash(1, "2026-03-01", Money::from_minor(-500), "Café du Métro", 0)
        );
        // And the second coffee kept its own ordinal.
        assert_eq!(
            row(&db, 2).2,
            dedupe_hash(1, "2026-03-01", Money::from_minor(-500), "Café du Métro", 1)
        );
    }

    #[test]
    fn a_row_edited_by_hand_keeps_its_fingerprint_but_gets_its_text_fixed() {
        let (_d, db) = db();
        insert(&db, 1, "original text", 0);
        db.with(|c| {
            c.execute(
                "UPDATE txn SET description_raw = 'inversiÃ³n' WHERE id = 1",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let before = row(&db, 1).2;

        let report = db.with(|c| Ok(repair_stored(c)?)).unwrap();
        assert_eq!(report.fingerprint_kept, 1);
        let (raw, _, hash) = row(&db, 1);
        assert_eq!(raw, "inversión");
        assert_eq!(
            hash, before,
            "no sequence reproduces it, so it is left alone"
        );
    }

    #[test]
    fn a_correctly_encoded_twin_is_not_merged_into() {
        let (_d, db) = db();
        insert(&db, 1, "inversión", 0);
        insert(&db, 2, "inversiÃ³n", 0);

        let report = db.with(|c| Ok(repair_stored(c)?)).unwrap();
        assert_eq!(
            report,
            StoredRepair {
                repaired: 1,
                fingerprint_kept: 1
            }
        );
        assert_eq!(row(&db, 2).0, "inversión", "the text is still fixed");
    }

    #[test]
    fn running_it_twice_changes_nothing_the_second_time() {
        let (_d, db) = db();
        insert(&db, 1, "UGC CinÃ©", 0);
        db.with(|c| Ok(repair_stored(c)?)).unwrap();
        let again = db.with(|c| Ok(repair_stored(c)?)).unwrap();
        assert_eq!(again, StoredRepair::default());
    }

    #[test]
    fn correct_rows_are_not_touched_at_all() {
        let (_d, db) = db();
        insert(&db, 1, "TRASPÀS PROPI", 0);
        let before = row(&db, 1);
        assert_eq!(db.with(|c| Ok(repair_stored(c)?)).unwrap().repaired, 0);
        assert_eq!(row(&db, 1), before);
    }
}
