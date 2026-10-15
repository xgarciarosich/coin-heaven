//! Bank statements that only come as a PDF.
//!
//! A statement PDF is a picture of a table, not a table: the file holds text
//! at positions on a page and nothing says which numbers belong to which
//! line. So this is a reader of last resort. It takes the text a PDF yields,
//! keeps the lines that start with a date and end in an amount, and hands
//! those to the same importer every other format goes through — where the
//! person still sees a preview and confirms the columns before anything is
//! stored. A scanned statement (an image, with no text in it) cannot be read
//! at all, and says so.
//!
//! **The parser never runs inside the app.** A PDF is the most complicated
//! file this program is ever handed, and it comes from outside. The library
//! that reads it is memory-safe Rust and executes nothing in the file — no
//! scripts, no embedded programs, no links followed — but a malformed file
//! can still make it panic or spin. So the app starts a copy of itself with
//! `--pdf-text`, which does nothing but turn one file into text on its
//! standard output and exit. If that process crashes or hangs, the app
//! reports a file it could not read; nothing else is affected, and the child
//! never opens the database.

use crate::error::{AppError, AppResult};
use once_cell::sync::Lazy;
use regex::Regex;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The flag the app is re-launched with to read one PDF.
pub const CHILD_FLAG: &str = "--pdf-text";

/// How long a statement may take to read before it is abandoned.
const TIMEOUT: Duration = Duration::from_secs(30);

/// More text than any statement holds; past this the file is something else.
const MAX_TEXT: usize = 8 * 1024 * 1024;

/// What the child process does: one file in, its text out, then exit.
/// Returns the process exit code.
pub fn child_main(path: &str) -> i32 {
    match pdf_extract::extract_text(path) {
        Ok(text) => {
            print!("{text}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            2
        }
    }
}

/// Read a PDF's text in a separate process. See the module notes for why.
pub fn extract_text_isolated(path: &str) -> AppResult<String> {
    let unreadable = |why: &str| AppError::Invalid(format!("cannot read this PDF: {why}"));
    let exe = std::env::current_exe().map_err(|e| unreadable(&e.to_string()))?;
    let mut child = Command::new(exe)
        .arg(CHILD_FLAG)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| unreadable(&e.to_string()))?;

    // Read on another thread so a child that fills the pipe cannot deadlock
    // against a parent that is only waiting for it to exit.
    let mut out = child.stdout.take().ok_or_else(|| unreadable("no output"))?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.by_ref().take(MAX_TEXT as u64).read_to_end(&mut buf);
        buf
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(unreadable("it took too long to read"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(e) => return Err(unreadable(&e.to_string())),
        }
    };
    let bytes = reader.join().unwrap_or_default();
    if !status.success() {
        return Err(unreadable("the file is damaged or protected"));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

static DATE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)^\s*(
            \d{4}-\d{2}-\d{2}
          | \d{1,2}[/.\-]\d{1,2}[/.\-]\d{2,4}
          | \d{1,2}\s+[[:alpha:]]{3,10}\.?\s+\d{4}
          | \d{1,2}[/.\-]\d{1,2}
        )(?:\s|$)",
    )
    .expect("date pattern")
});

/// An amount as statements print them: digits in thousands groups or not,
/// exactly two decimals, a sign in front or trailing, perhaps a currency.
static AMOUNT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?x)
        (?P<sign>[-+−])?\s?
        (?P<num>\d{1,3}(?:[.,\x20]\d{3})+[.,]\d{2}|\d+[.,]\d{2})
        (?P<trail>-)?
        (?:\s?(?:€|EUR|eur))?",
    )
    .expect("amount pattern")
});

/// Lines that sit between rows without belonging to one: page numbers,
/// carried-forward balances, repeated headers.
static FURNITURE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?ix)
          ^(p[áa]g(ina)?\.?|page|hoja|saldo|balance|total|iban|contin[úu]a|continued|fecha|date)\b
        | \d+\s*(de|of|/)\s*\d+\s*$",
    )
    .expect("furniture pattern")
});

fn amount_text(c: &regex::Captures) -> String {
    let negative =
        c.name("trail").is_some() || matches!(c.name("sign").map(|m| m.as_str()), Some("-" | "−"));
    // A space as thousands separator is dropped; the rest is left as printed
    // for the importer's own number detection.
    let num = c["num"].replace(' ', "");
    if negative { format!("-{num}") } else { num }
}

/// Turn a statement's text into rows of date, description, amount, balance.
///
/// A row starts at a line that opens with a date. Its last amount is the
/// balance when there are two or more, and the one before it the movement;
/// with only one, that is the movement. The description is what lies between.
/// A following line with no date and no amount is the rest of a description
/// that wrapped.
pub fn rows_from_text(text: &str) -> Vec<[String; 4]> {
    let mut rows: Vec<[String; 4]> = Vec::new();
    // Whether the previous line was a row that may still wrap onto this one.
    let mut open = false;

    for line in text.lines() {
        let line = line.trim_end();
        let Some(d) = DATE.captures(line) else {
            let extra = line.trim();
            if open
                && !extra.is_empty()
                && extra.len() <= 80
                && !AMOUNT.is_match(extra)
                && !FURNITURE.is_match(extra)
            {
                if let Some(last) = rows.last_mut() {
                    last[1] = format!("{} {}", last[1], extra).trim().to_string();
                }
            }
            open = false;
            continue;
        };
        let date = d[1].to_string();
        let mut rest = &line[d.get(0).map_or(0, |m| m.end())..];
        // Many statements print a value date straight after the booking date.
        if let Some(second) = DATE.captures(rest) {
            rest = &rest[second.get(0).map_or(0, |m| m.end())..];
        }
        let found: Vec<regex::Captures> = AMOUNT.captures_iter(rest).collect();
        let Some(first) = found.first() else {
            open = false;
            continue;
        };
        let description = rest[..first.get(0).map_or(0, |m| m.start())]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if description.is_empty() {
            open = false;
            continue;
        }
        let (amount, balance) = match found.len() {
            1 => (amount_text(&found[0]), String::new()),
            n => (amount_text(&found[n - 2]), amount_text(&found[n - 1])),
        };
        rows.push([date, description, amount, balance]);
        open = true;
    }
    rows
}

/// The rows as tab-separated text with a header, ready for the importer.
pub fn to_tsv(text: &str) -> AppResult<String> {
    let rows = rows_from_text(text);
    if rows.len() < 2 {
        return Err(AppError::Invalid(
            "no transactions could be read from this PDF. If it is a scan, it holds a picture \
             rather than text; download the statement as CSV or Excel instead"
                .into(),
        ));
    }
    let mut out = String::from("Date\tDescription\tAmount\tBalance\n");
    for r in rows {
        out.push_str(&r.map(|c| c.replace('\t', " ")).join("\t"));
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATEMENT: &str = "\
BANCO EJEMPLO S.A.                       Extracto de cuenta
Titular: ****                            IBAN ES** **** 1234

Fecha      F. valor   Concepto                              Importe      Saldo
02/01/2026 02/01/2026 COMPRA TARJ. MERCADONA VALENCIA        -45,30   2.954,70
03/01/2026 03/01/2026 RECIBO IBERDROLA CLIENTES SAU          -71,80   2.882,90
                      REF 000123
05/01/2026 05/01/2026 NOMINA ACME IBERIA SL               2.976,00   5.858,90
07/01/2026 07/01/2026 BIZUM DE MARIA                          20,00   5.878,90
Página 1 de 2
Saldo anterior                                                        3.000,00
";

    #[test]
    fn a_statement_becomes_rows_of_date_description_amount_balance() {
        let rows = rows_from_text(STATEMENT);
        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows[0],
            [
                "02/01/2026".to_string(),
                "COMPRA TARJ. MERCADONA VALENCIA".into(),
                "-45,30".into(),
                "2.954,70".into()
            ]
        );
        assert_eq!(rows[2][2], "2.976,00");
        assert_eq!(rows[2][3], "5.858,90");
    }

    #[test]
    fn a_description_that_wraps_is_joined_and_page_furniture_is_not() {
        let rows = rows_from_text(STATEMENT);
        assert_eq!(rows[1][1], "RECIBO IBERDROLA CLIENTES SAU REF 000123");
        assert_eq!(
            rows[3][1], "BIZUM DE MARIA",
            "the page footer is not a description"
        );
    }

    #[test]
    fn signs_are_read_wherever_the_bank_prints_them() {
        let text = "\
2026-03-01 Card payment coffee shop 4.50- 1,200.00
2026-03-02 Salary +2,500.00 EUR 3,700.00
2026-03-03 Transfer out −300.00
15 mar 2026 Alquiler 1 250,00- 2 150,00
";
        let rows = rows_from_text(text);
        assert_eq!(rows[0][2], "-4.50");
        assert_eq!(rows[1][2], "2,500.00");
        assert_eq!(rows[2][2], "-300.00");
        assert_eq!(rows[2][3], "", "one amount is the movement, not a balance");
        assert_eq!(rows[3][0], "15 mar 2026");
        assert_eq!(rows[3][2], "-1250,00");
        assert_eq!(rows[3][3], "2150,00");
    }

    #[test]
    fn text_with_no_transactions_is_an_error_not_an_empty_import() {
        assert!(to_tsv("Nothing here but a letter from the bank.").is_err());
        let tsv = to_tsv(STATEMENT).unwrap();
        assert!(tsv.starts_with("Date\tDescription\tAmount\tBalance\n"));
        assert_eq!(tsv.lines().count(), 5);
    }
}
