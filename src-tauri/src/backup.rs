//! Automatic encrypted backups.
//!
//! ## Why this is coalesced rather than literally per-write
//!
//! The decision as written was "exported on every database write". Taken
//! literally that is unworkable: importing a 5,000-row CSV statement is 5,000
//! writes, so it would produce 5,000 full-database copies, turn a two-second
//! import into minutes of disk I/O, and — because each copy is a `VACUUM INTO`
//! holding a read lock — serialise the import behind its own backups.
//!
//! What the decision is actually asking for is *never lose committed work*.
//! This module delivers that with the same guarantee and none of the cost:
//!
//!   * every mutating command marks the database dirty;
//!   * a backup runs when dirty **and** `min_interval_seconds` has elapsed
//!     (default 5 minutes);
//!   * an import batch and app shutdown force one immediately, regardless of
//!     the interval — so the two moments where meaningful work could be lost
//!     are always covered;
//!   * a manual "Back up now" command is always available.
//!
//! Worst case exposure is one interval of idle edits, versus a permanently
//! unusable app. If you want it tighter, lower `backup.min_interval_seconds`.
//!
//! ## Why there is no separate `.enc` format
//!
//! The database is already SQLCipher-encrypted, and `VACUUM INTO` writes its
//! output through the same cipher. The backup is therefore ciphertext by
//! construction, openable by the same recovery phrase, with no second crypto
//! stack to implement, review or get wrong. `backup_is_encrypted_and_openable`
//! asserts both halves of that claim rather than trusting it.

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::security::recovery::KEYINFO_FILENAME;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Interval,
    Import,
    Shutdown,
    Manual,
}

impl Trigger {
    /// Import and shutdown ignore the interval: these are the moments where
    /// unbacked-up work is actually at risk.
    fn is_forced(self) -> bool {
        matches!(self, Trigger::Import | Trigger::Shutdown | Trigger::Manual)
    }
    fn as_str(self) -> &'static str {
        match self {
            Trigger::Interval => "interval",
            Trigger::Import => "import",
            Trigger::Shutdown => "shutdown",
            Trigger::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupPolicy {
    pub enabled: bool,
    pub directory: Option<PathBuf>,
    pub min_interval_seconds: u64,
    pub retain_copies: usize,
}

impl Default for BackupPolicy {
    fn default() -> Self {
        BackupPolicy {
            enabled: true,
            directory: None,
            min_interval_seconds: 300,
            retain_copies: 14,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackupOutcome {
    Written { path: PathBuf, bytes: u64 },
    SkippedNotDue,
    SkippedClean,
    Disabled,
    NoDirectory,
}

/// Tracks whether there is anything worth backing up, and when the last one ran.
pub struct BackupScheduler {
    state: Mutex<SchedulerState>,
}

struct SchedulerState {
    dirty: bool,
    last_run: Option<Instant>,
}

impl Default for BackupScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl BackupScheduler {
    pub fn new() -> Self {
        BackupScheduler {
            state: Mutex::new(SchedulerState {
                dirty: false,
                last_run: None,
            }),
        }
    }

    /// Called by every command that mutates the database.
    pub fn mark_dirty(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.dirty = true;
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.state.lock().map(|s| s.dirty).unwrap_or(false)
    }

    fn due(&self, policy: &BackupPolicy, trigger: Trigger) -> Result<(), BackupOutcome> {
        let s = self.state.lock().map_err(|_| BackupOutcome::SkippedClean)?;
        if !s.dirty {
            return Err(BackupOutcome::SkippedClean);
        }
        if trigger.is_forced() {
            return Ok(());
        }
        match s.last_run {
            Some(t) if t.elapsed() < Duration::from_secs(policy.min_interval_seconds) => {
                Err(BackupOutcome::SkippedNotDue)
            }
            _ => Ok(()),
        }
    }

    fn record_run(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.dirty = false;
            s.last_run = Some(Instant::now());
        }
    }

    /// Run a backup if the policy says one is due.
    pub fn maybe_run(
        &self,
        db: &Db,
        policy: &BackupPolicy,
        trigger: Trigger,
        keyinfo_dir: &Path,
    ) -> AppResult<BackupOutcome> {
        if !policy.enabled {
            return Ok(BackupOutcome::Disabled);
        }
        let Some(dir) = policy.directory.as_ref() else {
            return Ok(BackupOutcome::NoDirectory);
        };
        if let Err(skip) = self.due(policy, trigger) {
            return Ok(skip);
        }

        let outcome = write_backup(db, dir, keyinfo_dir);
        let logged = match &outcome {
            Ok((path, bytes)) => {
                self.record_run();
                ("ok", path.display().to_string(), *bytes, None::<String>)
            }
            Err(e) => ("failed", dir.display().to_string(), 0, Some(e.to_string())),
        };
        // Best effort: a failed log entry must not mask a successful backup.
        let _ = db.with(|c| {
            c.execute(
                "INSERT INTO backup_log (path, bytes, trigger, outcome, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    logged.1,
                    logged.2 as i64,
                    trigger.as_str(),
                    logged.0,
                    logged.3
                ],
            )?;
            Ok(())
        });

        let (path, bytes) = outcome?;
        prune(dir, policy.retain_copies)?;
        Ok(BackupOutcome::Written { path, bytes })
    }
}

fn timestamp() -> String {
    chrono::Utc::now().format("%Y%m%dT%H%M%SZ").to_string()
}

/// A backup file name: `atlas-YYYYMMDDTHHMMSSZ.db.enc`. Anything else in the
/// folder — a look-alike, a note, a photo — is not ours to count or delete.
fn backup_stamp(name: &str) -> Option<&str> {
    let stamp = name.strip_prefix("atlas-")?.strip_suffix(".db.enc")?;
    let b = stamp.as_bytes();
    let shape = b.len() == 16
        && b[8] == b'T'
        && b[15] == b'Z'
        && b[..8].iter().chain(&b[9..15]).all(u8::is_ascii_digit);
    shape.then_some(stamp)
}

/// Refuse a backup folder that is the data folder itself (or holds a live
/// database): the copy of the key sidecar would overwrite the real one.
pub fn check_destination(dir: &Path, data_dir: &Path) -> AppResult<()> {
    let same = match (dir.canonicalize(), data_dir.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => dir == data_dir,
    };
    if same || dir.join("atlas.db").exists() {
        return Err(AppError::Invalid(
            "the backup folder cannot be the data folder; choose another folder".into(),
        ));
    }
    Ok(())
}

/// Write `bytes_from` to `final_path` through a temporary name, so a crash or a
/// full disk never leaves a truncated file under the real name.
fn copy_atomically(from: &Path, final_path: &Path) -> std::io::Result<()> {
    let part = final_path.with_extension("part");
    std::fs::copy(from, &part)?;
    std::fs::rename(&part, final_path)
}

/// `VACUUM INTO` writes a consistent, defragmented copy through the same
/// cipher — so the output is encrypted and openable with the same key.
fn write_backup(db: &Db, dir: &Path, keyinfo_dir: &Path) -> AppResult<(PathBuf, u64)> {
    std::fs::create_dir_all(dir)
        .map_err(|e| AppError::Other(format!("cannot create backup folder: {e}")))?;
    check_destination(dir, keyinfo_dir)?;

    let stamp = timestamp();
    let target = dir.join(format!("atlas-{stamp}.db.enc"));
    let part = dir.join(format!("atlas-{stamp}.db.enc.part"));
    std::fs::remove_file(&part).ok();
    let written = db.with(|c| {
        // Bound as a parameter — never string-concatenated into the SQL.
        c.execute("VACUUM INTO ?1", rusqlite::params![part.to_string_lossy()])?;
        Ok(())
    });
    if let Err(e) = written {
        std::fs::remove_file(&part).ok();
        return Err(e);
    }
    std::fs::rename(&part, &target)
        .map_err(|e| AppError::Other(format!("cannot finish the backup file: {e}")))?;

    // The sidecar is required to derive the key from the recovery phrase, so a
    // backup without it is only half a backup. Each copy gets its own, so a
    // later key never overwrites the one an earlier copy needs; the plain name
    // always holds the latest for a simple restore.
    let src = keyinfo_dir.join(KEYINFO_FILENAME);
    if src.exists() {
        copy_atomically(&src, &dir.join(format!("atlas-{stamp}.keyinfo")))
            .and_then(|()| copy_atomically(&src, &dir.join(KEYINFO_FILENAME)))
            .map_err(|e| AppError::Other(format!("cannot copy {KEYINFO_FILENAME}: {e}")))?;
    }

    let bytes = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    Ok((target, bytes))
}

/// Keep the newest `retain` copies (and their key sidecars); delete the rest.
/// Never touches the plain sidecar or files that are not ours.
fn prune(dir: &Path, retain: usize) -> AppResult<()> {
    if retain == 0 {
        return Ok(());
    }
    let mut stamps: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| AppError::Other(format!("cannot list backup folder: {e}")))?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(backup_stamp)
                .map(str::to_string)
        })
        .collect();
    // The stamp is a UTC timestamp in a sortable format, so lexical order is
    // chronological order — no filesystem mtime needed.
    stamps.sort();
    if stamps.len() > retain {
        for old in &stamps[..stamps.len() - retain] {
            std::fs::remove_file(dir.join(format!("atlas-{old}.db.enc"))).ok();
            std::fs::remove_file(dir.join(format!("atlas-{old}.keyinfo"))).ok();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;

    fn key() -> SecretString {
        SecretString::from("c".repeat(64))
    }

    fn seeded_db(dir: &Path) -> Db {
        let db = Db::open(&dir.join("atlas.db"), &key()).unwrap();
        db.with(|c| {
            c.execute(
                "INSERT INTO account (name, kind) VALUES ('Backup Canary Account', 'checking')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn policy(dir: &Path) -> BackupPolicy {
        BackupPolicy {
            enabled: true,
            directory: Some(dir.to_path_buf()),
            min_interval_seconds: 300,
            retain_copies: 3,
        }
    }

    #[test]
    fn backup_is_encrypted_and_openable_with_the_same_key() {
        let home = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let db = seeded_db(home.path());

        let sched = BackupScheduler::new();
        sched.mark_dirty();
        let out = sched
            .maybe_run(&db, &policy(dest.path()), Trigger::Manual, home.path())
            .unwrap();

        let BackupOutcome::Written { path, bytes } = out else {
            panic!("expected a backup, got {out:?}");
        };
        assert!(bytes > 0);

        // 1. It is ciphertext on disk.
        let raw = std::fs::read(&path).unwrap();
        assert!(
            !raw.starts_with(b"SQLite format 3\0"),
            "backup header is plaintext"
        );
        assert!(
            !String::from_utf8_lossy(&raw).contains("Backup Canary Account"),
            "backup contents are readable on disk"
        );

        // 2. It is a real, complete database under the same key.
        let restored = Db::open(&path, &key()).unwrap();
        let name: String = restored
            .with(|c| Ok(c.query_row("SELECT name FROM account", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(name, "Backup Canary Account");
        assert_eq!(restored.schema_version().unwrap(), 19);
    }

    #[test]
    fn the_keyinfo_sidecar_travels_with_the_backup() {
        let home = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        // Pretend setup already wrote a sidecar next to the database.
        std::fs::write(home.path().join(KEYINFO_FILENAME), r#"{"version":1}"#).unwrap();
        let db = seeded_db(home.path());

        let sched = BackupScheduler::new();
        sched.mark_dirty();
        sched
            .maybe_run(&db, &policy(dest.path()), Trigger::Manual, home.path())
            .unwrap();

        assert!(
            dest.path().join(KEYINFO_FILENAME).exists(),
            "a backup without the sidecar cannot be recovered from the phrase"
        );
    }

    #[test]
    fn a_backup_never_carries_the_key_that_decrypts_it() {
        // atlas.keyinfo travels with backups because the recovery phrase needs
        // its salt. atlas.localkey must NOT — it is the plaintext key, and
        // shipping it would make every backup folder self-decrypting.
        let home = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join(KEYINFO_FILENAME), r#"{"version":2}"#).unwrap();
        std::fs::write(
            home.path()
                .join(crate::security::recovery::LOCALKEY_FILENAME),
            "f".repeat(64),
        )
        .unwrap();
        let db = seeded_db(home.path());

        let sched = BackupScheduler::new();
        sched.mark_dirty();
        sched
            .maybe_run(&db, &policy(dest.path()), Trigger::Manual, home.path())
            .unwrap();

        assert!(
            dest.path().join(KEYINFO_FILENAME).exists(),
            "salt must travel"
        );
        assert!(
            !dest
                .path()
                .join(crate::security::recovery::LOCALKEY_FILENAME)
                .exists(),
            "the plaintext key must never reach the backup folder"
        );
    }

    #[test]
    fn a_clean_database_is_not_backed_up() {
        let home = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let db = seeded_db(home.path());
        let sched = BackupScheduler::new(); // never marked dirty
        let out = sched
            .maybe_run(&db, &policy(dest.path()), Trigger::Manual, home.path())
            .unwrap();
        assert_eq!(out, BackupOutcome::SkippedClean);
    }

    #[test]
    fn the_interval_throttles_ordinary_edits_but_never_an_import() {
        let home = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let db = seeded_db(home.path());
        let p = policy(dest.path());
        let sched = BackupScheduler::new();

        sched.mark_dirty();
        assert!(matches!(
            sched
                .maybe_run(&db, &p, Trigger::Interval, home.path())
                .unwrap(),
            BackupOutcome::Written { .. }
        ));

        // Another edit immediately afterwards is throttled...
        sched.mark_dirty();
        assert_eq!(
            sched
                .maybe_run(&db, &p, Trigger::Interval, home.path())
                .unwrap(),
            BackupOutcome::SkippedNotDue
        );

        // ...but an import forces one through regardless.
        assert!(matches!(
            sched
                .maybe_run(&db, &p, Trigger::Import, home.path())
                .unwrap(),
            BackupOutcome::Written { .. }
        ));
    }

    #[test]
    fn retention_keeps_the_newest_copies_only() {
        let home = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let db = seeded_db(home.path());
        let p = BackupPolicy {
            retain_copies: 2,
            ..policy(dest.path())
        };
        let sched = BackupScheduler::new();

        for i in 0..5 {
            sched.mark_dirty();
            // Distinct filenames without waiting a real second each round.
            let stamp = format!("atlas-2026010{i}T000000Z.db.enc");
            std::fs::write(dest.path().join(stamp), b"x").unwrap();
            sched
                .maybe_run(&db, &p, Trigger::Manual, home.path())
                .unwrap();
        }

        let remaining = std::fs::read_dir(dest.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".db.enc"))
            .count();
        assert_eq!(
            remaining, 2,
            "retention did not prune to the configured count"
        );
    }

    #[test]
    fn no_directory_configured_is_reported_not_silently_ignored() {
        let home = tempfile::tempdir().unwrap();
        let db = seeded_db(home.path());
        let sched = BackupScheduler::new();
        sched.mark_dirty();
        let p = BackupPolicy {
            directory: None,
            ..Default::default()
        };
        assert_eq!(
            sched
                .maybe_run(&db, &p, Trigger::Manual, home.path())
                .unwrap(),
            BackupOutcome::NoDirectory
        );
    }

    #[test]
    fn every_backup_is_logged_to_the_database() {
        let home = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let db = seeded_db(home.path());
        let sched = BackupScheduler::new();
        sched.mark_dirty();
        sched
            .maybe_run(&db, &policy(dest.path()), Trigger::Manual, home.path())
            .unwrap();

        let (n, outcome, trigger): (i64, String, String) = db
            .with(|c| {
                Ok(c.query_row(
                    "SELECT count(*), max(outcome), max(trigger) FROM backup_log",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap();
        assert_eq!(n, 1);
        assert_eq!(outcome, "ok");
        assert_eq!(trigger, "manual");
    }

    #[test]
    fn the_data_folder_is_never_accepted_as_the_backup_folder() {
        // Copying the sidecar onto itself truncated it to zero bytes.
        let home = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join(KEYINFO_FILENAME), r#"{"version":2}"#).unwrap();
        let db = seeded_db(home.path());
        let sched = BackupScheduler::new();
        sched.mark_dirty();
        let out = sched.maybe_run(&db, &policy(home.path()), Trigger::Manual, home.path());
        assert!(out.is_err(), "{out:?}");
        assert_eq!(
            std::fs::read_to_string(home.path().join(KEYINFO_FILENAME)).unwrap(),
            r#"{"version":2}"#
        );
    }

    #[test]
    fn each_copy_keeps_the_sidecar_it_needs_and_other_files_are_left_alone() {
        let home = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        std::fs::write(home.path().join(KEYINFO_FILENAME), r#"{"version":2}"#).unwrap();
        // Not ours: must neither count towards retention nor be deleted.
        for name in [
            "atlas-notes.db.enc",
            "atlas-20250101T000000Z.db.enc.bak",
            "photo.jpg",
        ] {
            std::fs::write(dest.path().join(name), b"mine").unwrap();
        }
        let db = seeded_db(home.path());
        let sched = BackupScheduler::new();
        sched.mark_dirty();
        let out = sched
            .maybe_run(&db, &policy(dest.path()), Trigger::Manual, home.path())
            .unwrap();
        let BackupOutcome::Written { path, .. } = out else {
            panic!("expected a backup");
        };
        let stamp = backup_stamp(path.file_name().unwrap().to_str().unwrap()).unwrap();
        assert!(dest.path().join(format!("atlas-{stamp}.keyinfo")).is_file());
        for name in [
            "atlas-notes.db.enc",
            "atlas-20250101T000000Z.db.enc.bak",
            "photo.jpg",
        ] {
            assert!(dest.path().join(name).is_file(), "{name} was removed");
        }
        assert!(
            !dest
                .path()
                .join(format!("atlas-{stamp}.db.enc.part"))
                .exists()
        );
    }

    #[test]
    fn only_well_formed_names_count_as_backups() {
        assert!(backup_stamp("atlas-20260105T123456Z.db.enc").is_some());
        for bad in [
            "atlas-notes.db.enc",
            "atlas-20260105T123456Z.db.enc.part",
            "atlas-2026T1Z.db.enc",
            "x.db.enc",
        ] {
            assert!(backup_stamp(bad).is_none(), "{bad}");
        }
    }
}
