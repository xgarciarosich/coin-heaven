//! Security regression tests for the key hierarchy, the encrypted database and
//! the backups, written against the public library API only.
//!
//! What each section pins down:
//!
//!   1. the database file is ciphertext, and a wrong key gets nothing;
//!   2. `atlas.keyinfo` fails closed when it is damaged or edited;
//!   3. recovery phrases are random and checked;
//!   4. a backup opens with the live key, detects damage, and retention only
//!      ever deletes Coin Heaven's own files;
//!   5. file permissions;
//!   6. adopting the previous data folder;
//!   7. setting and removing a passphrase;
//!   8. passphrase strength, and secrets staying out of output.
//!
//! A test marked `#[ignore = "FINDING: ..."]` asserts the behaviour that
//! should hold and currently does not. They stay in the file so the defect is
//! reproducible and the test is already there when it is fixed:
//!
//! ```text
//! cargo test --test security_crypto_backup -- --ignored
//! ```
//!
//! ## Speed
//!
//! Argon2 at production cost (64 MiB, three passes) is about 1.5 s per
//! derivation in a debug build. Tests whose subject is not the KDF cost build
//! their key info with `CHEAP_COST` through [`forge_wrap`], which is an
//! independent implementation of the documented wrap format. The library
//! honours the cost recorded in each wrap, so the code under test is the same
//! code; and `wraps_made_by_the_library_follow_the_documented_format` ties the
//! two implementations together at production cost.
//!
//! The Tauri commands cannot be called without the runtime. Where a test needs
//! a command's sequence it repeats its library calls, named after the command.
#![cfg_attr(not(unix), allow(dead_code, unused_imports))]

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use coin_heaven_lib::backup::{BackupOutcome, BackupPolicy, BackupScheduler, Trigger};
use coin_heaven_lib::bootstrap::{self, UnlockState};
use coin_heaven_lib::db::Db;
use coin_heaven_lib::error::{AppError, AppResult};
use coin_heaven_lib::security::recovery::{self, Dek, KeyInfo, Wrap, WrapKind};
use coin_heaven_lib::state::AppState;
use secrecy::{ExposeSecret, SecretString};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

const PASS: &str = "seven purple lanterns";

/// A string that must never be readable in anything written to disk.
const MARKER: &str = "Zx-Canary-Savings-8f3a1c";

/// The associated data the documented format authenticates each wrap with.
const WRAP_AAD: &[u8] = b"atlas-keywrap-v2";

/// Memory (KiB), passes, lanes. See "Speed" above.
const CHEAP_COST: (u32, u32, u32) = (64, 1, 1);

const DATA_FILES: [&str; 5] = [
    "atlas.db",
    "atlas.db-wal",
    "atlas.db-shm",
    recovery::KEYINFO_FILENAME,
    recovery::LOCALKEY_FILENAME,
];

/* ------------------------------------------------------------- helpers -- */

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn key_hex(dek: &Dek) -> String {
    recovery::dek_to_hex(dek).expose_secret().to_string()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Name -> contents of every file directly inside `dir`.
fn snapshot(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let e = e.ok()?;
            e.file_type().ok()?.is_file().then(|| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read(e.path()).unwrap(),
                )
            })
        })
        .collect()
}

fn file_names(dir: &Path) -> Vec<String> {
    snapshot(dir).into_keys().collect()
}

fn files_containing(dir: &Path, needle: &[u8]) -> Vec<String> {
    snapshot(dir)
        .into_iter()
        .filter(|(_, bytes)| contains(bytes, needle))
        .map(|(name, _)| name)
        .collect()
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Rows the tests look for: an account, forty transactions and a setting, all
/// carrying `MARKER`, so every kind of page holds recognisable text.
fn seed(db: &Db) {
    db.with(|c| {
        c.execute(
            "INSERT INTO account (name, kind) VALUES (?1, 'checking')",
            rusqlite::params![MARKER],
        )?;
        for i in 0..40 {
            c.execute(
                "INSERT INTO txn (account_id, booked_on, amount_minor, description_raw,
                                  description_norm, dedupe_hash)
                 VALUES (1, '2026-03-01', ?1, ?2, ?3, ?4)",
                rusqlite::params![
                    -100 - i,
                    format!("{MARKER} payment {i}"),
                    format!("{} payment {i}", MARKER.to_lowercase()),
                    format!("h{i}"),
                ],
            )?;
        }
        c.execute(
            "INSERT INTO app_setting (key, value_json) VALUES ('test.note', ?1)",
            rusqlite::params![format!("\"{MARKER} note\"")],
        )?;
        Ok(())
    })
    .unwrap();
}

fn account_names(db: &Db) -> Vec<String> {
    db.with(|c| {
        let mut s = c.prepare("SELECT name FROM account ORDER BY id")?;
        Ok(s.query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?)
    })
    .unwrap()
}

/// Every table's rows as text, ignoring order. `backup_log` is left out: the
/// live database writes its entry after the copy has been taken.
fn dump(db: &Db) -> BTreeMap<String, Vec<String>> {
    db.with(|c| {
        let tables: Vec<String> = c
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> 'backup_log'",
            )?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        let mut out = BTreeMap::new();
        for table in tables {
            let mut st = c.prepare(&format!("SELECT * FROM \"{table}\""))?;
            let columns = st.column_count();
            let mut rows: Vec<String> = st
                .query_map([], |r| {
                    let mut line = String::new();
                    for i in 0..columns {
                        line.push_str(&format!("{:?}|", r.get_ref(i)?));
                    }
                    Ok(line)
                })?
                .collect::<Result<_, _>>()?;
            rows.sort();
            out.insert(table, rows);
        }
        Ok(out)
    })
    .unwrap()
}

/// Opens `path` and reads every page. False for anything that does not come
/// back as a complete, authentic database.
fn is_readable(path: &Path, key: &SecretString) -> bool {
    let Ok(db) = Db::open(path, key) else {
        return false;
    };
    db.with(|c| {
        let mut s = c.prepare("PRAGMA integrity_check")?;
        let rows: Vec<String> = s.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(rows == ["ok"])
    })
    .unwrap_or(false)
}

/* ---- an independent implementation of the documented wrap format ---- */

fn kek(secret: &str, salt: &[u8], (m, t, p): (u32, u32, u32)) -> [u8; 32] {
    let params = Params::new(m, t, p, Some(32)).unwrap();
    let mut out = [0u8; 32];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(secret.as_bytes(), salt, &mut out)
        .unwrap();
    out
}

fn forge_wrap(dek: &Dek, secret: &str, kind: WrapKind, cost: (u32, u32, u32)) -> Wrap {
    let salt: [u8; 16] = rand::random();
    let nonce: [u8; 24] = rand::random();
    let secret = match kind {
        WrapKind::Recovery => recovery::normalise_phrase(secret),
        WrapKind::Passphrase => secret.to_string(),
    };
    let cipher = XChaCha20Poly1305::new_from_slice(&kek(&secret, &salt, cost)).unwrap();
    let ciphertext = cipher
        .encrypt(
            &XNonce::try_from(&nonce[..]).unwrap(),
            Payload {
                msg: dek.as_slice(),
                aad: WRAP_AAD,
            },
        )
        .unwrap();
    Wrap {
        kind,
        salt_b64: B64.encode(salt),
        nonce_b64: B64.encode(nonce),
        ciphertext_b64: B64.encode(ciphertext),
        m_cost_kib: cost.0,
        t_cost: cost.1,
        p_cost: cost.2,
    }
}

fn cheap_key_info(dek: &Dek, phrase: &str, passphrase: Option<&str>) -> KeyInfo {
    let mut wraps = vec![forge_wrap(dek, phrase, WrapKind::Recovery, CHEAP_COST)];
    if let Some(p) = passphrase {
        wraps.push(forge_wrap(dek, p, WrapKind::Passphrase, CHEAP_COST));
    }
    KeyInfo {
        version: 2,
        kdf: "argon2id".into(),
        wraps,
        verifier_b64: recovery::verifier_b64(dek),
    }
}

/* ---------------------------------------------------------- the world -- */

/// A data folder as `setup` leaves it: key info, a database holding the
/// seeded rows, and (with no passphrase) the key file. Built with the
/// library's own `KeyInfo::save`, `Db::open` and `save_local_key`.
fn build_world(dir: &Path, passphrase: Option<&str>) -> (Dek, String) {
    let dek = recovery::generate_dek();
    let phrase = recovery::generate_phrase()
        .unwrap()
        .expose_secret()
        .to_string();
    cheap_key_info(&dek, &phrase, passphrase).save(dir).unwrap();
    let db = Db::open(&dir.join("atlas.db"), &recovery::dek_to_hex(&dek)).unwrap();
    seed(&db);
    drop(db);
    if passphrase.is_none() {
        recovery::save_local_key(dir, &dek).unwrap();
    }
    (dek, phrase)
}

struct World {
    root: tempfile::TempDir,
    dir: PathBuf,
    dek: Dek,
    phrase: String,
}

impl World {
    fn new(passphrase: Option<&str>) -> World {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("data");
        let (dek, phrase) = build_world(&dir, passphrase);
        World {
            root,
            dir,
            dek,
            phrase,
        }
    }

    /// A fresh process looking at the same folder.
    fn launch(&self) -> AppState {
        AppState::new(self.dir.clone())
    }

    fn key_info(&self) -> KeyInfo {
        KeyInfo::load(&self.dir).unwrap().unwrap()
    }
}

fn run_backup(db: &Db, dest: &Path, keyinfo_dir: &Path, retain: usize) -> PathBuf {
    let scheduler = BackupScheduler::new();
    scheduler.mark_dirty();
    let policy = BackupPolicy {
        enabled: true,
        directory: Some(dest.to_path_buf()),
        min_interval_seconds: 0,
        retain_copies: retain,
    };
    match scheduler
        .maybe_run(db, &policy, Trigger::Manual, keyinfo_dir)
        .unwrap()
    {
        BackupOutcome::Written { path, .. } => path,
        other => panic!("expected a backup, got {other:?}"),
    }
}

/// `commands::passphrase_set`, without the Tauri `State` wrapper.
fn passphrase_set_as_the_command_does(st: &AppState, passphrase: &str) -> AppResult<()> {
    st.with_dek(|dek| recovery::set_passphrase(&st.data_dir, dek, passphrase))?;
    recovery::forget_local_key(&st.data_dir)
}

/// `commands::passphrase_remove`, without the Tauri `State` wrapper.
fn passphrase_remove_as_the_command_does(st: &AppState) -> AppResult<()> {
    recovery::remove_passphrase(&st.data_dir)?;
    st.with_dek(|dek| recovery::save_local_key(&st.data_dir, dek))
}

/* ================================================== 1. the database file == */

fn assert_ciphertext(dir: &Path, key: &SecretString) {
    let files = snapshot(dir);
    assert!(files.contains_key("atlas.db"), "nothing to inspect");
    for (name, bytes) in files {
        assert!(
            !contains(&bytes, MARKER.as_bytes()),
            "{name} holds readable row data"
        );
        assert!(
            !contains(&bytes, b"SQLite format 3"),
            "{name} carries the plaintext SQLite header"
        );
        assert!(
            !contains(&bytes, key.expose_secret().as_bytes()),
            "{name} holds the key that encrypts it"
        );
    }
}

#[test]
fn the_database_is_ciphertext_while_open_and_after_closing() {
    let dir = tempfile::tempdir().unwrap();
    let key = SecretString::from("a1".repeat(32));
    let db = Db::open(&dir.path().join("atlas.db"), &key).unwrap();
    seed(&db);

    // Open: recent pages are in the write-ahead log, which is a file too.
    assert_ciphertext(dir.path(), &key);
    // And SQLCipher is what is linked: plain SQLite would ignore PRAGMA key.
    let version: String = db
        .with(|c| Ok(c.query_row("PRAGMA cipher_version", [], |r| r.get(0))?))
        .unwrap();
    assert!(
        version
            .split('.')
            .next()
            .and_then(|v| v.parse::<u32>().ok())
            >= Some(4),
        "SQLCipher 4 or later is required, got {version:?}"
    );

    drop(db);
    assert_ciphertext(dir.path(), &key);
}

#[test]
fn a_wrong_key_is_refused_and_the_files_are_left_exactly_as_they_were() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("atlas.db");
    let key = SecretString::from("a1".repeat(32));
    seed(&Db::open(&path, &key).unwrap());
    let before = snapshot(dir.path());

    let mut off_by_one = key.expose_secret().to_string();
    off_by_one.pop();
    off_by_one.push('0');
    for wrong in ["0".repeat(64), "f".repeat(64), off_by_one] {
        let err = Db::open(&path, &SecretString::from(wrong.clone())).unwrap_err();
        assert!(matches!(err, AppError::Locked), "got {err:?}");
        assert!(
            !err.to_string().contains(&wrong),
            "the error repeats the key it was given"
        );
    }

    assert_eq!(
        snapshot(dir.path()),
        before,
        "a refused open changed the files"
    );
    // The right key still works afterwards: nothing was damaged.
    assert_eq!(account_names(&Db::open(&path, &key).unwrap()), [MARKER]);
}

#[test]
fn the_key_is_used_raw_as_the_threat_model_says() {
    // "The key is a 32-byte CSPRNG value passed as raw hex, so SQLCipher's KDF
    // is deliberately bypassed." If the hex were handed over as a passphrase
    // instead, this file would be stretched with PBKDF2 and the document
    // would be wrong in the way that matters to a reviewer.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("atlas.db");
    let hex = "5c".repeat(32);
    drop(Db::open(&path, &SecretString::from(hex.clone())).unwrap());

    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.pragma_update(None, "key", format!("x'{hex}'")).unwrap();
    raw.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    })
    .expect("the file opens under the raw key");
    drop(raw);

    let as_passphrase = rusqlite::Connection::open(&path).unwrap();
    as_passphrase.pragma_update(None, "key", &hex).unwrap();
    assert!(
        as_passphrase
            .query_row("SELECT count(*) FROM sqlite_master", [], |r| r
                .get::<_, i64>(0))
            .is_err(),
        "the same characters as a passphrase must not open it"
    );
}

#[test]
fn a_malformed_key_never_reaches_sqlcipher() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("atlas.db");
    let good = SecretString::from("a1".repeat(32));
    seed(&Db::open(&path, &good).unwrap());
    let before = snapshot(dir.path());

    let injection = format!("'; DROP TABLE account;--{}", "a".repeat(40));
    assert_eq!(injection.len(), 64, "the case must be the right length");
    let bad = [
        String::new(),
        "a".repeat(63),
        "a".repeat(65),
        injection,
        "g".repeat(64),
        // 64 bytes, but 63 characters: a byte-indexed slice would panic.
        format!("{}é", "a".repeat(62)),
        format!(" {}", "a".repeat(63)),
    ];
    for key in bad {
        let key = SecretString::from(key);
        assert!(
            matches!(Db::open(&path, &key), Err(AppError::Invalid(_))),
            "open accepted a malformed key of {} bytes",
            key.expose_secret().len()
        );
        assert!(
            matches!(Db::rekey(&path, &good, &key), Err(AppError::Invalid(_))),
            "rekey accepted a malformed new key"
        );
        assert!(
            matches!(Db::rekey(&path, &key, &good), Err(AppError::Invalid(_))),
            "rekey accepted a malformed old key"
        );
    }
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn rekeying_moves_the_data_and_the_old_key_stops_working() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("atlas.db");
    let old = SecretString::from("a1".repeat(32));
    let new = SecretString::from("b2".repeat(32));
    seed(&Db::open(&path, &old).unwrap());

    // The wrong old key changes nothing.
    let before = snapshot(dir.path());
    let stranger = SecretString::from("c3".repeat(32));
    assert!(matches!(
        Db::rekey(&path, &stranger, &new),
        Err(AppError::Locked)
    ));
    assert_eq!(snapshot(dir.path()), before);

    Db::rekey(&path, &old, &new).unwrap();
    assert!(matches!(Db::open(&path, &old), Err(AppError::Locked)));
    let db = Db::open(&path, &new).unwrap();
    assert_eq!(account_names(&db), [MARKER]);
    drop(db);
    assert_ciphertext(dir.path(), &new);
    assert!(
        files_containing(dir.path(), old.expose_secret().as_bytes()).is_empty(),
        "the previous key is still written somewhere"
    );
}

/* ====================================== 2. the wrapped key info file == */

/// Flip one bit of the byte at `at` in a base64 field, keeping it base64.
fn flip_b64(s: &str, at: usize) -> String {
    let mut bytes = B64.decode(s).unwrap();
    let at = at.min(bytes.len() - 1);
    bytes[at] ^= 0x01;
    B64.encode(bytes)
}

struct Keys {
    dek: Dek,
    phrase: String,
    info: KeyInfo,
}

fn keys() -> Keys {
    let dek = recovery::generate_dek();
    let phrase = recovery::generate_phrase()
        .unwrap()
        .expose_secret()
        .to_string();
    let info = cheap_key_info(&dek, &phrase, Some(PASS));
    Keys { dek, phrase, info }
}

/// `unlock`, as a result: a panic is reported rather than propagated.
fn unlock_without_panicking(
    info: &KeyInfo,
    secret: &str,
    kind: WrapKind,
) -> Result<AppResult<Dek>, ()> {
    catch_unwind(AssertUnwindSafe(|| info.unlock(secret, kind))).map_err(|_| ())
}

#[test]
fn the_right_secrets_unlock_and_the_wrong_ones_leave_the_app_locked() {
    let w = World::new(Some(PASS));
    let before = snapshot(&w.dir);

    let mut one_word_off: Vec<&str> = w.phrase.split(' ').collect();
    one_word_off.swap(0, 11);
    let one_word_off = one_word_off.join(" ");
    let other_phrase = recovery::generate_phrase().unwrap();

    let st = w.launch();
    for wrong in [
        "seven purple lantern",
        "Seven Purple Lanterns",
        "",
        w.phrase.as_str(),
    ] {
        assert!(
            bootstrap::unlock_with_passphrase(&st, wrong).is_err(),
            "passphrase {wrong:?} was accepted"
        );
    }
    for wrong in [
        other_phrase.expose_secret(),
        one_word_off.as_str(),
        PASS,
        "",
    ] {
        assert!(
            bootstrap::unlock_with_phrase(&st, wrong).is_err(),
            "a wrong phrase was accepted"
        );
    }

    // No partial success: still locked, no key held, nothing written.
    assert!(!st.is_open());
    assert!(matches!(st.db(), Err(AppError::Locked)));
    assert!(matches!(st.with_dek(|_| Ok(())), Err(AppError::Locked)));
    assert!(!st.has_pending_phrase());
    assert_eq!(snapshot(&w.dir), before);

    // The correct ones open the same data, by either door.
    let by_passphrase = w.launch();
    assert_eq!(
        bootstrap::unlock_with_passphrase(&by_passphrase, PASS).unwrap(),
        UnlockState::Opened
    );
    assert_eq!(account_names(by_passphrase.db().unwrap()), [MARKER]);

    let by_phrase = w.launch();
    let messy = format!("  {}  ", w.phrase.to_uppercase().replace(' ', "  "));
    assert_eq!(
        bootstrap::unlock_with_phrase(&by_phrase, &messy).unwrap(),
        UnlockState::Opened
    );
    assert_eq!(account_names(by_phrase.db().unwrap()), [MARKER]);
}

#[test]
fn the_two_doors_do_not_open_each_others_wraps() {
    let k = keys();
    // Each secret is only good for its own wrap, even though both are
    // plain strings and the passphrase here happens to be valid text.
    assert!(k.info.unlock(&k.phrase, WrapKind::Passphrase).is_err());
    assert!(k.info.unlock(PASS, WrapKind::Recovery).is_err());
    assert!(k.info.unlock(PASS, WrapKind::Passphrase).is_ok());
    assert!(k.info.unlock(&k.phrase, WrapKind::Recovery).is_ok());
}

#[test]
fn one_changed_byte_in_any_part_of_a_wrap_stops_that_wrap_unlocking() {
    let k = keys();
    type Edit = fn(&mut Wrap, usize);
    let regions: [(&str, Edit); 6] = [
        ("salt", |w, at| w.salt_b64 = flip_b64(&w.salt_b64, at)),
        ("nonce", |w, at| w.nonce_b64 = flip_b64(&w.nonce_b64, at)),
        ("ciphertext", |w, at| {
            w.ciphertext_b64 = flip_b64(&w.ciphertext_b64, at)
        }),
        // These three become 65 KiB, 0 passes and 0 lanes: one valid, two
        // that Argon2 itself refuses. Neither may panic or succeed.
        ("memory cost", |w, _| w.m_cost_kib ^= 1),
        ("pass count", |w, _| w.t_cost ^= 1),
        ("lanes", |w, _| w.p_cost ^= 1),
    ];

    for (kind, secret) in [
        (WrapKind::Recovery, k.phrase.as_str()),
        (WrapKind::Passphrase, PASS),
    ] {
        let other = if kind == WrapKind::Recovery {
            (WrapKind::Passphrase, PASS)
        } else {
            (WrapKind::Recovery, k.phrase.as_str())
        };
        for (region, edit) in regions {
            for at in [0, 8, 47] {
                let mut info = k.info.clone();
                edit(info.wraps.iter_mut().find(|w| w.kind == kind).unwrap(), at);
                match unlock_without_panicking(&info, secret, kind) {
                    Err(()) => panic!("{kind:?} {region} byte {at}: unlock panicked"),
                    Ok(Ok(_)) => panic!("{kind:?} {region} byte {at}: tampering was accepted"),
                    Ok(Err(_)) => {}
                }
                // The other wrap shares nothing with it, so it still opens.
                let still = info.unlock(other.1, other.0).unwrap();
                assert_eq!(still.as_slice(), k.dek.as_slice());
            }
        }
    }
}

#[test]
fn a_changed_verifier_is_not_accepted_even_when_the_wrap_itself_is_intact() {
    let k = keys();
    for at in [0, 15, 31] {
        let mut info = k.info.clone();
        info.verifier_b64 = flip_b64(&info.verifier_b64, at);
        for (kind, secret) in [
            (WrapKind::Recovery, k.phrase.as_str()),
            (WrapKind::Passphrase, PASS),
        ] {
            assert!(
                matches!(unlock_without_panicking(&info, secret, kind), Ok(Err(_))),
                "{kind:?}: a key that does not match the verifier was handed out"
            );
        }
    }
}

#[test]
fn malformed_fields_fail_cleanly_whatever_their_length() {
    let k = keys();
    let recovery_wrap = k.info.wrap_for(WrapKind::Recovery).unwrap().clone();
    let decoded = |s: &str| B64.decode(s).unwrap();

    let mut cases: Vec<(String, Wrap)> = Vec::new();
    let mut with = |name: &str, edit: &dyn Fn(&mut Wrap)| {
        let mut w = recovery_wrap.clone();
        edit(&mut w);
        cases.push((name.to_string(), w));
    };
    let salt = decoded(&recovery_wrap.salt_b64);
    let nonce = decoded(&recovery_wrap.nonce_b64);
    let ct = decoded(&recovery_wrap.ciphertext_b64);
    with("empty salt", &|w| w.salt_b64 = String::new());
    with("7-byte salt", &|w| w.salt_b64 = B64.encode(&salt[..7]));
    with("17-byte salt", &|w| {
        w.salt_b64 = B64.encode([salt.clone(), vec![0]].concat())
    });
    with("empty nonce", &|w| w.nonce_b64 = String::new());
    with("23-byte nonce", &|w| w.nonce_b64 = B64.encode(&nonce[..23]));
    with("25-byte nonce", &|w| {
        w.nonce_b64 = B64.encode([nonce.clone(), vec![0]].concat())
    });
    with("empty ciphertext", &|w| w.ciphertext_b64 = String::new());
    with("one byte of ciphertext", &|w| {
        w.ciphertext_b64 = B64.encode(&ct[..1])
    });
    with("ciphertext missing its last byte", &|w| {
        w.ciphertext_b64 = B64.encode(&ct[..ct.len() - 1])
    });
    with("ciphertext with an extra byte", &|w| {
        w.ciphertext_b64 = B64.encode([ct.clone(), vec![0]].concat())
    });
    with("salt that is not base64", &|w| w.salt_b64 = "!!!!".into());
    with("nonce that is not base64", &|w| w.nonce_b64 = "!!!!".into());
    with("ciphertext that is not base64", &|w| {
        w.ciphertext_b64 = "!!!!".into()
    });
    with("zero memory", &|w| w.m_cost_kib = 0);
    with("memory below the lane minimum", &|w| w.m_cost_kib = 7);
    with("zero passes", &|w| w.t_cost = 0);
    with("zero lanes", &|w| w.p_cost = 0);
    with("more lanes than memory allows", &|w| w.p_cost = 16_777_215);

    assert!(cases.len() >= 15);
    for (name, wrap) in cases {
        let r = catch_unwind(AssertUnwindSafe(|| recovery::unwrap_dek(&wrap, &k.phrase)));
        match r {
            Err(_) => panic!("{name}: unwrap panicked"),
            Ok(Ok(Some(_))) => panic!("{name}: produced a key"),
            Ok(Ok(None)) | Ok(Err(_)) => {}
        }
    }
}

/// What a wrap means once decoded. Two spellings of the same bytes are the
/// same wrap; this is what the sweep below compares.
fn meaning(w: &Wrap) -> String {
    format!(
        "{:?}/{:?}/{:?}/{:?}/{}/{}/{}",
        w.kind,
        B64.decode(&w.salt_b64).ok(),
        B64.decode(&w.nonce_b64).ok(),
        B64.decode(&w.ciphertext_b64).ok(),
        w.m_cost_kib,
        w.t_cost,
        w.p_cost
    )
}

#[test]
fn no_single_byte_changed_in_the_file_unlocks_anything_it_should_not() {
    let k = keys();
    let dir = tempfile::tempdir().unwrap();
    k.info.save(dir.path()).unwrap();
    let path = dir.path().join(recovery::KEYINFO_FILENAME);
    let original = std::fs::read(&path).unwrap();
    assert!(original.len() > 500, "the file is smaller than expected");

    let (mut rejected_on_load, mut wrap_refused, mut left_alone, mut metadata_only) = (0, 0, 0, 0);
    for i in 0..original.len() {
        // 0x01 turns letters into their neighbours (often still base64);
        // 0x20 swaps case, so more edits stay inside the alphabet.
        for mask in [0x01u8, 0x20] {
            let mut bytes = original.clone();
            bytes[i] ^= mask;
            std::fs::write(&path, &bytes).unwrap();

            let Ok(Some(info)) = KeyInfo::load(dir.path()) else {
                rejected_on_load += 1;
                continue;
            };
            if info.version != k.info.version || info.kdf != k.info.kdf {
                // Not covered by the authentication tag: see
                // `an_unknown_version_or_kdf_is_not_silently_treated_as_current`.
                metadata_only += 1;
                continue;
            }
            let verifier_same =
                B64.decode(&info.verifier_b64).ok() == B64.decode(&k.info.verifier_b64).ok();
            for (kind, secret) in [
                (WrapKind::Recovery, k.phrase.as_str()),
                (WrapKind::Passphrase, PASS),
            ] {
                let was = meaning(k.info.wrap_for(kind).unwrap());
                let unchanged =
                    verifier_same && info.wrap_for(kind).is_some_and(|w| meaning(w) == was);
                match unlock_without_panicking(&info, secret, kind) {
                    Err(()) => panic!("byte {i} ^ {mask:#x}: unlock panicked"),
                    Ok(Ok(dek)) => {
                        assert!(
                            unchanged,
                            "byte {i} ^ {mask:#x}: an edited {kind:?} wrap opened"
                        );
                        assert_eq!(dek.as_slice(), k.dek.as_slice(), "a different key came out");
                        left_alone += 1;
                    }
                    Ok(Err(_)) => {
                        assert!(
                            !unchanged,
                            "byte {i} ^ {mask:#x}: an untouched {kind:?} wrap stopped opening"
                        );
                        wrap_refused += 1;
                    }
                }
            }
        }
    }
    // Guard against a loop that quietly tested nothing.
    assert!(
        rejected_on_load > 100,
        "only {rejected_on_load} edits broke the JSON"
    );
    assert!(
        wrap_refused > 400,
        "only {wrap_refused} edited wraps were refused"
    );
    assert!(
        left_alone > 400,
        "only {left_alone} untouched wraps were checked"
    );
    assert!(metadata_only >= 1);
}

#[test]
fn wraps_of_one_database_use_a_fresh_salt_and_nonce_every_time() {
    // Production cost, because `wrap_dek` takes no parameters.
    let dek = recovery::generate_dek();
    let a = recovery::wrap_dek(&dek, PASS, WrapKind::Passphrase).unwrap();
    let b = recovery::wrap_dek(&dek, PASS, WrapKind::Passphrase).unwrap();

    assert_ne!(a.salt_b64, b.salt_b64, "the salt was reused");
    assert_ne!(a.nonce_b64, b.nonce_b64, "the nonce was reused");
    assert_ne!(a.ciphertext_b64, b.ciphertext_b64);
    // Both still open: different bytes, same key.
    for w in [&a, &b] {
        let got = recovery::unwrap_dek(w, PASS).unwrap().unwrap();
        assert_eq!(got.as_slice(), dek.as_slice());
    }
}

#[test]
fn wraps_made_by_the_library_follow_the_documented_format() {
    // An independent decryption of what `wrap_dek` wrote. This is also the
    // compatibility pin: the associated data is part of the on-disk format, so
    // changing it would strand every existing key info file.
    let dek = recovery::generate_dek();
    let wrap = recovery::wrap_dek(&dek, PASS, WrapKind::Passphrase).unwrap();

    let salt = B64.decode(&wrap.salt_b64).unwrap();
    let nonce = B64.decode(&wrap.nonce_b64).unwrap();
    let ciphertext = B64.decode(&wrap.ciphertext_b64).unwrap();
    assert_eq!(salt.len(), 16);
    assert_eq!(nonce.len(), 24, "XChaCha20 takes a 24-byte nonce");
    assert_eq!(
        ciphertext.len(),
        32 + 16,
        "a 32-byte key plus the Poly1305 tag"
    );

    // A floor on the cost, so a careless edit to the constants is caught:
    // OWASP's minimum for Argon2id is 19 MiB, two passes, one lane.
    assert!(
        wrap.m_cost_kib >= 19 * 1024,
        "memory cost {}",
        wrap.m_cost_kib
    );
    assert!(wrap.t_cost >= 2, "pass count {}", wrap.t_cost);
    assert!(wrap.p_cost >= 1);

    let cipher = XChaCha20Poly1305::new_from_slice(&kek(
        PASS,
        &salt,
        (wrap.m_cost_kib, wrap.t_cost, wrap.p_cost),
    ))
    .unwrap();
    let nonce = XNonce::try_from(&nonce[..]).unwrap();
    let open = |aad: &[u8]| {
        cipher.decrypt(
            &nonce,
            Payload {
                msg: &ciphertext,
                aad,
            },
        )
    };
    assert_eq!(open(WRAP_AAD).unwrap(), dek.as_slice());
    assert!(
        open(b"").is_err(),
        "the wrap is not bound to any associated data"
    );
    assert!(open(b"atlas-keywrap-v1").is_err());
}

#[test]
fn the_verifier_is_a_domain_separated_hash_not_the_key() {
    let dek = recovery::generate_dek();
    let mut h = Sha256::new();
    h.update(b"atlas-dek-verifier-v2");
    h.update(dek.as_slice());
    assert_eq!(recovery::verifier_b64(&dek), B64.encode(h.finalize()));

    // It is stored in the clear next to every backup, so it must say nothing
    // about the key itself and must tell two keys apart.
    let verifier = B64.decode(recovery::verifier_b64(&dek)).unwrap();
    assert_ne!(verifier.as_slice(), dek.as_slice());
    assert!(!contains(&verifier, dek.as_slice()));
    let other = recovery::generate_dek();
    let info = cheap_key_info(&dek, "unused", None);
    assert!(info.matches(&dek));
    assert!(!info.matches(&other));
}

#[test]
#[ignore = "FINDING: the wrap kind is not authenticated, so a passphrase wrap relabelled as a recovery wrap still opens"]
fn a_wrap_cannot_be_moved_to_the_other_kind_by_relabelling_it() {
    // Both kinds are encrypted with the same constant associated data, so
    // `kind` is only a label in the JSON. A lower-case, single-spaced
    // passphrase normalises to itself, which makes the swap succeed.
    let dek = recovery::generate_dek();
    let passphrase_wrap = forge_wrap(&dek, PASS, WrapKind::Passphrase, CHEAP_COST);
    let mut relabelled = passphrase_wrap.clone();
    relabelled.kind = WrapKind::Recovery;

    let got = recovery::unwrap_dek(&relabelled, PASS);
    assert!(
        matches!(got, Ok(None) | Err(_)),
        "a passphrase wrap was accepted as a recovery wrap"
    );

    let phrase = recovery::generate_phrase().unwrap();
    let recovery_wrap = forge_wrap(&dek, phrase.expose_secret(), WrapKind::Recovery, CHEAP_COST);
    let mut relabelled = recovery_wrap;
    relabelled.kind = WrapKind::Passphrase;
    let got = recovery::unwrap_dek(&relabelled, phrase.expose_secret());
    assert!(
        matches!(got, Ok(None) | Err(_)),
        "a recovery wrap was accepted as a passphrase wrap"
    );
}

#[test]
fn an_unknown_version_or_kdf_is_not_silently_treated_as_current() {
    let k = keys();
    for (version, kdf) in [(99, "argon2id"), (2, "scrypt"), (0, "")] {
        let mut info = k.info.clone();
        info.version = version;
        info.kdf = kdf.to_string();
        let dir = tempfile::tempdir().unwrap();
        info.save(dir.path()).unwrap();

        let loaded = KeyInfo::load(dir.path());
        let refused = match loaded {
            Err(_) => true,
            Ok(Some(i)) => i.unlock(&k.phrase, WrapKind::Recovery).is_err(),
            Ok(None) => true,
        };
        assert!(
            refused,
            "version {version} / kdf {kdf:?} was processed as if it were version 2"
        );
    }
}

#[test]
fn a_tampered_cost_cannot_make_unlock_run_forever() {
    // The stored cost is not authenticated, and a key info file is something
    // the app is told to restore from a backup folder. 8 KiB and u32::MAX
    // passes is small enough to be safe to *try* and never finishes.
    let k = keys();
    let mut info = k.info.clone();
    {
        let w = info
            .wraps
            .iter_mut()
            .find(|w| w.kind == WrapKind::Passphrase)
            .unwrap();
        w.m_cost_kib = 8;
        w.t_cost = u32::MAX;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    // Left running if it hangs; the test process exiting ends it.
    std::thread::spawn(move || {
        let _ = tx.send(info.unlock(PASS, WrapKind::Passphrase).is_err());
    });
    match rx.recv_timeout(std::time::Duration::from_secs(3)) {
        Ok(refused) => assert!(refused, "a tampered cost was honoured and accepted"),
        Err(_) => panic!("unlock was still running after 3 s: the stored cost has no upper bound"),
    }
}

/* ----------------------------------------------- the key file on disk -- */

#[test]
fn a_damaged_key_file_falls_back_to_the_phrase_rather_than_opening_or_panicking() {
    let w = World::new(None);
    let key_file = w.dir.join(recovery::LOCALKEY_FILENAME);
    let right = key_hex(&w.dek);

    // Control: the right key, with the trailing newline an editor adds.
    std::fs::write(&key_file, format!("{right}\n")).unwrap();
    let st = w.launch();
    assert_eq!(bootstrap::auto_unlock(&st).unwrap(), UnlockState::Opened);
    drop(st);

    let mut typo = right.clone();
    typo.pop();
    typo.push(if right.ends_with('0') { '1' } else { '0' });
    let stranger = key_hex(&recovery::generate_dek());
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("truncated", right.as_bytes()[..32].to_vec()),
        ("not hex", "z".repeat(64).into_bytes()),
        ("not UTF-8", vec![0xff; 64]),
        (
            "multi-byte character",
            format!("{}é", "a".repeat(62)).into_bytes(),
        ),
        ("one digit wrong", typo.into_bytes()),
        ("another database's key", stranger.into_bytes()),
    ];
    for (name, content) in cases {
        std::fs::write(&key_file, content).unwrap();
        let st = w.launch();
        let state = catch_unwind(AssertUnwindSafe(|| bootstrap::auto_unlock(&st)))
            .unwrap_or_else(|_| panic!("{name}: auto_unlock panicked"));
        assert_eq!(
            state.unwrap(),
            UnlockState::NeedsRecoveryPhrase,
            "{name}: the key file was accepted"
        );
        assert!(!st.is_open(), "{name}: the database was opened");
    }
}

/* ================================================= 3. recovery phrases == */

#[test]
fn generated_phrases_are_twelve_wordlist_words_and_never_the_same() {
    let list: std::collections::HashSet<&str> = bip39::Language::English
        .word_list()
        .iter()
        .copied()
        .collect();
    let phrases: Vec<String> = (0..25)
        .map(|_| {
            recovery::generate_phrase()
                .unwrap()
                .expose_secret()
                .to_string()
        })
        .collect();

    for p in &phrases {
        let words: Vec<&str> = p.split(' ').collect();
        assert_eq!(words.len(), 12, "{p:?}");
        assert!(words.iter().all(|w| list.contains(w)), "not wordlist words");
        recovery::validate_phrase(p).unwrap();
    }
    let distinct: std::collections::HashSet<&String> = phrases.iter().collect();
    assert_eq!(distinct.len(), phrases.len(), "a phrase repeated");
    let words: std::collections::HashSet<&str> =
        phrases.iter().flat_map(|p| p.split(' ')).collect();
    assert!(
        words.len() > 150,
        "300 words drawn from 2048 came out as only {} distinct",
        words.len()
    );
}

#[test]
fn a_phrase_with_a_word_swapped_or_misspelt_is_rejected() {
    // BIP-39 test vector 2: valid.
    let good = "legal winner thank year wave sausage worth useful legal winner thank yellow";
    recovery::validate_phrase(good).unwrap();

    let last_swapped = good.replace("yellow", "abandon");
    let misspelt = good.replace("yellow", "yelow");
    let homoglyph = good.replace("year", "ye\u{0430}r"); // Cyrillic a
    for (what, phrase) in [
        ("last word swapped for a valid one", last_swapped),
        ("a word misspelt", misspelt.clone()),
        ("a Cyrillic look-alike letter", homoglyph),
        (
            "11 words",
            "legal winner thank year wave sausage worth useful legal winner thank".into(),
        ),
        ("13 words", format!("{good} legal")),
        ("empty", String::new()),
        ("only spaces", "   \t ".into()),
        // Valid BIP-39, but not what this app issues.
        (
            "a 24-word phrase",
            format!("{} art", "abandon ".repeat(23).trim_end()),
        ),
    ] {
        assert!(
            recovery::validate_phrase(&phrase).is_err(),
            "{what} was accepted"
        );
    }

    // The message helps the person without repeating what they typed.
    let msg = recovery::validate_phrase(&misspelt)
        .unwrap_err()
        .to_string();
    assert!(
        !msg.contains("yelow"),
        "the error repeats the phrase: {msg}"
    );
}

#[test]
fn swapping_any_one_word_is_caught_unless_the_checksum_happens_to_match() {
    // Four checksum bits mean about one substitution in sixteen still
    // validates. That is BIP-39, not a defect; what must hold is that
    // `validate_phrase` agrees with the standard on every one of them.
    let phrase = recovery::generate_phrase().unwrap();
    let words: Vec<&str> = phrase.expose_secret().split(' ').collect();
    let (mut rejected, mut accepted) = (0, 0);
    for candidate in bip39::Language::English.word_list() {
        let mut swapped = words.clone();
        swapped[3] = candidate;
        let swapped = swapped.join(" ");
        let ours = recovery::validate_phrase(&swapped).is_ok();
        let standard = bip39::Mnemonic::parse_normalized(&swapped).is_ok();
        assert_eq!(ours, standard, "disagrees with BIP-39 on {swapped:?}");
        if ours {
            accepted += 1;
        } else {
            rejected += 1;
        }
    }
    assert!(accepted >= 1, "the original phrase itself was refused");
    assert!(
        rejected > 1800,
        "only {rejected} of 2048 substitutions were refused"
    );
}

/* ============================================================ 4. backups == */

#[test]
fn a_backup_opens_with_the_live_key_and_holds_every_row() {
    let w = World::new(None);
    let st = w.launch();
    assert_eq!(bootstrap::auto_unlock(&st).unwrap(), UnlockState::Opened);
    let live = st.db().unwrap();
    let dest = w.root.path().join("backups");

    let backup = run_backup(live, &dest, &st.data_dir, 14);
    assert!(backup.is_file());
    assert_eq!(backup.parent().unwrap(), dest);

    // Copied out of the backup folder, as someone restoring it would.
    let restored = w.root.path().join("restored.db");
    std::fs::copy(&backup, &restored).unwrap();
    let key = recovery::dek_to_hex(&w.dek);
    assert!(
        is_readable(&restored, &key),
        "the backup is not a clean database"
    );

    let copy = Db::open(&restored, &key).unwrap();
    assert_eq!(account_names(&copy), [MARKER]);
    assert_eq!(
        copy.schema_version().unwrap(),
        live.schema_version().unwrap()
    );
    let (original, restored_rows) = (dump(live), dump(&copy));
    assert!(
        original["txn"].len() >= 40,
        "the fixture lost its transactions"
    );
    assert_eq!(
        restored_rows, original,
        "the backup differs from the database"
    );

    // And it is as unreadable as the original without the key.
    let bytes = std::fs::read(&backup).unwrap();
    assert!(!contains(&bytes, MARKER.as_bytes()));
    assert!(!contains(&bytes, b"SQLite format 3"));
    let wrong = SecretString::from("0".repeat(64));
    let again = w.root.path().join("again.db");
    std::fs::copy(&backup, &again).unwrap();
    assert!(matches!(Db::open(&again, &wrong), Err(AppError::Locked)));
}

#[test]
fn a_damaged_backup_is_never_read_as_good_data() {
    let w = World::new(None);
    let st = w.launch();
    bootstrap::auto_unlock(&st).unwrap();
    let backup = run_backup(st.db().unwrap(), &w.root.path().join("bk"), &w.dir, 14);
    let original = std::fs::read(&backup).unwrap();
    let key = recovery::dek_to_hex(&w.dek);
    let page = 4096;
    assert_eq!(original.len() % page, 0);
    let pages = original.len() / page;
    assert!(pages > 20, "the backup is too small to be a useful check");

    let scratch = tempfile::tempdir().unwrap();
    let write = |name: String, bytes: &[u8]| -> PathBuf {
        let path = scratch.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    };

    // Control: an unmodified copy is readable, so the failures below mean something.
    assert!(is_readable(&write("control.db".into(), &original), &key));

    // One flipped byte in every page, at an offset that varies per page.
    for p in 0..pages {
        let mut bytes = original.clone();
        bytes[p * page + (p * 37) % page] ^= 0x01;
        let path = write(format!("page{p}.db"), &bytes);
        assert!(
            !is_readable(&path, &key),
            "a flipped byte in page {p} went unnoticed"
        );
    }

    // Other kinds of damage.
    let mut zeroed = original.clone();
    zeroed[2 * page..3 * page].fill(0);
    let mut last = original.clone();
    *last.last_mut().unwrap() ^= 0x80;
    for (what, bytes) in [
        ("a zeroed page", zeroed),
        ("a flipped last byte", last),
        (
            "the last page missing",
            original[..original.len() - page].to_vec(),
        ),
        ("half the file", original[..original.len() / 2].to_vec()),
        ("only the first page", original[..page].to_vec()),
        // An empty file is not damage SQLCipher can see: it is a new database.
        ("plain text", b"SQLite format 3\0 but not really".to_vec()),
    ] {
        let path = write(format!("{}.db", what.replace(' ', "-")), &bytes);
        assert!(!is_readable(&path, &key), "{what} was read as good data");
    }
}

#[test]
fn a_backup_folder_on_its_own_restores_with_the_recovery_phrase_or_passphrase() {
    for passphrase in [None, Some(PASS)] {
        let w = World::new(passphrase);
        let origin = w.launch();
        match passphrase {
            None => bootstrap::auto_unlock(&origin).unwrap(),
            Some(p) => bootstrap::unlock_with_passphrase(&origin, p).unwrap(),
        };
        let dest = w.root.path().join("backups");
        let backup = run_backup(origin.db().unwrap(), &dest, &w.dir, 14);
        drop(origin);

        // A new machine: only what is in the backup folder, no key file.
        let fresh = w.root.path().join("new-machine");
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::copy(&backup, fresh.join("atlas.db")).unwrap();
        std::fs::copy(
            dest.join(recovery::KEYINFO_FILENAME),
            fresh.join(recovery::KEYINFO_FILENAME),
        )
        .unwrap();

        let st = AppState::new(fresh.clone());
        let expected = if passphrase.is_some() {
            UnlockState::NeedsPassphrase
        } else {
            UnlockState::NeedsRecoveryPhrase
        };
        assert_eq!(bootstrap::auto_unlock(&st).unwrap(), expected);
        assert!(!st.is_open());

        assert_eq!(
            bootstrap::unlock_with_phrase(&st, &w.phrase).unwrap(),
            UnlockState::Opened
        );
        assert_eq!(account_names(st.db().unwrap()), [MARKER]);
        // Recovering with a passphrase set must not write the key down.
        assert_eq!(
            fresh.join(recovery::LOCALKEY_FILENAME).exists(),
            passphrase.is_none(),
            "key file present: {}",
            passphrase.is_some()
        );
    }
}

#[test]
fn nothing_in_the_backup_folder_can_decrypt_the_database_by_itself() {
    let w = World::new(Some(PASS));
    let st = w.launch();
    bootstrap::unlock_with_passphrase(&st, PASS).unwrap();
    // A key file left over from before the passphrase was set.
    recovery::save_local_key(&w.dir, &w.dek).unwrap();
    let dest = w.root.path().join("backups");
    run_backup(st.db().unwrap(), &dest, &w.dir, 14);

    let mut names = file_names(&dest);
    names.sort();
    // One copy, its own key sidecar, and the plain "latest" sidecar.
    assert_eq!(
        names.len(),
        3,
        "expected one backup and its sidecars, got {names:?}"
    );
    assert!(names.contains(&recovery::KEYINFO_FILENAME.to_string()));
    assert!(!names.contains(&recovery::LOCALKEY_FILENAME.to_string()));

    for secret in [
        key_hex(&w.dek).into_bytes(),
        w.dek.to_vec(),
        PASS.as_bytes().to_vec(),
        w.phrase.clone().into_bytes(),
    ] {
        assert!(
            files_containing(&dest, &secret).is_empty(),
            "a backup file holds key material"
        );
    }
}

#[test]
fn retention_deletes_only_old_backups_and_never_other_files() {
    let w = World::new(None);
    let st = w.launch();
    bootstrap::auto_unlock(&st).unwrap();
    let dest = w.root.path().join("backups");
    std::fs::create_dir_all(dest.join("older")).unwrap();

    // Old backups, in Coin Heaven's own naming.
    for day in 1..=5 {
        std::fs::write(
            dest.join(format!("atlas-2025010{day}T000000Z.db.enc")),
            b"old",
        )
        .unwrap();
    }
    // Things that are not Coin Heaven backups, including near misses.
    let others: [(&str, &[u8]); 7] = [
        ("atlas-notes.txt", b"my notes"),
        ("photo.jpg", b"\xff\xd8\xff\xe0 not really"),
        ("README", b"read me"),
        (recovery::KEYINFO_FILENAME, b"{}"),
        (
            "atlas-20250101T000000Z.db.enc-wal",
            b"a sidecar, not a backup",
        ),
        ("atlas-20250101T000000Z.db", b"unencrypted-looking name"),
        ("older/atlas-20240101T000000Z.db.enc", b"in a sub-folder"),
    ];
    for (name, content) in others {
        std::fs::write(dest.join(name), content).unwrap();
    }

    let new = run_backup(st.db().unwrap(), &dest, &w.dir, 2);

    let mut left: Vec<String> = file_names(&dest)
        .into_iter()
        .filter(|n| n.starts_with("atlas-") && n.ends_with(".db.enc"))
        .collect();
    left.sort();
    let new_name = new.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        left,
        ["atlas-20250105T000000Z.db.enc".to_string(), new_name],
        "retention did not keep exactly the newest two"
    );
    for (name, content) in others {
        // The sidecar is the live key info copied in; the rest are untouched.
        if name == recovery::KEYINFO_FILENAME {
            assert!(dest.join(name).is_file());
            continue;
        }
        assert_eq!(
            std::fs::read(dest.join(name)).unwrap(),
            content,
            "{name} was changed or deleted by retention"
        );
    }
}

#[test]
fn retention_does_not_count_look_alikes_and_never_drops_the_backup_it_just_wrote() {
    let w = World::new(None);
    let st = w.launch();
    bootstrap::auto_unlock(&st).unwrap();
    let dest = w.root.path().join("backups");
    std::fs::create_dir_all(&dest).unwrap();

    // Copies someone kept by hand. Letters sort after the digits of a
    // timestamp, so these are taken for the newest backups.
    let mine = ["atlas-notes.db.enc", "atlas-taxes.db.enc"];
    for name in mine {
        std::fs::write(dest.join(name), b"kept by hand").unwrap();
    }

    let new = run_backup(st.db().unwrap(), &dest, &w.dir, 2);

    assert!(
        new.is_file(),
        "{} was reported as written and then deleted by retention",
        new.display()
    );
    for name in mine {
        assert_eq!(
            std::fs::read(dest.join(name)).unwrap(),
            b"kept by hand",
            "{name} is not a Coin Heaven backup and was touched"
        );
    }
}

#[test]
fn choosing_the_data_folder_as_the_backup_folder_does_not_destroy_the_key_info() {
    let w = World::new(None);
    let st = w.launch();
    bootstrap::auto_unlock(&st).unwrap();
    let keyinfo = w.dir.join(recovery::KEYINFO_FILENAME);
    let before = std::fs::read(&keyinfo).unwrap();
    assert!(!before.is_empty());

    let scheduler = BackupScheduler::new();
    scheduler.mark_dirty();
    let policy = BackupPolicy {
        enabled: true,
        directory: Some(w.dir.clone()),
        min_interval_seconds: 0,
        retain_copies: 3,
    };
    // Refusing is fine, and so is writing a backup. Destroying the one
    // file the recovery phrase needs is not.
    let _ = scheduler.maybe_run(st.db().unwrap(), &policy, Trigger::Manual, &w.dir);

    assert_eq!(
        std::fs::read(&keyinfo).unwrap(),
        before,
        "atlas.keyinfo was overwritten by its own backup"
    );
    assert!(KeyInfo::load(&w.dir).unwrap().is_some());
}

/* ===================================================== 5. permissions == */

#[cfg(unix)]
#[test]
fn the_key_file_is_owner_only_however_it_comes_to_be_written() {
    let w = World::new(None);
    let key_file = w.dir.join(recovery::LOCALKEY_FILENAME);
    assert_eq!(mode(&key_file), 0o600, "written when the folder was built");

    // Overwriting a file that was group-readable (restored from a copy, say)
    // must tighten it, not keep what it had.
    set_mode(&key_file, 0o644);
    recovery::save_local_key(&w.dir, &w.dek).unwrap();
    assert_eq!(
        mode(&key_file),
        0o600,
        "an existing file kept its wider mode"
    );

    // And the recovery path, which writes it back after a lost key file.
    std::fs::remove_file(&key_file).unwrap();
    let st = w.launch();
    bootstrap::unlock_with_phrase(&st, &w.phrase).unwrap();
    assert_eq!(mode(&key_file), 0o600, "the recovered key file is wider");
}

#[cfg(unix)]
#[test]
fn first_run_without_a_passphrase_writes_only_what_it_should() {
    let root = tempfile::tempdir().unwrap();
    let st = AppState::new(root.path().join("data"));
    assert_eq!(bootstrap::setup(&st, None).unwrap(), UnlockState::Opened);
    seed(st.db().unwrap());

    let key_file = st.data_dir.join(recovery::LOCALKEY_FILENAME);
    assert_eq!(mode(&key_file), 0o600);

    let names = file_names(&st.data_dir);
    assert!(
        names.iter().all(|n| DATA_FILES.contains(&n.as_str())),
        "unexpected files: {names:?}"
    );

    // The key is on disk in exactly one place, because that is the choice
    // this setup path makes; the phrase and the key are in no other file.
    let dek = st.with_dek(|d| Ok(d.clone())).unwrap();
    assert_eq!(
        files_containing(&st.data_dir, key_hex(&dek).as_bytes()),
        [recovery::LOCALKEY_FILENAME]
    );
    assert!(files_containing(&st.data_dir, dek.as_slice()).is_empty());

    let phrase = st.take_phrase().expect("the phrase is shown once");
    assert!(
        files_containing(&st.data_dir, phrase.expose_secret().as_bytes()).is_empty(),
        "the recovery phrase was written to disk"
    );
    assert!(st.take_phrase().is_none(), "the phrase can be taken twice");

    // Nor is it inside the encrypted database, where a backup would carry it.
    let rows = dump(st.db().unwrap());
    let everything = rows
        .values()
        .flatten()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!everything.contains(phrase.expose_secret()));
    assert!(!everything.contains(&key_hex(&dek)));
}

#[cfg(unix)]
#[test]
fn first_run_with_a_passphrase_leaves_no_key_on_disk() {
    let root = tempfile::tempdir().unwrap();
    let st = AppState::new(root.path().join("data"));
    bootstrap::setup(&st, Some(PASS)).unwrap();
    seed(st.db().unwrap());
    let dek = st.with_dek(|d| Ok(d.clone())).unwrap();
    let phrase = st.take_phrase().unwrap();

    let names = file_names(&st.data_dir);
    assert!(
        !names.contains(&recovery::LOCALKEY_FILENAME.to_string()),
        "a key file would let anyone step around the passphrase"
    );
    for (what, secret) in [
        ("the key as hex", key_hex(&dek).into_bytes()),
        ("the key as bytes", dek.to_vec()),
        ("the passphrase", PASS.as_bytes().to_vec()),
        (
            "the recovery phrase",
            phrase.expose_secret().as_bytes().to_vec(),
        ),
    ] {
        assert!(
            files_containing(&st.data_dir, &secret).is_empty(),
            "{what} is written to disk"
        );
    }

    // A restart asks for the passphrase and stays locked until it has it.
    let next = AppState::new(st.data_dir.clone());
    assert_eq!(
        bootstrap::auto_unlock(&next).unwrap(),
        UnlockState::NeedsPassphrase
    );
    assert!(!next.is_open());
}

#[cfg(unix)]
#[test]
fn the_data_directory_the_app_creates_is_not_group_or_world_accessible() {
    // The folder does not exist yet, as on a first run. Whoever creates it
    // chooses its mode; the process umask (022 by default) must not.
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("com.coinheaven.app");
    let st = AppState::new(dir.clone());
    bootstrap::setup(&st, None).unwrap();

    let m = mode(&dir);
    assert_eq!(m & 0o077, 0, "the data folder is {m:o}, not owner-only");
}

#[cfg(unix)]
#[test]
fn the_database_and_key_info_files_are_not_group_or_world_readable() {
    // THREAT-MODEL.md: "Another user account on the same machine: 0600 file
    // permissions". The database is ciphertext, but key info is exactly what
    // an offline guess at a weak passphrase needs, and the claim should hold.
    let w = World::new(None);
    let st = w.launch();
    bootstrap::auto_unlock(&st).unwrap(); // open, so the -wal and -shm exist

    let loose: Vec<String> = file_names(&w.dir)
        .into_iter()
        .filter(|n| mode(&w.dir.join(n)) & 0o077 != 0)
        .map(|n| format!("{n} ({:o})", mode(&w.dir.join(&n))))
        .collect();
    assert!(loose.is_empty(), "readable by group or others: {loose:?}");
}

/* ================================= 6. adopting the previous data folder == */

const PREVIOUS: &str = "com.atlas.wealth";
const CURRENT: &str = "com.coinheaven.app";

#[test]
fn adopting_the_previous_folder_gives_a_working_install_and_leaves_the_old_one_alone() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join(PREVIOUS);
    let new = root.path().join(CURRENT);
    build_world(&old, None);
    std::fs::create_dir_all(old.join("backups")).unwrap();
    std::fs::write(old.join("backups/atlas-20250101T000000Z.db.enc"), b"older").unwrap();
    let original = snapshot(&old);

    assert!(bootstrap::adopt_previous_data_dir(&new).unwrap());
    assert_eq!(snapshot(&old), original, "the old folder was changed");
    assert!(new.join("backups/atlas-20250101T000000Z.db.enc").is_file());

    // The copy is a complete install: keys came across with the database.
    let st = AppState::new(new.clone());
    assert_eq!(bootstrap::auto_unlock(&st).unwrap(), UnlockState::Opened);
    assert_eq!(account_names(st.db().unwrap()), [MARKER]);
    drop(st);

    // Once only. A second launch must not touch what is there now.
    std::fs::write(new.join("atlas.db"), b"changed since").unwrap();
    assert!(!bootstrap::adopt_previous_data_dir(&new).unwrap());
    assert_eq!(
        std::fs::read(new.join("atlas.db")).unwrap(),
        b"changed since"
    );
}

#[test]
fn an_existing_database_is_never_replaced_and_its_keys_are_never_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join(PREVIOUS);
    let new = root.path().join(CURRENT);
    build_world(&old, None);
    std::fs::create_dir_all(&new).unwrap();
    let mine: [(&str, &[u8]); 3] = [
        ("atlas.db", b"the current database"),
        (recovery::KEYINFO_FILENAME, b"the current key info"),
        (recovery::LOCALKEY_FILENAME, b"the current key file"),
    ];
    for (name, content) in mine {
        std::fs::write(new.join(name), content).unwrap();
    }
    let before = snapshot(&new);

    assert!(!bootstrap::adopt_previous_data_dir(&new).unwrap());
    assert_eq!(
        snapshot(&new),
        before,
        "adoption changed an existing install"
    );
}

#[test]
fn a_previous_folder_without_a_database_is_not_adopted() {
    // Key files alone are not data, and copying them would only plant a
    // key file beside a database that does not use it.
    let root = tempfile::tempdir().unwrap();
    let old = root.path().join(PREVIOUS);
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join(recovery::KEYINFO_FILENAME), b"{}").unwrap();
    std::fs::write(old.join(recovery::LOCALKEY_FILENAME), "a".repeat(64)).unwrap();
    let new = root.path().join(CURRENT);

    assert!(!bootstrap::adopt_previous_data_dir(&new).unwrap());
    assert!(!new.exists(), "a folder was created for nothing");

    // And the old folder is not "adopted" into itself.
    build_world(&root.path().join("elsewhere"), None);
    assert!(!bootstrap::adopt_previous_data_dir(&old).unwrap());
}

#[cfg(unix)]
#[test]
fn adoption_does_not_make_the_key_file_more_accessible_than_it_was() {
    for original in [0o600, 0o400, 0o640] {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join(PREVIOUS);
        let new = root.path().join(CURRENT);
        build_world(&old, None);
        set_mode(&old.join(recovery::LOCALKEY_FILENAME), original);

        assert!(bootstrap::adopt_previous_data_dir(&new).unwrap());

        let adopted = mode(&new.join(recovery::LOCALKEY_FILENAME));
        assert_eq!(
            adopted & !original,
            0,
            "key file went from {original:o} to {adopted:o}"
        );
    }
}

/// An old folder holding a file that cannot be read, so the copy fails part
/// way whatever order the filesystem lists things in.
#[cfg(unix)]
fn old_folder_that_fails_midway(root: &Path) -> (PathBuf, PathBuf) {
    let old = root.join(PREVIOUS);
    std::fs::create_dir_all(&old).unwrap();
    for name in [
        "atlas.db",
        recovery::KEYINFO_FILENAME,
        recovery::LOCALKEY_FILENAME,
    ] {
        std::fs::write(old.join(name), name).unwrap();
    }
    let obstacle = old.join("unreadable.dat");
    std::fs::write(&obstacle, b"x").unwrap();
    set_mode(&obstacle, 0o000);
    assert!(
        std::fs::read(&obstacle).is_err(),
        "file modes are not enforced for this user"
    );
    (old, obstacle)
}

#[cfg(unix)]
#[test]
fn a_failed_adoption_does_not_leave_a_half_copied_folder_that_blocks_the_retry() {
    let root = tempfile::tempdir().unwrap();
    let (old, obstacle) = old_folder_that_fails_midway(root.path());
    let new = root.path().join(CURRENT);
    let needed = [
        "atlas.db",
        recovery::KEYINFO_FILENAME,
        recovery::LOCALKEY_FILENAME,
    ];

    assert!(bootstrap::adopt_previous_data_dir(&new).is_err());

    // The next launch decides from "does atlas.db exist": so the folder must
    // hold either nothing of the old data, or all of it.
    let missing: Vec<&str> = needed
        .iter()
        .copied()
        .filter(|n| !new.join(n).exists())
        .collect();
    assert!(
        missing.is_empty() || !new.join("atlas.db").exists(),
        "atlas.db was copied without {missing:?}, and the next launch will not try again"
    );

    // Once the obstacle is gone the retry has to finish the job.
    std::fs::remove_file(&obstacle).unwrap();
    bootstrap::adopt_previous_data_dir(&new).unwrap();
    for name in needed {
        assert!(
            new.join(name).is_file(),
            "{name} is still missing after a retry"
        );
    }
    assert!(old.join("atlas.db").is_file());
}

/* ================================ 7. setting and removing a passphrase == */

#[test]
fn setting_a_passphrase_removes_the_key_file_and_keeps_the_data_readable() {
    let w = World::new(None);
    let st = w.launch();
    assert_eq!(bootstrap::auto_unlock(&st).unwrap(), UnlockState::Opened);
    let dest = w.root.path().join("backups");

    passphrase_set_as_the_command_does(&st, PASS).unwrap();

    // What is on disk now.
    let names = file_names(&w.dir);
    assert!(
        names.iter().all(|n| DATA_FILES.contains(&n.as_str())),
        "unexpected files: {names:?}"
    );
    assert!(
        !names.contains(&recovery::LOCALKEY_FILENAME.to_string()),
        "the plaintext key is still on disk beside the passphrase"
    );
    assert!(w.key_info().has_passphrase());
    for (what, secret) in [
        ("key as hex", key_hex(&w.dek).into_bytes()),
        ("key as bytes", w.dek.to_vec()),
        ("passphrase", PASS.as_bytes().to_vec()),
    ] {
        assert!(
            files_containing(&w.dir, &secret).is_empty(),
            "the {what} is written to disk"
        );
    }
    // The same key: the database was not re-encrypted and still reads.
    assert!(w.key_info().matches(&w.dek));
    assert_eq!(account_names(st.db().unwrap()), [MARKER]);
    run_backup(st.db().unwrap(), &dest, &w.dir, 14);
    drop(st);

    // The next launch needs the passphrase, and then sees the same rows.
    let next = w.launch();
    assert_eq!(
        bootstrap::auto_unlock(&next).unwrap(),
        UnlockState::NeedsPassphrase
    );
    assert!(!next.is_open());
    bootstrap::unlock_with_passphrase(&next, PASS).unwrap();
    assert_eq!(account_names(next.db().unwrap()), [MARKER]);
}

#[test]
fn removing_a_passphrase_leaves_the_data_readable_and_no_old_wrap_behind() {
    let w = World::new(Some(PASS));
    let st = w.launch();
    bootstrap::unlock_with_passphrase(&st, PASS).unwrap();
    let dest = w.root.path().join("backups");

    // A backup taken while the passphrase was set carries a wrap that opens.
    run_backup(st.db().unwrap(), &dest, &w.dir, 14);
    let old_wrap = w
        .key_info()
        .wrap_for(WrapKind::Passphrase)
        .cloned()
        .unwrap();
    let in_backup = KeyInfo::load(&dest).unwrap().unwrap();
    assert!(in_backup.unlock(PASS, WrapKind::Passphrase).is_ok());

    passphrase_remove_as_the_command_does(&st).unwrap();

    // The live files.
    let info = w.key_info();
    assert!(!info.has_passphrase());
    assert_eq!(info.wraps.len(), 1);
    let text = std::fs::read_to_string(w.dir.join(recovery::KEYINFO_FILENAME)).unwrap();
    for (field, value) in [
        ("salt", &old_wrap.salt_b64),
        ("nonce", &old_wrap.nonce_b64),
        ("ciphertext", &old_wrap.ciphertext_b64),
    ] {
        assert!(
            !text.contains(value.as_str()),
            "the old wrap's {field} is still in the file"
        );
    }
    let names = file_names(&w.dir);
    assert!(
        names.iter().all(|n| DATA_FILES.contains(&n.as_str())),
        "stray files left behind: {names:?}"
    );
    assert!(info.unlock(PASS, WrapKind::Passphrase).is_err());

    // The data opens without a secret, by the phrase, and not by the old passphrase.
    drop(st);
    let auto = w.launch();
    assert_eq!(bootstrap::auto_unlock(&auto).unwrap(), UnlockState::Opened);
    assert_eq!(account_names(auto.db().unwrap()), [MARKER]);
    drop(auto);

    let by_phrase = w.launch();
    std::fs::remove_file(w.dir.join(recovery::LOCALKEY_FILENAME)).unwrap();
    bootstrap::unlock_with_phrase(&by_phrase, &w.phrase).unwrap();
    drop(by_phrase);

    let by_old = w.launch();
    assert!(bootstrap::unlock_with_passphrase(&by_old, PASS).is_err());
    assert!(!by_old.is_open());

    // The next backup replaces the sidecar that still listed the wrap.
    let st = w.launch();
    bootstrap::auto_unlock(&st).unwrap();
    run_backup(st.db().unwrap(), &dest, &w.dir, 14);
    let refreshed = KeyInfo::load(&dest).unwrap().unwrap();
    assert!(
        !refreshed.has_passphrase(),
        "the backup folder still holds a wrap for the removed passphrase"
    );
}

#[test]
fn recovering_with_the_phrase_does_not_write_the_key_when_a_passphrase_is_set() {
    let w = World::new(Some(PASS));
    let st = w.launch();
    bootstrap::unlock_with_phrase(&st, &w.phrase).unwrap();
    assert!(
        !w.dir.join(recovery::LOCALKEY_FILENAME).exists(),
        "recovering stored the key and defeated the passphrase"
    );
}

#[test]
fn a_stale_key_file_beside_a_passphrase_is_removed_when_the_app_starts() {
    // The command rewraps the key and only then deletes the key file; a
    // crash between the two leaves exactly this. Nothing notices afterwards,
    // so the passphrase silently protects nothing, for good.
    let w = World::new(Some(PASS));
    recovery::save_local_key(&w.dir, &w.dek).unwrap();

    let st = w.launch();
    assert_eq!(
        bootstrap::auto_unlock(&st).unwrap(),
        UnlockState::NeedsPassphrase
    );
    assert!(
        !w.dir.join(recovery::LOCALKEY_FILENAME).exists(),
        "the plaintext key still sits beside the passphrase-protected database"
    );
}

#[test]
fn setup_does_not_overwrite_a_damaged_key_info_next_to_an_existing_database() {
    // `unlock_status` reports a failed `auto_unlock` as "needs setup", so a
    // damaged sidecar puts the first-run screen in front of existing data.
    let w = World::new(None);
    let keyinfo = w.dir.join(recovery::KEYINFO_FILENAME);
    let good = std::fs::read(&keyinfo).unwrap();
    let damaged = good[..good.len() / 2].to_vec();
    std::fs::write(&keyinfo, &damaged).unwrap();

    let st = w.launch();
    assert!(bootstrap::setup(&st, None).is_err());
    assert_eq!(
        std::fs::read(&keyinfo).unwrap(),
        damaged,
        "the damaged sidecar was replaced; what was left of it can no longer be repaired by hand"
    );
}

#[test]
fn setup_does_not_write_new_keys_next_to_an_existing_database() {
    let w = World::new(None);
    std::fs::remove_file(w.dir.join(recovery::KEYINFO_FILENAME)).unwrap();

    let st = w.launch();
    assert!(bootstrap::setup(&st, None).is_err());
    assert!(
        !w.dir.join(recovery::KEYINFO_FILENAME).exists(),
        "a key info for a different key now sits beside the original database"
    );
}

#[cfg(unix)]
#[test]
fn a_failed_legacy_upgrade_leaves_the_database_openable_with_its_old_key() {
    let root = tempfile::tempdir().unwrap();
    let st = AppState::new(root.path().join("data"));
    std::fs::create_dir_all(&st.data_dir).unwrap();

    // A version 1 install: the key is Argon2id(phrase) itself.
    let phrase = recovery::generate_phrase().unwrap();
    let salt: [u8; 16] = rand::random();
    let cost = (1024, 1, 1);
    let old_key = SecretString::from(hex_of(&kek(
        &recovery::normalise_phrase(phrase.expose_secret()),
        &salt,
        cost,
    )));
    seed(&Db::open(&st.db_path, &old_key).unwrap());
    let v1 = serde_json::json!({
        "version": 1, "kdf": "argon2id",
        "mCostKib": cost.0, "tCost": cost.1, "pCost": cost.2,
        "saltB64": B64.encode(salt),
        "verifierB64": "unused",
    });
    let sidecar = st.data_dir.join(recovery::KEYINFO_FILENAME);
    std::fs::write(&sidecar, serde_json::to_string_pretty(&v1).unwrap()).unwrap();

    assert!(
        is_readable(&st.db_path, &old_key),
        "control: the v1 pair works"
    );

    // Make the folder impossible to write into, as a full disk or a read-only
    // volume would. Not meaningful for a user that ignores modes.
    set_mode(&st.data_dir, 0o555);
    if std::fs::File::create(st.data_dir.join("probe")).is_ok() {
        set_mode(&st.data_dir, 0o755);
        eprintln!("skipped: file modes are not enforced for this user");
        return;
    }
    let failed = bootstrap::upgrade_legacy(&st, phrase.expose_secret()).is_err();
    set_mode(&st.data_dir, 0o755);
    assert!(failed);

    assert!(
        is_readable(&st.db_path, &old_key),
        "the upgrade failed and the database no longer opens under the key it had"
    );
}

/* ======================================== 8. passphrase strength, output == */

#[test]
fn empty_and_short_passphrases_are_refused_everywhere_one_can_be_set() {
    // Rating: below the floor is not acceptable, and counts characters
    // rather than bytes.
    for short in ["", "a", "1234567", "ñññññññ", "p@ss1!x"] {
        let r = recovery::rate_passphrase(short);
        assert!(!r.acceptable, "{short:?} was rated acceptable");
        assert_eq!(r.score, 0);
    }
    assert!(recovery::rate_passphrase("Lm9#xQ2z").acceptable);
    assert!(recovery::rate_passphrase("ñññññññn").acceptable);

    // Enforcement does not rely on the interface having asked first.
    let w = World::new(None);
    let before = snapshot(&w.dir);
    for short in ["", "x", "1234567"] {
        assert!(matches!(
            recovery::set_passphrase(&w.dir, &w.dek, short),
            Err(AppError::Invalid(_))
        ));
    }
    assert_eq!(
        snapshot(&w.dir),
        before,
        "a refused passphrase changed files"
    );

    let fresh = tempfile::tempdir().unwrap();
    assert!(matches!(
        recovery::initialise(fresh.path(), Some("")),
        Err(AppError::Invalid(_))
    ));
    assert!(
        !fresh.path().join(recovery::KEYINFO_FILENAME).exists(),
        "a refused passphrase still produced a key info file"
    );
}

#[test]
fn common_passphrases_are_rated_as_very_weak_and_a_strong_one_is_not() {
    for common in [
        "password",
        "password123",
        "12345678",
        "qwertyuiop",
        "iloveyou",
        "letmein123",
        "11111111",
        "aaaaaaaa",
        "        ",
    ] {
        let r = recovery::rate_passphrase(common);
        assert!(r.score <= 1, "a common passphrase scored {}", r.score);
        assert!(
            !r.message.is_empty(),
            "no advice is given for a common passphrase"
        );
    }
    let strong = recovery::rate_passphrase("seven purple lanterns drift");
    assert!(strong.score >= 3, "score {}", strong.score);
    assert!(strong.acceptable);
}

#[test]
#[ignore = "FINDING: common and blank passphrases are `acceptable` and can be set; only length is enforced (a documented choice that needs the owner's decision)"]
fn a_common_or_blank_passphrase_is_not_acceptable() {
    // The code's own comment chooses to advise rather than block, and an
    // existing unit test locks that in. This asserts the stricter reading:
    // the passphrase is the only thing between a copy of atlas.keyinfo and
    // the database key, and Argon2 at this cost still falls to a short list
    // of common passwords in minutes.
    for common in [
        "password",
        "password123",
        "12345678",
        "qwertyuiop",
        "        ",
    ] {
        let r = recovery::rate_passphrase(common);
        assert!(
            !r.acceptable,
            "a common passphrase is acceptable (score {})",
            r.score
        );
    }

    let w = World::new(None);
    assert!(
        recovery::set_passphrase(&w.dir, &w.dek, "password123").is_err(),
        "set_passphrase accepted a passphrase that scores as the weakest"
    );
}

#[test]
fn rating_a_very_long_passphrase_returns_promptly() {
    // Called as the person types, on every keystroke; pasting a document
    // must not hang the window.
    for long in [
        "a".repeat(100_000),
        "correct horse battery staple ".repeat(2_000),
    ] {
        let started = std::time::Instant::now();
        let r = recovery::rate_passphrase(&long);
        assert!(r.acceptable);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "rating {} characters took {:?}",
            long.len(),
            started.elapsed()
        );
    }
}

#[test]
fn secrets_do_not_appear_in_debug_output_or_error_messages() {
    let phrase = recovery::generate_phrase().unwrap();
    let dek = recovery::generate_dek();

    // `Created` is hand-written for exactly this reason.
    let created = recovery::Created {
        phrase: phrase.clone(),
        dek: dek.clone(),
    };
    let shown = format!("{created:?} {:?}", phrase);
    assert!(
        !shown.contains(phrase.expose_secret()),
        "the phrase is printed"
    );
    assert!(!shown.contains(&key_hex(&dek)), "the key is printed as hex");
    assert!(
        !shown.contains(&format!("{:?}", *dek)),
        "the key is printed as bytes"
    );

    // What the person typed, wrongly, is a near miss of a real secret.
    let typed = "hunter2 hunter2 hunter2 hunter2";
    let info = cheap_key_info(&dek, phrase.expose_secret(), Some("my real passphrase"));
    let w = World::new(Some("my real passphrase"));
    let st = w.launch();
    let errors: Vec<AppError> = vec![
        info.unlock(typed, WrapKind::Recovery).unwrap_err(),
        info.unlock(typed, WrapKind::Passphrase).unwrap_err(),
        recovery::validate_phrase(typed).unwrap_err(),
        bootstrap::unlock_with_passphrase(&st, typed).unwrap_err(),
        bootstrap::unlock_with_phrase(&st, typed).unwrap_err(),
        Db::open(
            &w.dir.join("atlas.db"),
            &SecretString::from(typed.to_string()),
        )
        .unwrap_err(),
    ];
    for err in errors {
        let json = serde_json::to_string(&err).unwrap();
        for text in [err.to_string(), err.localised(), json] {
            assert!(
                !text.contains("hunter2"),
                "an error repeats the input: {text}"
            );
        }
    }

    // A wrong but well-formed key: the error must not echo it either.
    let wrong = "d4".repeat(32);
    let err = Db::open(&w.dir.join("atlas.db"), &SecretString::from(wrong.clone())).unwrap_err();
    assert!(!err.to_string().contains(&wrong));
}
