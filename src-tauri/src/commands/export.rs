//! Taking your data out.
//!
//! A local-first app is only as trustworthy as the way out of it. This writes
//! everything someone has put in as plain CSV files in a folder they choose,
//! in a form a spreadsheet opens directly. Nothing is encrypted and nothing
//! is summarised: the point is that the data is theirs, readable without
//! Coin Heaven.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use chrono::NaiveDate;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    pub name: String,
    pub rows: usize,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutcome {
    pub folder: String,
    pub files: Vec<ExportFile>,
}

/// How numbers are written: spreadsheets in a comma-decimal locale expect a
/// semicolon between columns and a comma inside a number, and silently turn
/// "1234.56" into text — so the way out follows the region it is read in.
#[derive(Clone, Copy)]
pub struct Style {
    pub delimiter: u8,
    pub decimal: char,
}

/// Minor units as a plain decimal: no symbol, no thousands separator.
fn amount(minor: i64, exponent: u32, decimal: char) -> String {
    if exponent == 0 {
        return minor.to_string();
    }
    let scale = 10i64.pow(exponent);
    let sign = if minor < 0 { "-" } else { "" };
    let abs = minor.unsigned_abs();
    format!(
        "{sign}{}{decimal}{:0width$}",
        abs / scale as u64,
        abs % scale as u64,
        width = exponent as usize
    )
}

fn exponent(code: &str) -> u32 {
    crate::currency::lookup(code).map_or(2, |c| u32::from(c.exponent))
}

/// Units are held ×1e8; written with only the digits that are there.
fn units(e8: i64, decimal: char) -> String {
    let s = amount(e8, 8, decimal);
    if s.contains(decimal) {
        s.trim_end_matches('0')
            .trim_end_matches(decimal)
            .to_string()
    } else {
        s
    }
}

/// A cell that a spreadsheet would read as a formula. Bank statements are
/// untrusted text: a description like `=HYPERLINK("http://…")` or
/// `=cmd|' /C calc'!A0` must reach the file as text. A plain signed number is
/// left alone so a negative amount stays an amount.
fn neutralise(cell: &str) -> std::borrow::Cow<'_, str> {
    let risky = matches!(
        cell.as_bytes().first(),
        Some(b'=' | b'+' | b'-' | b'@' | b'\t' | b'\r')
    );
    let plain_number = {
        let rest = cell.strip_prefix(['-', '+']).unwrap_or(cell);
        !rest.is_empty()
            && rest.bytes().next().is_some_and(|b| b.is_ascii_digit())
            && rest
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'.' || b == b',')
    };
    if risky && !plain_number {
        std::borrow::Cow::Owned(format!("'{cell}"))
    } else {
        std::borrow::Cow::Borrowed(cell)
    }
}

fn write(
    dir: &Path,
    name: &str,
    header: &[&str],
    rows: &[Vec<String>],
    style: Style,
) -> AppResult<ExportFile> {
    let path = dir.join(name);
    let mut buf: Vec<u8> = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM, so accents survive Excel
    {
        let mut w = csv::WriterBuilder::new()
            .delimiter(style.delimiter)
            .terminator(csv::Terminator::CRLF)
            .from_writer(&mut buf);
        w.write_record(header)
            .map_err(|e| AppError::Other(e.to_string()))?;
        for r in rows {
            w.write_record(r.iter().map(|c| neutralise(c).into_owned()))
                .map_err(|e| AppError::Other(e.to_string()))?;
        }
        w.flush().map_err(|e| AppError::Other(e.to_string()))?;
    }
    std::fs::write(&path, buf)
        .map_err(|e| AppError::Other(format!("cannot write {}: {e}", path.display())))?;
    Ok(ExportFile {
        name: name.to_string(),
        rows: rows.len(),
    })
}

fn query(
    c: &rusqlite::Connection,
    sql: &str,
    cols: usize,
    f: impl Fn(&rusqlite::Row<'_>) -> rusqlite::Result<Vec<String>>,
) -> AppResult<Vec<Vec<String>>> {
    let mut stmt = c.prepare(sql)?;
    let rows = stmt
        .query_map([], |r| f(r))?
        .collect::<Result<Vec<_>, _>>()?;
    debug_assert!(rows.iter().all(|r| r.len() == cols));
    Ok(rows)
}

/// Write every table someone has typed into or imported, as CSV, into `dir`.
pub fn export_all(
    c: &rusqlite::Connection,
    dir: &Path,
    style: Style,
) -> AppResult<Vec<ExportFile>> {
    let d = style.decimal;
    let mut files = Vec::new();

    let txns = query(
        c,
        "SELECT t.id, t.booked_on, a.name, t.description_raw, COALESCE(t.counterparty,''),
                t.amount_minor, t.currency, COALESCE(k.name,''), COALESCE(p.name,''),
                t.is_neutral_transfer, COALESCE(t.notes,'')
         FROM txn t
         JOIN account a ON a.id = t.account_id
         LEFT JOIN v_txn_effective e ON e.id = t.id
         LEFT JOIN pl_category k ON k.id = e.effective_category_id
         LEFT JOIN project_event p ON p.id = t.project_id
         ORDER BY t.booked_on, t.id",
        11,
        |r| {
            let cur: String = r.get(6)?;
            Ok(vec![
                r.get::<_, i64>(0)?.to_string(),
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                amount(r.get(5)?, exponent(&cur), d),
                cur,
                r.get(7)?,
                r.get(8)?,
                if r.get::<_, i64>(9)? != 0 {
                    "yes"
                } else {
                    "no"
                }
                .into(),
                r.get(10)?,
            ])
        },
    )?;
    files.push(write(
        dir,
        "transactions.csv",
        &[
            "id",
            "date",
            "account",
            "description",
            "counterparty",
            "amount",
            "currency",
            "category",
            "event",
            "internal_transfer",
            "notes",
        ],
        &txns,
        style,
    )?);

    let cats = query(
        c,
        "SELECT name, kind FROM pl_category WHERE is_archived = 0 ORDER BY sort_order, name",
        2,
        |r| Ok(vec![r.get(0)?, r.get(1)?]),
    )?;
    files.push(write(
        dir,
        "categories.csv",
        &["name", "kind"],
        &cats,
        style,
    )?);

    let worth = query(
        c,
        "SELECT s.taken_on, k.name, l.label, l.gross_value_minor, l.debt_minor, l.currency, l.source
         FROM wealth_snapshot s
         JOIN wealth_line l ON l.snapshot_id = s.id
         JOIN asset_class k ON k.key = l.bucket
         ORDER BY s.taken_on, k.sort_order, l.label",
        7,
        |r| {
            let cur: String = r.get(5)?;
            let e = exponent(&cur);
            Ok(vec![
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                amount(r.get(3)?, e, d),
                amount(r.get(4)?, e, d),
                cur,
                r.get(6)?,
            ])
        },
    )?;
    files.push(write(
        dir,
        "net_worth.csv",
        &[
            "date",
            "asset_class",
            "item",
            "value",
            "owed",
            "currency",
            "source",
        ],
        &worth,
        style,
    )?);

    let holdings = query(
        c,
        "SELECT h.name, COALESCE(h.symbol,''), k.name, h.is_archived
         FROM holding h JOIN asset_class k ON k.key = h.asset_class
         ORDER BY h.name",
        4,
        |r| {
            Ok(vec![
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                if r.get::<_, i64>(3)? != 0 {
                    "yes"
                } else {
                    "no"
                }
                .into(),
            ])
        },
    )?;
    files.push(write(
        dir,
        "holdings.csv",
        &["name", "symbol", "asset_class", "archived"],
        &holdings,
        style,
    )?);

    let moves = query(
        c,
        "SELECT t.on_date, h.name, t.kind, t.amount_minor, t.units_e8, t.skipped
         FROM holding_txn t JOIN holding h ON h.id = t.holding_id
         ORDER BY t.on_date, t.id",
        6,
        |r| {
            Ok(vec![
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                amount(r.get(3)?, 2, d),
                r.get::<_, Option<i64>>(4)?
                    .map_or(String::new(), |u| units(u, d)),
                if r.get::<_, i64>(5)? != 0 {
                    "yes"
                } else {
                    "no"
                }
                .into(),
            ])
        },
    )?;
    files.push(write(
        dir,
        "holding_movements.csv",
        &["date", "holding", "kind", "amount", "units", "skipped"],
        &moves,
        style,
    )?);

    let prices = query(
        c,
        "SELECT p.on_date, h.name, p.price_micro, p.source
         FROM holding_price p JOIN holding h ON h.id = p.holding_id
         ORDER BY p.on_date, h.name",
        4,
        |r| {
            Ok(vec![
                r.get(0)?,
                r.get(1)?,
                amount(r.get(2)?, 6, d),
                r.get(3)?,
            ])
        },
    )?;
    files.push(write(
        dir,
        "holding_prices.csv",
        &["date", "holding", "price", "source"],
        &prices,
        style,
    )?);

    let budgets = query(
        c,
        "SELECT k.name, b.period, b.amount_minor
         FROM budget b JOIN pl_category k ON k.id = b.category_id
         ORDER BY b.period, k.sort_order",
        3,
        |r| Ok(vec![r.get(0)?, r.get(1)?, amount(r.get(2)?, 2, d)]),
    )?;
    files.push(write(
        dir,
        "budgets.csv",
        &["category", "period", "amount"],
        &budgets,
        style,
    )?);

    let plans = query(
        c,
        "SELECT i.period, i.sort_order + 1, i.kind, i.name, i.amount_minor,
                CASE WHEN m.mode = 'individual' THEN ''
                     WHEN i.owner = 'a' THEN COALESCE(NULLIF(m.person_a, ''), 'person 1')
                     WHEN i.owner = 'b' THEN COALESCE(NULLIF(m.person_b, ''), 'person 2')
                     ELSE 'shared' END
         FROM plan_item i JOIN plan_month m ON m.period = i.period
         ORDER BY i.period, i.sort_order, i.id",
        6,
        |r| {
            Ok(vec![
                r.get(0)?,
                r.get::<_, i64>(1)?.to_string(),
                r.get(2)?,
                r.get(3)?,
                amount(r.get(4)?, 2, d),
                r.get(5)?,
            ])
        },
    )?;
    files.push(write(
        dir,
        "monthly_plans.csv",
        &["month", "position", "type", "name", "amount", "whose"],
        &plans,
        style,
    )?);

    Ok(files)
}

/// A folder name that does not exist yet: "Coin Heaven export 2026-10-13", then
/// "… (2)". An export never overwrites an earlier one.
fn fresh_folder(parent: &Path, today: NaiveDate) -> PathBuf {
    let base = format!("Coin Heaven export {today}");
    let mut p = parent.join(&base);
    let mut n = 2;
    while p.exists() {
        p = parent.join(format!("{base} ({n})"));
        n += 1;
    }
    p
}

#[tauri::command]
pub fn data_export(
    state: State<'_, AppState>,
    folder: String,
    delimiter: String,
    decimal: String,
) -> AppResult<ExportOutcome> {
    let delimiter = match delimiter.as_str() {
        ";" => b';',
        "\t" => b'\t',
        _ => b',',
    };
    let decimal = if decimal == "," { ',' } else { '.' };
    let parent = PathBuf::from(&folder);
    if !parent.is_dir() {
        return Err(AppError::Invalid(format!(
            "{} is not a folder",
            parent.display()
        )));
    }
    let dir = fresh_folder(&parent, chrono::Local::now().date_naive());
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Other(format!("cannot create {}: {e}", dir.display())))?;
    let files = state
        .db()?
        .with(|c| export_all(c, &dir, Style { delimiter, decimal }))?;
    Ok(ExportOutcome {
        folder: dir.display().to_string(),
        files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_are_plain_decimals_in_either_convention() {
        assert_eq!(amount(123456, 2, '.'), "1234.56");
        assert_eq!(amount(-5, 2, ','), "-0,05");
        assert_eq!(amount(100, 2, '.'), "1.00");
        assert_eq!(amount(1500, 0, '.'), "1500", "a currency with no decimals");
        assert_eq!(amount(1_500, 3, '.'), "1.500");
        assert_eq!(units(150_000_000, '.'), "1.5");
        assert_eq!(units(100_000_000, '.'), "1");
        assert_eq!(units(1, '.'), "0.00000001");
    }

    #[test]
    fn spreadsheet_formulas_are_written_as_text_but_amounts_stay_numbers() {
        for evil in [
            "=1+1",
            "+SUM(A1)",
            "-2+3",
            "@SUM(A1)",
            "\tcmd",
            "\rx",
            "=cmd|' /C calc'!A0",
        ] {
            assert_eq!(neutralise(evil), format!("'{evil}"), "{evil:?}");
        }
        for fine in [
            "-12.34",
            "+5",
            "1234.56",
            "-0,5",
            "2026-01-31",
            "Coffee",
            "",
        ] {
            assert_eq!(neutralise(fine), fine, "{fine:?}");
        }
    }
}
