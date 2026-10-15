//! Deterministic categorisation rules.
//!
//! These run on every import, before anything else is asked. On a typical
//! statement a dozen rules cover most rows — the same shops recur every month —
//! which is the whole point: the assistant should only ever see what rules
//! could not classify, and a user with no AI configured should still get a
//! categorised ledger.
//!
//! Matching is against `description_norm` (casefolded, digits and punctuation
//! stripped), so "MERCADONA VALENCIA 4471" and "Mercadona Valencia 9982" are
//! the same shop to a rule written once.

use crate::money::Money;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: Option<i64>,
    pub name: String,
    pub priority: i64,
    /// contains | starts_with | exact | regex
    pub match_kind: String,
    pub pattern: String,
    pub account_id: Option<i64>,
    pub min_amount_minor: Option<i64>,
    pub max_amount_minor: Option<i64>,
    pub set_category_id: Option<i64>,
    pub set_project_id: Option<i64>,
    pub set_neutral_transfer: bool,
    pub is_enabled: bool,
}

/// The fields of a transaction a rule can look at.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: i64,
    pub account_id: i64,
    pub description_norm: String,
    pub amount_minor: Money,
}

/// Does this rule apply?
///
/// An amount range is inclusive and compares the **signed** amount, so
/// "anything over 500 out" is written as a max of -50000 rather than being
/// silently reinterpreted.
pub fn matches(rule: &Rule, c: &Candidate) -> bool {
    if !rule.is_enabled {
        return false;
    }
    if let Some(a) = rule.account_id {
        if a != c.account_id {
            return false;
        }
    }
    if let Some(min) = rule.min_amount_minor {
        if c.amount_minor.minor() < min {
            return false;
        }
    }
    if let Some(max) = rule.max_amount_minor {
        if c.amount_minor.minor() > max {
            return false;
        }
    }

    let pattern = crate::security::pii::normalise_description(&rule.pattern);
    let hay = &c.description_norm;
    match rule.match_kind.as_str() {
        "contains" => !pattern.is_empty() && hay.contains(&pattern),
        "starts_with" => !pattern.is_empty() && hay.starts_with(&pattern),
        "exact" => !pattern.is_empty() && hay == &pattern,
        // A bad pattern matches nothing rather than taking the import down.
        "regex" => regex::Regex::new(&rule.pattern)
            .map(|re| re.is_match(hay))
            .unwrap_or(false),
        _ => false,
    }
}

/// The first rule that matches, in priority order. Ties break on id so the
/// outcome does not depend on how SQLite happened to return the rows.
pub fn first_match<'a>(rules: &'a [Rule], c: &Candidate) -> Option<&'a Rule> {
    let mut hits: Vec<&Rule> = rules.iter().filter(|r| matches(r, c)).collect();
    hits.sort_by_key(|r| (r.priority, r.id.unwrap_or(i64::MAX)));
    hits.first().copied()
}

#[cfg(test)]
mod tests;
