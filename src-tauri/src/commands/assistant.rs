//! Module 5 — the assistant.
//!
//! Every outbound request follows the same three steps, in this order:
//!
//!   1. **build and redact** the payload locally;
//!   2. **log** its hash and the exact preview to `ai_egress_log`;
//!   3. **send**, and record the outcome.
//!
//! The log entry is written *before* the network call, so a request that fails
//! or hangs still leaves a record of what was about to leave. "What has this
//! app ever sent?" is answerable from the database, not from trust.

use crate::ai::provider::{self, Provider, ProviderConfig};
use crate::ai::{self, ChatMessage, EgressPreview, MerchantItem};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

const DEFAULT_SYSTEM: &str = "You are a careful bookkeeping assistant. Answer only from the data given. \
     Never invent figures. If the data cannot answer the question, say so.";

/* ------------------------------------------------------------ settings -- */

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
    pub enabled: bool,
    pub require_preview: bool,
    pub allow_raw_descriptions: bool,
    /// Base URL of a self-hosted search engine (SearXNG), for models that
    /// cannot search the web themselves. Empty when there is none.
    #[serde(default)]
    pub search_url: String,
    /// "off", "duckduckgo", "exa" or "searxng": who answers a model's searches.
    #[serde(default)]
    pub search_engine: String,
    /// Whether an Exa key is stored. The key itself never leaves the database.
    #[serde(default)]
    pub exa_key_set: bool,
    /// Only on the way in: `None` keeps the stored key, an empty string
    /// removes it, anything else replaces it.
    #[serde(default, skip_serializing)]
    pub exa_key: Option<String>,
}

/// A string setting, stored as JSON like the rest of `app_setting`.
fn read_str(state: &AppState, key: &str, default: &str) -> String {
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
                .ok()
                .and_then(|v| serde_json::from_str::<String>(&v).ok()))
            })
            .ok()
            .flatten()
        })
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn currency(state: &AppState) -> String {
    read_str(state, "ui.currency.primary", crate::currency::DEFAULT)
}

fn country(state: &AppState) -> String {
    read_str(state, "ui.region.country", "an unspecified country")
}

/// Where Exa's key is kept: the encrypted database, like the provider keys.
const EXA_KEY: &str = "ai.search.exa_key";

/// Where a model that cannot search gets its results from.
#[derive(Clone)]
enum Engine {
    DuckDuckGo,
    Exa(String),
    Searx(String),
}

/// An address saved before there was a choice means SearXNG; nothing saved
/// means no search, because a search is a request that leaves the machine.
fn search_engine(state: &AppState) -> String {
    let saved = read_str(state, "ai.search.engine", "");
    match saved.as_str() {
        "off" | "duckduckgo" | "exa" | "searxng" => saved,
        _ if search_address(state).is_some() => "searxng".into(),
        _ => "off".into(),
    }
}

fn search_address(state: &AppState) -> Option<String> {
    let u = read_str(state, "ai.search.url", "");
    let u = u.trim().trim_end_matches('/').to_string();
    (u.starts_with("http://") || u.starts_with("https://")).then_some(u)
}

fn search_url(state: &AppState) -> Option<Engine> {
    match search_engine(state).as_str() {
        "duckduckgo" => Some(Engine::DuckDuckGo),
        "exa" => {
            let key = read_str(state, EXA_KEY, "");
            (!key.is_empty()).then_some(Engine::Exa(key))
        }
        "searxng" => search_address(state).map(Engine::Searx),
        _ => None,
    }
}

fn read_flag(state: &AppState, key: &str, default: bool) -> bool {
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
                .unwrap_or(default))
            })
            .ok()
        })
        .unwrap_or(default)
}

#[tauri::command]
pub fn ai_settings_get(state: State<'_, AppState>) -> AppResult<AiSettings> {
    Ok(AiSettings {
        enabled: read_flag(&state, "privacy.ai.enabled", false),
        require_preview: read_flag(&state, "privacy.ai.require_preview", true),
        allow_raw_descriptions: read_flag(&state, "privacy.ai.allow_raw_descriptions", false),
        search_url: read_str(&state, "ai.search.url", ""),
        search_engine: search_engine(&state),
        exa_key_set: !read_str(&state, EXA_KEY, "").is_empty(),
        exa_key: None,
    })
}

#[tauri::command]
pub fn ai_settings_save(state: State<'_, AppState>, settings: AiSettings) -> AppResult<()> {
    let url = settings.search_url.trim();
    if !url.is_empty() && !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(AppError::Invalid(
            "the search engine address has to start with http:// or https://".into(),
        ));
    }
    if !matches!(
        settings.search_engine.as_str(),
        "off" | "duckduckgo" | "exa" | "searxng"
    ) {
        return Err(AppError::Invalid(
            "the search engine has to be off, duckduckgo, exa or searxng".into(),
        ));
    }
    state.db()?.with(|c| {
        c.execute(
            "INSERT INTO app_setting (key, value_json) VALUES ('ai.search.engine', ?1)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
            rusqlite::params![serde_json::Value::from(settings.search_engine.as_str()).to_string()],
        )?;
        c.execute(
            "INSERT INTO app_setting (key, value_json) VALUES ('ai.search.url', ?1)
             ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
            rusqlite::params![serde_json::Value::from(url).to_string()],
        )?;
        if let Some(key) = settings.exa_key.as_deref() {
            c.execute(
                "INSERT INTO app_setting (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
                rusqlite::params![EXA_KEY, serde_json::Value::from(key.trim()).to_string()],
            )?;
        }
        for (k, v) in [
            ("privacy.ai.enabled", settings.enabled),
            ("privacy.ai.require_preview", settings.require_preview),
            (
                "privacy.ai.allow_raw_descriptions",
                settings.allow_raw_descriptions,
            ),
        ] {
            c.execute(
                "INSERT INTO app_setting (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
                     updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
                rusqlite::params![k, if v { "true" } else { "false" }],
            )?;
        }
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/* ----------------------------------------------------------- providers -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRow {
    #[serde(flatten)]
    pub config: ProviderConfig,
    pub id: i64,
    pub has_key: bool,
    pub is_local: bool,
    pub requires_key: bool,
    pub default_base_url: String,
}

#[tauri::command]
pub fn ai_provider_list(state: State<'_, AppState>) -> AppResult<Vec<ProviderRow>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, provider, display_name, model, base_url, is_enabled,
                    max_output_tokens, temperature, system_prompt,
                    (api_key IS NOT NULL AND length(api_key) > 0)
             FROM ai_provider_config ORDER BY provider",
        )?;
        Ok(stmt
            .query_map([], |r| {
                let name: String = r.get(1)?;
                let p = Provider::parse(&name).unwrap_or(Provider::OpenaiCompatible);
                Ok(ProviderRow {
                    id: r.get(0)?,
                    has_key: r.get::<_, i64>(9)? != 0,
                    is_local: p.is_local_at(r.get::<_, Option<String>>(4)?.as_deref()),
                    requires_key: p.requires_key(),
                    default_base_url: p.default_base_url().to_string(),
                    config: ProviderConfig {
                        provider: name,
                        display_name: r.get(2)?,
                        model: r.get(3)?,
                        base_url: r.get(4)?,
                        is_enabled: r.get::<_, i64>(5)? != 0,
                        max_output_tokens: r.get(6)?,
                        temperature: r.get(7)?,
                        system_prompt: r.get(8)?,
                    },
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    })
}

/// Save a provider. `api_key` of `None` leaves any stored key untouched, so the
/// UI never has to round-trip a secret it does not display.
#[tauri::command]
pub fn ai_provider_save(
    state: State<'_, AppState>,
    config: ProviderConfig,
    api_key: Option<String>,
) -> AppResult<i64> {
    let p = Provider::parse(&config.provider)
        .ok_or_else(|| AppError::Invalid(format!("unknown provider: {}", config.provider)))?;
    if config.model.trim().is_empty() {
        return Err(AppError::Invalid("a model name is required".into()));
    }

    if let Some(url) = config.base_url.as_deref().filter(|u| !u.trim().is_empty()) {
        provider::check_endpoint(url).map_err(AppError::Invalid)?;
    }
    let id = state.db()?.with(|c| {
        // One provider answers at a time. Several enabled at once were chosen
        // between alphabetically, so "switching to local" could keep sending to
        // a hosted service.
        if config.is_enabled {
            c.execute(
                "UPDATE ai_provider_config SET is_enabled = 0 WHERE provider <> ?1",
                rusqlite::params![p.as_str()],
            )?;
        }
        c.execute(
            "INSERT INTO ai_provider_config
               (provider, display_name, model, base_url, is_enabled,
                max_output_tokens, temperature, system_prompt)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)
             ON CONFLICT(provider) DO UPDATE SET
               display_name = excluded.display_name, model = excluded.model,
               base_url = excluded.base_url, is_enabled = excluded.is_enabled,
               max_output_tokens = excluded.max_output_tokens,
               temperature = excluded.temperature,
               system_prompt = excluded.system_prompt",
            rusqlite::params![
                p.as_str(),
                config.display_name,
                config.model.trim(),
                config.base_url,
                i64::from(config.is_enabled),
                config.max_output_tokens,
                config.temperature,
                config.system_prompt,
            ],
        )?;
        if let Some(key) = api_key.as_deref() {
            c.execute(
                "UPDATE ai_provider_config SET api_key = ?1 WHERE provider = ?2",
                rusqlite::params![
                    if key.trim().is_empty() {
                        None
                    } else {
                        Some(key.trim())
                    },
                    p.as_str()
                ],
            )?;
        }
        Ok(c.query_row(
            "SELECT id FROM ai_provider_config WHERE provider = ?1",
            rusqlite::params![p.as_str()],
            |r| r.get::<_, i64>(0),
        )?)
    })?;
    state.backup.mark_dirty();
    Ok(id)
}

#[tauri::command]
pub fn ai_provider_delete(state: State<'_, AppState>, provider: String) -> AppResult<()> {
    state.db()?.with(|c| {
        c.execute(
            "DELETE FROM ai_provider_config WHERE provider = ?1",
            rusqlite::params![provider],
        )?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

fn active(state: &AppState) -> AppResult<(Provider, ProviderConfig, String)> {
    if !read_flag(state, "privacy.ai.enabled", false) {
        return Err(AppError::Invalid(
            "the assistant is switched off. Turn it on in Settings first.".into(),
        ));
    }
    state.db()?.with(|c| {
        let row = c.query_row(
            "SELECT provider, display_name, model, base_url, max_output_tokens,
                    temperature, system_prompt, COALESCE(api_key, '')
             FROM ai_provider_config WHERE is_enabled = 1 ORDER BY provider LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    ProviderConfig {
                        provider: r.get(0)?,
                        display_name: r.get(1)?,
                        model: r.get(2)?,
                        base_url: r.get(3)?,
                        is_enabled: true,
                        max_output_tokens: r.get(4)?,
                        temperature: r.get(5)?,
                        system_prompt: r.get(6)?,
                    },
                    r.get::<_, String>(7)?,
                ))
            },
        );
        let (name, cfg, key) = row.map_err(|_| {
            AppError::Invalid("no provider is enabled. Add one in Settings.".into())
        })?;
        let p = Provider::parse(&name)
            .ok_or_else(|| AppError::Invalid(format!("unknown provider: {name}")))?;
        if p.requires_key() && key.trim().is_empty() {
            return Err(AppError::Invalid(format!(
                "{name} needs an API key before it can be used"
            )));
        }
        Ok((p, cfg, key))
    })
}

/* ------------------------------------------- categorising by merchant -- */

/// How many unfiled merchants one request asks about.
const AUTOCAT_BATCH: usize = 60;
/// How many of the household's own decisions travel as examples.
const AUTOCAT_HABITS: usize = 80;

/// Build the request that asks a model to file the merchants nothing local
/// could: no rule matches them and the person has never filed them before.
///
/// What is sent is one line per *merchant*, not per transaction — the key
/// word, one masked example, the direction and a rounded typical amount —
/// together with the category list and a sample of past decisions.
#[tauri::command]
pub fn ai_autocat_preview(state: State<'_, AppState>) -> AppResult<EgressPreview> {
    let (p, cfg, _) = active(&state)?;
    let allow_raw = read_flag(&state, "privacy.ai.allow_raw_descriptions", false);

    let (items, categories, habits) = state.db()?.with(|c| {
        let groups = super::autocat::suggest(c)?;
        let items: Vec<MerchantItem> = groups
            .groups
            .iter()
            .filter(|g| g.suggested_category_id.is_none())
            .take(AUTOCAT_BATCH)
            .enumerate()
            .map(|(i, g)| MerchantItem {
                id: i as i64,
                merchant: g.key.clone(),
                example: g
                    .samples
                    .first()
                    .map(|s| s.description.clone())
                    .unwrap_or_default(),
                money_in: g.total_minor.minor() > 0,
                typical: g.total_minor.minor().abs() / g.txn_count.max(1),
                count: g.txn_count,
            })
            .collect();

        let mut stmt = c.prepare(
            "SELECT name, kind FROM pl_category WHERE is_archived = 0 ORDER BY sort_order",
        )?;
        let categories = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;

        // The merchants this household files most often, and where.
        let mut tally: std::collections::HashMap<(String, String), i64> = Default::default();
        let mut hstmt = c.prepare(
            "SELECT t.description_norm, k.name FROM txn t
             JOIN v_txn_effective e ON e.id = t.id
             JOIN pl_category k ON k.id = e.effective_category_id
             WHERE t.is_neutral_transfer = 0",
        )?;
        let rows = hstmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (norm, cat) in rows {
            if let Some(k) = crate::import::merchant::key(&norm) {
                *tally.entry((k, cat)).or_insert(0) += 1;
            }
        }
        let mut habits: Vec<((String, String), i64)> = tally.into_iter().collect();
        habits.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let habits = habits
            .into_iter()
            .take(AUTOCAT_HABITS)
            .map(|(pair, _)| pair)
            .collect::<Vec<_>>();
        Ok((items, categories, habits))
    })?;

    if items.is_empty() {
        return Err(AppError::Invalid(
            "nothing is left for the assistant: every unfiled merchant already has a suggestion"
                .into(),
        ));
    }
    let (payload, red) = ai::merchant_payload(
        &items,
        &categories,
        &habits,
        &country(&state),
        &currency(&state),
        allow_raw,
    );
    Ok(ai::preview(
        p.as_str(),
        &cfg.model,
        "categorize",
        payload,
        items.len(),
        red,
    ))
}

/// A category the model proposes for one merchant group.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MerchantSuggestion {
    /// The merchant key, as the auto-categorise list knows the group.
    pub key: String,
    pub category_id: i64,
    pub category_name: String,
    pub confidence_bps: i64,
}

/// Send the request and turn the reply into suggestions. Nothing is written:
/// they land in the same list as the local ones, for a person to accept.
#[tauri::command]
pub async fn ai_autocat_send(
    state: State<'_, AppState>,
    preview: EgressPreview,
) -> AppResult<Vec<MerchantSuggestion>> {
    let reply = send(&state, &preview, Some(ai::CATEGORISE_SYSTEM.to_string())).await?;
    let asked = ai::merchants_in(&preview.payload);
    let categories: std::collections::HashMap<String, i64> = state.db()?.with(|c| {
        let mut stmt = c.prepare("SELECT name, id FROM pl_category WHERE is_archived = 0")?;
        Ok(stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?)
    })?;
    Ok(ai::parse_merchant_answers(&reply)
        .into_iter()
        .filter_map(|a| {
            // An id that was never asked about, or a category that does not
            // exist, is not a suggestion for anything.
            let key = asked.get(&a.id)?.clone();
            let name = a.category?;
            let category_id = *categories.get(name.trim())?;
            Some(MerchantSuggestion {
                key,
                category_id,
                category_name: name.trim().to_string(),
                confidence_bps: (a.confidence.unwrap_or(0.5).clamp(0.0, 1.0) * 10_000.0) as i64,
            })
        })
        .collect())
}

/* ------------------------------------------------------------- the chat -- */

/// Which parts of the person's records a chat may draw on.
#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct ChatScope {
    pub cashflow: bool,
    pub net_worth: bool,
    pub portfolio: bool,
    pub fire: bool,
}

fn euros(minor: i64) -> i64 {
    (minor as f64 / 100.0).round() as i64
}

/// The aggregates a chat is given: totals by month and category, net worth by
/// class, the portfolio by holding, the FIRE plan's own figures. No single
/// transaction, no description, no account, no name typed into a balance.
fn chat_context(
    c: &rusqlite::Connection,
    scope: ChatScope,
    currency: &str,
) -> AppResult<serde_json::Value> {
    use chrono::Datelike;
    let today = chrono::Local::now().date_naive();
    let mut ctx = serde_json::json!({ "today": today.to_string(), "currency": currency });

    if scope.cashflow {
        let from = chrono::NaiveDate::from_ymd_opt(today.year() - 1, today.month(), 1)
            .unwrap_or(today)
            .to_string();
        let mut stmt = c.prepare(
            "SELECT substr(n.booked_on, 1, 7) AS month,
                    COALESCE(k.name, 'Uncategorised'), COALESCE(k.kind, 'expense'),
                    SUM(n.net_minor)
             FROM v_txn_net n
             LEFT JOIN pl_category k ON k.id = n.effective_category_id
             WHERE n.booked_on >= ?1
             GROUP BY month, n.effective_category_id
             ORDER BY month",
        )?;
        let rows: Vec<(String, String, String, i64)> = stmt
            .query_map(rusqlite::params![from], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<Result<_, _>>()?;
        let (mut income, mut spending) = (0i64, 0i64);
        let mut months = std::collections::BTreeSet::new();
        for (m, _, kind, net) in &rows {
            months.insert(m.clone());
            match kind.as_str() {
                "income" => income += net,
                "expense" => spending += net,
                _ => {}
            }
        }
        ctx["cash_flow"] = serde_json::json!({
            "note": "Net per category per month, refunds netted. Negative is money out. \
                     'neutral' categories are money moved, not earned or spent.",
            "months_covered": months.len(),
            "income_total": euros(income),
            "spending_total": euros(spending),
            // Worked out here rather than left to the model: a small model
            // divides badly, and a wrong savings rate stated with confidence
            // is worse than none.
            "saved_total": euros(income + spending),
            "savings_rate_pct": (income > 0)
                .then(|| ((income + spending) as f64 / income as f64 * 1000.0).round() / 10.0),
            "average_monthly_spending": (!months.is_empty())
                .then(|| euros(spending / months.len() as i64)),
            "by_month_and_category": rows.iter().map(|(m, cat, kind, net)| serde_json::json!({
                "month": m, "category": cat, "kind": kind, "net": euros(*net),
            })).collect::<Vec<_>>(),
        });
    }

    if scope.net_worth {
        let mut stmt = c.prepare(
            "SELECT taken_on, net_worth_minor, liquid_minor, re_equity_minor, total_debt_minor
             FROM v_net_worth ORDER BY taken_on DESC LIMIT 12",
        )?;
        let history: Vec<serde_json::Value> = stmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "date": r.get::<_, String>(0)?,
                    "net_worth": euros(r.get(1)?),
                    "counts_toward_fire": euros(r.get(2)?),
                    "property_equity": euros(r.get(3)?),
                    "debt": euros(r.get(4)?),
                }))
            })?
            .collect::<Result<_, _>>()?;
        let mut cstmt = c.prepare(
            "SELECT k.name, k.kind,
                    SUM(CASE WHEN k.kind = 'liability' THEN -(l.gross_value_minor + l.debt_minor)
                             ELSE l.gross_value_minor - l.debt_minor END)
             FROM wealth_line l JOIN asset_class k ON k.key = l.bucket
             WHERE l.snapshot_id = (SELECT id FROM wealth_snapshot ORDER BY taken_on DESC LIMIT 1)
             GROUP BY k.key ORDER BY k.sort_order",
        )?;
        let classes: Vec<serde_json::Value> = cstmt
            .query_map([], |r| {
                Ok(serde_json::json!({
                    "class": r.get::<_, String>(0)?,
                    "kind": r.get::<_, String>(1)?,
                    "net": euros(r.get(2)?),
                }))
            })?
            .collect::<Result<_, _>>()?;
        ctx["net_worth"] = serde_json::json!({
            "latest_by_asset_class": classes,
            "history_newest_first": history,
        });
    }

    if scope.portfolio {
        let pv = super::portfolio::portfolio_at(c, today)?;
        ctx["portfolio"] = serde_json::json!({
            "value": euros(pv.value.minor()),
            "money_put_in": euros(pv.invested.minor()),
            "gain": euros(pv.gain.minor()),
            "annual_return_pct": pv.xirr_bps.map(|b| b as f64 / 100.0),
            "holdings": pv.holdings.iter().map(|h| serde_json::json!({
                "name": h.name,
                "symbol": h.symbol,
                "asset_class": h.asset_class,
                "value": h.value.map(|v| euros(v.minor())),
                "money_put_in": euros(h.invested.minor()),
                "gain": h.gain.map(|v| euros(v.minor())),
                "annual_return_pct": h.xirr_bps.map(|b| b as f64 / 100.0),
                "price_date": h.price_date,
                "recurring_buys": h.schedules.len(),
            })).collect::<Vec<_>>(),
        });
    }

    if scope.fire {
        let setting = |key: &str| -> Option<String> {
            c.query_row(
                "SELECT value_json FROM app_setting WHERE key = ?1",
                rusqlite::params![key],
                |r| r.get(0),
            )
            .ok()
        };
        let params: crate::fire::FireParams = setting("fire.params")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        let plan_params: crate::fire::PlanParams = setting("fire.plan")
            .and_then(|v| serde_json::from_str(&v).ok())
            .unwrap_or_default();
        if let Some(balances) = super::latest_balances(c)? {
            if let Ok(plan) = crate::fire::plan(&params, &balances, &plan_params) {
                ctx["fire_plan"] = serde_json::json!({
                    "annual_spending": euros(params.annual_spend_target.minor()),
                    "annual_saving": euros(plan_params.annual_savings.minor()),
                    "annual_net_rent": euros(params.annual_net_rental_income.minor()),
                    "withdrawal_rate_pct": params.swr_bps as f64 / 100.0,
                    "real_return_pct": plan_params.real_return_bps as f64 / 100.0,
                    "portfolio_counted": euros(balances.liquid.minor()),
                    "portfolio_needed": euros(plan.target_now.minor()),
                    "years_to_target": plan.years_to_target,
                    "age_now": plan_params.current_age,
                    "age_at_target": plan.age_at_target,
                    "wished_retirement_age": plan.goal.as_ref().map(|g| g.age),
                    "on_track_for_wished_age": plan.goal.as_ref().map(|g| g.on_track),
                    "pension_from_age": plan_params.pension_age,
                    "annual_pension": euros(plan_params.annual_pension.minor()),
                });
            }
        }
    }
    Ok(ctx)
}

/// Assemble one turn of the chat: exactly what would be sent, for the person
/// to read first if they have asked to.
#[tauri::command]
pub fn ai_chat_preview(
    state: State<'_, AppState>,
    messages: Vec<ChatMessage>,
    scope: ChatScope,
) -> AppResult<EgressPreview> {
    let (p, cfg, _) = active(&state)?;
    let last = messages.last().filter(|m| m.role == "user");
    let Some(last) = last.filter(|m| !m.content.trim().is_empty()) else {
        return Err(AppError::Invalid("ask something first".into()));
    };
    if last.content.chars().count() > ai::CHAT_MAX_CHARS {
        return Err(AppError::Invalid(
            "that message is too long — ask one thing at a time".into(),
        ));
    }
    let cur = currency(&state);
    let context = state.db()?.with(|c| chat_context(c, scope, &cur))?;
    let (payload, red) = ai::chat_payload(&context, &messages);
    Ok(ai::preview(
        p.as_str(),
        &cfg.model,
        "nl_query",
        payload,
        messages.len(),
        red,
    ))
}

#[tauri::command]
pub async fn ai_chat_send(state: State<'_, AppState>, preview: EgressPreview) -> AppResult<String> {
    let language = match crate::i18n::current() {
        crate::i18n::Lang::Es => "Spanish (Spain)",
        _ => "English",
    };
    let system = ai::chat_system(language, &currency(&state), &country(&state));
    send(&state, &preview, Some(system)).await
}

/* --------------------------------------------------------------- send --- */

fn log_egress(
    state: &AppState,
    pv: &EgressPreview,
    approved: bool,
    outcome: &str,
    response_bytes: Option<usize>,
) -> AppResult<i64> {
    state.db()?.with(|c| {
        c.execute(
            "INSERT INTO ai_egress_log
               (provider, model, purpose, payload_sha256, payload_preview, row_count,
                approved_by_user, request_bytes, response_bytes, outcome)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                pv.provider,
                pv.model,
                pv.purpose,
                pv.payload_sha256,
                // Store the whole payload: an audit trail that elides the
                // interesting part is not an audit trail.
                pv.payload,
                pv.row_count as i64,
                i64::from(approved),
                pv.bytes as i64,
                response_bytes.map(|b| b as i64),
                outcome,
            ],
        )?;
        Ok(c.last_insert_rowid())
    })
}

/// Send a prepared payload. `system` is the task's own standing instruction;
/// without one the provider's configured prompt, or the default, is used.
async fn send(state: &AppState, pv: &EgressPreview, system: Option<String>) -> AppResult<String> {
    send_with(state, pv, system, 0, false).await
}

/// `send`, with at least `min_tokens` of room for the answer. A reasoning
/// model spends part of its limit thinking, and an answer cut off halfway
/// is worse than a long one.
async fn send_with(
    state: &AppState,
    pv: &EgressPreview,
    system: Option<String>,
    min_tokens: i64,
    json: bool,
) -> AppResult<String> {
    let (p, mut cfg, key) = active(state)?;
    if min_tokens > 0 {
        cfg.max_output_tokens = Some(cfg.max_output_tokens.unwrap_or(0).max(min_tokens));
    }
    let system = system.unwrap_or_else(|| {
        cfg.system_prompt
            .clone()
            .unwrap_or_else(|| DEFAULT_SYSTEM.into())
    });

    // Recorded before the request is made, so an attempt that fails or hangs
    // still leaves a trace of what was about to leave.
    let log_id = log_egress(state, pv, true, "sent", None)?;

    let mut req = provider::build(p, &cfg, &key, &system, &pv.payload);
    if json {
        provider::json_mode(p, &mut req);
    }
    let client = reqwest::Client::builder()
        .redirect(crate::net::redirect_policy())
        // A model running on the person's own machine can take minutes.
        .timeout(std::time::Duration::from_secs(
            if p.is_local_at(cfg.base_url.as_deref()) {
                600
            } else {
                120
            },
        ))
        .build()
        .map_err(|e| AppError::Network(e.to_string()))?;

    let outcome = async {
        let mut attempt = 0;
        let (status, text) = loop {
            let mut rb = client.post(&req.url).json(&req.body);
            for (k, v) in &req.headers {
                rb = rb.header(k.as_str(), v.as_str());
            }
            let resp = rb
                .send()
                .await
                .map_err(|e| AppError::Network(e.to_string()))?;
            let status = resp.status();
            let text = crate::net::text_capped(resp, crate::net::MAX_BODY).await?;
            // A model that does not know a switch refuses the request that
            // carries it — the reasoning switch on a local model, JSON mode
            // on an older or self-hosted endpoint. Take each out and ask
            // again; the answer is still checked in code either way.
            attempt += 1;
            if status.as_u16() == 400
                && attempt <= 3
                && (provider::without_think(&mut req) || provider::without_json_mode(&mut req))
            {
                continue;
            }
            break (status, text);
        };
        if !status.is_success() {
            // The body can contain the key on some providers' error paths, so
            // only the status and a short prefix are surfaced.
            return Err(AppError::Network(format!(
                "{} returned {}: {}",
                p.as_str(),
                status,
                text.chars().take(200).collect::<String>()
            )));
        }
        let body: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| AppError::Network(format!("unreadable response: {e}")))?;
        provider::extract_text(p, &body)
            .ok_or_else(|| AppError::Network("the response contained no text".into()))
            .map(|t| (t, text.len()))
    }
    .await;

    match outcome {
        Ok((text, bytes)) => {
            let _ = state.db()?.with(|c| {
                c.execute(
                    "UPDATE ai_egress_log SET response_bytes = ?1 WHERE id = ?2",
                    rusqlite::params![bytes as i64, log_id],
                )?;
                Ok(())
            });
            state.backup.mark_dirty();
            Ok(text)
        }
        Err(e) => {
            let _ = state.db()?.with(|c| {
                c.execute(
                    "UPDATE ai_egress_log SET outcome = 'failed' WHERE id = ?1",
                    rusqlite::params![log_id],
                )?;
                Ok(())
            });
            Err(e)
        }
    }
}

/// Record that the user looked at a payload and declined to send it. Refusals
/// are part of the audit trail too.
#[tauri::command]
pub fn ai_decline(state: State<'_, AppState>, preview: EgressPreview) -> AppResult<()> {
    log_egress(&state, &preview, false, "declined", None)?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EgressEntry {
    pub id: i64,
    pub occurred_at: String,
    pub provider: String,
    pub model: String,
    pub purpose: String,
    pub payload_sha256: String,
    pub payload_preview: String,
    pub row_count: Option<i64>,
    pub request_bytes: Option<i64>,
    pub response_bytes: Option<i64>,
    pub outcome: String,
}

/// Everything this app has ever sent, or been about to send.
#[tauri::command]
pub fn ai_egress_log(state: State<'_, AppState>) -> AppResult<Vec<EgressEntry>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, occurred_at, provider, model, purpose, payload_sha256,
                    payload_preview, row_count, request_bytes, response_bytes, outcome
             FROM ai_egress_log ORDER BY id DESC LIMIT 100",
        )?;
        Ok(stmt
            .query_map([], |r| {
                Ok(EgressEntry {
                    id: r.get(0)?,
                    occurred_at: r.get(1)?,
                    provider: r.get(2)?,
                    model: r.get(3)?,
                    purpose: r.get(4)?,
                    payload_sha256: r.get(5)?,
                    payload_preview: r.get(6)?,
                    row_count: r.get(7)?,
                    request_bytes: r.get(8)?,
                    response_bytes: r.get(9)?,
                    outcome: r.get(10)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    })
}

/* ----------------------------------------------- format detection help -- */

/// Ask a model to work out a bank export's layout.
///
/// Offered only when local detection has already struggled. Sends a header row
/// and up to four sample rows — redacted — and never the file itself.
#[tauri::command]
pub fn ai_mapping_preview(
    state: State<'_, AppState>,
    path: String,
    header_row: Option<usize>,
    first_data_row: usize,
) -> AppResult<crate::ai::EgressPreview> {
    let (p, cfg, _) = active(&state)?;
    let bytes =
        std::fs::read(&path).map_err(|e| AppError::Invalid(format!("cannot read {path}: {e}")))?;
    let tbl = crate::import::table::load(&path, &bytes, None, None)?;

    let headers = header_row
        .and_then(|i| tbl.rows.get(i).cloned())
        .unwrap_or_default();
    let samples: Vec<Vec<String>> = tbl
        .rows
        .iter()
        .skip(first_data_row)
        .filter(|r| r.iter().any(|c| !c.trim().is_empty()))
        .take(4)
        .cloned()
        .collect();

    let (payload, red) = crate::ai::mapping_payload(&headers, &samples);
    Ok(crate::ai::preview(
        p.as_str(),
        &cfg.model,
        "detect_format",
        payload,
        samples.len(),
        red,
    ))
}

/// Send the detection request and fold the reply into a mapping.
///
/// The model's answer is a *hint*: each field is applied only if it is present
/// and in range for this file, and anything else keeps whatever local detection
/// worked out. A confused reply degrades to the local guess rather than
/// producing a mapping that parses nothing.
#[tauri::command]
pub async fn ai_mapping_send(
    state: State<'_, AppState>,
    preview: crate::ai::EgressPreview,
    base: crate::import::parser::BankMapping,
    width: usize,
) -> AppResult<crate::import::parser::BankMapping> {
    let reply = send(&state, &preview, None).await?;
    let Some(hint) = crate::ai::parse_mapping_hint(&reply) else {
        return Err(AppError::Invalid(
            "the model did not return a usable layout; adjust the columns by hand".into(),
        ));
    };

    let mut m = base;
    let in_range = |c: Option<usize>| c.filter(|&c| c < width);

    if let Some(c) = in_range(hint.date_col) {
        m.date_col = c;
    }
    if let Some(f) = hint.date_format.filter(|f| !f.trim().is_empty()) {
        m.date_format = f;
    }
    if let Some(c) = in_range(hint.description_col) {
        m.description_col = c;
    }
    if let Some(c) = in_range(hint.amount_col) {
        m.amount_col = Some(c);
    }
    if let Some(d) = hint.decimal_separator.and_then(|s| s.chars().next()) {
        if d == ',' || d == '.' {
            m.decimal_separator = d;
            m.thousands_separator = if d == ',' { ".".into() } else { ",".into() };
        }
    }
    // The separator comes from a model's reading of a few sample rows and is
    // then applied to every amount, so only a real grouping mark is accepted.
    if let Some(t) = hint.thousands_separator {
        if matches!(
            t.as_str(),
            "," | "." | " " | "'" | "\u{a0}" | "\u{202f}" | ""
        ) && t != m.decimal_separator.to_string()
        {
            m.thousands_separator = t;
        }
    }
    Ok(m)
}

/* --------------------------------------------------------- price checks -- */

const PRICE_SYSTEM: &str = "You are a data-extraction function that looks up prices of \
     investments. You are not a conversational assistant: never greet, explain, apologise or \
     reason aloud.\n\
     OUTPUT: reply with exactly one JSON array and nothing else — no text before or after it, no \
     markdown code fences, no comments. One object per price asked for; never add, drop or \
     renumber an id. Each object has exactly these keys: \"id\", \"date\" (YYYY-MM-DD), \"price\" \
     (a JSON number or null), \"source_url\" (a string or null), \"note\" (at most eight words, or \
     null).\n\
     PRICE: a plain JSON number in the currency asked for — a dot for decimals, no thousands \
     separators, no symbol, no quotes. Search the web and answer only from pages you actually \
     found, for THIS asset and no similar one. The latest quote for a current price; for a past \
     date the close on that date or on the last trading day before it. If a price exists only in \
     another currency, convert it with that day's exchange rate and say so in the note. Never \
     estimate, average, round or recall a price from memory; ignore percentages, highs, lows and \
     market caps. Abbreviated figures (76.1K) are not exact: find the full figure or use null.\n\
     NULL: when you cannot find a price, set price and source_url to null and give a short note. \
     A null is a correct answer; a guess is a failure.";

/// Past dates asked about for one asset at most. A lookup of sixty dates is
/// one nobody waits for.
const HISTORY_DATES: usize = 18;

/// One asset to price: its id, name, symbol, and the past dates wanted.
pub struct PriceAsk {
    pub id: i64,
    pub name: String,
    pub symbol: Option<String>,
    pub history: Vec<String>,
}

/// What will be sent to look prices up: asset names, symbols and dates.
/// No amounts, no units, nothing about how much the person holds.
///
/// Every asset is asked for its current price. An asset with buys recorded by
/// their cost alone is also asked for the month-end closes since it was first
/// bought: with those, each buy can be turned into the units it bought, and
/// the history chart has real prices to stand on.
pub fn price_payload(assets: &[PriceAsk], today: &str, currency: &str) -> String {
    let mut out = format!(
        "Today is {today}. For each asset below, find its current price in {currency} (the \
         latest quote or close). Where an asset lists past dates, also find its closing price \
         in {currency} on each of those dates (or on the last trading day before it).\n\
         Reply with only a JSON array, one object per price: \
         [{{\"id\": <id>, \"date\": \"YYYY-MM-DD\", \"price\": <number or null>, \
         \"source_url\": \"<page the price came from>\", \"note\": \"<optional>\"}}]\n\
         Use {today} as the date of a current price.\n\nAssets:\n"
    );
    for a in assets {
        out.push_str(&format!("- id {}: {}", a.id, a.name));
        if let Some(s) = &a.symbol {
            out.push_str(&format!(" ({s})"));
        }
        if !a.history.is_empty() {
            out.push_str(&format!(" — past dates: {}", a.history.join(", ")));
        }
        out.push('\n');
    }
    out.push_str(&format!("\n{}", ai::ANSWER_CUE));
    out
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PriceProposal {
    pub holding_id: i64,
    pub name: String,
    /// Millionths of a currency unit per unit held; `None` when nothing was
    /// found.
    pub price_micro: Option<i64>,
    /// The date the price is for. For a current price it can be a day or two
    /// back when markets were shut.
    pub price_date: Option<String>,
    /// True for a past price asked for to fill in history, false for the
    /// current one.
    pub historical: bool,
    pub source_url: Option<String>,
    /// The source is one of the pages the search really returned. When it is
    /// not, the model named a page it did not read, and the price should be
    /// checked by hand before it is trusted.
    pub source_seen: bool,
    /// The words the model says the price was copied from. For a price read
    /// from search results this has been checked against them.
    #[serde(default)]
    pub evidence: Option<String>,
    pub note: Option<String>,
}

fn host(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let h = rest.split(['/', '?', '#']).next()?.to_ascii_lowercase();
    Some(h.trim_start_matches("www.").to_string()).filter(|h| !h.is_empty())
}

/// Read the model's JSON, and check each claimed source against what the
/// search actually returned. A price dated more than a week before `today`
/// is a historical one.
pub fn parse_prices(
    reply: &str,
    names: &std::collections::HashMap<i64, String>,
    seen: &[String],
    today: chrono::NaiveDate,
) -> Vec<PriceProposal> {
    let items = ai::answer_items(reply);
    let seen_hosts: Vec<String> = seen.iter().filter_map(|u| host(u)).collect();
    items
        .into_iter()
        .filter_map(|it| {
            // An id as text ("1") is the same id.
            let id = it.get("id").and_then(|i| {
                i.as_i64()
                    .or_else(|| i.as_str().and_then(|s| s.trim().parse().ok()))
            })?;
            // An id the request never mentioned is not a price for anything
            // the user holds.
            let name = names.get(&id)?.clone();
            let price = it
                .get("price")
                .or_else(|| it.get("price_eur"))
                .and_then(|p| match p {
                    serde_json::Value::String(s) => ai::numbers_in(s).first().copied(),
                    other => other.as_f64(),
                })
                // A price is a positive, finite number of a plausible size:
                // zero and absurd magnitudes are a misreading, not a quote.
                .filter(|p| p.is_finite() && *p > 0.0 && *p < 1.0e9)
                .map(|p| (p * 1_000_000.0).round() as i64);
            let url = it
                .get("source_url")
                .and_then(|u| u.as_str())
                .map(str::to_string)
                .filter(|u| u.starts_with("https://") || u.starts_with("http://"));
            let source_seen = url.as_deref().and_then(host).is_some_and(|h| {
                seen_hosts.iter().any(|s| {
                    s == &h || s.ends_with(&format!(".{h}")) || h.ends_with(&format!(".{s}"))
                })
            });
            let date = it
                .get("date")
                .and_then(|d| d.as_str())
                .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
                // A date in the future is not a price anyone can have seen.
                .filter(|d| *d <= today);
            Some(PriceProposal {
                holding_id: id,
                name,
                price_micro: price,
                historical: date.is_some_and(|d| (today - d).num_days() > 7),
                price_date: date.map(|d| d.to_string()),
                source_url: url,
                source_seen,
                evidence: it
                    .get("evidence")
                    .and_then(|e| e.as_str())
                    .map(str::to_string)
                    .filter(|e| !e.trim().is_empty()),
                note: it
                    .get("note")
                    .and_then(|n| n.as_str())
                    .map(str::to_string)
                    .filter(|n| !n.is_empty()),
            })
        })
        .collect()
}

/// Hold a price read from search results to what the results say.
///
/// A price stays only if the words the model quoted are in one of that
/// asset's results and contain the number. Its source becomes the result the
/// words were found in rather than the address the model typed, so neither
/// can be invented. Anything else is kept as a null with the reason, so the
/// review shows what happened instead of silently losing a row.
fn enforce_evidence(
    proposals: &mut [PriceProposal],
    found: &[(i64, Vec<ai::SearchHit>)],
    today: chrono::NaiveDate,
) {
    for p in proposals.iter_mut() {
        // The reader is not asked for a date or a note: a price read from
        // today's results is today's, and the reason for a null is Coin Heaven's.
        p.price_date.get_or_insert_with(|| today.to_string());
        let Some(micro) = p.price_micro else {
            p.note
                .get_or_insert_with(|| "no result states a price".into());
            continue;
        };
        let hits: &[ai::SearchHit] = found
            .iter()
            .find(|f| f.0 == p.holding_id)
            .map_or(&[], |f| f.1.as_slice());
        let price = micro as f64 / 1_000_000.0;
        match p
            .evidence
            .as_deref()
            .and_then(|e| ai::check_evidence(price, e, hits))
        {
            Some(hit) => {
                p.source_url = Some(hit.url.clone());
                p.source_seen = true;
            }
            None => {
                p.price_micro = None;
                p.source_seen = false;
                p.note = Some(
                    "discarded: the words quoted for this price are not in the search results"
                        .into(),
                );
            }
        }
    }
}

/// Every holding, with the past dates worth asking a price for.
fn price_asks(state: &AppState) -> AppResult<Vec<PriceAsk>> {
    let today = chrono::Local::now().date_naive();
    state.db()?.with(|c| {
        let heads: Vec<(i64, String, Option<String>)> = {
            let mut st = c.prepare(
                "SELECT id, name, symbol FROM holding WHERE is_archived = 0 ORDER BY name",
            )?;
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<_, _>>()?
        };
        heads
            .into_iter()
            .map(|(id, name, symbol)| {
                Ok(PriceAsk {
                    history: super::portfolio::history_dates_wanted(c, id, today, HISTORY_DATES)?
                        .into_iter()
                        .map(|d| d.to_string())
                        .collect(),
                    id,
                    name,
                    symbol,
                })
            })
            .collect()
    })
}

/// Prepare a price lookup: what will be asked, and of whom.
///
/// A provider that can search the web is asked directly. One that cannot — a
/// model running on the person's own machine, typically — can still be used
/// if a self-hosted search engine is configured: Coin Heaven runs the searches
/// there and hands the results to the model to read. With neither, there is
/// nothing to look a price up *in*, and the request is refused rather than
/// answered from the model's memory.
#[tauri::command]
pub fn ai_price_preview(state: State<'_, AppState>) -> AppResult<EgressPreview> {
    let (p, cfg, _) = active(&state)?;
    let native = provider::build_search(p, &cfg, "", "", "").is_some();
    let engine = search_url(&state);
    if !native && engine.is_none() {
        return Err(AppError::Invalid(format!(
            "{} cannot search the web on its own. Turn on a search engine (DuckDuckGo, Exa or \
             your own SearXNG) in Settings, or enter prices by hand",
            p.as_str()
        )));
    }
    let asks = price_asks(&state)?;
    if asks.is_empty() {
        return Err(AppError::Invalid(
            "there are no holdings to price yet".into(),
        ));
    }
    let today = chrono::Local::now().date_naive().to_string();
    let cur = currency(&state);
    let (purpose_provider, payload) = match (native, engine) {
        (true, _) => (p.as_str().to_string(), price_payload(&asks, &today, &cur)),
        (false, Some(eng)) => {
            let (head, tag) = match &eng {
                Engine::DuckDuckGo => (
                    "These searches go to DuckDuckGo, which sees the words and your IP address \
                     but nothing else from Coin Heaven:"
                        .to_string(),
                    "duckduckgo",
                ),
                Engine::Exa(_) => (
                    "These searches go to Exa, which sees the words, your IP address and the \
                     account the key belongs to, but nothing else from Coin Heaven:"
                        .to_string(),
                    "exa",
                ),
                Engine::Searx(url) => (
                    format!("Searches run on your own search engine at {url}:"),
                    "searxng",
                ),
            };
            let mut out = format!("{head}\n");
            for a in &asks {
                out.push_str(&format!(
                    "- {}\n",
                    ai::price_query(&a.name, a.symbol.as_deref(), &cur)
                ));
            }
            out.push_str(&format!(
                "\nThe results, with these asset names, are then read by {} ({}).",
                p.as_str(),
                cfg.model
            ));
            (format!("{tag}+{}", p.as_str()), out)
        }
        (false, None) => unreachable!("refused above"),
    };
    Ok(ai::preview(
        &purpose_provider,
        &cfg.model,
        "price_check",
        payload,
        asks.len(),
        Default::default(),
    ))
}

/// One request to a provider that searches the web itself.
async fn price_via_provider(
    p: Provider,
    cfg: &ProviderConfig,
    key: &str,
    payload: &str,
) -> AppResult<(String, Vec<String>, usize)> {
    let mut req = provider::build_search(p, cfg, key, PRICE_SYSTEM, payload)
        .ok_or_else(|| AppError::Invalid("this provider cannot search the web".into()))?;
    let client = reqwest::Client::builder()
        .redirect(crate::net::redirect_policy())
        // Searching several assets and dates can take a while.
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| AppError::Network(e.to_string()))?;

    let mut text = String::new();
    let mut seen = Vec::new();
    let mut bytes = 0;
    // A long search can be paused by the server; resuming means sending
    // the conversation back as it stands, without a new user message.
    let mut swapped = false;
    for _ in 0..6 {
        let mut rb = client.post(&req.url).json(&req.body);
        for (k, v) in &req.headers {
            rb = rb.header(k.as_str(), v.as_str());
        }
        let resp = rb
            .send()
            .await
            .map_err(|e| AppError::Network(e.to_string()))?;
        let status = resp.status();
        let raw = crate::net::text_capped(resp, crate::net::MAX_BODY).await?;
        bytes += raw.len();
        // A model newer or older than this build knows may want the other
        // version of the search tool. One retry with it costs nothing and
        // means a model released next year does not need a new Coin Heaven.
        if status.as_u16() == 400 && !swapped && provider::other_search_tool(&mut req) {
            swapped = true;
            continue;
        }
        if !status.is_success() {
            return Err(AppError::Network(format!(
                "{} returned {}: {}",
                p.as_str(),
                status,
                raw.chars().take(200).collect::<String>()
            )));
        }
        let body: serde_json::Value = serde_json::from_str(&raw)
            .map_err(|e| AppError::Network(format!("unreadable response: {e}")))?;
        seen.extend(provider::source_urls(p, &body));
        if let Some(t) = provider::extract_all_text(p, &body) {
            text.push_str(&t);
        }
        if provider::paused(p, &body) {
            let content = body.get("content").cloned().unwrap_or_default();
            req.body["messages"] = serde_json::json!([
                { "role": "user", "content": payload },
                { "role": "assistant", "content": content },
            ]);
            continue;
        }
        break;
    }
    if text.is_empty() {
        return Err(AppError::Network("the response contained no text".into()));
    }
    Ok((text, seen, bytes))
}

/// Percent-encode a query-string value.
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A browser-like identity. DuckDuckGo's plain-HTML page answers a bare HTTP
/// client with a challenge; saying plainly what this is would get no results.
const SEARCH_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

/// Run each asset's query on the engine the person chose.
async fn search_hits(
    engine: &Engine,
    asks: &[PriceAsk],
    currency: &str,
) -> AppResult<Vec<(i64, String, Option<String>, Vec<ai::SearchHit>)>> {
    let client = reqwest::Client::builder()
        .redirect(crate::net::redirect_policy())
        .timeout(std::time::Duration::from_secs(30))
        .user_agent(SEARCH_AGENT)
        .build()
        .map_err(|e| AppError::Network(e.to_string()))?;
    let mut out = Vec::with_capacity(asks.len());
    let mut throttled: Vec<String> = Vec::new();
    for (n, a) in asks.iter().enumerate() {
        let q = ai::price_query(&a.name, a.symbol.as_deref(), currency);
        let hits = match engine {
            Engine::DuckDuckGo => {
                // A public page, not an API: one request at a time and a
                // pause between them, or it starts refusing.
                if n > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(2500)).await;
                }
                // POST, as a browser submitting the search box does; this is
                // what the maintained open-source clients send. A challenge
                // is a "slow down", so it is waited out twice before giving
                // up on this asset — and giving up on one does not cost the
                // others their lookup.
                let mut hits = Vec::new();
                let mut last = String::new();
                for attempt in 0..3u64 {
                    if attempt > 0 {
                        tokio::time::sleep(std::time::Duration::from_secs(4 * attempt)).await;
                    }
                    let resp = client
                        .post("https://html.duckduckgo.com/html/")
                        .header("Content-Type", "application/x-www-form-urlencoded")
                        .body(format!("q={}&b=&l=wt-wt", urlencode(&q)))
                        .send()
                        .await
                        .map_err(|e| {
                            AppError::Network(format!("DuckDuckGo did not answer: {e}"))
                        })?;
                    let status = resp.status();
                    let html = crate::net::text_capped(resp, crate::net::MAX_BODY).await?;
                    hits = ai::ddg_hits(&html, 6);
                    last = status.to_string();
                    if status.is_success() && !hits.is_empty() {
                        break;
                    }
                    hits.clear();
                }
                if hits.is_empty() {
                    throttled.push(last);
                }
                hits
            }
            Engine::Exa(key) => {
                if n > 0 {
                    // Exa allows far more than this; the pause only keeps a
                    // long list of holdings from arriving as one burst.
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
                let body = serde_json::json!({
                    "query": q,
                    "numResults": 6,
                    "type": "auto",
                    "contents": { "highlights": { "query": q, "maxCharacters": 400 } },
                });
                let resp = client
                    .post("https://api.exa.ai/search")
                    .header("x-api-key", key)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| AppError::Network(format!("Exa did not answer: {e}")))?;
                let status = resp.status();
                let raw = crate::net::text_capped(resp, crate::net::MAX_BODY).await?;
                match status.as_u16() {
                    200 => {}
                    401 | 403 => {
                        return Err(AppError::Network(
                            "Exa did not accept the key. Check it in Settings".into(),
                        ));
                    }
                    402 | 429 => {
                        return Err(AppError::Network(
                            "Exa is out of credits or limiting requests. Check your Exa account"
                                .into(),
                        ));
                    }
                    other => {
                        return Err(AppError::Network(format!("Exa returned {other}")));
                    }
                }
                let body: serde_json::Value = serde_json::from_str(&raw)
                    .map_err(|_| AppError::Network("Exa did not return JSON".into()))?;
                ai::exa_hits(&body, 6)
            }
            Engine::Searx(base) => {
                let resp = client
                    .get(format!("{base}/search?q={}&format=json", urlencode(&q)))
                    .send()
                    .await
                    .map_err(|e| {
                        AppError::Network(format!("the search engine did not answer: {e}"))
                    })?;
                let status = resp.status();
                let raw = crate::net::text_capped(resp, crate::net::MAX_BODY).await?;
                if !status.is_success() {
                    // SearXNG answers 403 when its JSON output is not switched on.
                    return Err(AppError::Network(format!(
                        "the search engine returned {status}. In SearXNG's settings.yml, \
                         `search.formats` has to include `json`"
                    )));
                }
                let body: serde_json::Value = serde_json::from_str(&raw).map_err(|_| {
                    AppError::Network("the search engine did not return JSON".into())
                })?;
                ai::searx_hits(&body, 6)
            }
        };
        out.push((a.id, a.name.clone(), a.symbol.clone(), hits));
    }
    if let Some(status) = throttled.first().filter(|_| throttled.len() == asks.len()) {
        return Err(AppError::Network(format!(
            "DuckDuckGo returned no results ({status}). It may be limiting requests — wait a few \
             minutes, or use your own SearXNG"
        )));
    }
    Ok(out)
}

#[tauri::command]
pub async fn ai_price_send(
    state: State<'_, AppState>,
    preview: EgressPreview,
) -> AppResult<Vec<PriceProposal>> {
    let (p, cfg, key) = active(&state)?;
    let asks = price_asks(&state)?;
    let names: std::collections::HashMap<i64, String> =
        asks.iter().map(|a| (a.id, a.name.clone())).collect();
    let today = chrono::Local::now().date_naive();
    let cur = currency(&state);
    let native = provider::build_search(p, &cfg, "", "", "").is_some();

    let log_id = log_egress(&state, &preview, true, "sent", None)?;
    let mut found_hits: Vec<(i64, Vec<ai::SearchHit>)> = Vec::new();
    let result: AppResult<(String, Vec<String>, usize)> = if native {
        price_via_provider(p, &cfg, &key, &preview.payload).await
    } else {
        match search_url(&state) {
            None => Err(AppError::Invalid(
                "this provider cannot search the web".into(),
            )),
            Some(engine) => match search_hits(&engine, &asks, &cur).await {
                Err(e) => Err(e),
                Ok(found) => {
                    found_hits = found.iter().map(|f| (f.0, f.3.clone())).collect();
                    let seen: Vec<String> = found
                        .iter()
                        .flat_map(|f| f.3.iter().map(|h| h.url.clone()))
                        .collect();
                    // What the model is given is logged in full, like every
                    // other request: the results came from the web, and the
                    // record of what left for the model should hold them.
                    let payload = ai::price_from_hits_payload(&found, &today.to_string(), &cur);
                    let pv = ai::preview(
                        p.as_str(),
                        &cfg.model,
                        "price_check",
                        payload,
                        found.len(),
                        Default::default(),
                    );
                    send_with(
                        &state,
                        &pv,
                        Some(ai::READER_SYSTEM.to_string()),
                        8_000,
                        true,
                    )
                    .await
                    .map(|text| {
                        let n = text.len();
                        (text, seen, n)
                    })
                }
            },
        }
    };

    let outcome = match &result {
        Ok((_, _, b)) => ("sent", Some(*b as i64)),
        Err(_) => ("failed", None),
    };
    let _ = state.db()?.with(|c| {
        c.execute(
            "UPDATE ai_egress_log SET outcome = ?1, response_bytes = ?2 WHERE id = ?3",
            rusqlite::params![outcome.0, outcome.1, log_id],
        )?;
        Ok(())
    });
    state.backup.mark_dirty();
    let (text, seen, _) = result?;
    let mut proposals = parse_prices(&text, &names, &seen, today);
    if !native {
        enforce_evidence(&mut proposals, &found_hits, today);
    }
    if proposals.is_empty() {
        // What the model actually said, so a failure can be read rather than
        // guessed at. The reply is the model's own words about prices.
        let said: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        return Err(AppError::Invalid(if said.is_empty() {
            "the model sent back an empty answer; it may have run out of room while thinking. \
             Try again, or use a model without a thinking mode"
                .into()
        } else {
            format!(
                "the model did not return prices Coin Heaven could read. It said: {}",
                said.chars().take(400).collect::<String>()
            )
        }));
    }
    Ok(proposals)
}

#[cfg(test)]
mod price_tests {
    use super::*;

    fn names() -> std::collections::HashMap<i64, String> {
        [
            (1, "Bitcoin".to_string()),
            (2, "Vanguard All-World".to_string()),
        ]
        .into_iter()
        .collect()
    }

    fn day(s: &str) -> chrono::NaiveDate {
        chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn the_request_carries_names_and_dates_and_nothing_about_holdings() {
        let p = price_payload(
            &[
                PriceAsk {
                    id: 1,
                    name: "Bitcoin".into(),
                    symbol: Some("BTC".into()),
                    history: vec!["2025-01-31".into(), "2025-02-28".into()],
                },
                PriceAsk {
                    id: 2,
                    name: "Fund".into(),
                    symbol: None,
                    history: Vec::new(),
                },
            ],
            "2026-06-30",
            "EUR",
        );
        assert!(p.contains("Today is 2026-06-30"));
        assert!(p.contains("id 1: Bitcoin (BTC) — past dates: 2025-01-31, 2025-02-28"));
        assert!(p.contains("- id 2: Fund\n"), "no past dates, none asked");
        assert!(p.contains("in EUR"));
        assert!(!p.contains('€'), "no amounts");
    }

    #[test]
    fn prices_parse_and_sources_are_checked_against_what_was_searched() {
        let reply = r#"Here you go:
[{"id":1,"price":93210.55,"date":"2026-06-30","source_url":"https://www.coindesk.com/price/bitcoin"},
 {"id":2,"price_eur":131.2,"date":"2026-06-29","source_url":"https://made-up.example/x","note":"Friday close"},
 {"id":1,"price":61000,"date":"2025-01-31","source_url":"https://coindesk.com/history"},
 {"id":9,"price":1.0,"date":"2026-06-30","source_url":"https://x.com"}]"#;
        let seen = vec!["https://coindesk.com/price/bitcoin?x=1".to_string()];
        let out = parse_prices(reply, &names(), &seen, day("2026-06-30"));
        assert_eq!(out.len(), 3, "an id that was never asked about is dropped");
        assert_eq!(out[0].price_micro, Some(93_210_550_000));
        assert!(out[0].source_seen, "www. and query strings do not matter");
        assert!(!out[0].historical);
        assert!(!out[1].source_seen, "a page the search never returned");
        assert_eq!(out[1].price_date.as_deref(), Some("2026-06-29"));
        assert!(
            !out[1].historical,
            "the last close before today is still current"
        );
        assert_eq!(out[1].note.as_deref(), Some("Friday close"));
        assert!(out[2].historical);
        assert_eq!(out[2].price_micro, Some(61_000_000_000));
    }

    #[test]
    fn a_search_query_is_safe_in_a_url() {
        assert_eq!(
            urlencode("S&P 500 ETF price €"),
            "S%26P%20500%20ETF%20price%20%E2%82%AC"
        );
    }

    #[test]
    fn nothing_found_is_a_null_not_a_zero_and_the_future_is_not_a_date() {
        let out = parse_prices(
            r#"[{"id":1,"price":null,"date":null},{"id":2,"price":5,"date":"2031-01-01"}]"#,
            &names(),
            &[],
            day("2026-06-30"),
        );
        assert_eq!(out[0].price_micro, None);
        assert_eq!(out[0].price_date, None);
        assert_eq!(out[1].price_date, None);
    }
}

#[cfg(test)]
mod live {
    use super::*;

    /// Needs the internet. Proves DuckDuckGo answers Coin Heaven's own HTTP client
    /// and that its page is still one the parser understands — the two things
    /// a fixture cannot.
    #[tokio::test]
    #[ignore = "queries DuckDuckGo"]
    async fn duckduckgo_answers_the_real_client_with_readable_results() {
        let asks = vec![
            PriceAsk {
                id: 1,
                name: "Bitcoin".into(),
                symbol: Some("BTC".into()),
                history: vec![],
            },
            PriceAsk {
                id: 2,
                name: "Vanguard FTSE All-World UCITS ETF".into(),
                symbol: Some("VWCE".into()),
                history: vec![],
            },
        ];
        let found = search_hits(&Engine::DuckDuckGo, &asks, "EUR")
            .await
            .unwrap();
        assert_eq!(found.len(), 2);
        for (_, name, _, hits) in &found {
            assert!(!hits.is_empty(), "no results for {name}");
            println!("{name}:");
            for h in hits {
                println!("  {} — {}\n    {}", h.title, h.url, h.snippet);
            }
        }
    }
}
