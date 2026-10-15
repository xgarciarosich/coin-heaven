//! CSV parsing against a per-bank column mapping.
//!
//! Every bank exports something slightly different, so nothing here is
//! hardcoded: column positions, date format, decimal and thousands separators,
//! signed-amount versus debit/credit pairs, header rows to skip, and the text
//! encoding all come from a `BankMapping` row the user can edit.
//!
//! Parsing never fails the whole file. A malformed row is collected as a
//! rejection with its line number and reason, and the rest still import —
//! a single unparseable line in a year of statements should not block the year.

use crate::i18n::pick;
use crate::import::dates;
use crate::import::table::Table;
use crate::money::Money;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BankMapping {
    pub name: String,
    pub delimiter: char,
    /// "utf-8", "windows-1252" or "iso-8859-1"; Spanish banks still emit Latin-1.
    pub encoding: String,
    pub has_header: bool,
    pub skip_rows: usize,
    pub date_col: usize,
    /// chrono strftime, e.g. "%d/%m/%Y".
    pub date_format: String,
    pub description_col: usize,
    /// The single signed amount column. Outgoings are negative; a file that
    /// writes them positive is handled by `invert_sign`.
    pub amount_col: Option<usize>,
    pub counterparty_col: Option<usize>,
    pub decimal_separator: char,
    pub thousands_separator: String,
    /// Some exports write outgoings as positive numbers.
    pub invert_sign: bool,
    pub currency: String,
}

impl Default for BankMapping {
    /// Sensible for a Spanish bank: day-first dates, comma decimals.
    fn default() -> Self {
        BankMapping {
            name: "New mapping".into(),
            delimiter: ';',
            encoding: "utf-8".into(),
            has_header: true,
            skip_rows: 0,
            date_col: 0,
            date_format: "%d/%m/%Y".into(),
            description_col: 1,
            amount_col: Some(2),
            counterparty_col: None,
            decimal_separator: ',',
            thousands_separator: ".".into(),
            invert_sign: false,
            currency: crate::currency::DEFAULT.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedRow {
    pub line: usize,
    pub booked_on: String,
    pub amount_minor: Money,
    pub description: String,
    pub counterparty: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rejection {
    pub line: usize,
    pub reason: String,
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParseResult {
    pub rows: Vec<ParsedRow>,
    pub rejected: Vec<Rejection>,
    /// Header cells, when the mapping says there is a header — lets the UI
    /// show real column names while the user picks indices.
    pub headers: Vec<String>,
}

/// Turn a bank's number string into integer minor units.
///
/// Handles "1.234,56", "1,234.56", "-45,00", "45,00-" (trailing sign, used by
/// some German and Spanish exports) and parenthesised negatives.
pub fn parse_amount(raw: &str, decimal: char, thousands: &str, exponent: u32) -> Option<Money> {
    let mut s = raw.trim().to_string();
    if s.is_empty() {
        return None;
    }

    let mut negative = false;
    if s.starts_with('(') && s.ends_with(')') {
        negative = true;
        s = s[1..s.len() - 1].to_string();
    }
    if let Some(rest) = s.strip_suffix('-') {
        negative = true;
        s = rest.trim().to_string();
    }
    if let Some(rest) = s.strip_prefix('-') {
        negative = true;
        s = rest.trim().to_string();
    }
    s = s.trim_start_matches('+').trim().to_string();

    // Strip currency symbols, spaces and the thousands separator.
    if !thousands.is_empty() {
        s = s.replace(thousands, "");
    }
    s.retain(|c| c.is_ascii_digit() || c == decimal);

    let (whole, frac) = match s.split_once(decimal) {
        Some((w, f)) => (w.to_string(), f.to_string()),
        None => (s.clone(), String::new()),
    };
    if whole.is_empty() && frac.is_empty() {
        return None;
    }

    let scale = 10i64.pow(exponent);
    let whole_v: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };

    // Pad or truncate the fraction to the currency's exponent.
    let mut frac_digits = frac;
    frac_digits.truncate(exponent as usize);
    while frac_digits.len() < exponent as usize {
        frac_digits.push('0');
    }
    let frac_v: i64 = if frac_digits.is_empty() {
        0
    } else {
        frac_digits.parse().ok()?
    };

    let minor = whole_v.checked_mul(scale)?.checked_add(frac_v)?;
    Some(Money::from_minor(if negative { -minor } else { minor }))
}

fn cell(row: &[String], idx: usize) -> Option<&str> {
    row.get(idx).map(|s| s.trim())
}

/// Parse a loaded table against a mapping.
///
/// Rows are never dropped silently: anything unusable becomes a `Rejection`
/// carrying the file line and the reason, so a single bad line in a year of
/// statements does not block the year.
pub fn parse(table: &Table, m: &BankMapping) -> ParseResult {
    let exponent = crate::currency::lookup(&m.currency)
        .map(|c| c.exponent as u32)
        .unwrap_or(2);

    let mut rows = Vec::new();
    let mut rejected = Vec::new();
    let mut headers = Vec::new();
    let skip = m.skip_rows + usize::from(m.has_header);

    for (i, rec) in table.rows.iter().enumerate() {
        let line = i + 1;
        if i < skip {
            if m.has_header && i == m.skip_rows {
                headers = rec.iter().map(|c| c.trim().to_string()).collect();
            }
            continue;
        }
        if rec.iter().all(|c| c.trim().is_empty()) {
            continue; // blank line, not an error
        }
        let raw = rec.join(" | ");

        let Some(date_raw) = cell(rec, m.date_col) else {
            rejected.push(Rejection {
                line,
                reason: pick(
                    format!("no column {}", m.date_col),
                    format!("no existe la columna {}", m.date_col),
                ),
                raw,
            });
            continue;
        };
        // Lenient: a column that turns out to carry a timestamp under a
        // date-only format is read rather than rejected.
        let Some(date) = dates::parse_lenient(date_raw, &m.date_format) else {
            rejected.push(Rejection {
                line,
                reason: pick(
                    format!("date '{date_raw}' does not match {}", m.date_format),
                    format!("la fecha «{date_raw}» no encaja con {}", m.date_format),
                ),
                raw,
            });
            continue;
        };

        let Some(amount_col) = m.amount_col else {
            rejected.push(Rejection {
                line,
                reason: pick("no amount column is set", "no hay columna de importe").into(),
                raw,
            });
            continue;
        };
        let amount = cell(rec, amount_col)
            .and_then(|v| parse_amount(v, m.decimal_separator, &m.thousands_separator, exponent));
        let Some(mut amount) = amount else {
            rejected.push(Rejection {
                line,
                reason: pick("no usable amount", "no hay un importe utilizable").into(),
                raw,
            });
            continue;
        };
        if m.invert_sign {
            amount = -amount;
        }

        let description = cell(rec, m.description_col).unwrap_or("").to_string();
        if description.is_empty() {
            rejected.push(Rejection {
                line,
                reason: pick("empty description", "descripción vacía").into(),
                raw,
            });
            continue;
        }

        rows.push(ParsedRow {
            line,
            booked_on: date.format("%Y-%m-%d").to_string(),
            amount_minor: amount,
            description,
            counterparty: m
                .counterparty_col
                .and_then(|c| cell(rec, c))
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        });
    }

    ParseResult {
        rows,
        rejected,
        headers,
    }
}

#[cfg(test)]
mod tests;
