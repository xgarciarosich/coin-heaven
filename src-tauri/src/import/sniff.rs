//! Working out a bank's export format from the file itself.
//!
//! The original design made the user describe their bank by hand: column
//! indices, date format, separators. That is a lot to ask of someone who just
//! wants to see where their money went, and it is all derivable from the file.
//!
//! Detection is deliberately **local and deterministic**. No network, no API
//! key, no data leaving the machine, and every rule is unit-testable. The
//! assistant is offered only as a fallback when confidence is low, and even
//! then it sees a header row and three sample rows, redacted — never the file.
//!
//! Where a genuine ambiguity exists — 03/04/2026 is either 3 April or 4 March —
//! the detector says so in `notes` rather than quietly picking one.

use crate::i18n::pick;
use crate::import::dates;
use crate::import::parser::BankMapping;
use crate::import::table::{Source, Table};
use serde::Serialize;

/// Header words that name each role, across the languages a Spanish or
/// European user is likely to meet.
const DATE_WORDS: [&str; 10] = [
    "fecha",
    "date",
    "data",
    "datum",
    "fecha operacion",
    "fecha valor",
    "f. valor",
    "buchung",
    "transaction date",
    "posted",
];
const DESC_WORDS: [&str; 12] = [
    "concepto",
    "descripcion",
    "description",
    "detalle",
    "detail",
    "narrative",
    "referencia",
    "beneficiario",
    "payee",
    "memo",
    "text",
    "verwendungszweck",
];
const AMOUNT_WORDS: [&str; 9] = [
    "importe",
    "amount",
    "cantidad",
    "valor",
    "monto",
    "movimiento",
    "betrag",
    "importo",
    "value",
];
/// Only used to recognise a shape the importer no longer supports, so the file
/// can be refused clearly instead of half-read.
const DEBIT_WORDS: [&str; 6] = ["debe", "cargo", "debit", "salida", "pago", "soll"];
const CREDIT_WORDS: [&str; 6] = ["haber", "abono", "credit", "entrada", "ingreso", "haben"];
/// A running balance is numeric in every row and must never be read as the
/// amount — doing so silently turns a statement into nonsense.
const BALANCE_WORDS: [&str; 6] = [
    "saldo",
    "balance",
    "solde",
    "saldo posterior",
    "running",
    "total",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub mapping: BankMapping,
    /// 0–100. Below ~60 the UI should ask the user to confirm, or offer the
    /// assistant.
    pub confidence: u8,
    /// Plain-language account of what was decided and why.
    pub notes: Vec<String>,
    /// Ambiguities the user should look at, even at high confidence.
    pub warnings: Vec<String>,
    pub header_row: Option<usize>,
    pub first_data_row: usize,
}

fn fold(s: &str) -> String {
    s.trim()
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' => 'u',
            'ñ' => 'n',
            'ü' => 'u',
            c => c,
        })
        .collect()
}

fn header_matches(cell: &str, words: &[&str]) -> bool {
    let f = fold(cell);
    !f.is_empty() && words.iter().any(|w| f.contains(w))
}

/// Try every known format; return the one that parses.
///
/// Delegates to the shared catalogue so the detector, the parser and the
/// dropdown can never disagree about what is supported.
pub fn detect_date_format(s: &str) -> Option<&'static str> {
    dates::detect(s)
}

/// Does this look like a monetary figure, and if so with which decimal mark?
fn number_shape(s: &str) -> Option<char> {
    let t: String = s
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '€' | '$' | '£' | '+' | '(' | ')' | '\u{a0}'))
        .collect();
    let core = t.trim_start_matches('-').trim_end_matches('-');
    if core.is_empty() || !core.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    if !core
        .chars()
        .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
    {
        return None;
    }
    // The *last* separator followed by 1-2 digits is the decimal mark.
    let last_comma = core.rfind(',');
    let last_dot = core.rfind('.');
    match (last_comma, last_dot) {
        (Some(c), Some(d)) => Some(if c > d { ',' } else { '.' }),
        (Some(c), None) => {
            let tail = core.len() - c - 1;
            Some(if (1..=2).contains(&tail) { ',' } else { '.' })
        }
        (None, Some(d)) => {
            let tail = core.len() - d - 1;
            Some(if (1..=2).contains(&tail) { '.' } else { ',' })
        }
        (None, None) => Some('.'), // a bare integer commits to nothing
    }
}

fn is_number(s: &str) -> bool {
    number_shape(s).is_some()
}

/// Detect the layout of a loaded table.
pub fn detect(table: &Table) -> Detection {
    let mut notes = Vec::new();
    let mut warnings = Vec::new();
    let width = table.width();

    // --- 1. where the data starts ------------------------------------------
    // The first row that carries a parseable date *and* a number is data;
    // anything above it is bank letterhead.
    let rows: Vec<(usize, &Vec<String>)> = table.data_rows().collect();
    let first_data_idx = rows
        .iter()
        .position(|(_, r)| {
            r.iter().any(|c| detect_date_format(c).is_some()) && r.iter().any(|c| is_number(c))
        })
        .unwrap_or(0);
    let first_data_row = rows.get(first_data_idx).map(|(i, _)| *i).unwrap_or(0);

    // The row above the data, if it is text rather than numbers, is the header.
    let header_row = first_data_idx
        .checked_sub(1)
        .and_then(|i| rows.get(i))
        .filter(|(_, r)| {
            r.iter().any(|c| !c.trim().is_empty())
                && !r.iter().any(|c| detect_date_format(c).is_some())
        })
        .map(|(i, _)| *i);

    let headers: Vec<String> = header_row
        .map(|i| table.rows[i].clone())
        .unwrap_or_default();
    if let Some(h) = header_row {
        notes.push(pick(
            format!("Header found on line {}.", h + 1),
            format!("Cabecera encontrada en la línea {}.", h + 1),
        ));
    }
    // Whatever sits above the header — or above the data, when there is no
    // header — is the bank's letterhead.
    let preamble = header_row.unwrap_or(first_data_row);
    if preamble > 0 {
        notes.push(pick(
            format!("Skipped {preamble} line(s) of preamble above the data."),
            format!("Se omitieron {preamble} línea(s) de encabezado sobre los datos."),
        ));
    }

    // --- 2. classify each column over the data rows -------------------------
    let data: Vec<&Vec<String>> = rows[first_data_idx..]
        .iter()
        .map(|(_, r)| *r)
        .take(200)
        .collect();
    let n = data.len().max(1);

    let mut date_hits = vec![0usize; width];
    let mut num_hits = vec![0usize; width];
    let mut filled = vec![0usize; width];
    let mut text_len = vec![0usize; width];
    let mut fmt_votes: Vec<std::collections::HashMap<&'static str, usize>> =
        vec![Default::default(); width];
    let mut dec_votes = (0usize, 0usize); // (comma, dot)

    for row in &data {
        for col in 0..width {
            let cell = row.get(col).map(String::as_str).unwrap_or("").trim();
            if cell.is_empty() {
                continue;
            }
            filled[col] += 1;
            let mut parsed_as_date = false;
            for pattern in dates::patterns() {
                if dates::parse_strict(cell, pattern).is_some() {
                    parsed_as_date = true;
                    *fmt_votes[col].entry(pattern).or_default() += 1;
                }
            }
            if parsed_as_date {
                date_hits[col] += 1;
            } else if let Some(mark) = number_shape(cell) {
                num_hits[col] += 1;
                // Only cells that actually carry a separator vote on it.
                if cell.contains(',') || cell.contains('.') {
                    if mark == ',' {
                        dec_votes.0 += 1
                    } else {
                        dec_votes.1 += 1
                    }
                }
            } else {
                text_len[col] += cell.chars().count();
            }
        }
    }

    // --- 3. the date column -------------------------------------------------
    let date_col = (0..width)
        .filter(|&c| date_hits[c] > 0)
        .max_by_key(|&c| {
            // Header agreement breaks ties between a booking date and a value
            // date, both of which parse.
            let bonus = usize::from(
                headers
                    .get(c)
                    .is_some_and(|h| header_matches(h, &DATE_WORDS)),
            );
            (date_hits[c] * 2 + bonus * n, std::cmp::Reverse(c))
        })
        .unwrap_or(0);

    // A format has to account for *every* date in the column. Picking the most
    // popular instead would let a run of ambiguous rows outvote the single
    // unambiguous one that actually settles the question — 03/25/2026 proves a
    // file is month-first no matter how many 04/01-style rows surround it.
    let explains_all: Vec<&'static str> = dates::patterns()
        .into_iter()
        .filter(|p| {
            date_hits[date_col] > 0
                && fmt_votes[date_col].get(p).copied().unwrap_or(0) == date_hits[date_col]
        })
        .collect();

    let date_format = explains_all.first().copied().unwrap_or_else(|| {
        // Nothing explains the whole column — dirty data. Fall back to the most
        // frequent and let the rejected-rows list show what did not fit.
        fmt_votes[date_col]
            .iter()
            .max_by_key(|&(_, v)| *v)
            .map(|(f, _)| *f)
            .unwrap_or("%d/%m/%Y")
    });

    // Day-first versus month-first cannot be told apart when every day is 12 or
    // below. Say so rather than pretending.
    if matches!(
        date_format,
        "%d/%m/%Y" | "%m/%d/%Y" | "%d/%m/%y" | "%m/%d/%y"
    ) {
        let decisive = data.iter().any(|r| {
            r.get(date_col)
                .and_then(|c| c.split(['/', '-', '.']).next())
                .and_then(|p| p.trim().parse::<u32>().ok())
                .is_some_and(|v| v > 12)
        });
        if !decisive {
            warnings.push(
                pick(
                    "Every day in this file is 12 or lower, so day-first and month-first \
                     both parse. Assuming day-first — check a row you recognise.",
                    "Ningún día de este archivo pasa de 12, así que tanto día/mes como \
                     mes/día encajan. Se asume día/mes: comprueba alguna fila que reconozcas.",
                )
                .into(),
            );
        } else {
            notes.push(dates_read_as(date_format));
        }
    } else {
        notes.push(dates_read_as(date_format));
    }

    // --- 4. amount columns --------------------------------------------------
    let numeric: Vec<usize> = (0..width)
        .filter(|&c| c != date_col && num_hits[c] > 0 && num_hits[c] * 2 >= filled[c])
        .collect();

    let is_balance = |c: usize| {
        headers.get(c).is_some_and(|h| header_matches(h, &BALANCE_WORDS))
            // A running balance is present on essentially every row.
            || (filled[c] == n && numeric.len() > 2)
    };
    let named = |c: usize, words: &[&str]| headers.get(c).is_some_and(|h| header_matches(h, words));

    // One signed amount column. Separate debit/credit pairs are not supported;
    // where one is recognised the file is refused rather than half-read, since
    // picking either column alone would silently drop half the statement.
    let candidates: Vec<usize> = numeric
        .iter()
        .copied()
        .filter(|&c| !is_balance(c))
        .collect();
    let amount_col = candidates
        .iter()
        .copied()
        .find(|&c| named(c, &AMOUNT_WORDS))
        .or_else(|| {
            candidates.iter().copied().max_by_key(|&c| {
                let negatives = data
                    .iter()
                    .filter(|r| r.get(c).is_some_and(|v| v.trim_start().starts_with('-')))
                    .count();
                (usize::from(negatives > 0), num_hits[c])
            })
        });
    if amount_col.is_some() {
        notes.push(
            pick(
                "Single signed amount column.",
                "Una sola columna de importe con signo.",
            )
            .into(),
        );
    }

    let named_pair = candidates.iter().any(|&c| named(c, &DEBIT_WORDS))
        && candidates.iter().any(|&c| named(c, &CREDIT_WORDS));
    let silent_pair = candidates.len() == 2
        && data.iter().all(|r| {
            let filled_count = candidates
                .iter()
                .filter(|&&c| !r.get(c).map(|v| v.trim().is_empty()).unwrap_or(true))
                .count();
            filled_count <= 1
        })
        && data.iter().any(|r| {
            candidates
                .iter()
                .filter(|&&c| !r.get(c).map(|v| v.trim().is_empty()).unwrap_or(true))
                .count()
                == 1
        });
    if named_pair || silent_pair {
        warnings.push(
            pick(
                "This file looks like it has separate debit and credit columns, which \
                 Coin Heaven does not read. Pick the single column that carries the signed \
                 amount, or export the statement with one amount column.",
                "Parece que este archivo tiene columnas separadas de cargo y abono, que \
                 Coin Heaven no lee. Elige la única columna con el importe con signo, o exporta \
                 el extracto con una sola columna de importe.",
            )
            .into(),
        );
    }

    if let Some(b) = (0..width).find(|&c| is_balance(c) && Some(c) != amount_col) {
        if headers
            .get(b)
            .is_some_and(|h| header_matches(h, &BALANCE_WORDS))
        {
            notes.push(
                pick(
                    "Ignored a running-balance column.",
                    "Se ignoró una columna de saldo.",
                )
                .into(),
            );
        }
    }

    // --- 5. description -----------------------------------------------------
    let description_col = (0..width)
        .filter(|&c| Some(c) != amount_col && c != date_col)
        .max_by_key(|&c| {
            let bonus = usize::from(named(c, &DESC_WORDS));
            (bonus * 10_000 + text_len[c], std::cmp::Reverse(c))
        })
        .unwrap_or(1.min(width.saturating_sub(1)));

    // --- 6. separators and confidence --------------------------------------
    let decimal_separator = if dec_votes.0 >= dec_votes.1 { ',' } else { '.' };
    let thousands_separator = if decimal_separator == ',' { "." } else { "," };

    let (delimiter, encoding) = match &table.source {
        Source::Delimited {
            delimiter,
            encoding,
        } => (*delimiter, encoding.clone()),
        Source::Spreadsheet { sheet } => {
            notes.push(pick(
                format!("Read from worksheet \"{sheet}\"."),
                format!("Leído de la hoja «{sheet}»."),
            ));
            (',', "utf-8".into())
        }
    };

    let mut confidence = 0u32;
    confidence += (date_hits[date_col] * 45 / n.max(1)) as u32;
    if amount_col.is_some() {
        confidence += 30;
    }
    if text_len[description_col] > 0 {
        confidence += 15;
    }
    if header_row.is_some() {
        confidence += 10;
    }
    let confidence = confidence.min(100) as u8;

    if data.is_empty() {
        warnings.push(
            pick(
                "No data rows were recognised in this file.",
                "No se reconoció ninguna fila de datos en este archivo.",
            )
            .into(),
        );
    }

    Detection {
        mapping: BankMapping {
            name: suggest_name(&headers, &table.source),
            delimiter,
            encoding,
            has_header: header_row.is_some(),
            skip_rows: header_row.unwrap_or(first_data_row),
            date_col,
            date_format: date_format.to_string(),
            description_col,
            amount_col,
            counterparty_col: None,
            decimal_separator,
            thousands_separator: thousands_separator.to_string(),
            invert_sign: false,
            currency: crate::currency::DEFAULT.into(),
        },
        confidence,
        notes,
        warnings,
        header_row,
        first_data_row,
    }
}

fn suggest_name(headers: &[String], source: &Source) -> String {
    match source {
        Source::Spreadsheet { sheet } => format!("Detected · {sheet}"),
        Source::Delimited { .. } => {
            if headers.is_empty() {
                "Detected format".into()
            } else {
                format!(
                    "Detected · {}",
                    headers
                        .iter()
                        .filter(|h| !h.trim().is_empty())
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("/")
                )
            }
        }
    }
}

#[cfg(test)]
mod tests;

fn dates_read_as(format: &str) -> String {
    pick(
        format!("Dates read as {format}."),
        format!("Fechas leídas como {format}."),
    )
}
