//! Turning parsed rows into transactions.
//!
//! The whole import is one SQL transaction: either every row lands or none
//! does, so a failure halfway through cannot leave a half-imported statement
//! that the user has no way to identify.
//!
//! Duplicate detection is delegated to the database. Each row carries a
//! `dedupe_hash` and the unique index refuses a repeat, so re-importing an
//! overlapping statement inserts only what is genuinely new — enforced by the
//! schema rather than by remembering to check.

use crate::db::Db;
use crate::error::AppResult;
use crate::import::dedupe::{self, RawRow};
use crate::import::parser::ParseResult;
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub batch_id: i64,
    pub row_count: usize,
    pub inserted: usize,
    pub duplicates: usize,
    pub rejected: usize,
}

pub fn file_sha256(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn run(
    db: &Db,
    account_id: i64,
    mapping_id: Option<i64>,
    filename: &str,
    source_sha256: &str,
    parsed: &ParseResult,
    currency: &str,
) -> AppResult<ImportSummary> {
    let raw: Vec<RawRow> = parsed
        .rows
        .iter()
        .map(|r| RawRow {
            booked_on: r.booked_on.clone(),
            amount_minor: r.amount_minor,
            description: r.description.clone(),
        })
        .collect();
    let prepared = dedupe::prepare(account_id, &raw);

    db.with(|c| {
        let tx = c.unchecked_transaction()?;

        tx.execute(
            "INSERT INTO import_batch
               (account_id, mapping_id, source_filename, source_sha256,
                row_count, inserted_count, duplicate_count, rejected_count)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 0, ?6)",
            rusqlite::params![
                account_id,
                mapping_id,
                filename,
                source_sha256,
                parsed.rows.len() as i64,
                parsed.rejected.len() as i64,
            ],
        )?;
        let batch_id = tx.last_insert_rowid();

        let mut inserted = 0usize;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO txn
                   (account_id, booked_on, amount_minor, currency,
                    description_raw, description_norm, counterparty,
                    dedupe_hash, import_batch_id, needs_review)
                 SELECT ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 1
                 -- A row someone deleted stays deleted when the statement it
                 -- came from is loaded again.
                 WHERE NOT EXISTS (SELECT 1 FROM txn_removed r
                                    WHERE r.account_id = ?1 AND r.dedupe_hash = ?8)
                 ON CONFLICT(account_id, dedupe_hash) DO NOTHING",
            )?;
            for (row, src) in prepared.iter().zip(parsed.rows.iter()) {
                let n = stmt.execute(rusqlite::params![
                    account_id,
                    row.booked_on,
                    row.amount_minor.minor(),
                    currency,
                    row.description_raw,
                    row.description_norm,
                    src.counterparty,
                    row.dedupe_hash,
                    batch_id,
                ])?;
                inserted += n;
            }
        }

        let duplicates = prepared.len() - inserted;
        tx.execute(
            "UPDATE import_batch SET inserted_count = ?1, duplicate_count = ?2 WHERE id = ?3",
            rusqlite::params![inserted as i64, duplicates as i64, batch_id],
        )?;
        tx.commit()?;

        Ok(ImportSummary {
            batch_id,
            row_count: parsed.rows.len(),
            inserted,
            duplicates,
            rejected: parsed.rejected.len(),
        })
    })
}

/// The SQL condition for "the person has done something to this row": filed
/// it, put it in an event, linked it to another, or marked it a transfer.
const TOUCHED: &str = "(categorized_by <> 'none' OR project_id IS NOT NULL
                       OR category_override_id IS NOT NULL OR parent_txn_id IS NOT NULL
                       OR is_neutral_transfer <> 0)";

/// What removing an import would take with it, so the person is told before
/// it happens rather than after.
#[derive(Debug, serde::Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Impact {
    pub filename: Option<String>,
    /// Transactions this import still holds.
    pub total: i64,
    /// Of those, ones the person has since worked on.
    pub touched: i64,
    /// Rows other transactions are linked to (a refund booked against one).
    pub linked_from_elsewhere: i64,
}

pub fn impact(c: &rusqlite::Connection, batch_id: i64) -> AppResult<Impact> {
    let n = |sql: &str| -> AppResult<i64> {
        Ok(c.query_row(sql, rusqlite::params![batch_id], |r| r.get(0))?)
    };
    Ok(Impact {
        filename: c
            .query_row(
                "SELECT source_filename FROM import_batch WHERE id = ?1",
                rusqlite::params![batch_id],
                |r| r.get(0),
            )
            .unwrap_or(None),
        total: n("SELECT count(*) FROM txn WHERE import_batch_id = ?1")?,
        touched: n(&format!(
            "SELECT count(*) FROM txn WHERE import_batch_id = ?1 AND {TOUCHED}"
        ))?,
        linked_from_elsewhere: n("SELECT count(*) FROM txn k WHERE k.parent_txn_id IN
               (SELECT id FROM txn WHERE import_batch_id = ?1)
               AND (k.import_batch_id IS NULL OR k.import_batch_id <> ?1)")?,
    })
}

/// Remove an import.
///
/// With `everything` false this removes only the rows nobody has touched and
/// keeps the ones the person has worked on — the safe reading of "undo".
/// With it true, every transaction the import brought is deleted, including
/// those, together with whatever was done to them: their category, event and
/// notes are gone with them. Anything that was linked to a removed row (a
/// refund booked against a bill) is unlinked and stays as its own entry.
///
/// Neither remembers the rows' fingerprints: importing the same file again
/// brings them back, which is the point of removing an import.
pub fn undo(db: &Db, batch_id: i64, everything: bool) -> AppResult<usize> {
    db.with(|c| {
        let tx = c.unchecked_transaction()?;
        let scope = if everything {
            String::new()
        } else {
            format!(" AND NOT {TOUCHED}")
        };
        if everything {
            tx.execute(
                "UPDATE txn SET parent_txn_id = NULL, link_kind = NULL
                 WHERE parent_txn_id IN (SELECT id FROM txn WHERE import_batch_id = ?1)",
                rusqlite::params![batch_id],
            )?;
        }
        let removed = tx.execute(
            &format!("DELETE FROM txn WHERE import_batch_id = ?1{scope}"),
            rusqlite::params![batch_id],
        )?;
        tx.execute(
            "DELETE FROM import_batch WHERE id = ?1",
            rusqlite::params![batch_id],
        )?;
        tx.commit()?;
        Ok(removed)
    })
}

#[cfg(test)]
mod tests;
