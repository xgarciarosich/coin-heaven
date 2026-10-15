//! Startup: decide what the app may do before it has a key.
//!
//! Two rules shape this module.
//!
//! **Nothing is created until the user has chosen.** An earlier version
//! generated a key the instant it launched, so the first thing anyone saw was a
//! system password prompt for a decision they had never been offered.
//!
//! **The OS keychain is not involved.** It cost a modal prompt on every launch
//! of an unsigned build — macOS treats each rebuild as a different application
//! — for protection that a local file plus an account password and full-disk
//! encryption already provide. When the user declines a passphrase the key is
//! written to `atlas.localkey`; when they set one, no such file exists.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::security::recovery::{self, Dek, KeyInfo, WrapKind};
use crate::state::AppState;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

/// The app was first built under another identifier, which moved its data
/// folder. A folder next to the new one that holds a database is copied over
/// once, only when the new folder holds none, so nobody loses data to a rename.
/// The old folder is left in place as a backup.
pub fn adopt_previous_data_dir(new_dir: &std::path::Path) -> std::io::Result<bool> {
    const PREVIOUS: &str = "com.atlas.wealth";
    let (Some(parent), true) = (new_dir.parent(), !new_dir.join("atlas.db").exists()) else {
        return Ok(false);
    };
    let old = parent.join(PREVIOUS);
    if old == new_dir || !old.join("atlas.db").is_file() {
        return Ok(false);
    }
    fn copy(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy(&entry.path(), &target)?;
            } else {
                std::fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }
    // Copied beside the destination and renamed into place, so an interruption
    // leaves nothing at the real name: the next start simply tries again,
    // instead of finding a database without its key files and calling it new.
    let staging = parent.join(format!("{}.adopting", file_name(new_dir)));
    let _ = std::fs::remove_dir_all(&staging);
    if let Err(e) = copy(&old, &staging) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(e);
    }
    // An empty folder may already be there (the framework creates it).
    let _ = std::fs::remove_dir(new_dir);
    std::fs::rename(&staging, new_dir)?;
    Ok(true)
}

fn file_name(p: &std::path::Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnlockState {
    /// No database yet. Ask how the user wants it protected, then create it.
    NeedsSetup,
    /// Open and ready.
    Opened,
    /// Protected by a passphrase, which is not cached anywhere. Ask for it.
    NeedsPassphrase,
    /// No passphrase, but the cached key is gone. Fall back to the phrase.
    NeedsRecoveryPhrase,
    /// Created before the envelope scheme. Needs a one-time upgrade.
    NeedsLegacyUpgrade,
}

/// The v1 sidecar: the key was Argon2id(recovery phrase) itself, so exactly one
/// secret could ever open the database.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyKeyInfoV1 {
    version: u32,
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
    salt_b64: String,
}

fn load_legacy(state: &AppState) -> Option<LegacyKeyInfoV1> {
    let raw = std::fs::read_to_string(state.data_dir.join(recovery::KEYINFO_FILENAME)).ok()?;
    let v: LegacyKeyInfoV1 = serde_json::from_str(&raw).ok()?;
    (v.version == 1).then_some(v)
}

/// Work out where we stand, without creating or unlocking anything the user
/// has not asked for.
pub fn auto_unlock(state: &AppState) -> AppResult<UnlockState> {
    let info = match KeyInfo::load(&state.data_dir) {
        Ok(Some(info)) => info,
        Ok(None) => return Ok(UnlockState::NeedsSetup),
        // A parse failure is most likely a v1 sidecar, which has no `wraps`.
        Err(e) => {
            return if load_legacy(state).is_some() {
                Ok(UnlockState::NeedsLegacyUpgrade)
            } else {
                Err(e)
            };
        }
    };

    // A passphrase only means anything if the key is not sitting in the
    // keychain beside it, so that combination is never cached.
    if info.has_passphrase() {
        // A key file left from before the passphrase was set (a crash between
        // the two steps) would make the passphrase protect nothing.
        let _ = recovery::forget_local_key(&state.data_dir);
        return Ok(UnlockState::NeedsPassphrase);
    }

    // No passphrase: the key should be sitting beside the database. If it is
    // missing or does not belong to this database — a half-restored backup,
    // say — fall back to the recovery phrase rather than looking broken.
    match recovery::load_local_key(&state.data_dir) {
        Ok(Some(dek)) if info.matches(&dek) => {
            open(state, &dek)?;
            Ok(UnlockState::Opened)
        }
        Ok(_) => Ok(UnlockState::NeedsRecoveryPhrase),
        Err(e) => {
            tracing::warn!("local key unreadable, falling back to the recovery phrase: {e}");
            Ok(UnlockState::NeedsRecoveryPhrase)
        }
    }
}

/// First run. `passphrase` of `None` means "no passphrase" — an explicit
/// choice the user made, not a default we picked for them.
pub fn setup(state: &AppState, passphrase: Option<&str>) -> AppResult<UnlockState> {
    if KeyInfo::load(&state.data_dir).ok().flatten().is_some() {
        return Err(AppError::Invalid(
            "this database has already been set up".into(),
        ));
    }
    // A database that is already here has keys this call would replace, and
    // the data would be unreachable for good. Starting fresh is deliberate:
    // delete the files first (the setup screen says how).
    if std::fs::metadata(&state.db_path).is_ok_and(|m| m.len() > 0) {
        return Err(AppError::Invalid(
            "a database already exists here but its key info is missing or unreadable; \
             restore atlas.keyinfo from a backup folder"
                .into(),
        ));
    }
    let created = recovery::initialise(&state.data_dir, passphrase)?;
    open(state, &created.dek)?;

    // With a passphrase there is no key on disk at all; without one, the key
    // is written beside the database so the app opens straight away.
    if passphrase.is_none() {
        store_local(state, &created.dek);
    }

    state.stash_phrase(created.phrase);
    Ok(UnlockState::Opened)
}

pub fn unlock_with_passphrase(state: &AppState, passphrase: &str) -> AppResult<UnlockState> {
    if state.is_open() {
        return Ok(UnlockState::Opened);
    }
    let info = require_info(state)?;
    let dek = info.unlock(passphrase, WrapKind::Passphrase)?;
    open(state, &dek)?;
    Ok(UnlockState::Opened)
}

pub fn unlock_with_phrase(state: &AppState, phrase: &str) -> AppResult<UnlockState> {
    if state.is_open() {
        return Ok(UnlockState::Opened);
    }
    recovery::validate_phrase(phrase)?;
    let info = require_info(state)?;
    let dek = info.unlock(phrase, WrapKind::Recovery)?;
    open(state, &dek)?;
    // Recovering means the local key was missing; put it back, unless a
    // passphrase is in play, in which case storing it would defeat the point.
    if !info.has_passphrase() {
        store_local(state, &dek);
    }
    Ok(UnlockState::Opened)
}

/// One-time upgrade of a v1 database onto the envelope scheme.
///
/// Needs the recovery phrase, because under v1 that phrase *was* the key —
/// there is nothing else that can open the file.
pub fn upgrade_legacy(state: &AppState, phrase: &str) -> AppResult<UnlockState> {
    recovery::validate_phrase(phrase)?;
    let legacy = load_legacy(state)
        .ok_or_else(|| AppError::Other("no legacy key info to upgrade".into()))?;
    let salt = base64_decode(&legacy.salt_b64)?;

    let old_key = legacy_derive(
        phrase,
        &salt,
        legacy.m_cost_kib,
        legacy.t_cost,
        legacy.p_cost,
    )?;
    let new_dek = recovery::generate_dek();
    let new_key = recovery::dek_to_hex(&new_dek);

    // The new key info is written to a sibling first, so that a full disk or a
    // read-only folder fails here — before any page of the database has been
    // touched — and the old, still-working pair is left exactly as it was.
    // Only then are the pages rewritten, and the sidecar renamed into place.
    let wrap = recovery::wrap_dek(&new_dek, phrase, WrapKind::Recovery)?;
    let info = KeyInfo {
        version: 2,
        kdf: "argon2id".into(),
        wraps: vec![wrap],
        verifier_b64: recovery::verifier_b64(&new_dek),
    };
    let staged = state
        .data_dir
        .join(format!("{}.upgrade", recovery::KEYINFO_FILENAME));
    info.save_as(&staged)?;
    if let Err(e) = Db::rekey(&state.db_path, &old_key, &new_key) {
        let _ = std::fs::remove_file(&staged);
        return Err(e);
    }
    std::fs::rename(&staged, state.data_dir.join(recovery::KEYINFO_FILENAME)).map_err(|e| {
        AppError::Other(format!("cannot write {}: {e}", recovery::KEYINFO_FILENAME))
    })?;

    open(state, &new_dek)?;
    store_local(state, &new_dek);
    Ok(UnlockState::Opened)
}

/* ------------------------------------------------------------ helpers -- */

fn require_info(state: &AppState) -> AppResult<KeyInfo> {
    KeyInfo::load(&state.data_dir)?.ok_or_else(|| {
        AppError::Invalid(format!(
            "no {} found next to the database. Restore it from a backup folder — \
             it is written alongside every backup.",
            recovery::KEYINFO_FILENAME
        ))
    })
}

fn open(state: &AppState, dek: &Dek) -> AppResult<()> {
    let db = Db::open(&state.db_path, &recovery::dek_to_hex(dek))?;
    // The language is known as soon as the books are open, so anything said
    // from here on — including an error on the very next command — is in it.
    if let Ok(Some(lang)) = db.with(|c| {
        Ok(c.query_row(
            "SELECT json_extract(value_json, '$') FROM app_setting WHERE key = 'ui.language'",
            [],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap_or(None))
    }) {
        crate::i18n::set(&lang);
    }
    repair_text_once(&db);
    state.set_db(db)?;
    state.remember_dek(dek.clone());
    Ok(())
}

/// Undo encoding damage in rows imported before the importer learned to.
///
/// Once per database, recorded in `app_setting`, and best effort: a failure
/// here leaves the text as it was, which is how it has always been, rather than
/// refusing to open the books.
fn repair_text_once(db: &Db) {
    const KEY: &str = "repair.mojibake.v1";
    let outcome = db.with(|c| {
        let done: i64 = c.query_row(
            "SELECT count(*) FROM app_setting WHERE key = ?1",
            rusqlite::params![KEY],
            |r| r.get(0),
        )?;
        if done > 0 {
            return Ok(None);
        }
        let report = crate::import::text::repair_stored(c)?;
        c.execute(
            "INSERT INTO app_setting (key, value_json) VALUES (?1, ?2)",
            rusqlite::params![
                KEY,
                format!(
                    "{{\"repaired\":{},\"fingerprintKept\":{}}}",
                    report.repaired, report.fingerprint_kept
                )
            ],
        )?;
        Ok(Some(report))
    });
    match outcome {
        Ok(Some(r)) if r.repaired > 0 => tracing::info!(
            "repaired encoding in {} transaction(s); {} kept their fingerprint",
            r.repaired,
            r.fingerprint_kept
        ),
        Ok(_) => {}
        Err(e) => tracing::warn!("text repair skipped: {e}"),
    }
}

/// Writing the local key is best effort: if it fails the app still runs, and
/// the next launch falls back to the recovery phrase rather than to nothing.
fn store_local(state: &AppState, dek: &Dek) {
    if let Err(e) = recovery::save_local_key(&state.data_dir, dek) {
        tracing::warn!("could not store the database key beside the database: {e}");
    }
}

fn base64_decode(s: &str) -> AppResult<Vec<u8>> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| AppError::Other(format!("legacy key info is invalid: {e}")))
}

/// v1 derivation, kept only so existing databases can be read once.
fn legacy_derive(
    phrase: &str,
    salt: &[u8],
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
) -> AppResult<SecretString> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let params = Params::new(m_cost_kib, t_cost, p_cost, Some(32))
        .map_err(|e| AppError::Other(format!("bad Argon2 parameters: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = zeroize::Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(
            recovery::normalise_phrase(phrase).as_bytes(),
            salt,
            out.as_mut_slice(),
        )
        .map_err(|e| AppError::Other(format!("key derivation failed: {e}")))?;
    Ok(SecretString::from(
        out.iter().map(|b| format!("{b:02x}")).collect::<String>(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::recovery::{KeyInfo, WrapKind};
    use secrecy::ExposeSecret;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(dir.path().to_path_buf());
        (dir, state)
    }

    #[test]
    fn a_fresh_install_asks_before_creating_anything() {
        let (dir, st) = state();
        assert_eq!(auto_unlock(&st).unwrap(), UnlockState::NeedsSetup);
        // The point of the state: nothing on disk, nothing in the keychain.
        assert!(!dir.path().join(recovery::KEYINFO_FILENAME).exists());
        assert!(!st.db_path.exists());
        assert!(!st.is_open());
    }

    #[test]
    fn setup_without_a_passphrase_writes_the_key_beside_the_database() {
        let (dir, st) = state();
        setup(&st, None).unwrap();
        let key_file = dir.path().join(recovery::LOCALKEY_FILENAME);
        assert!(
            key_file.exists(),
            "no passphrase means the key lives on disk"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&key_file).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "the key file must be owner-only");
        }

        // Reopening needs no secret from the user at all.
        let next = AppState::new(st.data_dir.clone());
        assert_eq!(auto_unlock(&next).unwrap(), UnlockState::Opened);
    }

    #[test]
    fn setup_with_a_passphrase_leaves_no_key_on_disk() {
        let (dir, st) = state();
        setup(&st, Some("seven purple lanterns")).unwrap();
        assert!(
            !dir.path().join(recovery::LOCALKEY_FILENAME).exists(),
            "a stored key would let anyone step around the passphrase"
        );
    }

    #[test]
    fn a_missing_local_key_falls_back_to_the_recovery_phrase() {
        let (dir, st) = state();
        setup(&st, None).unwrap();
        std::fs::remove_file(dir.path().join(recovery::LOCALKEY_FILENAME)).unwrap();

        let next = AppState::new(st.data_dir.clone());
        assert_eq!(
            auto_unlock(&next).unwrap(),
            UnlockState::NeedsRecoveryPhrase
        );
    }

    #[test]
    fn recovering_restores_the_local_key_so_the_next_launch_is_automatic() {
        let (dir, st) = state();
        setup(&st, None).unwrap();
        let phrase = st.take_phrase().unwrap();
        std::fs::remove_file(dir.path().join(recovery::LOCALKEY_FILENAME)).unwrap();

        let back = AppState::new(st.data_dir.clone());
        unlock_with_phrase(&back, phrase.expose_secret()).unwrap();
        assert!(dir.path().join(recovery::LOCALKEY_FILENAME).exists());
    }

    #[test]
    fn setup_without_a_passphrase_opens_immediately() {
        let (_dir, st) = state();
        assert_eq!(setup(&st, None).unwrap(), UnlockState::Opened);
        assert!(st.is_open());
        assert!(st.has_pending_phrase(), "the phrase must be shown once");

        let info = KeyInfo::load(&st.data_dir).unwrap().unwrap();
        assert!(!info.has_passphrase());
    }

    #[test]
    fn setup_with_a_passphrase_records_a_passphrase_wrap() {
        let (_dir, st) = state();
        setup(&st, Some("seven purple lanterns")).unwrap();

        let info = KeyInfo::load(&st.data_dir).unwrap().unwrap();
        assert!(info.has_passphrase());
        assert!(info.wrap_for(WrapKind::Recovery).is_some());
    }

    #[test]
    fn setting_up_twice_is_refused() {
        let (_dir, st) = state();
        setup(&st, None).unwrap();
        let second = AppState::new(st.data_dir.clone());
        let err = setup(&second, None).unwrap_err();
        assert!(
            err.to_string().contains("already been set up"),
            "got: {err}"
        );
    }

    #[test]
    fn a_passphrase_database_reports_that_it_needs_one() {
        let (_dir, st) = state();
        setup(&st, Some("seven purple lanterns")).unwrap();

        // A second launch: fresh state, same folder.
        let next = AppState::new(st.data_dir.clone());
        assert_eq!(auto_unlock(&next).unwrap(), UnlockState::NeedsPassphrase);
        assert!(
            !next.is_open(),
            "it must stay locked until the passphrase arrives"
        );
    }

    #[test]
    fn the_passphrase_opens_it_and_a_wrong_one_does_not() {
        let (_dir, st) = state();
        setup(&st, Some("seven purple lanterns")).unwrap();

        let bad = AppState::new(st.data_dir.clone());
        assert!(unlock_with_passphrase(&bad, "wrong").is_err());
        assert!(!bad.is_open());

        let good = AppState::new(st.data_dir.clone());
        assert_eq!(
            unlock_with_passphrase(&good, "seven purple lanterns").unwrap(),
            UnlockState::Opened
        );
        assert!(good.is_open());
    }

    #[test]
    fn the_recovery_phrase_opens_a_passphrase_protected_database() {
        // Forgetting the passphrase must be survivable.
        let (_dir, st) = state();
        setup(&st, Some("seven purple lanterns")).unwrap();
        let phrase = st.take_phrase().unwrap();

        let next = AppState::new(st.data_dir.clone());
        assert_eq!(
            unlock_with_phrase(&next, phrase.expose_secret()).unwrap(),
            UnlockState::Opened
        );
    }

    #[test]
    fn a_legacy_v1_database_is_detected_and_upgraded_in_place() {
        let (dir, st) = state();

        // Build a v1 world by hand: key derived straight from the phrase.
        let phrase = recovery::generate_phrase().unwrap();
        let salt = [7u8; 16];
        let old_key = legacy_derive(phrase.expose_secret(), &salt, 65_536, 3, 1).unwrap();
        {
            let db = Db::open(&st.db_path, &old_key).unwrap();
            db.with(|c| {
                c.execute(
                    "INSERT INTO account (name, kind) VALUES ('Legacy Account', 'checking')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        }
        let v1 = serde_json::json!({
            "version": 1, "kdf": "argon2id",
            "mCostKib": 65_536, "tCost": 3, "pCost": 1,
            "saltB64": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD, salt),
            "verifierB64": "unused",
        });
        std::fs::write(
            dir.path().join(recovery::KEYINFO_FILENAME),
            serde_json::to_string_pretty(&v1).unwrap(),
        )
        .unwrap();

        // It must be recognised rather than reported as corrupt.
        let detect = AppState::new(st.data_dir.clone());
        assert_eq!(
            auto_unlock(&detect).unwrap(),
            UnlockState::NeedsLegacyUpgrade
        );

        // Upgrading rewrites the file under a new random key...
        let up = AppState::new(st.data_dir.clone());
        assert_eq!(
            upgrade_legacy(&up, phrase.expose_secret()).unwrap(),
            UnlockState::Opened
        );

        // ...and the data survives it.
        let name: String = up
            .db()
            .unwrap()
            .with(|c| Ok(c.query_row("SELECT name FROM account", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(name, "Legacy Account");

        let info = KeyInfo::load(&up.data_dir).unwrap().unwrap();
        assert_eq!(info.version, 2);
        assert!(!info.has_passphrase());

        // The old key must no longer open the file.
        assert!(Db::open(&up.db_path, &old_key).is_err());
    }

    #[test]
    fn upgrading_with_the_wrong_phrase_leaves_the_database_alone() {
        let (dir, st) = state();
        let phrase = recovery::generate_phrase().unwrap();
        let salt = [9u8; 16];
        let old_key = legacy_derive(phrase.expose_secret(), &salt, 65_536, 3, 1).unwrap();
        drop(Db::open(&st.db_path, &old_key).unwrap());

        let v1 = serde_json::json!({
            "version": 1, "kdf": "argon2id",
            "mCostKib": 65_536, "tCost": 3, "pCost": 1,
            "saltB64": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD, salt),
            "verifierB64": "unused",
        });
        std::fs::write(
            dir.path().join(recovery::KEYINFO_FILENAME),
            serde_json::to_string(&v1).unwrap(),
        )
        .unwrap();

        let other = recovery::generate_phrase().unwrap();
        let up = AppState::new(st.data_dir.clone());
        assert!(upgrade_legacy(&up, other.expose_secret()).is_err());

        // Still openable with the original key: nothing was rewritten.
        assert!(Db::open(&st.db_path, &old_key).is_ok());
    }

    #[test]
    fn data_from_the_previous_identifier_is_copied_once() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("com.atlas.wealth");
        std::fs::create_dir_all(old.join("backups")).unwrap();
        std::fs::write(old.join("atlas.db"), b"db").unwrap();
        std::fs::write(old.join("backups").join("b.enc"), b"b").unwrap();
        let new = root.path().join("com.coinheaven.app");
        assert!(adopt_previous_data_dir(&new).unwrap());
        assert_eq!(std::fs::read(new.join("atlas.db")).unwrap(), b"db");
        assert!(new.join("backups").join("b.enc").is_file());
        assert!(
            old.join("atlas.db").is_file(),
            "the old folder stays as a backup"
        );
        std::fs::write(new.join("atlas.db"), b"changed").unwrap();
        assert!(!adopt_previous_data_dir(&new).unwrap(), "never overwrites");
        assert_eq!(std::fs::read(new.join("atlas.db")).unwrap(), b"changed");
    }
}
