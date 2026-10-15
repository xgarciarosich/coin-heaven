//! The IPC surface.
//!
//! Split by domain: this file holds app status, unlock/recovery, backup, the
//! simulator and FIRE; `import` holds everything to do with getting bank data
//! in. Every command is explicit and narrowly typed, and the webview has no
//! filesystem, shell or HTTP permission, so these files are the whole boundary
//! between the UI and the machine.

pub mod assistant;
pub mod autocat;
pub mod budget;
pub mod export;
pub mod import;
pub mod ledger;
pub mod market;
pub mod plan;
pub mod portfolio;
pub mod recurring;
pub mod sample;
pub mod settings;
pub mod update;
pub mod wealth;

use crate::error::{AppError, AppResult};
use crate::security::pii;
use crate::sim::{self, SimParams, SimResult};
use crate::state::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub schema_version: i64,
    pub unlocked: bool,
    pub db_path: String,
    pub version: &'static str,
}

#[tauri::command]
pub fn app_status(state: State<'_, AppState>) -> AppResult<AppStatus> {
    let unlocked = state.is_open();
    Ok(AppStatus {
        schema_version: if unlocked {
            state.db()?.schema_version()?
        } else {
            0
        },
        unlocked,
        db_path: state.db_path.display().to_string(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Default scenario for a fresh simulator view.
#[tauri::command]
pub fn sim_defaults() -> SimParams {
    SimParams::default()
}

/// Run the projection. Pure and fast enough to call on every slider change.
#[tauri::command]
pub fn sim_run(params: SimParams) -> AppResult<SimResult> {
    sim::run(&params).map_err(AppError::SimParams)
}

/// Show the user exactly what redaction would do to a string, before any
/// AI feature is allowed to send anything.
#[tauri::command]
pub fn pii_preview(text: String) -> pii::Masked {
    pii::mask(&text)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScenarioSummary {
    pub id: i64,
    pub name: String,
    pub is_default: bool,
}

#[tauri::command]
pub fn sim_list_scenarios(state: State<'_, AppState>) -> AppResult<Vec<ScenarioSummary>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare("SELECT id, name, is_default FROM sim_scenario ORDER BY name")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ScenarioSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    is_default: r.get::<_, i64>(2)? != 0,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[tauri::command]
pub fn sim_save_scenario(
    state: State<'_, AppState>,
    name: String,
    params: SimParams,
) -> AppResult<i64> {
    if name.trim().is_empty() {
        return Err(AppError::Invalid("scenario name is required".into()));
    }
    sim::run(&params).map_err(AppError::SimParams)?;
    let json = serde_json::to_string(&params)
        .map_err(|e| AppError::Other(format!("cannot serialise scenario: {e}")))?;
    state.db()?.with(|c| {
        c.execute(
            "INSERT INTO sim_scenario (name, params_json, params_version)
             VALUES (?1, ?2, 1)
             ON CONFLICT(name) DO UPDATE
               SET params_json = excluded.params_json,
                   updated_at  = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
            rusqlite::params![name.trim(), json],
        )?;
        Ok(c.query_row(
            "SELECT id FROM sim_scenario WHERE name = ?1",
            rusqlite::params![name.trim()],
            |r| r.get(0),
        )?)
    })
}

#[tauri::command]
pub fn sim_load_scenario(state: State<'_, AppState>, id: i64) -> AppResult<SimParams> {
    let json: String = state.db()?.with(|c| {
        Ok(c.query_row(
            "SELECT params_json FROM sim_scenario WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )?)
    })?;
    serde_json::from_str(&json)
        .map_err(|e| AppError::Other(format!("stored scenario is unreadable: {e}")))
}

// ---------------------------------------------------------------- recovery

use crate::backup::{BackupOutcome, Trigger};
use crate::bootstrap::{self, UnlockState};
use crate::fire::{self, Balances, FireParams, FireResult};
use crate::money::Money;
use secrecy::ExposeSecret;
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockStatus {
    pub state: UnlockState,
    pub has_passphrase: bool,
    pub phrase_pending: bool,
    /// The user typed the phrase back, so we know they really have it.
    pub phrase_confirmed: bool,
    /// The user has seen the phrase screen and moved on — possibly by
    /// deferring. Gates the app; `phrase_confirmed` drives the reminder.
    pub phrase_acknowledged: bool,
}

#[tauri::command]
pub fn unlock_status(state: State<'_, AppState>) -> AppResult<UnlockStatus> {
    let info = crate::security::recovery::KeyInfo::load(&state.data_dir)
        .ok()
        .flatten();

    let flag = |key: &str| -> bool {
        if !state.is_open() {
            return false;
        }
        state
            .db()
            .ok()
            .and_then(|db| {
                db.with(|c| {
                    Ok(c.query_row(
                        "SELECT value_json FROM app_setting WHERE key = ?1",
                        rusqlite::params![key],
                        |r| r.get::<_, String>(0),
                    )
                    .map(|v| v == "true")
                    .unwrap_or(false))
                })
                .ok()
            })
            .unwrap_or(false)
    };
    let confirmed = flag("recovery.phrase_confirmed");
    let acknowledged = confirmed || flag("recovery.phrase_acknowledged");

    Ok(UnlockStatus {
        state: if state.is_open() {
            UnlockState::Opened
        } else {
            bootstrap::auto_unlock(&state).unwrap_or(UnlockState::NeedsSetup)
        },
        has_passphrase: info.map(|i| i.has_passphrase()).unwrap_or(false),
        phrase_pending: state.has_pending_phrase(),
        phrase_confirmed: confirmed,
        phrase_acknowledged: acknowledged,
    })
}

/// Let the user past the recovery-phrase screen without typing it back.
///
/// Deliberately does **not** set `recovery.phrase_confirmed`: we have no
/// evidence they saved it, so the sidebar keeps reminding them. Useful for a
/// throwaway database you are only poking at.
#[tauri::command]
pub fn recovery_defer_confirmation(state: State<'_, AppState>) -> AppResult<()> {
    state.db()?.with(|c| {
        c.execute(
            "INSERT INTO app_setting (key, value_json) VALUES ('recovery.phrase_acknowledged', 'true')
             ON CONFLICT(key) DO UPDATE SET value_json = 'true',
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
            [],
        )?;
        Ok(())
    })
}

/// Rate a candidate passphrase as the user types. Nothing is stored.
#[tauri::command]
pub fn passphrase_strength(passphrase: String) -> crate::security::recovery::PassphraseStrength {
    crate::security::recovery::rate_passphrase(&passphrase)
}

/// First run. `passphrase` of `null` is an explicit "no passphrase" — the
/// database is still encrypted, but it opens without asking.
#[tauri::command]
pub fn setup_database(
    state: State<'_, AppState>,
    passphrase: Option<String>,
) -> AppResult<UnlockState> {
    bootstrap::setup(&state, passphrase.as_deref())
}

/// Hand the freshly generated recovery phrase to the UI. Returns it **once**;
/// a second call returns `None`. It is never written to the database.
#[tauri::command]
pub fn recovery_take_phrase(state: State<'_, AppState>) -> Option<String> {
    state.take_phrase().map(|p| p.expose_secret().to_string())
}

/// Confirm the user copied the phrase down correctly. Deriving from it is the
/// only honest check: it proves the words actually open this database.
#[tauri::command]
pub fn recovery_confirm_phrase(state: State<'_, AppState>, phrase: String) -> AppResult<bool> {
    use crate::security::recovery::{KeyInfo, WrapKind};
    let info = KeyInfo::load(&state.data_dir)?
        .ok_or_else(|| AppError::Other("key info is missing".into()))?;
    info.unlock(&phrase, WrapKind::Recovery)?;

    state.db()?.with(|c| {
        c.execute(
            "UPDATE app_setting SET value_json = 'true',
                    updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE key = 'recovery.phrase_confirmed'",
            [],
        )?;
        Ok(())
    })?;
    Ok(true)
}

#[tauri::command]
pub fn unlock_with_passphrase(
    state: State<'_, AppState>,
    passphrase: String,
) -> AppResult<UnlockState> {
    bootstrap::unlock_with_passphrase(&state, &passphrase)
}

#[tauri::command]
pub fn unlock_with_recovery_phrase(
    state: State<'_, AppState>,
    phrase: String,
) -> AppResult<UnlockState> {
    bootstrap::unlock_with_phrase(&state, &phrase)
}

/// One-time upgrade for a database created before the envelope scheme.
#[tauri::command]
pub fn upgrade_legacy_database(
    state: State<'_, AppState>,
    phrase: String,
) -> AppResult<UnlockState> {
    bootstrap::upgrade_legacy(&state, &phrase)
}

/// Add or change the passphrase on an open database. Rewraps the key; the
/// database itself is untouched, so this is instant at any size.
#[tauri::command]
pub fn passphrase_set(state: State<'_, AppState>, passphrase: String) -> AppResult<()> {
    state.with_dek(|dek| {
        crate::security::recovery::set_passphrase(&state.data_dir, dek, &passphrase)
    })?;
    // A key on disk would let anyone step around the passphrase entirely.
    crate::security::recovery::forget_local_key(&state.data_dir)?;
    state.backup.mark_dirty();
    Ok(())
}

/// Remove the passphrase and go back to opening automatically.
#[tauri::command]
pub fn passphrase_remove(state: State<'_, AppState>) -> AppResult<()> {
    crate::security::recovery::remove_passphrase(&state.data_dir)?;
    state.with_dek(|dek| crate::security::recovery::save_local_key(&state.data_dir, dek))?;
    state.backup.mark_dirty();
    Ok(())
}

// ------------------------------------------------------------------ backup

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupStatus {
    pub enabled: bool,
    pub directory: Option<String>,
    pub min_interval_seconds: u64,
    pub retain_copies: usize,
    pub dirty: bool,
    pub last_backup_at: Option<String>,
    pub last_outcome: Option<String>,
}

#[tauri::command]
pub fn backup_status(state: State<'_, AppState>) -> AppResult<BackupStatus> {
    let policy = state.backup_policy()?;
    let (at, outcome) = state.db()?.with(|c| {
        // "Last copy" is the last copy that worked; a failure after it is
        // reported through `last_outcome` rather than passing for a backup.
        let ok_at: Option<String> = c
            .query_row(
                "SELECT created_at FROM backup_log WHERE outcome = 'ok' ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();
        let latest: Option<String> = c
            .query_row(
                "SELECT outcome FROM backup_log ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();
        Ok((ok_at, latest))
    })?;
    Ok(BackupStatus {
        enabled: policy.enabled,
        directory: policy.directory.map(|p| p.display().to_string()),
        min_interval_seconds: policy.min_interval_seconds,
        retain_copies: policy.retain_copies,
        dirty: state.backup.is_dirty(),
        last_backup_at: at,
        last_outcome: outcome,
    })
}

#[tauri::command]
pub fn backup_set_directory(state: State<'_, AppState>, path: String) -> AppResult<()> {
    let dir = PathBuf::from(&path);
    if !dir.is_dir() {
        return Err(AppError::Invalid(format!("{path} is not a folder")));
    }
    crate::backup::check_destination(&dir, &state.data_dir)?;
    let json = serde_json::to_string(&path)
        .map_err(|e| AppError::Other(format!("cannot encode path: {e}")))?;
    state.db()?.with(|c| {
        c.execute(
            "INSERT INTO app_setting (key, value_json) VALUES ('backup.directory', ?1)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
            rusqlite::params![json],
        )?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

#[tauri::command]
pub fn backup_now(state: State<'_, AppState>) -> AppResult<BackupOutcome> {
    state.backup.mark_dirty();
    let policy = state.backup_policy()?;
    state
        .backup
        .maybe_run(state.db()?, &policy, Trigger::Manual, &state.data_dir)
}

// -------------------------------------------------------------------- FIRE

#[tauri::command]
pub fn fire_defaults() -> FireParams {
    FireParams::default()
}

#[tauri::command]
pub fn fire_calculate(params: FireParams, balances: Balances) -> AppResult<FireResult> {
    fire::calculate(&params, &balances)
        .map_err(|e| AppError::Invalid(format!("{} — {}", e[0].field, e[0].message)))
}

/// Balances from the most recent net-worth snapshot.
///
/// "Liquid" is whatever the user has classed as liquid — by default cash,
/// equities, crypto and pensions. Pensions are included because the decision
/// said so; the UI still notes that pension capital is usually locked until a
/// statutory age, so a liquid FIRE number leaning on it is not the same as
/// being able to stop working today. Someone who disagrees now has somewhere
/// to say so: reclassify the pension as an ordinary asset.
#[tauri::command]
pub fn fire_latest_balances(state: State<'_, AppState>) -> AppResult<Option<Balances>> {
    state.db()?.with(latest_balances)
}

/// The same read against a connection in hand, so the class-kind arithmetic
/// can be tested against a real schema.
pub fn latest_balances(c: &rusqlite::Connection) -> AppResult<Option<Balances>> {
    {
        let snapshot_id: Option<i64> = c
            .query_row(
                "SELECT id FROM wealth_snapshot ORDER BY taken_on DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();
        let Some(id) = snapshot_id else {
            return Ok(None);
        };
        // "Counts toward FIRE" is the class's own flag, exactly as in
        // `v_net_worth`, so the FIRE panel and the net-worth screen can never
        // disagree about it. The three asset figures are disjoint: a class
        // that counts is in the first and nowhere else, so they still add up
        // to everything owned.
        let b = c.query_row(
            "SELECT
               COALESCE(SUM(CASE WHEN k.kind <> 'liability' AND k.counts_fire = 1
                                 THEN l.gross_value_minor - l.debt_minor ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN k.kind = 'property' AND k.counts_fire = 0
                                 THEN l.gross_value_minor - l.debt_minor ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN k.kind IN ('other_asset','liquid') AND k.counts_fire = 0
                                 THEN l.gross_value_minor - l.debt_minor ELSE 0 END), 0),
               COALESCE(SUM(CASE WHEN k.kind = 'liability'
                                 THEN l.gross_value_minor + l.debt_minor ELSE 0 END), 0)
             FROM wealth_line l
             JOIN asset_class k ON k.key = l.bucket
             WHERE l.snapshot_id = ?1",
            rusqlite::params![id],
            |r| {
                Ok(Balances {
                    liquid: Money::from_minor(r.get(0)?),
                    real_estate_equity: Money::from_minor(r.get(1)?),
                    other_assets: Money::from_minor(r.get(2)?),
                    other_debt: Money::from_minor(r.get(3)?),
                })
            },
        )?;
        Ok(Some(b))
    }
}

/// The webview reports here when it fails to mount.
///
/// Added after `freezePrototype` silently broke React initialisation: the
/// window was blank and nothing anywhere said why. A healthy launch never
/// calls this.
#[tauri::command]
pub fn js_diagnostic(payload: String) {
    // Whatever the page sends, only a short, single-line prefix is written.
    let payload: String = payload
        .chars()
        .take(2_000)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    tracing::error!("webview failed to start: {payload}");
    eprintln!("coin-heaven: webview failed to start: {payload}");
}

// ------------------------------------------------------------------ misc

#[tauri::command]
pub fn currency_list() -> &'static [crate::currency::Currency] {
    crate::currency::CURRENCIES
}
