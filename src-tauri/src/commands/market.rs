//! Looking up the prices a holding's buys are missing.
//!
//! The person sees what will be asked, and of whom, before it is asked: the
//! preview lists each holding with how many buys have no price and the
//! source, and nothing is requested until they confirm. What is sent is a
//! ticker and a date range — never an amount, a unit count or a balance.

use super::portfolio::{blind_dates, resync_all};
use crate::error::{AppError, AppResult};
use crate::market::{self, Problem, Resolver, Series};
use crate::state::AppState;
use chrono::NaiveDate;
use rusqlite::{Connection, params};
use serde::Serialize;
use tauri::State;

const SOURCE_NAME: &str = "Yahoo Finance";
/// A price older than this is not "the price on" a buy day.
const STALE_DAYS: i64 = 7;

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MarketAsk {
    pub holding_id: i64,
    pub name: String,
    pub symbol: Option<String>,
    pub asset_class: String,
    pub buys_missing: usize,
    pub from: String,
    pub source: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MarketResult {
    pub holding_id: i64,
    pub name: String,
    pub symbol_used: Option<String>,
    pub currency: Option<String>,
    /// How many dates now carry a price.
    pub stored: usize,
    /// Buys the source had no price for (before the instrument existed, say).
    pub left_out: usize,
    pub from: Option<String>,
    pub to: Option<String>,
    pub problem: Option<Problem>,
}

/// id, name, symbol, asset class.
type Head = (i64, String, Option<String>, String);

fn heads(c: &Connection) -> AppResult<Vec<Head>> {
    let mut st = c.prepare(
        "SELECT id, name, symbol, asset_class FROM holding WHERE is_archived = 0 ORDER BY name",
    )?;
    Ok(st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?)
}

/// Holdings with buys that have no units and no price to work them out from.
pub fn asks(c: &Connection) -> AppResult<Vec<MarketAsk>> {
    super::portfolio::materialise(c, chrono::Local::now().date_naive())?;
    let mut out = Vec::new();
    for (id, name, symbol, class) in heads(c)? {
        let missing: Vec<NaiveDate> = blind_dates(c, id)?
            .into_iter()
            .filter(|d| {
                // Already priced on that very day: nothing to ask.
                c.query_row(
                    "SELECT count(*) FROM holding_price WHERE holding_id = ?1 AND on_date = ?2",
                    params![id, d.to_string()],
                    |r| r.get::<_, i64>(0),
                )
                .map(|n| n == 0)
                .unwrap_or(true)
            })
            .collect();
        if let Some(first) = missing.first() {
            out.push(MarketAsk {
                holding_id: id,
                name,
                symbol,
                asset_class: class,
                buys_missing: missing.len(),
                from: first.to_string(),
                source: SOURCE_NAME.into(),
            });
        }
    }
    Ok(out)
}

/// Write the prices a series gives for the dates asked about, and the latest
/// close. A price someone typed is never replaced.
pub fn apply(
    c: &Connection,
    holding: i64,
    series: &Series,
    wanted: &[NaiveDate],
) -> AppResult<(usize, usize)> {
    let url = market::quote_page(&series.symbol);
    let micro = |p: f64| (p * 1_000_000.0).round() as i64;
    let tx = c.unchecked_transaction()?;
    let (mut stored, mut left_out) = (0, 0);
    let put = |on: NaiveDate, price: f64| -> rusqlite::Result<()> {
        tx.execute(
            "INSERT INTO holding_price (holding_id, on_date, price_micro, source, source_url)
             VALUES (?1, ?2, ?3, 'market', ?4)
             ON CONFLICT(holding_id, on_date) DO UPDATE SET
                 price_micro = excluded.price_micro, source = excluded.source,
                 source_url = excluded.source_url,
                 recorded_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')
             WHERE holding_price.source <> 'manual'",
            params![holding, on.to_string(), micro(price), url],
        )?;
        Ok(())
    };
    for d in wanted {
        match market::close_on_or_before(series, *d, STALE_DAYS) {
            Some((_, price)) => {
                put(*d, price)?;
                stored += 1;
            }
            None => left_out += 1,
        }
    }
    if let Some(&(on, price)) = series.closes.last() {
        put(on, price)?;
    }
    resync_all(&tx)?;
    tx.commit()?;
    Ok((stored, left_out))
}

#[tauri::command]
pub fn market_preview(state: State<'_, AppState>) -> AppResult<Vec<MarketAsk>> {
    state.db()?.with(asks)
}

fn client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(crate::net::redirect_policy())
        .timeout(std::time::Duration::from_secs(20))
        // A plain browser-style identity: the source answers a bare HTTP
        // client with "too many requests".
        .user_agent("Mozilla/5.0")
        .build()
        .map_err(|e| AppError::Network(e.to_string()))
}

async fn get_json(client: &reqwest::Client, url: &str) -> Result<serde_json::Value, Problem> {
    for attempt in 0..3u64 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(3 * attempt)).await;
        }
        let resp = client
            .get(url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|_| Problem::Unreachable)?;
        match resp.status().as_u16() {
            200 => {
                let text = crate::net::text_capped(resp, crate::net::MAX_BODY)
                    .await
                    .map_err(|_| Problem::Unreachable)?;
                return serde_json::from_str(&text).map_err(|_| Problem::Unreachable);
            }
            404 => return Err(Problem::NotFound),
            429 => continue,
            _ => return Err(Problem::Unreachable),
        }
    }
    Err(Problem::RateLimited)
}

async fn series_for(
    client: &reqwest::Client,
    symbol: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<Series, Problem> {
    let body = get_json(client, &market::chart_url(symbol, from, to)).await;
    match body {
        Ok(b) => market::parse_chart(&b),
        // The source answers 404 with a JSON body that names the problem.
        Err(Problem::NotFound) => Err(Problem::NotFound),
        Err(e) => Err(e),
    }
}

#[tauri::command]
pub async fn market_fetch(
    state: State<'_, AppState>,
    holding_ids: Vec<i64>,
) -> AppResult<Vec<MarketResult>> {
    let today = chrono::Local::now().date_naive();
    let currency = state
        .db()?
        .with(|c| {
            Ok(c.query_row(
                "SELECT value_json FROM app_setting WHERE key = 'ui.currency.primary'",
                [],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|v| serde_json::from_str::<String>(&v).ok())
            .unwrap_or_else(|| crate::currency::DEFAULT.to_string()))
        })?
        .to_uppercase();
    let client = client()?;
    let mut out = Vec::new();

    for (n, id) in holding_ids.iter().enumerate() {
        let info = state.db()?.with(|c| {
            let h = heads(c)?.into_iter().find(|h| h.0 == *id);
            let dates = blind_dates(c, *id)?;
            Ok((h, dates))
        })?;
        let (Some((_, name, symbol, class)), dates) = info else {
            continue;
        };
        let mut result = MarketResult {
            holding_id: *id,
            name: name.clone(),
            symbol_used: None,
            currency: None,
            stored: 0,
            left_out: 0,
            from: None,
            to: None,
            problem: None,
        };
        let Some(first) = dates.first().copied() else {
            out.push(result);
            continue;
        };
        if n > 0 {
            // One request at a time, with a pause: it is a public endpoint.
            tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
        }

        let mut r = Resolver::new(&currency);
        let mut found: Option<Series> = None;
        let guesses = market::first_guesses(symbol.as_deref(), class == "crypto", &currency);
        for g in &guesses {
            if r.wants(g) {
                found = r.offer(series_for(&client, g, first, today).await);
            }
            if found.is_some() || r.must_stop() {
                break;
            }
        }
        if found.is_none() && !r.must_stop() {
            // Ask the source's own search, by ticker or by name.
            let q = symbol
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(&name);
            tokio::time::sleep(std::time::Duration::from_millis(600)).await;
            let candidates = match get_json(&client, &market::search_url(q)).await {
                Ok(b) => market::parse_search(&b),
                Err(Problem::RateLimited) => {
                    out.push(MarketResult {
                        problem: Some(Problem::RateLimited),
                        ..result
                    });
                    break;
                }
                Err(_) => Vec::new(),
            };
            for cand in candidates.into_iter().take(6) {
                if r.wants(&cand.symbol) {
                    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
                    found = r.offer(series_for(&client, &cand.symbol, first, today).await);
                }
                if found.is_some() || r.must_stop() {
                    break;
                }
            }
        }

        match found {
            Some(series) => {
                let wanted: Vec<NaiveDate> = dates;
                let (stored, left_out) = state.db()?.with(|c| apply(c, *id, &series, &wanted))?;
                result.symbol_used = Some(series.symbol.clone());
                result.currency = Some(series.currency.clone());
                result.stored = stored;
                result.left_out = left_out;
                result.from = series.closes.first().map(|c| c.0.to_string());
                result.to = series.closes.last().map(|c| c.0.to_string());
                if stored == 0 {
                    result.problem = Some(Problem::NoData);
                }
            }
            None => result.problem = Some(r.failure()),
        }
        let stop = result.problem == Some(Problem::RateLimited);
        out.push(result);
        if stop {
            break;
        }
    }
    state.backup.mark_dirty();
    Ok(out)
}

/// Take back what a lookup wrote for one holding.
#[tauri::command]
pub fn market_clear(state: State<'_, AppState>, holding_id: i64) -> AppResult<usize> {
    let n = state.db()?.with(|c| {
        let tx = c.unchecked_transaction()?;
        let n = tx.execute(
            "DELETE FROM holding_price WHERE holding_id = ?1 AND source = 'market'",
            params![holding_id],
        )?;
        resync_all(&tx)?;
        tx.commit()?;
        Ok(n)
    })?;
    state.backup.mark_dirty();
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::Series;

    /// Needs the internet: the real source, through the real client. It
    /// proves the request is accepted and the answer is still readable —
    /// the two things a recorded response cannot.
    #[tokio::test]
    #[ignore = "queries Yahoo Finance"]
    async fn the_real_source_answers_the_real_client() {
        let c = client().unwrap();
        let from = NaiveDate::from_ymd_opt(2024, 3, 1).unwrap();
        let to = NaiveDate::from_ymd_opt(2024, 3, 31).unwrap();
        let s = series_for(&c, "SAP.DE", from, to).await.unwrap();
        assert_eq!(s.currency, "EUR");
        assert!(s.closes.len() >= 15, "{} points", s.closes.len());
        println!("SAP.DE: {} closes, first {:?}", s.closes.len(), s.closes[0]);

        let body = get_json(&c, &market::search_url("SAP")).await.unwrap();
        let cands = market::parse_search(&body);
        assert!(cands.iter().any(|c| c.symbol == "SAP.DE"), "{cands:?}");

        let btc = series_for(&c, "BTC-EUR", from, to).await.unwrap();
        assert_eq!(btc.currency, "EUR");
        assert!(btc.closes.len() >= 25, "a coin trades every day");

        let nope = series_for(&c, "NOSUCHTICKERXYZ", from, to).await;
        assert!(matches!(nope, Err(Problem::NotFound)), "{nope:?}");
    }

    #[test]
    fn nothing_here_calls_a_source_in_a_unit_test() {
        // The network paths are exercised live (ignored) and through the
        // pure functions in `crate::market`; this only pins the constants.
        assert_eq!(SOURCE_NAME, "Yahoo Finance");
        let s = Series {
            symbol: "X".into(),
            currency: "EUR".into(),
            closes: vec![],
        };
        assert!(s.closes.is_empty());
    }
}
