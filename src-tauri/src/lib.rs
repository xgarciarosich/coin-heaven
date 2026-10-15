// Nested `if let` blocks are kept as they are written: collapsing them into
// let-chains would not make them clearer, and it would tie every contributor
// to the newest compiler.
#![allow(clippy::collapsible_if)]

pub mod ai;
pub mod backup;
pub mod bootstrap;
pub mod commands;
pub mod currency;
pub mod db;
pub mod error;
pub mod fire;
pub mod i18n;
pub mod import;
pub mod market;
pub mod money;
pub mod net;
pub mod portfolio;
pub mod security;
pub mod sim;
pub mod state;

use backup::Trigger;
use state::AppState;
use tauri::{Manager, RunEvent, WindowEvent};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "coin_heaven_lib=info,warn".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            if let Err(e) = bootstrap::adopt_previous_data_dir(&dir) {
                tracing::warn!("could not copy the previous data folder: {e}");
            }
            let state = AppState::new(dir);

            // A failure here is not fatal: the UI shows a recovery screen and
            // asks for the phrase. Panicking would make a lost keychain entry
            // look like a broken app.
            match bootstrap::auto_unlock(&state) {
                Ok(s) => tracing::info!("startup unlock: {s:?}"),
                Err(e) => tracing::warn!("database is locked at startup: {e}"),
            }

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_status,
            commands::sim_defaults,
            commands::sim_run,
            commands::pii_preview,
            commands::sim_list_scenarios,
            commands::sim_save_scenario,
            commands::sim_load_scenario,
            commands::unlock_status,
            commands::passphrase_strength,
            commands::setup_database,
            commands::recovery_take_phrase,
            commands::recovery_confirm_phrase,
            commands::recovery_defer_confirmation,
            commands::unlock_with_passphrase,
            commands::unlock_with_recovery_phrase,
            commands::upgrade_legacy_database,
            commands::passphrase_set,
            commands::passphrase_remove,
            commands::backup_status,
            commands::backup_set_directory,
            commands::backup_now,
            commands::fire_defaults,
            commands::fire_calculate,
            commands::fire_latest_balances,
            // Module 1 — import
            commands::import::account_list,
            commands::import::account_create,
            commands::import::mapping_list,
            commands::import::mapping_save,
            commands::import::mapping_delete,
            commands::import::date_formats,
            commands::import::import_inspect,
            commands::import::import_preview,
            commands::import::import_run,
            commands::import::import_batches,
            commands::import::import_undo,
            commands::import::matching_pending_transfers,
            commands::import::matching_pending_splits,
            commands::import::matching_decide,
            commands::import::rule_list,
            commands::import::rule_save,
            commands::import::rule_delete,
            commands::import::rules_apply,
            // Global settings and category management
            commands::settings::region_apply,
            commands::settings::settings_get,
            commands::settings::settings_save,
            commands::settings::category_usage,
            commands::settings::category_delete,
            commands::settings::category_reorder,
            // Module 2 — categories, events, ledger
            commands::ledger::category_list,
            commands::ledger::category_save,
            commands::ledger::category_archive,
            commands::ledger::event_list,
            commands::ledger::event_save,
            commands::ledger::event_set_closed,
            commands::ledger::event_delete,
            commands::ledger::txn_list,
            commands::ledger::txn_assign,
            commands::ledger::txn_set_neutral,
            commands::ledger::txn_link_reimbursement,
            commands::ledger::pl_summary,
            commands::ledger::txn_update,
            commands::ledger::txn_delete,
            commands::ledger::txn_years,
            commands::ledger::cashflow_monthly,
            commands::ledger::cashflow_yearly,
            commands::ledger::cashflow_summary,
            // Budgets and the category x month matrix
            commands::budget::pl_matrix,
            commands::budget::budget_plan,
            commands::budget::budget_save,
            commands::budget::budget_save_every_month,
            commands::budget::budget_remove,
            commands::budget::budget_seed_from_year,
            commands::export::data_export,
            commands::market::market_preview,
            commands::market::market_fetch,
            commands::market::market_clear,
            commands::recurring::recurring_list,
            commands::update::update_check,
            commands::sample::sample_status,
            commands::sample::sample_load,
            commands::sample::sample_remove,
            commands::plan::plan_list,
            commands::plan::plan_save,
            commands::plan::plan_delete,
            // Pre-assigning categories from the description
            commands::autocat::autocat_suggest,
            commands::autocat::autocat_apply,
            // Module 3 — net worth and FIRE
            commands::wealth::snapshot_list,
            commands::wealth::snapshot_save,
            commands::wealth::snapshot_delete,
            commands::wealth::wealth_line_rename,
            commands::wealth::snapshot_prompt,
            commands::wealth::fire_view,
            commands::wealth::fire_plan,
            commands::wealth::fire_params_save,
            commands::wealth::fire_ledger_suggestion,
            // Portfolio, accounts, loans and the snapshot lines computed from them
            commands::portfolio::portfolio_view,
            commands::portfolio::portfolio_gain_share,
            commands::portfolio::portfolio_history_view,
            commands::portfolio::holding_save,
            commands::portfolio::holding_delete,
            commands::portfolio::holding_txns,
            commands::portfolio::holding_txn_save,
            commands::portfolio::holding_txn_delete,
            commands::portfolio::schedule_save,
            commands::portfolio::schedule_delete,
            commands::portfolio::prices_save,
            commands::assistant::ai_price_preview,
            commands::assistant::ai_price_send,
            commands::wealth::asset_class_list,
            commands::wealth::asset_class_save,
            commands::wealth::asset_class_delete,
            commands::wealth::asset_class_reorder,
            // Module 5 — assistant
            commands::assistant::ai_settings_get,
            commands::assistant::ai_settings_save,
            commands::assistant::ai_provider_list,
            commands::assistant::ai_provider_save,
            commands::assistant::ai_provider_delete,
            commands::assistant::ai_autocat_preview,
            commands::assistant::ai_autocat_send,
            commands::assistant::ai_chat_preview,
            commands::assistant::ai_chat_send,
            commands::assistant::ai_decline,
            commands::assistant::ai_egress_log,
            commands::assistant::ai_mapping_preview,
            commands::assistant::ai_mapping_send,
            commands::currency_list,
            commands::js_diagnostic,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Coin Heaven")
        .run(|app, event| {
            // Shutdown is one of the two moments where unbacked-up work is
            // genuinely at risk, so it forces a backup regardless of interval.
            let closing = matches!(
                event,
                RunEvent::ExitRequested { .. }
                    | RunEvent::WindowEvent {
                        event: WindowEvent::Destroyed,
                        ..
                    }
            );
            if closing {
                let state = app.state::<AppState>();
                if let (Ok(db), Ok(policy)) = (state.db(), state.backup_policy()) {
                    match state
                        .backup
                        .maybe_run(db, &policy, Trigger::Shutdown, &state.data_dir)
                    {
                        Ok(outcome) => tracing::info!("shutdown backup: {outcome:?}"),
                        Err(e) => tracing::error!("shutdown backup failed: {e}"),
                    }
                }
                // Fold the write-ahead log back into atlas.db, so that copying
                // the folder by hand (or restoring only the .db) is complete.
                if let Ok(db) = state.db() {
                    let _ = db.with(|c| {
                        c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
                        Ok(())
                    });
                }
            }
        });
}
