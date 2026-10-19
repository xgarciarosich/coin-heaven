//! Pre-assigning categories from the description.
//!
//! Three things can categorise a transaction, in descending order of how much
//! they can be trusted:
//!
//!   1. a rule the user wrote, which is explicit and deterministic;
//!   2. what the user did last time with the same shop, which is a fact about
//!      their own ledger rather than a guess;
//!   3. nothing, in which case the row is left alone.
//!
//! The assistant is deliberately not in that list. It runs separately, behind
//! its own consent gate, on whatever is left — this pass is local, free and
//! instant, and on a normal statement it clears most of the work.
//!
//! Nothing here writes until the user has seen the grouping and said yes.

use crate::error::{AppError, AppResult};
use crate::import::{
    merchant,
    rules::{self, Candidate, Rule},
};
use crate::money::Money;
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleTxn {
    pub id: i64,
    pub booked_on: String,
    pub description: String,
    pub amount_minor: Money,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedGroup {
    /// The word these transactions were grouped by, and the pattern a rule
    /// would be written with.
    pub key: String,
    pub txn_ids: Vec<i64>,
    pub txn_count: i64,
    pub total_minor: Money,
    pub first_on: String,
    pub last_on: String,
    /// Up to three real rows, so the grouping can be checked rather than
    /// trusted.
    pub samples: Vec<SampleTxn>,
    pub suggested_category_id: Option<i64>,
    pub suggested_category_name: Option<String>,
    /// "rule" — a rule the user wrote matches every row here.
    /// "history" — this is what they filed the same shop under before.
    /// "none" — nothing to go on; the group is shown so it can be done by hand.
    pub source: String,
    pub rule_name: Option<String>,
    /// For "history": the share of past decisions that went this way, in basis
    /// points, and how many there were.
    pub confidence_bps: i64,
    pub evidence_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestions {
    pub groups: Vec<SuggestedGroup>,
    pub uncategorised_total: i64,
    /// How many of those a group could suggest something for.
    pub covered: i64,
}

struct Row {
    id: i64,
    account_id: i64,
    booked_on: String,
    description: String,
    norm: String,
    amount: Money,
    category_id: Option<i64>,
}

fn load_rows(c: &rusqlite::Connection) -> rusqlite::Result<Vec<Row>> {
    let mut stmt = c.prepare(
        "SELECT t.id, t.account_id, t.booked_on, t.description_raw, t.description_norm,
                t.amount_minor, e.effective_category_id
         FROM txn t LEFT JOIN v_txn_effective e ON e.id = t.id
         WHERE t.is_neutral_transfer = 0
         ORDER BY t.booked_on",
    )?;
    stmt.query_map([], |r| {
        Ok(Row {
            id: r.get(0)?,
            account_id: r.get(1)?,
            booked_on: r.get(2)?,
            description: r.get(3)?,
            norm: r.get(4)?,
            amount: Money::from_minor(r.get(5)?),
            category_id: r.get(6)?,
        })
    })?
    .collect()
}

/// Group everything uncategorised by merchant and propose a category for each
/// group. Writes nothing.
#[tauri::command]
pub fn autocat_suggest(state: State<'_, AppState>) -> AppResult<Suggestions> {
    state.db()?.with(suggest)
}

/// The body of `autocat_suggest`, against a connection, so the grouping can be
/// tested against a real ledger rather than only through the IPC layer.
pub fn suggest(c: &rusqlite::Connection) -> AppResult<Suggestions> {
    {
        let rows = load_rows(c)?;
        let rules = crate::commands::import::load_rules_conn(c)?;
        let names: HashMap<i64, String> = {
            let mut stmt = c.prepare("SELECT id, name FROM pl_category")?;
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<_, _>>()?
        };

        // What the user has already decided, keyed by merchant.
        let mut history: HashMap<String, HashMap<i64, i64>> = HashMap::new();
        for r in rows.iter().filter(|r| r.category_id.is_some()) {
            if let Some(k) = merchant::key(&r.norm) {
                *history
                    .entry(k)
                    .or_default()
                    .entry(r.category_id.unwrap())
                    .or_insert(0) += 1;
            }
        }

        let mut groups: HashMap<String, Vec<&Row>> = HashMap::new();
        let mut uncategorised_total = 0;
        for r in rows.iter().filter(|r| r.category_id.is_none()) {
            uncategorised_total += 1;
            if let Some(k) = merchant::key(&r.norm) {
                groups.entry(k).or_default().push(r);
            }
        }

        let mut out: Vec<SuggestedGroup> = groups
            .into_iter()
            .map(|(key, members)| {
                let rule_hit = first_rule_for_all(&rules, &members);
                let (category_id, source, rule_name, confidence_bps, evidence) = match rule_hit {
                    Some(rule) => (
                        rule.set_category_id,
                        "rule",
                        Some(rule.name.clone()),
                        10_000,
                        members.len() as i64,
                    ),
                    None => match history.get(&key).and_then(majority) {
                        Some((cat, hits, total)) => (
                            Some(cat),
                            "history",
                            None,
                            (hits * 10_000) / total.max(1),
                            total,
                        ),
                        None => (None, "none", None, 0, 0),
                    },
                };

                SuggestedGroup {
                    txn_ids: members.iter().map(|m| m.id).collect(),
                    txn_count: members.len() as i64,
                    total_minor: members.iter().map(|m| m.amount).sum(),
                    first_on: members
                        .first()
                        .map(|m| m.booked_on.clone())
                        .unwrap_or_default(),
                    last_on: members
                        .last()
                        .map(|m| m.booked_on.clone())
                        .unwrap_or_default(),
                    samples: members
                        .iter()
                        .take(3)
                        .map(|m| SampleTxn {
                            id: m.id,
                            booked_on: m.booked_on.clone(),
                            description: m.description.clone(),
                            amount_minor: m.amount,
                        })
                        .collect(),
                    suggested_category_name: category_id.and_then(|id| names.get(&id).cloned()),
                    suggested_category_id: category_id,
                    source: source.to_string(),
                    rule_name,
                    confidence_bps,
                    evidence_count: evidence,
                    key,
                }
            })
            .collect();

        // Biggest wins first: clearing a group of forty is worth more of the
        // user's attention than one of one.
        out.sort_by(|a, b| {
            let rank = |g: &SuggestedGroup| match g.source.as_str() {
                "rule" => 0,
                "history" => 1,
                _ => 2,
            };
            rank(a)
                .cmp(&rank(b))
                .then_with(|| b.txn_count.cmp(&a.txn_count))
                .then_with(|| a.key.cmp(&b.key))
        });

        let covered = out
            .iter()
            .filter(|g| g.suggested_category_id.is_some())
            .map(|g| g.txn_count)
            .sum();

        Ok(Suggestions {
            groups: out,
            uncategorised_total,
            covered,
        })
    }
}

/// The category the user picked most often for this merchant, with the count
/// and the total, or `None` when there is no history at all.
fn majority(tally: &HashMap<i64, i64>) -> Option<(i64, i64, i64)> {
    let total: i64 = tally.values().sum();
    tally
        .iter()
        // A tie must not depend on hash order.
        .max_by_key(|(id, n)| (**n, std::cmp::Reverse(**id)))
        .map(|(id, n)| (*id, *n, total))
}

/// A rule only speaks for a group if it applies to *every* row in it.
///
/// A rule that catches half the group would categorise the other half by
/// accident when the group is accepted as a whole, which is exactly the kind
/// of silent mistake a preview is supposed to prevent.
fn first_rule_for_all<'a>(rules: &'a [Rule], members: &[&Row]) -> Option<&'a Rule> {
    let candidate = |r: &Row| Candidate {
        id: r.id,
        account_id: r.account_id,
        description_norm: r.norm.clone(),
        amount_minor: r.amount,
    };
    let first = rules::first_match(rules, &candidate(members.first()?))?;
    first.set_category_id?;
    members
        .iter()
        .all(|m| rules::first_match(rules, &candidate(m)).map(|r| r.id) == Some(first.id))
        .then_some(first)
}

/* --------------------------------------------------------------- learn -- */

/// How sure the ledger has to be before it files something on its own: the
/// share of past decisions for this merchant that went the same way.
const LEARN_CONFIDENCE_BPS: i64 = 8_000;

/// File what the person has already told us how to file.
///
/// "I put Mercadona under Food" is a decision about Mercadona, not about one
/// receipt. So every uncategorised transaction from a merchant the person has
/// categorised before takes the category they chose — provided their past
/// choices agree with each other. A merchant filed two ways (a supermarket
/// that is sometimes groceries and sometimes a gift) is left for a person.
///
/// Runs after every import and after every manual categorisation. It only
/// ever fills an empty category; it never changes one that is set.
pub fn learn(c: &rusqlite::Connection) -> AppResult<usize> {
    let rows = load_rows(c)?;
    let mut history: HashMap<String, HashMap<i64, i64>> = HashMap::new();
    for r in rows.iter().filter(|r| r.category_id.is_some()) {
        if let (Some(k), Some(cat)) = (merchant::key(&r.norm), r.category_id) {
            *history.entry(k).or_default().entry(cat).or_insert(0) += 1;
        }
    }
    let mut filed = 0;
    for r in rows.iter().filter(|r| r.category_id.is_none()) {
        let Some((cat, hits, total)) = merchant::key(&r.norm)
            .and_then(|k| history.get(&k))
            .and_then(majority)
        else {
            continue;
        };
        if hits * 10_000 < total * LEARN_CONFIDENCE_BPS {
            continue;
        }
        filed += c.execute(
            "UPDATE txn SET category_override_id = ?1, categorized_by = 'rule', needs_review = 0
             WHERE id = ?2 AND category_override_id IS NULL",
            rusqlite::params![cat, r.id],
        )?;
    }
    Ok(filed)
}

/* --------------------------------------------------------------- apply -- */

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignment {
    /// The exact rows the user saw. Sending ids rather than the merchant key
    /// means the set cannot drift between the preview and the decision.
    pub txn_ids: Vec<i64>,
    /// The category to file them under; `None` leaves the category to the
    /// event's default, when an event is given.
    pub category_id: Option<i64>,
    /// An event (a trip, a renovation) to put the whole bundle in.
    #[serde(default)]
    pub event_id: Option<i64>,
    /// Also write a rule, so next month's import needs no decision at all.
    pub create_rule: bool,
    pub key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyOutcome {
    pub categorised: usize,
    pub rules_created: usize,
}

#[tauri::command]
pub fn autocat_apply(
    state: State<'_, AppState>,
    assignments: Vec<Assignment>,
) -> AppResult<ApplyOutcome> {
    if assignments.is_empty() {
        return Ok(ApplyOutcome {
            categorised: 0,
            rules_created: 0,
        });
    }
    let outcome = state.db()?.with(|c| {
        let tx = c.unchecked_transaction()?;
        let mut categorised = 0;
        let mut rules_created = 0;

        for a in &assignments {
            if a.category_id.is_none() && a.event_id.is_none() {
                continue;
            }
            if let Some(cat) = a.category_id {
                let exists: i64 = tx.query_row(
                    "SELECT count(*) FROM pl_category WHERE id = ?1",
                    rusqlite::params![cat],
                    |r| r.get(0),
                )?;
                if exists == 0 {
                    return Err(AppError::Invalid(
                        "that category no longer exists — reload and try again".into(),
                    ));
                }
            }
            if let Some(ev) = a.event_id {
                let exists: i64 = tx.query_row(
                    "SELECT count(*) FROM project_event WHERE id = ?1",
                    rusqlite::params![ev],
                    |r| r.get(0),
                )?;
                if exists == 0 {
                    return Err(AppError::Invalid(
                        "that event no longer exists — reload and try again".into(),
                    ));
                }
            }
            for id in &a.txn_ids {
                // Each part is set only when it was chosen, so an event alone
                // leaves the category to the event's default, and a category
                // alone leaves any event the row already has.
                categorised += tx.execute(
                    "UPDATE txn SET
                        category_override_id = COALESCE(?1, category_override_id),
                        categorized_by = CASE WHEN ?1 IS NULL THEN categorized_by ELSE 'rule' END,
                        project_id = COALESCE(?2, project_id),
                        needs_review = 0
                     WHERE id = ?3",
                    rusqlite::params![a.category_id, a.event_id, id],
                )?;
            }
            let Some(category_id) = a.category_id else {
                continue;
            };
            if a.create_rule && !a.key.trim().is_empty() {
                // A rule the user already has covers this; a second one would
                // only be another line to read.
                let dup: i64 = tx.query_row(
                    "SELECT count(*) FROM categorization_rule
                     WHERE match_kind = 'contains' AND pattern = ?1 AND set_category_id = ?2",
                    rusqlite::params![a.key.trim(), category_id],
                    |r| r.get(0),
                )?;
                if dup == 0 {
                    tx.execute(
                        "INSERT INTO categorization_rule
                           (name, priority, match_kind, pattern, set_category_id, is_enabled)
                         VALUES (?1, 200, 'contains', ?2, ?3, 1)",
                        rusqlite::params![
                            crate::i18n::pick(
                                format!("Anything mentioning \"{}\"", a.key.trim()),
                                format!("Todo lo que mencione «{}»", a.key.trim()),
                            ),
                            a.key.trim(),
                            category_id
                        ],
                    )?;
                    rules_created += 1;
                }
            }
        }
        tx.commit()?;
        Ok(ApplyOutcome {
            categorised,
            rules_created,
        })
    })?;
    state.backup.mark_dirty();
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: i64, pattern: &str, cat: i64) -> Rule {
        Rule {
            id: Some(id),
            name: format!("r{id}"),
            priority: 100,
            match_kind: "contains".into(),
            pattern: pattern.into(),
            account_id: None,
            min_amount_minor: None,
            max_amount_minor: None,
            set_category_id: Some(cat),
            set_project_id: None,
            set_neutral_transfer: false,
            is_enabled: true,
        }
    }

    fn row(id: i64, desc: &str) -> Row {
        Row {
            id,
            account_id: 1,
            booked_on: "2026-01-01".into(),
            description: desc.into(),
            norm: crate::security::pii::normalise_description(desc),
            amount: Money::from_minor(-1000),
            category_id: None,
        }
    }

    #[test]
    fn a_rule_covering_only_part_of_a_group_does_not_speak_for_it() {
        let rules = vec![rule(1, "mercadona", 7)];
        let a = row(1, "COMPRA MERCADONA VALENCIA");
        let b = row(2, "COMPRA CARREFOUR VALENCIA");
        assert!(first_rule_for_all(&rules, &[&a]).is_some());
        assert!(first_rule_for_all(&rules, &[&a, &b]).is_none());
    }

    #[test]
    fn a_tie_in_history_is_decided_the_same_way_every_time() {
        let tally: HashMap<i64, i64> = [(3, 2), (9, 2)].into_iter().collect();
        for _ in 0..50 {
            assert_eq!(majority(&tally).unwrap().0, 3);
        }
    }

    #[test]
    fn majority_reports_the_share_it_is_based_on() {
        let tally: HashMap<i64, i64> = [(3, 7), (9, 3)].into_iter().collect();
        let (cat, hits, total) = majority(&tally).unwrap();
        assert_eq!((cat, hits, total), (3, 7, 10));
    }
}
