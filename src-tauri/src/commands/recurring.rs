//! Payments that come round on a schedule: subscriptions, rent, insurance,
//! the gym. Found from the ledger rather than declared, because nobody
//! remembers all of them and the forgotten ones are the point.
//!
//! A payment counts as recurring when the same merchant is paid at a steady
//! interval — roughly weekly, monthly, quarterly, or yearly — and for about
//! the same amount. A supermarket visited every few days for different sums
//! is not one: the amounts are what tell a subscription from a habit.

use crate::error::AppResult;
use crate::import::merchant;
use crate::state::AppState;
use chrono::{Duration, NaiveDate};
use serde::Serialize;
use std::collections::HashMap;
use tauri::State;

/// How far back to look. Two years finds a yearly payment twice.
const LOOKBACK_DAYS: i64 = 2 * 366 + 31;

/// (label, typical days between payments, tolerance in days)
const PERIODS: &[(&str, i64, i64)] = &[
    ("weekly", 7, 2),
    ("fortnightly", 14, 3),
    ("monthly", 30, 4),
    ("quarterly", 91, 8),
    ("half-yearly", 182, 12),
    ("yearly", 365, 15),
];

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recurring {
    /// What identifies it across runs: the merchant and the rhythm. Used to
    /// remember that the person said "this is not a recurring payment".
    pub key: String,
    /// What it is called: the description the merchant is most often seen as.
    pub name: String,
    pub category_name: Option<String>,
    /// "weekly", "fortnightly", "monthly", "quarterly", "half-yearly", "yearly".
    pub every: String,
    /// The usual amount, as a positive magnitude.
    pub typical_minor: i64,
    /// The amounts wander (a utility bill) rather than repeat exactly.
    pub varies: bool,
    pub count: usize,
    pub first_on: String,
    pub last_on: String,
    pub next_on: String,
    /// What a year of it costs at this rate.
    pub yearly_minor: i64,
    /// Still going: the last payment is recent enough that the next is not
    /// yet overdue by much.
    pub active: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RecurringReport {
    pub items: Vec<Recurring>,
    /// What was found but the person has said is not a recurring payment.
    pub hidden: Vec<Recurring>,
    /// What the ones still going cost over a year, and a twelfth of that.
    pub yearly_minor: i64,
    pub monthly_minor: i64,
}

fn median(v: &mut [i64]) -> i64 {
    v.sort_unstable();
    v[v.len() / 2]
}

struct Row {
    on: NaiveDate,
    minor: i64,
    raw: String,
    category: Option<String>,
}

fn classify(intervals: &[i64]) -> Option<(&'static str, i64)> {
    let mut v = intervals.to_vec();
    let m = median(&mut v);
    let &(label, days, tol) = PERIODS.iter().find(|(_, d, t)| (m - d).abs() <= *t)?;
    // Most gaps have to be near the usual one; a missed month (a double gap)
    // or two is a payment that paused, not a different rhythm.
    let regular = intervals
        .iter()
        .filter(|g| (**g - days).abs() <= tol || (**g - 2 * days).abs() <= tol)
        .count();
    (regular * 10 >= intervals.len() * 7).then_some((label, days))
}

pub fn detect(c: &rusqlite::Connection, today: NaiveDate) -> AppResult<RecurringReport> {
    let since = (today - Duration::days(LOOKBACK_DAYS)).to_string();
    let mut stmt = c.prepare(
        "SELECT n.booked_on, n.net_minor, t.description_raw, t.description_norm, k.name
         FROM v_txn_net n
         JOIN txn t ON t.id = n.id
         LEFT JOIN pl_category k ON k.id = n.effective_category_id
         WHERE n.net_minor < 0 AND n.booked_on >= ?1 AND n.booked_on <= ?2
         ORDER BY n.booked_on",
    )?;
    let mut by_key: HashMap<String, Vec<Row>> = HashMap::new();
    let mut rows = stmt.query(rusqlite::params![since, today.to_string()])?;
    while let Some(r) = rows.next()? {
        let on: String = r.get(0)?;
        let Ok(on) = NaiveDate::parse_from_str(&on, "%Y-%m-%d") else {
            continue;
        };
        let norm: String = r.get(3)?;
        // A line that is nothing but boilerplate has no merchant to follow.
        let Some(key) = merchant::key(&norm) else {
            continue;
        };
        by_key.entry(key).or_default().push(Row {
            on,
            minor: -r.get::<_, i64>(1)?,
            raw: r.get(2)?,
            category: r.get(4)?,
        });
    }

    let mut items = Vec::new();
    for (merchant_key, mut rows) in by_key {
        rows.sort_by_key(|r| r.on);
        // The same merchant can be two different payments: a phone line and a
        // phone insurance, a rent and a parking space. Amounts within a factor
        // of two are one payment (a bill that wanders is still one bill); the
        // rhythm, not the amount, is what decides whether it recurs.
        let mut groups: Vec<Vec<Row>> = Vec::new();
        for r in rows {
            match groups.iter_mut().find(|g| {
                let mut a: Vec<i64> = g.iter().map(|x| x.minor).collect();
                let m = median(&mut a);
                r.minor * 2 >= m && r.minor <= m * 2
            }) {
                Some(g) => g.push(r),
                None => groups.push(vec![r]),
            }
        }
        for g in groups {
            // Two on one day are one payment's two halves, or a duplicate.
            let mut dates: Vec<NaiveDate> = g.iter().map(|r| r.on).collect();
            dates.dedup();
            let span = (dates.last().copied().unwrap_or(today) - dates[0]).num_days();
            let needed = if span >= 330 { 2 } else { 3 };
            if dates.len() < needed {
                continue;
            }
            let gaps: Vec<i64> = dates.windows(2).map(|w| (w[1] - w[0]).num_days()).collect();
            let Some((label, days)) = classify(&gaps) else {
                continue;
            };
            // A short rhythm is easy to find by chance in a handful of
            // shopping trips, so it takes more evidence than a monthly one.
            let enough = match label {
                "weekly" => 6,
                "fortnightly" => 5,
                _ => needed,
            };
            if dates.len() < enough {
                continue;
            }
            let mut amounts: Vec<i64> = g.iter().map(|r| r.minor).collect();
            let typical = median(&mut amounts);
            let tight = g
                .iter()
                .filter(|r| (r.minor - typical).abs() * 100 <= typical * 8)
                .count();
            let varies = tight * 10 < g.len() * 7;
            // A weekly payment of a different amount each time is a habit —
            // the shop, the café — not something that is charged to you.
            if varies && matches!(label, "weekly" | "fortnightly") {
                continue;
            }

            let last = *dates.last().unwrap();
            let active = (today - last).num_days() <= days + days / 2 + 5;
            let mut names: HashMap<&str, usize> = HashMap::new();
            for r in &g {
                *names.entry(r.raw.as_str()).or_default() += 1;
            }
            let name = names
                .into_iter()
                .max_by_key(|(n, k)| (*k, std::cmp::Reverse(n.len())))
                .map(|(n, _)| n.to_string())
                .unwrap_or_default();
            items.push(Recurring {
                key: format!("{merchant_key}|{label}"),
                name,
                category_name: g.iter().find_map(|r| r.category.clone()),
                every: label.to_string(),
                typical_minor: typical,
                varies,
                count: dates.len(),
                first_on: dates[0].to_string(),
                last_on: last.to_string(),
                next_on: (last + Duration::days(days)).to_string(),
                yearly_minor: typical * 365 / days,
                active,
            });
        }
    }

    // What the person has said is not recurring is set apart, not forgotten:
    // it can be put back, and it is never counted in the totals.
    let dismissed = dismissed_keys(c);
    let (hidden, mut items): (Vec<_>, Vec<_>) =
        items.into_iter().partition(|i| dismissed.contains(&i.key));
    let mut hidden = hidden;
    hidden.sort_by(|a, b| a.name.cmp(&b.name));

    items.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.yearly_minor.cmp(&a.yearly_minor))
            .then(a.name.cmp(&b.name))
    });
    let yearly: i64 = items
        .iter()
        .filter(|i| i.active)
        .map(|i| i.yearly_minor)
        .sum();
    Ok(RecurringReport {
        items,
        hidden,
        yearly_minor: yearly,
        monthly_minor: yearly / 12,
    })
}

const DISMISSED: &str = "recurring.dismissed";

fn dismissed_keys(c: &rusqlite::Connection) -> Vec<String> {
    c.query_row(
        "SELECT value_json FROM app_setting WHERE key = ?1",
        [DISMISSED],
        |r| r.get::<_, String>(0),
    )
    .ok()
    .and_then(|j| serde_json::from_str(&j).ok())
    .unwrap_or_default()
}

/// Say whether a detected payment is one: `false` removes it from the list,
/// `true` puts it back. Nothing is deleted from the ledger either way.
pub fn set_recurring(c: &rusqlite::Connection, key: &str, is_recurring: bool) -> AppResult<()> {
    let mut keys = dismissed_keys(c);
    keys.retain(|k| k != key);
    if !is_recurring {
        keys.push(key.to_string());
    }
    let json = serde_json::to_string(&keys)
        .map_err(|e| crate::error::AppError::Other(format!("cannot save the list: {e}")))?;
    c.execute(
        "INSERT INTO app_setting (key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json,
             updated_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
        rusqlite::params![DISMISSED, json],
    )?;
    Ok(())
}

#[tauri::command]
pub fn recurring_set(state: State<'_, AppState>, key: String, is_recurring: bool) -> AppResult<()> {
    state.db()?.with(|c| set_recurring(c, &key, is_recurring))?;
    state.backup.mark_dirty();
    Ok(())
}

#[tauri::command]
pub fn recurring_list(state: State<'_, AppState>) -> AppResult<RecurringReport> {
    let today = chrono::Local::now().date_naive();
    state.db()?.with(|c| detect(c, today))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rhythms_are_recognised_with_a_missed_payment_tolerated() {
        assert_eq!(classify(&[30, 31, 29, 30]).map(|c| c.0), Some("monthly"));
        assert_eq!(classify(&[7, 7, 8, 6]).map(|c| c.0), Some("weekly"));
        assert_eq!(classify(&[365]).map(|c| c.0), Some("yearly"));
        // One skipped month (a 60-day gap) does not break a monthly payment.
        assert_eq!(
            classify(&[30, 61, 30, 31, 30]).map(|c| c.0),
            Some("monthly")
        );
        // Irregular gaps are not a rhythm.
        assert_eq!(classify(&[3, 40, 12, 90, 5]), None);
        assert_eq!(classify(&[20, 45, 10]), None);
    }
}
