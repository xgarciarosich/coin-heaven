use crate::backup::{BackupPolicy, BackupScheduler};
use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::security::recovery::Dek;
use secrecy::SecretString;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Process-wide handles.
pub struct AppState {
    pub data_dir: PathBuf,
    pub db_path: PathBuf,
    db: OnceLock<Db>,
    pub backup: BackupScheduler,
    /// A freshly minted recovery phrase, held until the UI has shown it once.
    /// Never persisted, never logged, and cleared as soon as it is read.
    pending_phrase: Mutex<Option<SecretString>>,
    /// The unwrapped database key, kept for the life of the session so the
    /// passphrase can be changed without asking for a secret again. SQLCipher
    /// holds the same key anyway; this is not an additional exposure.
    dek: Mutex<Option<Dek>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        AppState {
            db_path: data_dir.join("atlas.db"),
            data_dir,
            db: OnceLock::new(),
            backup: BackupScheduler::new(),
            pending_phrase: Mutex::new(None),
            dek: Mutex::new(None),
        }
    }

    pub fn set_db(&self, db: Db) -> AppResult<()> {
        self.db
            .set(db)
            .map_err(|_| AppError::Other("database already initialised".into()))
    }

    pub fn is_open(&self) -> bool {
        self.db.get().is_some()
    }

    pub fn db(&self) -> AppResult<&Db> {
        self.db.get().ok_or(AppError::Locked)
    }

    pub fn remember_dek(&self, dek: Dek) {
        if let Ok(mut slot) = self.dek.lock() {
            *slot = Some(dek);
        }
    }

    pub fn with_dek<T>(&self, f: impl FnOnce(&Dek) -> AppResult<T>) -> AppResult<T> {
        let guard = self
            .dek
            .lock()
            .map_err(|_| AppError::Other("key mutex poisoned".into()))?;
        let dek = guard.as_ref().ok_or(AppError::Locked)?;
        f(dek)
    }

    pub fn stash_phrase(&self, phrase: SecretString) {
        if let Ok(mut slot) = self.pending_phrase.lock() {
            *slot = Some(phrase);
        }
    }

    /// Hand the phrase to the UI exactly once, then forget it.
    pub fn take_phrase(&self) -> Option<SecretString> {
        self.pending_phrase.lock().ok().and_then(|mut s| s.take())
    }

    pub fn has_pending_phrase(&self) -> bool {
        self.pending_phrase
            .lock()
            .map(|s| s.is_some())
            .unwrap_or(false)
    }

    /// Read the backup policy out of `app_setting`.
    pub fn backup_policy(&self) -> AppResult<BackupPolicy> {
        let db = self.db()?;
        db.with(|c| {
            let get = |key: &str| -> Option<String> {
                c.query_row(
                    "SELECT value_json FROM app_setting WHERE key = ?1",
                    rusqlite::params![key],
                    |r| r.get::<_, String>(0),
                )
                .ok()
            };
            let json = |key: &str| {
                get(key).and_then(|v| serde_json::from_str::<serde_json::Value>(&v).ok())
            };

            Ok(BackupPolicy {
                enabled: json("backup.enabled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                directory: json("backup.directory").and_then(|v| v.as_str().map(PathBuf::from)),
                min_interval_seconds: json("backup.min_interval_seconds")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(300),
                retain_copies: json("backup.retain_copies")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(14) as usize,
            })
        })
    }
}
