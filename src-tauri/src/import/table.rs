//! One shape for every supported file.
//!
//! CSV, tab-separated text and Excel workbooks all become the same thing: a
//! grid of trimmed strings. Everything downstream — format detection, parsing,
//! preview — works on that grid, so adding a format means writing one loader
//! rather than threading a new branch through the whole module.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Delimited { delimiter: char, encoding: String },
    Spreadsheet { sheet: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub rows: Vec<Vec<String>>,
    pub source: Source,
}

impl Table {
    pub fn width(&self) -> usize {
        self.rows.iter().map(Vec::len).max().unwrap_or(0)
    }

    pub fn cell(&self, row: usize, col: usize) -> &str {
        self.rows
            .get(row)
            .and_then(|r| r.get(col))
            .map(String::as_str)
            .unwrap_or("")
    }

    /// Non-empty rows only — blank lines carry no information for detection.
    pub fn data_rows(&self) -> impl Iterator<Item = (usize, &Vec<String>)> {
        self.rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.iter().any(|c| !c.trim().is_empty()))
    }
}

/* ------------------------------------------------------------- decoding -- */

/// Decode bytes to text, honouring a BOM and falling back sensibly.
///
/// Spanish banks still emit Windows-1252, and a file that is not valid UTF-8 is
/// far more likely to be that than genuinely corrupt.
pub fn decode(bytes: &[u8], forced: Option<&str>) -> (String, String) {
    if let Some(enc) = forced.filter(|e| !e.is_empty() && *e != "auto") {
        let e = named_encoding(enc);
        return (e.decode(bytes).0.into_owned(), e.name().to_lowercase());
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return (
            encoding_rs::UTF_8.decode(bytes).0.into_owned(),
            "utf-8".into(),
        );
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return (
            encoding_rs::UTF_16LE.decode(bytes).0.into_owned(),
            "utf-16le".into(),
        );
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => (s.to_string(), "utf-8".into()),
        Err(_) => (
            encoding_rs::WINDOWS_1252.decode(bytes).0.into_owned(),
            "windows-1252".into(),
        ),
    }
}

fn named_encoding(name: &str) -> &'static encoding_rs::Encoding {
    match name.to_ascii_lowercase().replace('_', "-").as_str() {
        "windows-1252" | "cp1252" | "iso-8859-1" | "latin1" | "latin-1" => {
            encoding_rs::WINDOWS_1252
        }
        "utf-16le" => encoding_rs::UTF_16LE,
        "utf-16be" => encoding_rs::UTF_16BE,
        _ => encoding_rs::UTF_8,
    }
}

/* ------------------------------------------------------------ delimiter -- */

const CANDIDATES: [char; 4] = [';', ',', '\t', '|'];

/// Pick the delimiter that splits the file into the most *consistent* number of
/// columns.
///
/// Counting occurrences alone is wrong: a European file using ';' also contains
/// a comma in every amount, and a description can contain either. What marks
/// the real delimiter is that it yields the same field count on line after
/// line.
pub fn sniff_delimiter(text: &str) -> char {
    let sample: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(30)
        .collect();
    if sample.is_empty() {
        return ',';
    }

    let mut best = (',', 0.0f64, 0usize);
    for &d in &CANDIDATES {
        let counts: Vec<usize> = sample
            .iter()
            .map(|l| split_respecting_quotes(l, d).len())
            .collect();
        let max = *counts.iter().max().unwrap_or(&1);
        if max < 2 {
            continue;
        }
        let agree = counts.iter().filter(|&&c| c == max).count() as f64;
        let score = agree / counts.len() as f64;
        // Prefer stronger agreement; break ties on more columns, which favours
        // the delimiter that actually structures the row.
        if score > best.1 + 1e-9 || ((score - best.1).abs() < 1e-9 && max > best.2) {
            best = (d, score, max);
        }
    }
    best.0
}

fn split_respecting_quotes(line: &str, delim: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cur.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == delim && !quoted => {
                out.push(std::mem::take(&mut cur));
            }
            c => cur.push(c),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).collect()
}

/* -------------------------------------------------------------- loaders -- */

pub fn from_delimited(
    bytes: &[u8],
    forced_encoding: Option<&str>,
    forced_delim: Option<char>,
) -> AppResult<Table> {
    let (text, encoding) = decode(bytes, forced_encoding);
    let delimiter = forced_delim.unwrap_or_else(|| sniff_delimiter(&text));
    let rows: Vec<Vec<String>> = text
        .lines()
        .map(|l| split_respecting_quotes(l, delimiter))
        .collect();
    if rows.is_empty() {
        return Err(AppError::Invalid("the file is empty".into()));
    }
    Ok(Table {
        rows,
        source: Source::Delimited {
            delimiter,
            encoding,
        },
    })
}

/// Read the first non-empty worksheet of an Excel workbook.
/// The most a workbook may expand to once unzipped, and how many parts it may
/// have. A real statement is a few hundred kilobytes; a "zip bomb" is a small
/// file that inflates to gigabytes before a parser has read a single row.
const MAX_UNZIPPED: u64 = 256 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 4_096;

/// Add up what a ZIP container says it holds, from its central directory
/// alone — nothing is inflated. `None` means it is not a ZIP we can read;
/// the caller then lets the real parser say so.
fn zip_declared_size(bytes: &[u8]) -> Option<(usize, u64)> {
    let le16 = |at: usize| {
        bytes
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let le32 = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    // End-of-central-directory record: the last 22 bytes plus up to 64 KiB of comment.
    let tail_from = bytes.len().saturating_sub(22 + 65_535);
    let eocd = (tail_from..bytes.len().saturating_sub(21))
        .rev()
        .find(|&i| le32(i) == Some(0x0605_4b50))?;
    let entries = usize::from(le16(eocd + 10)?);
    let mut at = le32(eocd + 16)? as usize;
    let mut total: u64 = 0;
    for _ in 0..entries {
        if le32(at)? != 0x0201_4b50 {
            return None;
        }
        let size = le32(at + 24)?;
        // 0xFFFFFFFF means "see the zip64 record": treat as enormous.
        total = total.saturating_add(if size == u32::MAX {
            u64::MAX / 4
        } else {
            u64::from(size)
        });
        let (n, m, k) = (le16(at + 28)?, le16(at + 30)?, le16(at + 32)?);
        at += 46 + usize::from(n) + usize::from(m) + usize::from(k);
    }
    Some((entries, total))
}

pub fn from_spreadsheet(bytes: &[u8]) -> AppResult<Table> {
    use calamine::Reader;

    if bytes.starts_with(b"PK") {
        if let Some((entries, total)) = zip_declared_size(bytes) {
            if entries > MAX_ZIP_ENTRIES || total > MAX_UNZIPPED {
                return Err(AppError::Invalid(
                    "this spreadsheet expands to far more than a statement can hold".into(),
                ));
            }
        }
    }

    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut wb = calamine::open_workbook_auto_from_rs(cursor)
        .map_err(|e| AppError::Invalid(format!("cannot read spreadsheet: {e}")))?;

    // An .xlsx is read cell by cell, never into the dense grid the range API
    // builds: one cell at the far corner of a sheet would ask that grid for
    // billions of entries. Limits are enforced as the cells arrive.
    if let calamine::Sheets::Xlsx(x) = &mut wb {
        let (sheet, rows) = xlsx_sparse(x)?
            .ok_or_else(|| AppError::Invalid("the workbook has no readable sheet".into()))?;
        return Ok(Table {
            rows,
            source: Source::Spreadsheet { sheet },
        });
    }

    let sheets = wb.sheet_names().to_vec();
    let mut chosen: Option<(String, Vec<Vec<String>>)> = None;
    for name in sheets {
        let Ok(range) = wb.worksheet_range(&name) else {
            continue;
        };
        let rows: Vec<Vec<String>> = range
            .rows()
            .map(|r| r.iter().map(cell_to_string).collect())
            .collect();
        if rows.iter().any(|r| r.iter().any(|c| !c.trim().is_empty())) {
            chosen = Some((name, rows));
            break;
        }
    }
    let (sheet, rows) =
        chosen.ok_or_else(|| AppError::Invalid("the workbook has no readable sheet".into()))?;

    Ok(Table {
        rows,
        source: Source::Spreadsheet { sheet },
    })
}

/// Rows and columns a statement can have, and the most cells and grid slots
/// one may use. Real statements are a few thousand cells.
const MAX_SHEET_ROWS: u32 = 250_000;
const MAX_SHEET_COLS: u32 = 512;
const MAX_SHEET_CELLS: usize = 3_000_000;
const MAX_SHEET_SLOTS: usize = 12_000_000;

fn too_big() -> AppError {
    AppError::Invalid("this sheet holds far more than a statement can".into())
}

/// The first sheet with anything in it, read cell by cell with the limits
/// above. `Ok(None)` means every sheet was empty.
fn xlsx_sparse<RS: std::io::Read + std::io::Seek>(
    x: &mut calamine::Xlsx<RS>,
) -> AppResult<Option<(String, Vec<Vec<String>>)>> {
    use calamine::Reader;
    let names: Vec<String> = x.sheet_names();
    for name in names {
        let Ok(mut reader) = x.worksheet_cells_reader(&name) else {
            continue;
        };
        let mut cells: Vec<(u32, u32, String)> = Vec::new();
        while let Some(cell) = reader
            .next_cell()
            .map_err(|e| AppError::Invalid(format!("cannot read spreadsheet: {e}")))?
        {
            let (r, c) = cell.get_position();
            let value: calamine::Data = cell.get_value().clone().into();
            let text = cell_to_string(&value);
            if text.trim().is_empty() {
                continue;
            }
            if r >= MAX_SHEET_ROWS || c >= MAX_SHEET_COLS || cells.len() >= MAX_SHEET_CELLS {
                return Err(too_big());
            }
            cells.push((r, c, text));
        }
        if cells.is_empty() {
            continue;
        }
        // Rows and columns count from the first used cell, as the range API's do.
        let min_r = cells.iter().map(|c| c.0).min().unwrap_or(0);
        let min_c = cells.iter().map(|c| c.1).min().unwrap_or(0);
        let height = (cells.iter().map(|c| c.0).max().unwrap_or(0) - min_r + 1) as usize;
        let mut widths = vec![0usize; height];
        for (r, c, _) in &cells {
            let (r, c) = ((r - min_r) as usize, (c - min_c) as usize);
            widths[r] = widths[r].max(c + 1);
        }
        if widths.iter().sum::<usize>() > MAX_SHEET_SLOTS {
            return Err(too_big());
        }
        let mut rows: Vec<Vec<String>> = widths.iter().map(|w| vec![String::new(); *w]).collect();
        for (r, c, text) in cells {
            rows[(r - min_r) as usize][(c - min_c) as usize] = text;
        }
        // Rows are as wide as the widest, as they are in a CSV.
        let width = rows.iter().map(Vec::len).max().unwrap_or(0);
        for row in &mut rows {
            row.resize(width, String::new());
        }
        if width * rows.len() > MAX_SHEET_SLOTS {
            return Err(too_big());
        }
        return Ok(Some((name, rows)));
    }
    Ok(None)
}

/// Excel stores dates as serial numbers. Rendering one as "45678" would defeat
/// date detection entirely, so real dates are converted to ISO here and the
/// detector simply finds them already in a known format.
fn cell_to_string(c: &calamine::Data) -> String {
    // `as_date` lives on the DataType trait, not on Data itself.
    use calamine::{Data, DataType};
    match c {
        Data::Empty => String::new(),
        Data::String(s) => s.trim().to_string(),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::Error(e) => format!("#{e:?}"),
        Data::Float(f) => {
            // Keep the full precision of amounts; trim a trailing ".0".
            let s = format!("{f}");
            s.trim_end_matches(".0").to_string()
        }
        Data::DateTime(_) | Data::DateTimeIso(_) | Data::DurationIso(_) => c
            .as_date()
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| c.to_string()),
    }
}

/// Route by extension, falling back to content when the extension lies.
pub fn load(
    path: &str,
    bytes: &[u8],
    forced_encoding: Option<&str>,
    forced_delim: Option<char>,
) -> AppResult<Table> {
    let ext = std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();

    // XLSX and XLS are ZIP/OLE containers; their magic bytes are unambiguous.
    let looks_zip = bytes.starts_with(b"PK\x03\x04");
    let looks_ole = bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]);

    let mut table = match ext.as_str() {
        // A PDF is read as text in a separate process and reduced to rows;
        // from there it is a tab-separated file like any other.
        _ if ext == "pdf" || bytes.starts_with(b"%PDF") => {
            let tsv = super::pdf::to_tsv(&super::pdf::extract_text_isolated(path)?)?;
            from_delimited(tsv.as_bytes(), Some("utf-8"), Some('\t'))
        }
        "xlsx" | "xlsm" | "xls" | "ods" => super::sheet::read(bytes),
        _ if looks_zip || looks_ole => super::sheet::read(bytes),
        _ => from_delimited(bytes, forced_encoding, forced_delim),
    }?;

    // Damage from an earlier Latin-1 decode travels inside perfectly valid
    // UTF-8 — and inside spreadsheet cells — so no choice of decoder here can
    // avoid it. It is undone per cell, after decoding, whatever the source.
    for row in &mut table.rows {
        for cell in row.iter_mut() {
            if let std::borrow::Cow::Owned(fixed) = super::text::repair(cell) {
                *cell = fixed;
            }
        }
    }
    Ok(table)
}

#[cfg(test)]
mod tests;
