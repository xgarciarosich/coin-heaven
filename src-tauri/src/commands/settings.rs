//! Global settings: display currency, module visibility, category management.
//!
//! Settings live in `app_setting` inside the encrypted database rather than in
//! a plist or a dotfile, so they inherit the same protection as the financial
//! data and travel with a backup.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModuleToggles {
    pub import: bool,
    pub ledger: bool,
    pub wealth: bool,
    pub fire: bool,
    pub simulator: bool,
    pub assistant: bool,
}

impl ModuleToggles {
    fn any_enabled(&self) -> bool {
        self.import || self.ledger || self.wealth || self.fire || self.simulator || self.assistant
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Governs symbols, grouping and decimal places. It does **not** convert
    /// anything: see `settings_save`.
    pub primary_currency: String,
    pub locale: String,
    /// "en" or "es". Empty until someone has chosen, so the UI can offer the
    /// system language as the default instead of assuming one.
    pub language: String,
    /// The regional profile in use: an id from the `regions/` folder.
    #[serde(default = "default_region")]
    pub region: String,
    /// That region's country, in English, for prompts that need to say where
    /// the person lives.
    #[serde(default = "default_country")]
    pub country: String,
    /// Look for a newer release on start. Off until chosen: it is a request
    /// that leaves the machine, however harmless.
    #[serde(default)]
    pub check_updates: bool,
    pub modules: ModuleToggles,
}

/// Empty until chosen: the interface proposes one from the system's locale
/// on first run, and writes it down.
fn default_region() -> String {
    String::new()
}
fn default_country() -> String {
    String::new()
}

fn get_json(c: &rusqlite::Connection, key: &str) -> Option<serde_json::Value> {
    c.query_row(
        "SELECT value_json FROM app_setting WHERE key = ?1",
        rusqlite::params![key],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|v| serde_json::from_str(&v).ok())
}

fn get_bool(c: &rusqlite::Connection, key: &str, default: bool) -> bool {
    get_json(c, key)
        .and_then(|v| v.as_bool())
        .unwrap_or(default)
}

fn get_str(c: &rusqlite::Connection, key: &str, default: &str) -> String {
    get_json(c, key)
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| default.to_string())
}

fn put(c: &rusqlite::Connection, key: &str, value: &serde_json::Value) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO app_setting (key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        rusqlite::params![key, value.to_string()],
    )?;
    Ok(())
}

/// The active region's words for cleaning merchant names. The region files
/// live with the interface; this is how the importer, which is Rust, learns
/// them. Nothing is stored: it is sent again at every start.
#[tauri::command]
pub fn region_apply(merchant_noise: Vec<String>) -> AppResult<()> {
    crate::import::merchant::set_noise(merchant_noise);
    Ok(())
}

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> AppResult<AppSettings> {
    state.db()?.with(|c| {
        Ok(AppSettings {
            primary_currency: get_str(c, "ui.currency.primary", crate::currency::DEFAULT),
            locale: get_str(c, "ui.locale", ""),
            language: {
                let l = get_str(c, "ui.language", "");
                if !l.is_empty() {
                    crate::i18n::set(&l);
                }
                l
            },
            region: get_str(c, "ui.region", ""),
            country: get_str(c, "ui.region.country", ""),
            check_updates: get_bool(c, "updates.check", false),
            modules: ModuleToggles {
                import: get_bool(c, "modules.import.enabled", true),
                ledger: get_bool(c, "modules.ledger.enabled", true),
                wealth: get_bool(c, "modules.wealth.enabled", true),
                fire: get_bool(c, "modules.fire.enabled", true),
                simulator: get_bool(c, "modules.simulator.enabled", true),
                assistant: get_bool(c, "modules.assistant.enabled", true),
            },
        })
    })
}

/// Save settings.
///
/// Changing the primary currency changes **presentation only** — symbol,
/// digit grouping and decimal places. It converts nothing, because there is no
/// rate source (`fx_rate` is user-entered and unused). Amounts already recorded
/// in another currency keep their own, and the UI shows those rows in the
/// currency they were booked in rather than relabelling them.
#[tauri::command]
pub fn settings_save(state: State<'_, AppState>, settings: AppSettings) -> AppResult<()> {
    if !crate::currency::is_supported(&settings.primary_currency) {
        return Err(AppError::Invalid(format!(
            "unsupported currency: {}",
            settings.primary_currency
        )));
    }
    if !matches!(settings.language.as_str(), "en" | "es") {
        return Err(AppError::Invalid(format!(
            "unsupported language: {}",
            settings.language
        )));
    }
    // Set before validating the rest, so a refusal below is already worded in
    // the language the user just picked.
    crate::i18n::set(&settings.language);
    if !settings.modules.any_enabled() {
        return Err(AppError::Invalid(
            "at least one module has to stay visible".into(),
        ));
    }
    state.db()?.with(|c| {
        let m = &settings.modules;
        put(
            c,
            "ui.currency.primary",
            &settings.primary_currency.to_uppercase().into(),
        )?;
        put(c, "ui.locale", &settings.locale.clone().into())?;
        put(c, "ui.region", &settings.region.clone().into())?;
        put(c, "ui.region.country", &settings.country.clone().into())?;
        let previous = get_str(c, "ui.language", "");
        put(c, "ui.language", &settings.language.clone().into())?;
        // Starter categories and asset classes are named in the language the
        // app was first opened in; switching language renames the ones nobody
        // has renamed since.
        if previous != settings.language {
            crate::i18n::rename_seeds(c, &settings.language)?;
        }
        put(c, "updates.check", &settings.check_updates.into())?;
        put(c, "modules.import.enabled", &m.import.into())?;
        put(c, "modules.ledger.enabled", &m.ledger.into())?;
        put(c, "modules.wealth.enabled", &m.wealth.into())?;
        put(c, "modules.fire.enabled", &m.fire.into())?;
        put(c, "modules.simulator.enabled", &m.simulator.into())?;
        put(c, "modules.assistant.enabled", &m.assistant.into())?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/* -------------------------------------------------- category management -- */

/// What would be affected if this category were removed.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryUsage {
    pub transactions: i64,
    pub events: i64,
    pub rules: i64,
}

#[tauri::command]
pub fn category_usage(state: State<'_, AppState>, id: i64) -> AppResult<CategoryUsage> {
    state.db()?.with(|c| {
        let one = |sql: &str| -> rusqlite::Result<i64> {
            c.query_row(sql, rusqlite::params![id], |r| r.get(0))
        };
        Ok(CategoryUsage {
            transactions: one("SELECT count(*) FROM txn WHERE category_override_id = ?1")?,
            events: one("SELECT count(*) FROM project_event WHERE default_category_id = ?1")?,
            rules: one("SELECT count(*) FROM categorization_rule WHERE set_category_id = ?1")?,
        })
    })
}

/// Delete a category, moving anything attached to it somewhere else first.
///
/// `reassign_to` is required whenever the category is in use. Deleting without
/// it would either fail on a foreign key or silently strip the category from
/// history, and neither is a reasonable thing to do to somebody's books.
#[tauri::command]
pub fn category_delete(
    state: State<'_, AppState>,
    id: i64,
    reassign_to: Option<i64>,
) -> AppResult<()> {
    if reassign_to == Some(id) {
        return Err(AppError::Invalid(
            "a category cannot be reassigned to itself".into(),
        ));
    }
    state.db()?.with(|c| {
        let tx = c.unchecked_transaction()?;

        let in_use: i64 = tx.query_row(
            "SELECT (SELECT count(*) FROM txn WHERE category_override_id = ?1)
                  + (SELECT count(*) FROM project_event WHERE default_category_id = ?1)
                  + (SELECT count(*) FROM categorization_rule WHERE set_category_id = ?1)",
            rusqlite::params![id],
            |r| r.get(0),
        )?;

        match reassign_to {
            Some(target) => {
                let exists: i64 = tx.query_row(
                    "SELECT count(*) FROM pl_category WHERE id = ?1",
                    rusqlite::params![target],
                    |r| r.get(0),
                )?;
                if exists == 0 {
                    return Err(AppError::Invalid(
                        "the replacement category does not exist".into(),
                    ));
                }
                tx.execute(
                    "UPDATE txn SET category_override_id = ?2 WHERE category_override_id = ?1",
                    rusqlite::params![id, target],
                )?;
                tx.execute(
                    "UPDATE project_event SET default_category_id = ?2
                     WHERE default_category_id = ?1",
                    rusqlite::params![id, target],
                )?;
                tx.execute(
                    "UPDATE categorization_rule SET set_category_id = ?2
                     WHERE set_category_id = ?1",
                    rusqlite::params![id, target],
                )?;
            }
            None if in_use > 0 => {
                return Err(AppError::Invalid(format!(
                    "{in_use} item(s) still use this category — choose a replacement first"
                )));
            }
            None => {}
        }

        tx.execute(
            "DELETE FROM pl_category WHERE id = ?1",
            rusqlite::params![id],
        )?;
        tx.commit()?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/// Reorder a category, so the P&L reads in an order the user chose.
#[tauri::command]
pub fn category_reorder(state: State<'_, AppState>, ordered_ids: Vec<i64>) -> AppResult<()> {
    state.db()?.with(|c| reorder_categories(c, &ordered_ids))?;
    state.backup.mark_dirty();
    Ok(())
}

pub fn reorder_categories(c: &rusqlite::Connection, ordered_ids: &[i64]) -> AppResult<()> {
    let tx = c.unchecked_transaction()?;
    for (i, id) in ordered_ids.iter().enumerate() {
        tx.execute(
            "UPDATE pl_category SET sort_order = ?1 WHERE id = ?2",
            rusqlite::params![(i as i64 + 1) * 10, id],
        )?;
    }
    tx.commit()?;
    Ok(())
}
