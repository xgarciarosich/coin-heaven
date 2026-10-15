//! Key hierarchy: envelope encryption with two independent unlock paths.
//!
//! ```text
//!                 ┌──────────────────────────────┐
//!                 │  DEK — 32 random bytes       │──── encrypts the database
//!                 └──────────────────────────────┘
//!                    ▲              ▲           ▲
//!        wrapped by  │              │           │  cached as-is in
//!   Argon2id(phrase) │              │           │  the OS keychain
//!                    │   Argon2id(passphrase)   │  (only when no passphrase)
//!             always present        optional
//! ```
//!
//! The database is encrypted with a random Data Encryption Key that is never
//! derived from anything. That key is then *wrapped* — encrypted — once under
//! the recovery phrase and, if the user wants one, again under a passphrase.
//!
//! This is what makes the two paths independent. Setting, changing or removing
//! a passphrase rewraps a 32-byte key; it never re-encrypts the database. And
//! forgetting the passphrase is survivable, because the recovery phrase still
//! unwraps the same DEK.
//!
//! The earlier design derived the database key from the recovery phrase
//! directly. That allowed exactly one secret, which is why adding a passphrase
//! required this restructure rather than another `derive_key` call.
//!
//! `atlas.keyinfo` holds the wraps. It is not secret — each wrap is ciphertext
//! under a key derived from a secret the file does not contain — but it *is*
//! required, so it travels next to every backup.

use crate::error::{AppError, AppResult};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use bip39::Mnemonic;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use zeroize::{Zeroize, Zeroizing};

pub const KEYINFO_FILENAME: &str = "atlas.keyinfo";

/// Where the key lives when the user chose not to set a passphrase.
///
/// Deliberately a *separate* file from `atlas.keyinfo`, because the sidecar is
/// copied next to every backup. Putting the raw key there would ship each
/// backup with its own decryption key; keeping it here means a backup folder
/// on its own is still ciphertext.
pub const LOCALKEY_FILENAME: &str = "atlas.localkey";
const VERIFIER_DOMAIN: &[u8] = b"atlas-dek-verifier-v2";
const AAD_DOMAIN: &[u8] = b"atlas-keywrap-v2";

/// Argon2id cost: ~64 MiB and a fraction of a second on current hardware.
/// Stored per wrap so it can be raised later without breaking existing files.
const M_COST_KIB: u32 = 65_536;
const T_COST: u32 = 3;
const P_COST: u32 = 1;

/// Shortest passphrase accepted. Deliberately a length floor rather than a
/// character-class rule: "correct horse battery staple" beats "P@ss1!".
pub const MIN_PASSPHRASE_LEN: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WrapKind {
    Recovery,
    Passphrase,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Wrap {
    pub kind: WrapKind,
    pub salt_b64: String,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KeyInfo {
    pub version: u32,
    pub kdf: String,
    pub wraps: Vec<Wrap>,
    /// SHA-256 over a domain tag and the DEK. Lets a wrong secret be reported
    /// as "wrong passphrase" rather than SQLCipher's "file is not a database".
    pub verifier_b64: String,
}

/// A 32-byte data encryption key, zeroed on drop.
pub type Dek = Zeroizing<[u8; 32]>;

pub fn generate_dek() -> Dek {
    let mut dek = [0u8; 32];
    rand::rng().fill_bytes(&mut dek);
    Zeroizing::new(dek)
}

/// Hex form, for `PRAGMA key = "x'...'"`.
pub fn dek_to_hex(dek: &Dek) -> SecretString {
    SecretString::from(dek.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// Public form of the verifier, for callers assembling a `KeyInfo` by hand
/// (the one-time v1 upgrade does this).
pub fn verifier_b64(dek: &Dek) -> String {
    B64.encode(verifier(dek))
}

fn verifier(dek: &Dek) -> Vec<u8> {
    let mut h = Sha256::new();
    h.update(VERIFIER_DOMAIN);
    h.update(dek.as_slice());
    h.finalize().to_vec()
}

/// Argon2id over a user secret, producing a 32-byte key-encryption key.
fn derive_kek(
    secret: &str,
    salt: &[u8],
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
) -> AppResult<Zeroizing<[u8; 32]>> {
    let params = Params::new(m_cost_kib, t_cost, p_cost, Some(32))
        .map_err(|e| AppError::Other(format!("bad Argon2 parameters: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(secret.as_bytes(), salt, out.as_mut_slice())
        .map_err(|e| AppError::Other(format!("key derivation failed: {e}")))?;
    Ok(out)
}

/// Normalise before deriving, so spacing and case in a transcribed phrase do
/// not change the key. Passphrases are *not* normalised — they are typed by
/// the person who chose them, and folding case would throw away entropy.
fn prepare_secret(secret: &str, kind: WrapKind) -> String {
    match kind {
        WrapKind::Recovery => normalise_phrase(secret),
        WrapKind::Passphrase => secret.to_string(),
    }
}

pub fn wrap_dek(dek: &Dek, secret: &str, kind: WrapKind) -> AppResult<Wrap> {
    // Fresh from the system's random source every time: a wrap never reuses a
    // salt or a nonce.
    let mut salt: [u8; 16] = rand::random();
    let mut nonce: [u8; 24] = rand::random();

    let kek = derive_kek(
        &prepare_secret(secret, kind),
        &salt,
        M_COST_KIB,
        T_COST,
        P_COST,
    )?;
    let cipher = XChaCha20Poly1305::new_from_slice(kek.as_slice())
        .map_err(|_| AppError::Other("invalid key-encryption key".into()))?;
    let ciphertext = cipher
        .encrypt(
            &XNonce::try_from(&nonce[..])
                .map_err(|_| AppError::Other("bad nonce length".into()))?,
            chacha20poly1305::aead::Payload {
                msg: dek.as_slice(),
                aad: AAD_DOMAIN,
            },
        )
        .map_err(|_| AppError::Other("could not wrap the database key".into()))?;

    let wrap = Wrap {
        kind,
        salt_b64: B64.encode(salt),
        nonce_b64: B64.encode(nonce),
        ciphertext_b64: B64.encode(&ciphertext),
        m_cost_kib: M_COST_KIB,
        t_cost: T_COST,
        p_cost: P_COST,
    };
    salt.zeroize();
    nonce.zeroize();
    Ok(wrap)
}

/// Returns `None` when the secret is wrong — the AEAD tag simply fails to
/// verify, which is exactly the check we want and is constant time.
pub fn unwrap_dek(wrap: &Wrap, secret: &str) -> AppResult<Option<Dek>> {
    let salt = B64
        .decode(&wrap.salt_b64)
        .map_err(|e| AppError::Other(format!("key info salt is invalid: {e}")))?;
    let nonce = B64
        .decode(&wrap.nonce_b64)
        .map_err(|e| AppError::Other(format!("key info nonce is invalid: {e}")))?;
    let ct = B64
        .decode(&wrap.ciphertext_b64)
        .map_err(|e| AppError::Other(format!("key info ciphertext is invalid: {e}")))?;
    if nonce.len() != 24 {
        return Err(AppError::Other("key info nonce has the wrong size".into()));
    }

    // The file is not authenticated before it is read, so its cost parameters
    // are bounded: a tampered value must not be able to ask for terabytes.
    if !(8..=1_048_576).contains(&wrap.m_cost_kib)
        || !(1..=32).contains(&wrap.t_cost)
        || !(1..=16).contains(&wrap.p_cost)
    {
        return Err(AppError::Other(
            "key info uses unreasonable key-derivation settings".into(),
        ));
    }
    let kek = derive_kek(
        &prepare_secret(secret, wrap.kind),
        &salt,
        wrap.m_cost_kib,
        wrap.t_cost,
        wrap.p_cost,
    )?;
    let cipher = XChaCha20Poly1305::new_from_slice(kek.as_slice())
        .map_err(|_| AppError::Other("invalid key-encryption key".into()))?;
    let Ok(plain) = cipher.decrypt(
        &XNonce::try_from(&nonce[..]).map_err(|_| AppError::Other("bad nonce length".into()))?,
        chacha20poly1305::aead::Payload {
            msg: &ct,
            aad: AAD_DOMAIN,
        },
    ) else {
        return Ok(None);
    };
    if plain.len() != 32 {
        return Err(AppError::Other("unwrapped key has the wrong size".into()));
    }
    let mut dek = [0u8; 32];
    dek.copy_from_slice(&plain);
    Ok(Some(Zeroizing::new(dek)))
}

impl KeyInfo {
    pub fn load(dir: &Path) -> AppResult<Option<KeyInfo>> {
        let path = dir.join(KEYINFO_FILENAME);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| AppError::Other(format!("cannot read {KEYINFO_FILENAME}: {e}")))?;
        serde_json::from_str(&raw)
            .map(Some)
            .map_err(|e| AppError::Other(format!("{KEYINFO_FILENAME} is corrupt: {e}")))
    }

    pub fn save(&self, dir: &Path) -> AppResult<()> {
        std::fs::create_dir_all(dir)
            .map_err(|e| AppError::Other(format!("cannot create {}: {e}", dir.display())))?;
        self.save_as(&dir.join(KEYINFO_FILENAME))
    }

    /// Write to an explicit path (private, and replaced whole).
    pub fn save_as(&self, path: &Path) -> AppResult<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Other(format!("cannot serialise key info: {e}")))?;
        write_private(path, json.as_bytes())
            .map_err(|e| AppError::Other(format!("cannot write {KEYINFO_FILENAME}: {e}")))
    }

    pub fn wrap_for(&self, kind: WrapKind) -> Option<&Wrap> {
        self.wraps.iter().find(|w| w.kind == kind)
    }

    pub fn has_passphrase(&self) -> bool {
        self.wrap_for(WrapKind::Passphrase).is_some()
    }

    /// Is this the DEK this file was built around?
    pub fn matches(&self, dek: &Dek) -> bool {
        match B64.decode(&self.verifier_b64) {
            Ok(expected) => {
                let got = verifier(dek);
                expected.len() == got.len()
                    && expected
                        .iter()
                        .zip(got.iter())
                        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                        == 0
            }
            Err(_) => false,
        }
    }

    /// Unlock with one secret, trying only the wrap of that kind.
    pub fn unlock(&self, secret: &str, kind: WrapKind) -> AppResult<Dek> {
        // Only what this build knows how to read. A file from a newer version
        // or another scheme must fail loudly, not be read as if it were ours.
        if self.version != 2 || self.kdf != "argon2id" {
            return Err(AppError::Other(
                "key info is from a version this build cannot read".into(),
            ));
        }
        let wrap = self.wrap_for(kind).ok_or_else(|| match kind {
            WrapKind::Passphrase => {
                AppError::Invalid("this database does not use a passphrase".into())
            }
            WrapKind::Recovery => {
                AppError::Other("key info has no recovery wrap — it is corrupt".into())
            }
        })?;
        match unwrap_dek(wrap, secret)? {
            Some(dek) if self.matches(&dek) => Ok(dek),
            _ => Err(AppError::Invalid(match kind {
                WrapKind::Passphrase => "that passphrase is not correct".into(),
                WrapKind::Recovery => "that recovery phrase does not match this database".into(),
            })),
        }
    }
}

/* ------------------------------------------------------------- phrases -- */

/// A fresh 12-word English recovery phrase.
pub fn generate_phrase() -> AppResult<SecretString> {
    let mut entropy = [0u8; 16]; // 128 bits -> 12 words
    rand::rng().fill_bytes(&mut entropy);
    let m = Mnemonic::from_entropy(&entropy)
        .map_err(|e| AppError::Other(format!("cannot generate recovery phrase: {e}")))?;
    entropy.zeroize();
    Ok(SecretString::from(m.to_string()))
}

pub fn normalise_phrase(input: &str) -> String {
    input
        .split_whitespace()
        .map(|w| w.trim().to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn validate_phrase(input: &str) -> AppResult<()> {
    let norm = normalise_phrase(input);
    let words = norm.split_whitespace().count();
    if words != 12 {
        return Err(AppError::Invalid(format!(
            "a recovery phrase has 12 words; got {words}"
        )));
    }
    Mnemonic::parse_normalized(&norm).map_err(|e| {
        AppError::Invalid(format!(
            "that is not a valid recovery phrase ({e}). Check for a mistyped word."
        ))
    })?;
    Ok(())
}

/// How good is this passphrase? Score is zxcvbn's 0–4.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PassphraseStrength {
    pub score: u8,
    pub acceptable: bool,
    pub message: String,
}

pub fn rate_passphrase(passphrase: &str) -> PassphraseStrength {
    if passphrase.chars().count() < MIN_PASSPHRASE_LEN {
        return PassphraseStrength {
            score: 0,
            acceptable: false,
            message: crate::i18n::pick(
                format!("At least {MIN_PASSPHRASE_LEN} characters."),
                format!("Al menos {MIN_PASSPHRASE_LEN} caracteres."),
            ),
        };
    }
    let score = u8::from(zxcvbn::zxcvbn(passphrase, &[]).score());
    use crate::i18n::pick;
    let message = match score {
        0 | 1 => pick(
            "Very guessable. A few unrelated words would be far stronger.",
            "Muy fácil de adivinar. Unas cuantas palabras sin relación serían mucho más seguras.",
        ),
        2 => pick(
            "Still guessable. Consider adding another word.",
            "Todavía se puede adivinar. Plantéate añadir otra palabra.",
        ),
        3 => pick("Good.", "Buena."),
        _ => pick("Strong.", "Robusta."),
    };
    PassphraseStrength {
        score,
        // Length is the hard floor; a short-but-odd string is still refused
        // above. Beyond that we advise rather than block — refusing a
        // passphrase someone will actually remember helps nobody.
        acceptable: true,
        message: message.into(),
    }
}

/* ----------------------------------------------------------- lifecycle -- */

/// Hex -> key. Returns `None` for anything that is not 64 hex characters.
pub fn dek_from_hex(hex: &str) -> Option<Dek> {
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(Zeroizing::new(out))
}

/// Write the key beside the database, readable only by this user.
///
/// This is what "open automatically" means, stated plainly: the key sits next
/// to the data it unlocks, so anything that can read your home folder can read
/// your finances. The protection is your account password and full-disk
/// encryption — see docs/THREAT-MODEL.md. Choosing a passphrase instead means
/// no such file exists.
pub fn save_local_key(dir: &Path, dek: &Dek) -> AppResult<()> {
    std::fs::create_dir_all(dir)
        .map_err(|e| AppError::Other(format!("cannot create {}: {e}", dir.display())))?;
    write_private(
        &dir.join(LOCALKEY_FILENAME),
        dek_to_hex(dek).expose_secret().as_bytes(),
    )
    .map_err(|e| AppError::Other(format!("cannot write {LOCALKEY_FILENAME}: {e}")))
}

/// Write a file that only this user may read, without a window in which it is
/// wider or half-written: the bytes go to a sibling created `0600`, are flushed
/// to disk, and only then replace the real file by rename. A crash or a full
/// disk leaves the previous file intact instead of a truncated key.
/// (On Windows a file inherits the user-profile folder's owner-only ACL.)
pub(crate) fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let tmp = path.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = (|| {
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn load_local_key(dir: &Path) -> AppResult<Option<Dek>> {
    let path = dir.join(LOCALKEY_FILENAME);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| AppError::Other(format!("cannot read {LOCALKEY_FILENAME}: {e}")))?;
    Ok(dek_from_hex(raw.trim()))
}

pub fn forget_local_key(dir: &Path) -> AppResult<()> {
    let path = dir.join(LOCALKEY_FILENAME);
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| AppError::Other(format!("cannot remove {LOCALKEY_FILENAME}: {e}")))?;
    }
    Ok(())
}

/// The outcome of first-run setup: the phrase to show once, and the key.
pub struct Created {
    pub phrase: SecretString,
    pub dek: Dek,
}

/// Hand-written rather than derived. `Zeroizing<[u8; 32]>` prints its bytes,
/// so a derived `Debug` would spill the database key into any log line or
/// panic message that formatted this struct.
impl std::fmt::Debug for Created {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Created")
            .field("phrase", &"[redacted]")
            .field("dek", &"[redacted]")
            .finish()
    }
}

/// Create a fresh key hierarchy. A passphrase is optional; the recovery phrase
/// is not, because it is the only thing that survives everything else.
pub fn initialise(dir: &Path, passphrase: Option<&str>) -> AppResult<Created> {
    let dek = generate_dek();
    let phrase = generate_phrase()?;

    let mut wraps = vec![wrap_dek(&dek, phrase.expose_secret(), WrapKind::Recovery)?];
    if let Some(pass) = passphrase {
        if pass.chars().count() < MIN_PASSPHRASE_LEN {
            return Err(AppError::Invalid(format!(
                "a passphrase needs at least {MIN_PASSPHRASE_LEN} characters"
            )));
        }
        wraps.push(wrap_dek(&dek, pass, WrapKind::Passphrase)?);
    }

    KeyInfo {
        version: 2,
        kdf: "argon2id".into(),
        wraps,
        verifier_b64: B64.encode(verifier(&dek)),
    }
    .save(dir)?;

    Ok(Created { phrase, dek })
}

/// Add or replace the passphrase wrap. Rewraps the same DEK — the database is
/// untouched, so this is instant regardless of how large it has grown.
pub fn set_passphrase(dir: &Path, dek: &Dek, passphrase: &str) -> AppResult<()> {
    if passphrase.chars().count() < MIN_PASSPHRASE_LEN {
        return Err(AppError::Invalid(format!(
            "a passphrase needs at least {MIN_PASSPHRASE_LEN} characters"
        )));
    }
    let mut info = KeyInfo::load(dir)?
        .ok_or_else(|| AppError::Other(format!("{KEYINFO_FILENAME} is missing")))?;
    if !info.matches(dek) {
        return Err(AppError::Other(
            "refusing to rewrap: this key does not belong to this database".into(),
        ));
    }
    info.wraps.retain(|w| w.kind != WrapKind::Passphrase);
    info.wraps
        .push(wrap_dek(dek, passphrase, WrapKind::Passphrase)?);
    info.save(dir)
}

/// Drop the passphrase wrap. The recovery phrase still opens the database.
pub fn remove_passphrase(dir: &Path) -> AppResult<()> {
    let mut info = KeyInfo::load(dir)?
        .ok_or_else(|| AppError::Other(format!("{KEYINFO_FILENAME} is missing")))?;
    info.wraps.retain(|w| w.kind != WrapKind::Passphrase);
    info.save(dir)
}

#[cfg(test)]
mod tests;
