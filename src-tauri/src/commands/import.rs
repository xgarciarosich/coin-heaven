//! Getting bank data in: accounts, per-bank mappings, preview, import, undo.
//!
//! File reading happens here rather than in the webview, which has no
//! filesystem permission at all. The UI passes a path chosen through the
//! native file dialog and never sees the bytes.

use crate::error::{AppError, AppResult};
use crate::import::dates::{self, DateFormat};
use crate::import::matching::{self, SplitParams, TransferParams, TxnRef};
use crate::import::parser::{self, BankMapping, ParseResult};
use crate::import::runner::{self, ImportSummary};
use crate::import::sniff::{self, Detection};
use crate::import::table::{self, Source};
use crate::money::Money;
use crate::state::AppState;
use serde::Serialize;
use std::collections::HashSet;
use tauri::State;

/// Bank exports are small; this ceiling exists so a mistyped path cannot make
/// the app read a multi-gigabyte file into memory.
const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;

/* ------------------------------------------------------------ accounts -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub institution: Option<String>,
    pub kind: String,
    pub currency: String,
    pub txn_count: i64,
}

#[tauri::command]
pub fn account_list(state: State<'_, AppState>) -> AppResult<Vec<Account>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT a.id, a.name, a.institution, a.kind, a.currency,
                    (SELECT count(*) FROM txn t WHERE t.account_id = a.id)
             FROM account a WHERE a.is_archived = 0 ORDER BY a.name",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Account {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    institution: r.get(2)?,
                    kind: r.get(3)?,
                    currency: r.get(4)?,
                    txn_count: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[tauri::command]
pub fn account_create(
    state: State<'_, AppState>,
    name: String,
    kind: String,
    currency: String,
    institution: Option<String>,
) -> AppResult<i64> {
    if name.trim().is_empty() {
        return Err(AppError::Invalid("an account needs a name".into()));
    }
    if !crate::currency::is_supported(&currency) {
        return Err(AppError::Invalid(format!(
            "unsupported currency: {currency}"
        )));
    }
    let id = state.db()?.with(|c| {
        c.execute(
            "INSERT INTO account (name, kind, currency, institution) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![name.trim(), kind, currency.to_uppercase(), institution],
        )?;
        Ok(c.last_insert_rowid())
    })?;
    state.backup.mark_dirty();
    Ok(id)
}

/// Every date format the importer understands, for the picker.
///
/// Served from the same catalogue detection and parsing use, so the dropdown
/// cannot drift out of step with what actually works.
#[tauri::command]
pub fn date_formats() -> Vec<DateFormat> {
    dates::catalogue()
}

/* ------------------------------------------------------------ mappings -- */

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredMapping {
    pub id: i64,
    #[serde(flatten)]
    pub mapping: BankMapping,
}

fn row_to_mapping(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredMapping> {
    let delim: String = r.get(2)?;
    let dec: String = r.get(10)?;
    Ok(StoredMapping {
        id: r.get(0)?,
        mapping: BankMapping {
            name: r.get(1)?,
            delimiter: delim.chars().next().unwrap_or(';'),
            encoding: r.get(3)?,
            has_header: r.get::<_, i64>(4)? != 0,
            skip_rows: r.get::<_, i64>(5)? as usize,
            date_col: r.get::<_, i64>(6)? as usize,
            date_format: r.get(7)?,
            description_col: r.get::<_, i64>(8)? as usize,
            amount_col: r.get::<_, Option<i64>>(9)?.map(|v| v as usize),
            decimal_separator: dec.chars().next().unwrap_or(','),
            thousands_separator: r.get(11)?,
            invert_sign: r.get::<_, i64>(12)? != 0,
            currency: r.get(13)?,
            counterparty_col: r.get::<_, Option<i64>>(14)?.map(|v| v as usize),
        },
    })
}

const MAPPING_COLS: &str = "id, name, delimiter, encoding, has_header, skip_rows, date_col,
     date_format, description_col, amount_col,
     decimal_separator, thousands_separator, invert_sign, currency, counterparty_col";

#[tauri::command]
pub fn mapping_list(state: State<'_, AppState>) -> AppResult<Vec<StoredMapping>> {
    state.db()?.with(|c| {
        let sql = format!("SELECT {MAPPING_COLS} FROM bank_mapping ORDER BY name");
        let mut stmt = c.prepare(&sql)?;
        Ok(stmt
            .query_map([], row_to_mapping)?
            .collect::<Result<Vec<_>, _>>()?)
    })
}

#[tauri::command]
pub fn mapping_save(
    state: State<'_, AppState>,
    id: Option<i64>,
    mapping: BankMapping,
) -> AppResult<i64> {
    validate_mapping(&mapping)?;
    let id = state.db()?.with(|c| {
        let p = rusqlite::params![
            mapping.name.trim(),
            mapping.delimiter.to_string(),
            mapping.encoding,
            i64::from(mapping.has_header),
            mapping.skip_rows as i64,
            mapping.date_col as i64,
            mapping.date_format,
            mapping.description_col as i64,
            mapping.amount_col.map(|v| v as i64),
            mapping.decimal_separator.to_string(),
            mapping.thousands_separator,
            i64::from(mapping.invert_sign),
            mapping.currency.to_uppercase(),
            mapping.counterparty_col.map(|v| v as i64),
        ];
        match id {
            Some(existing) => {
                c.execute(
                    "UPDATE bank_mapping SET name=?1, delimiter=?2, encoding=?3, has_header=?4,
                       skip_rows=?5, date_col=?6, date_format=?7, description_col=?8,
                       amount_col=?9, decimal_separator=?10, thousands_separator=?11,
                       invert_sign=?12, currency=?13, counterparty_col=?14
                     WHERE id=?15",
                    rusqlite::params_from_iter(
                        p.iter()
                            .copied()
                            .chain(std::iter::once(&existing as &dyn rusqlite::ToSql)),
                    ),
                )?;
                Ok(existing)
            }
            None => {
                c.execute(
                    "INSERT INTO bank_mapping
                       (name, delimiter, encoding, has_header, skip_rows, date_col, date_format,
                        description_col, amount_col, decimal_separator,
                        thousands_separator, invert_sign, currency, counterparty_col)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
                    p,
                )?;
                Ok(c.last_insert_rowid())
            }
        }
    })?;
    state.backup.mark_dirty();
    Ok(id)
}

#[tauri::command]
pub fn mapping_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.db()?.with(|c| {
        c.execute(
            "DELETE FROM bank_mapping WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

fn validate_mapping(m: &BankMapping) -> AppResult<()> {
    if m.name.trim().is_empty() {
        return Err(AppError::Invalid("a mapping needs a name".into()));
    }
    if m.amount_col.is_none() {
        return Err(AppError::Invalid(
            "choose the column that holds the signed amount".into(),
        ));
    }
    if m.date_format.trim().is_empty() {
        return Err(AppError::Invalid(
            "a date format is required, e.g. %d/%m/%Y".into(),
        ));
    }
    if !crate::currency::is_supported(&m.currency) {
        return Err(AppError::Invalid(format!(
            "unsupported currency: {}",
            m.currency
        )));
    }
    Ok(())
}

/* --------------------------------------------------------------- files -- */

fn read_file(path: &str) -> AppResult<Vec<u8>> {
    let meta = std::fs::metadata(path)
        .map_err(|e| AppError::Invalid(format!("cannot open {path}: {e}")))?;
    if meta.len() > MAX_IMPORT_BYTES {
        return Err(AppError::Invalid(format!(
            "{path} is {} MB; the limit is {} MB",
            meta.len() / 1_048_576,
            MAX_IMPORT_BYTES / 1_048_576
        )));
    }
    std::fs::read(path).map_err(|e| AppError::Invalid(format!("cannot read {path}: {e}")))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    #[serde(flatten)]
    pub parsed: ParseResult,
    /// First few raw lines, so the user can see what they are mapping against.
    pub sample_lines: Vec<String>,
    pub total_rows: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub detection: Detection,
    #[serde(flatten)]
    pub preview: Preview,
    pub source: Source,
    /// True when detection is shaky enough that the user should look, or ask
    /// the assistant.
    pub needs_confirmation: bool,
}

/// Open a file and work out its format, with no configuration from the user.
///
/// This is the whole point of the import screen: pick a file and it is
/// understood. Detection is local and deterministic — nothing is sent anywhere
/// — and the returned mapping is the same shape the user can then edit by hand.
#[tauri::command]
pub fn import_inspect(path: String) -> AppResult<Inspection> {
    let bytes = read_file(&path)?;
    let tbl = table::load(&path, &bytes, None, None)?;
    let detection = sniff::detect(&tbl);

    let mut preview = {
        let mut parsed = parser::parse(&tbl, &detection.mapping);
        let total_rows = parsed.rows.len();
        parsed.rows.truncate(50);
        parsed.rejected.truncate(50);
        Preview {
            parsed,
            sample_lines: tbl.rows.iter().take(6).map(|r| r.join(" | ")).collect(),
            total_rows,
        }
    };
    // A mapping that detects confidently but parses nothing is not confident.
    if preview.total_rows == 0 {
        preview.parsed.rejected.truncate(20);
    }
    let needs_confirmation =
        detection.confidence < 70 || preview.total_rows == 0 || !detection.warnings.is_empty();

    Ok(Inspection {
        source: tbl.source.clone(),
        detection,
        preview,
        needs_confirmation,
    })
}

/// Parse without writing anything. Lets the user adjust a mapping until the
/// preview looks right before committing to an import.
#[tauri::command]
pub fn import_preview(path: String, mapping: BankMapping) -> AppResult<Preview> {
    let bytes = read_file(&path)?;
    let tbl = table::load(
        &path,
        &bytes,
        Some(&mapping.encoding),
        Some(mapping.delimiter),
    )?;
    let mut parsed = parser::parse(&tbl, &mapping);
    let total_rows = parsed.rows.len();
    parsed.rows.truncate(50);
    parsed.rejected.truncate(50);
    Ok(Preview {
        parsed,
        sample_lines: tbl.rows.iter().take(6).map(|r| r.join(" | ")).collect(),
        total_rows,
    })
}

#[tauri::command]
pub fn import_run(
    state: State<'_, AppState>,
    account_id: i64,
    mapping_id: Option<i64>,
    path: String,
    mut mapping: BankMapping,
) -> AppResult<ImportSummary> {
    validate_mapping(&mapping)?;
    // The account decides the currency, not the file. A mapping carries
    // whatever the format sniffer assumed, and an amount is read with the
    // currency's decimal places: a yen statement read as euros is out by a
    // factor of a hundred, and every row would be stored as the wrong
    // currency.
    mapping.currency = state.db()?.with(|c| {
        c.query_row(
            "SELECT currency FROM account WHERE id = ?1",
            rusqlite::params![account_id],
            |r| r.get::<_, String>(0),
        )
        .map_err(|_| AppError::Invalid("that account no longer exists".into()))
    })?;
    let bytes = read_file(&path)?;
    let tbl = table::load(
        &path,
        &bytes,
        Some(&mapping.encoding),
        Some(mapping.delimiter),
    )?;
    let parsed = parser::parse(&tbl, &mapping);
    if parsed.rows.is_empty() {
        return Err(AppError::Invalid(
            "no rows could be read with this mapping — check the preview".into(),
        ));
    }
    let filename = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    let summary = runner::run(
        state.db()?,
        account_id,
        mapping_id,
        &filename,
        &runner::file_sha256(&bytes),
        &parsed,
        &mapping.currency,
    )?;

    // Deterministic rules run first, so most rows are already classified before
    // the review queue — or the assistant — ever sees them.
    let _ = apply_rules(state.db()?, Some(summary.batch_id));
    // Then what the person has already decided about these merchants.
    let _ = state.db()?.with(super::autocat::learn);

    // An import is one of the two moments where losing work would actually
    // hurt, so it forces a backup regardless of the interval.
    state.backup.mark_dirty();
    if let Ok(policy) = state.backup_policy() {
        let _ = state.backup.maybe_run(
            state.db()?,
            &policy,
            crate::backup::Trigger::Import,
            &state.data_dir,
        );
    }
    Ok(summary)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub id: i64,
    pub account_name: String,
    pub source_filename: Option<String>,
    pub imported_at: String,
    pub inserted_count: i64,
    pub duplicate_count: i64,
    pub rejected_count: i64,
}

#[tauri::command]
pub fn import_batches(state: State<'_, AppState>) -> AppResult<Vec<Batch>> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT b.id, a.name, b.source_filename, b.imported_at,
                    b.inserted_count, b.duplicate_count, b.rejected_count
             FROM import_batch b JOIN account a ON a.id = b.account_id
             ORDER BY b.id DESC LIMIT 50",
        )?;
        Ok(stmt
            .query_map([], |r| {
                Ok(Batch {
                    id: r.get(0)?,
                    account_name: r.get(1)?,
                    source_filename: r.get(2)?,
                    imported_at: r.get(3)?,
                    inserted_count: r.get(4)?,
                    duplicate_count: r.get(5)?,
                    rejected_count: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?)
    })
}

/// What removing this import would delete, before anything is.
#[tauri::command]
pub fn import_impact(state: State<'_, AppState>, batch_id: i64) -> AppResult<runner::Impact> {
    state.db()?.with(|c| runner::impact(c, batch_id))
}

/// `everything` deletes every transaction the import brought, including ones
/// the person has since filed or linked; without it only untouched ones go.
#[tauri::command]
pub fn import_undo(
    state: State<'_, AppState>,
    batch_id: i64,
    everything: Option<bool>,
) -> AppResult<usize> {
    let n = runner::undo(state.db()?, batch_id, everything.unwrap_or(false))?;
    state.backup.mark_dirty();
    Ok(n)
}

/* ------------------------------------------------------------ matching -- */

type MatcherInputs = (
    Vec<TxnRef>,
    HashSet<(i64, i64)>,
    std::collections::HashMap<i64, String>,
);

fn matcher_inputs(state: &AppState) -> AppResult<MatcherInputs> {
    state.db()?.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, account_id, booked_on, amount_minor, currency, description_raw,
                    is_neutral_transfer, (parent_txn_id IS NOT NULL), counterparty
             FROM txn ORDER BY booked_on, id",
        )?;
        let txns = stmt
            .query_map([], |r| {
                let date: String = r.get(2)?;
                Ok(TxnRef {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    booked_on: chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                        .unwrap_or_default(),
                    amount_minor: Money::from_minor(r.get(3)?),
                    currency: r.get(4)?,
                    description: r.get(5)?,
                    is_neutral_transfer: r.get::<_, i64>(6)? != 0,
                    has_parent: r.get::<_, i64>(7)? != 0,
                    counterparty: r.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let mut d = c.prepare("SELECT txn_lo_id, txn_hi_id FROM match_decision")?;
        let decided = d
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<Result<HashSet<_>, _>>()?;

        let mut a = c.prepare("SELECT id, name FROM account")?;
        let accounts = a
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<std::collections::HashMap<_, _>, _>>()?;
        Ok((txns, decided, accounts))
    })
}

/// One side of a proposed pair, with everything needed to recognise it.
///
/// A match is only reviewable if both halves are fully described: a transfer
/// between two accounts cannot be judged from one date and one amount, and the
/// account names are usually what settles it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchSide {
    pub id: i64,
    pub booked_on: String,
    pub description: String,
    pub counterparty: Option<String>,
    pub account_name: String,
    pub amount: Money,
    pub currency: String,
}

fn side(t: &TxnRef, accounts: &std::collections::HashMap<i64, String>) -> MatchSide {
    MatchSide {
        id: t.id,
        booked_on: t.booked_on.format("%Y-%m-%d").to_string(),
        description: t.description.clone(),
        counterparty: t.counterparty.clone(),
        account_name: accounts
            .get(&t.account_id)
            .cloned()
            .unwrap_or_else(|| format!("account {}", t.account_id)),
        amount: t.amount_minor,
        currency: t.currency.clone(),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingTransfer {
    pub out_id: i64,
    pub in_id: i64,
    pub out: MatchSide,
    pub incoming: MatchSide,
    /// `out + in`. Zero for a clean pair; anything else is the fee.
    pub residual_minor: Money,
    pub day_gap: i64,
    /// Why these two were put together, in words.
    pub reason: String,
    /// Same account name on both sides — worth a second look, since a real
    /// internal transfer moves between two different accounts.
    pub same_account_name: bool,
}

#[tauri::command]
pub fn matching_pending_transfers(state: State<'_, AppState>) -> AppResult<Vec<PendingTransfer>> {
    let (txns, decided, accounts) = matcher_inputs(&state)?;
    let by_id: std::collections::HashMap<i64, &TxnRef> = txns.iter().map(|t| (t.id, t)).collect();

    Ok(
        matching::find_transfers(&txns, TransferParams::default(), &decided)
            .into_iter()
            .filter_map(|c| {
                let o = by_id.get(&c.out_id)?;
                let i = by_id.get(&c.in_id)?;
                let out = side(o, &accounts);
                let incoming = side(i, &accounts);
                // Amounts are left to the UI, which has the locale formatter; this
                // explains the reasoning, which is what the reviewer is judging.
                let when = day_phrase(c.day_gap);
                let reason = if c.residual_minor.is_zero() {
                    pick(
                        format!("Equal and opposite, {when}, between two different accounts."),
                        format!(
                            "Iguales y de signo contrario, {when}, entre dos cuentas distintas."
                        ),
                    )
                } else {
                    pick(
                        format!(
                            "Opposite signs {when}, differing by a small amount — consistent \
                             with a transfer fee."
                        ),
                        format!(
                            "Signos contrarios {when}, con una pequeña diferencia: encaja \
                             con una comisión de transferencia."
                        ),
                    )
                };
                Some(PendingTransfer {
                    out_id: c.out_id,
                    in_id: c.in_id,
                    residual_minor: c.residual_minor,
                    day_gap: c.day_gap,
                    same_account_name: out.account_name == incoming.account_name,
                    reason,
                    out,
                    incoming,
                })
            })
            .collect(),
    )
}

fn day_phrase(days: i64) -> String {
    match days {
        0 => pick("the same day", "el mismo día").into(),
        1 => pick("a day later", "un día después").into(),
        n => pick(format!("{n} days later"), format!("{n} días después")),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingSplit {
    pub parent_id: i64,
    pub reimbursement_id: i64,
    pub parent: MatchSide,
    pub reimbursement: MatchSide,
    pub fraction_denominator: u32,
    pub residual_minor: Money,
    pub day_gap: i64,
    pub reason: String,
    /// What the bill would net to if this link were confirmed, counting any
    /// reimbursements already attached to it.
    pub parent_net_after: Money,
    /// Reimbursements already linked to this bill.
    pub existing_links: i64,
}

#[tauri::command]
pub fn matching_pending_splits(state: State<'_, AppState>) -> AppResult<Vec<PendingSplit>> {
    let (txns, decided, accounts) = matcher_inputs(&state)?;
    let by_id: std::collections::HashMap<i64, &TxnRef> = txns.iter().map(|t| (t.id, t)).collect();

    let candidates = matching::find_split_reimbursements(&txns, SplitParams::default(), &decided);

    state.db()?.with(|c| {
        let mut out = Vec::new();
        for cand in candidates {
            let (Some(p), Some(r)) = (
                by_id.get(&cand.parent_id),
                by_id.get(&cand.reimbursement_id),
            ) else {
                continue;
            };
            let (existing_links, already): (i64, i64) = c.query_row(
                "SELECT count(*), COALESCE(SUM(amount_minor), 0) FROM txn
                 WHERE parent_txn_id = ?1 AND link_kind = 'reimbursement'",
                rusqlite::params![cand.parent_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;

            let parent = side(p, &accounts);
            let reimbursement = side(r, &accounts);
            let net_after = parent.amount + Money::from_minor(already) + reimbursement.amount;

            let fraction = match cand.fraction_denominator {
                2 => pick("half", "la mitad").to_string(),
                3 => pick("a third", "un tercio").to_string(),
                4 => pick("a quarter", "un cuarto").to_string(),
                5 => pick("a fifth", "un quinto").to_string(),
                n => pick(format!("one {n}th"), format!("una {n}ª parte")),
            };
            let exact = cand.residual_minor.is_zero();
            let when = day_phrase(cand.day_gap);
            let already = match existing_links {
                0 => String::new(),
                1 => pick(
                    ". 1 refund already linked to this bill".to_string(),
                    ". Ya hay 1 devolución vinculada a esta factura".to_string(),
                ),
                n => pick(
                    format!(". {n} refunds already linked to this bill"),
                    format!(". Ya hay {n} devoluciones vinculadas a esta factura"),
                ),
            };
            let reason = pick(
                format!(
                    "The refund is {} {fraction} of the bill, arriving {when}{already}.",
                    if exact { "exactly" } else { "close to" },
                ),
                format!(
                    "La devolución es {}{fraction} de la factura y llega {when}{already}.",
                    if exact {
                        "exactamente "
                    } else {
                        "aproximadamente "
                    },
                ),
            );

            out.push(PendingSplit {
                parent_id: cand.parent_id,
                reimbursement_id: cand.reimbursement_id,
                fraction_denominator: cand.fraction_denominator,
                residual_minor: cand.residual_minor,
                day_gap: cand.day_gap,
                parent_net_after: net_after,
                existing_links,
                reason,
                parent,
                reimbursement,
            });
        }
        Ok(out)
    })
}

/// Record the user's decision, and apply it when confirmed. Nothing is ever
/// merged without passing through here.
#[tauri::command]
pub fn matching_decide(
    state: State<'_, AppState>,
    kind: String,
    txn_a: i64,
    txn_b: i64,
    confirm: bool,
) -> AppResult<()> {
    if kind != "transfer" && kind != "split" {
        return Err(AppError::Invalid(format!("unknown match kind: {kind}")));
    }
    let (lo, hi) = matching::pair_key(txn_a, txn_b);
    let decision = if confirm { "confirmed" } else { "rejected" };

    state.db()?.with(|c| {
        c.execute(
            "INSERT INTO match_decision (kind, txn_lo_id, txn_hi_id, decision)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(kind, txn_lo_id, txn_hi_id) DO UPDATE
               SET decision = excluded.decision,
                   decided_at = strftime('%Y-%m-%dT%H:%M:%SZ','now')",
            rusqlite::params![kind, lo, hi, decision],
        )?;
        if confirm && kind == "transfer" {
            c.execute(
                "UPDATE txn SET is_neutral_transfer = 1, transfer_group_id = ?1,
                        needs_review = 0
                 WHERE id IN (?2, ?3)",
                rusqlite::params![format!("tg-{lo}-{hi}"), lo, hi],
            )?;
        } else if confirm && kind == "split" {
            c.execute(
                "UPDATE txn SET parent_txn_id = ?1, link_kind = 'reimbursement',
                        needs_review = 0
                 WHERE id = ?2",
                rusqlite::params![txn_a, txn_b],
            )?;
        }
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/* --------------------------------------------------------------- rules -- */

use crate::i18n::pick;
use crate::import::rules::{self, Candidate, Rule};

fn load_rules(db: &crate::db::Db) -> AppResult<Vec<Rule>> {
    db.with(|c| Ok(load_rules_conn(c)?))
}

/// The same read against a connection already in hand, so a command that is
/// mid-transaction does not have to reach back through `Db` and deadlock.
pub fn load_rules_conn(c: &rusqlite::Connection) -> rusqlite::Result<Vec<Rule>> {
    let mut stmt = c.prepare(
        "SELECT id, name, priority, match_kind, pattern, account_id,
                    min_amount_minor, max_amount_minor, set_category_id,
                    set_project_id, set_neutral_transfer, is_enabled
             FROM categorization_rule ORDER BY priority, id",
    )?;
    stmt.query_map([], |r| {
        Ok(Rule {
            id: r.get(0)?,
            name: r.get(1)?,
            priority: r.get(2)?,
            match_kind: r.get(3)?,
            pattern: r.get(4)?,
            account_id: r.get(5)?,
            min_amount_minor: r.get(6)?,
            max_amount_minor: r.get(7)?,
            set_category_id: r.get(8)?,
            set_project_id: r.get(9)?,
            set_neutral_transfer: r.get::<_, i64>(10)? != 0,
            is_enabled: r.get::<_, i64>(11)? != 0,
        })
    })?
    .collect::<Result<Vec<_>, _>>()
}

/// Run the rules over transactions that nothing has classified yet.
///
/// Only ever touches rows still marked for review with no category of their
/// own, so re-running is safe and never overwrites a decision the user made by
/// hand.
pub fn apply_rules(db: &crate::db::Db, batch_id: Option<i64>) -> AppResult<usize> {
    let rules = load_rules(db)?;
    if rules.is_empty() {
        return Ok(0);
    }

    db.with(|c| {
        let mut stmt = c.prepare(
            "SELECT id, account_id, description_norm, amount_minor FROM txn
             WHERE needs_review = 1 AND category_override_id IS NULL
               AND is_neutral_transfer = 0
               AND (?1 IS NULL OR import_batch_id = ?1)",
        )?;
        let candidates = stmt
            .query_map(rusqlite::params![batch_id], |r| {
                Ok(Candidate {
                    id: r.get(0)?,
                    account_id: r.get(1)?,
                    description_norm: r.get(2)?,
                    amount_minor: Money::from_minor(r.get(3)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        let tx = c.unchecked_transaction()?;
        let mut applied = 0usize;
        for cand in &candidates {
            let Some(rule) = rules::first_match(&rules, cand) else {
                continue;
            };
            if rule.set_neutral_transfer {
                tx.execute(
                    "UPDATE txn SET is_neutral_transfer = 1, needs_review = 0 WHERE id = ?1",
                    rusqlite::params![cand.id],
                )?;
            } else {
                tx.execute(
                    "UPDATE txn SET category_override_id = COALESCE(?1, category_override_id),
                            project_id = COALESCE(?2, project_id),
                            categorized_by = 'rule', needs_review = 0
                     WHERE id = ?3",
                    rusqlite::params![rule.set_category_id, rule.set_project_id, cand.id],
                )?;
            }
            applied += 1;
        }
        tx.commit()?;
        Ok(applied)
    })
}

#[tauri::command]
pub fn rule_list(state: State<'_, AppState>) -> AppResult<Vec<Rule>> {
    load_rules(state.db()?)
}

#[tauri::command]
pub fn rule_save(state: State<'_, AppState>, rule: Rule) -> AppResult<i64> {
    if rule.name.trim().is_empty() || rule.pattern.trim().is_empty() {
        return Err(AppError::Invalid(
            "a rule needs a name and a pattern".into(),
        ));
    }
    if !matches!(
        rule.match_kind.as_str(),
        "contains" | "starts_with" | "exact" | "regex"
    ) {
        return Err(AppError::Invalid(format!(
            "unknown match kind: {}",
            rule.match_kind
        )));
    }
    if rule.match_kind == "regex" {
        regex::Regex::new(&rule.pattern)
            .map_err(|e| AppError::Invalid(format!("that regular expression is not valid: {e}")))?;
    }
    if rule.set_category_id.is_none() && rule.set_project_id.is_none() && !rule.set_neutral_transfer
    {
        return Err(AppError::Invalid(
            "a rule has to do something: set a category, an event, or mark a transfer".into(),
        ));
    }

    let id = state.db()?.with(|c| {
        let p = rusqlite::params![
            rule.name.trim(),
            rule.priority,
            rule.match_kind,
            rule.pattern.trim(),
            rule.account_id,
            rule.min_amount_minor,
            rule.max_amount_minor,
            rule.set_category_id,
            rule.set_project_id,
            i64::from(rule.set_neutral_transfer),
            i64::from(rule.is_enabled),
        ];
        match rule.id {
            Some(existing) => {
                c.execute(
                    "UPDATE categorization_rule SET name=?1, priority=?2, match_kind=?3,
                       pattern=?4, account_id=?5, min_amount_minor=?6, max_amount_minor=?7,
                       set_category_id=?8, set_project_id=?9, set_neutral_transfer=?10,
                       is_enabled=?11 WHERE id=?12",
                    rusqlite::params_from_iter(
                        p.iter()
                            .copied()
                            .chain(std::iter::once(&existing as &dyn rusqlite::ToSql)),
                    ),
                )?;
                Ok(existing)
            }
            None => {
                c.execute(
                    "INSERT INTO categorization_rule
                       (name, priority, match_kind, pattern, account_id, min_amount_minor,
                        max_amount_minor, set_category_id, set_project_id,
                        set_neutral_transfer, is_enabled)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
                    p,
                )?;
                Ok(c.last_insert_rowid())
            }
        }
    })?;
    state.backup.mark_dirty();
    Ok(id)
}

#[tauri::command]
pub fn rule_delete(state: State<'_, AppState>, id: i64) -> AppResult<()> {
    state.db()?.with(|c| {
        c.execute(
            "DELETE FROM categorization_rule WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    })?;
    state.backup.mark_dirty();
    Ok(())
}

/// Run the rules across everything still awaiting review.
#[tauri::command]
pub fn rules_apply(state: State<'_, AppState>) -> AppResult<usize> {
    let n = apply_rules(state.db()?, None)?;
    state.backup.mark_dirty();
    Ok(n)
}
