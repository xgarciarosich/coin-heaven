//! What the fuzz targets have in common. Nothing here is under test.
//!
//! - [`Input`] splits a fuzz input into a few option bytes and the payload, so
//!   a seed file is `printf '\x00\x00\x00'` followed by a real statement.
//! - [`file_case`] is the shared "a file arrives" step of the import targets.
//! - [`xlsx`] wraps hand-written sheet XML in a valid workbook, so the fuzzer
//!   mutates the XML instead of fighting the ZIP checksums.
//! - [`Guarded`] turns a memory bomb into a plain crash instead of a frozen
//!   laptop.
//! - [`known_findings`] decides whether defects that are already reported are
//!   fuzzed again.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};

use coin_heaven_lib::import::dates;
use coin_heaven_lib::import::parser::{self, BankMapping, ParseResult};
use coin_heaven_lib::import::sniff::Detection;
use coin_heaven_lib::import::table::{Source, Table};

/* ------------------------------------------------------------ known -- */

/// Whether to fuzz past defects that are already written up.
///
/// A target that crashes on a known defect cannot look for the next one, so
/// the harness skips just that shape of input unless
/// `COIN_HEAVEN_FUZZ_KNOWN=1` is set. It is the fuzzing counterpart of
/// `cargo test -- --ignored`: every skip names the finding it belongs to, and
/// the input that reproduces it is kept in `fuzz/regressions/<target>/`.
pub fn known_findings() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("COIN_HEAVEN_FUZZ_KNOWN").is_some_and(|v| v != "0"))
}

/* ------------------------------------------------------------ input -- */

/// A fuzz input read from the front: option bytes, then NUL-terminated fields,
/// then whatever is left.
pub struct Input<'a> {
    rest: &'a [u8],
}

impl<'a> Input<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Input { rest: data }
    }

    /// The next option byte; 0 once the input has run out.
    pub fn byte(&mut self) -> u8 {
        match self.rest.split_first() {
            Some((b, rest)) => {
                self.rest = rest;
                *b
            }
            None => 0,
        }
    }

    /// One of `options`, chosen by the next byte.
    pub fn pick<T: Copy>(&mut self, options: &[T]) -> T {
        options[self.byte() as usize % options.len()]
    }

    /// The bytes up to the next NUL (which is consumed), or all that is left.
    pub fn field(&mut self) -> &'a [u8] {
        match self.rest.iter().position(|&b| b == 0) {
            Some(n) => {
                let (head, tail) = self.rest.split_at(n);
                self.rest = &tail[1..];
                head
            }
            None => std::mem::take(&mut self.rest),
        }
    }

    /// [`Input::field`] as text, with invalid UTF-8 replaced.
    pub fn text(&mut self) -> String {
        String::from_utf8_lossy(self.field()).into_owned()
    }

    /// Everything not yet read.
    pub fn rest(&self) -> &'a [u8] {
        self.rest
    }
}

/* ------------------------------------------------------------ files -- */

const NAMES: [&str; 12] = [
    "statement.csv",
    "statement.CSV",
    "statement.tsv",
    "statement.txt",
    "statement",
    "statement.xlsx",
    "statement.XLSX",
    "statement.xls",
    "statement.xlsm",
    "statement.ods",
    "statement.csv.xlsx",
    ".xlsx",
];

const ENCODINGS: [Option<&str>; 9] = [
    None,
    Some("auto"),
    Some(""),
    Some("utf-8"),
    Some("windows-1252"),
    Some("latin1"),
    Some("utf-16le"),
    Some("utf-16be"),
    Some("klingon"),
];

const DELIMITERS: [Option<char>; 9] = [
    None,
    Some(';'),
    Some(','),
    Some('\t'),
    Some('|'),
    Some('"'),
    Some('é'),
    Some('\n'),
    Some('\0'),
];

/// What `import::table::load` is called with: a name, the bytes, and the two
/// options the import screen can force.
pub struct FileCase<'a> {
    pub name: String,
    pub encoding: Option<&'static str>,
    pub delimiter: Option<char>,
    pub bytes: &'a [u8],
}

/// Read a file case from a fuzz input: three option bytes, then the file.
///
/// The first byte picks a file name from a list, or — from 128 up — takes the
/// name from the front of the payload, so any extension can turn up. `None`
/// for a `.pdf`, or for anything starting `%PDF`: `table::load` reads those
/// through a child process (`current_exe --pdf-text`), which for a fuzz binary
/// would start a second fuzzer. The text a PDF yields is fuzzed directly by
/// `pdf_rows`.
pub fn file_case(data: &[u8]) -> Option<FileCase<'_>> {
    let mut inp = Input::new(data);
    let name_byte = inp.byte();
    let encoding = inp.pick(&ENCODINGS);
    let delimiter = inp.pick(&DELIMITERS);
    let name = if (name_byte as usize) < 128 {
        NAMES[name_byte as usize % NAMES.len()].to_string()
    } else {
        inp.text()
    };
    let bytes = inp.rest();
    let ext = std::path::Path::new(&name)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    if ext.as_deref() == Some("pdf") || bytes.starts_with(b"%PDF") {
        return None;
    }
    Some(FileCase {
        name,
        encoding,
        delimiter,
        bytes,
    })
}

/// What holds of every table the loader hands back, whatever it was built from.
pub fn check_table(t: &Table, input_len: usize) {
    assert!(
        !t.rows.is_empty(),
        "a loaded table is never empty: an empty file is an error"
    );
    let width = t.width();
    for (i, row) in t.rows.iter().enumerate() {
        assert!(row.len() <= width);
        for (j, cell) in row.iter().enumerate() {
            assert_eq!(t.cell(i, j), cell, "cell({i},{j})");
        }
    }
    assert_eq!(t.cell(t.rows.len(), 0), "", "past the last row is empty");
    assert_eq!(t.cell(0, width), "", "past the last column is empty");

    // `data_rows` keeps the real row index, and only yields rows with content.
    for (i, row) in t.data_rows() {
        assert!(std::ptr::eq(row, &t.rows[i]));
        assert!(row.iter().any(|c| !c.trim().is_empty()));
    }

    if let Source::Delimited { delimiter, encoding } = &t.source {
        assert!(
            ["utf-8", "windows-1252", "utf-16le", "utf-16be"].contains(&encoding.as_str()),
            "unexpected encoding label {encoding:?}"
        );
        // A delimited file cannot grow: a cell costs at least one delimiter or
        // one line end of the decoded text, which is at most 3 bytes a byte.
        let cells: usize = t.rows.iter().map(Vec::len).sum();
        assert!(
            cells <= 3 * input_len + 2,
            "{cells} cells from {input_len} bytes, delimiter {delimiter:?}"
        );
    }
}

/* ---------------------------------------------------------- mapping -- */

const DATE_ODDITIES: [&str; 6] = ["", "%", "%Y-%m-%d %Z", "%s", "%d/%m/%Y %H", "no percent"];
const THOUSANDS: [&str; 8] = [".", ",", "", " ", "'", "ab", "..", "\u{a0}"];
const CURRENCIES: [&str; 6] = ["EUR", "JPY", "KWD", "USD", "XXX", ""];
/// A decimal separator the person can type into the import screen's one-letter
/// box. The multi-byte ones are known to panic `parse_amount`; see
/// [`known_findings`].
const DECIMALS: [char; 7] = [',', '.', '٫', '·', ';', '5', '-'];

/// A bank mapping read from ten option bytes.
///
/// `has_header`, `invert_sign` and the optional columns come from the first
/// byte; the rest pick a column, a separator, a currency and a date format
/// (a catalogue entry, or — from 200 up — one of a few malformed patterns).
pub fn mapping(inp: &mut Input) -> BankMapping {
    let flags = inp.byte();
    let skip_rows = inp.byte() as usize % 5;
    let date_col = inp.byte() as usize % 6;
    let description_col = inp.byte() as usize % 6;
    let amount_col = inp.byte() as usize % 7;
    let counterparty_col = inp.byte() as usize % 7;
    let decimal = inp.byte() as usize;
    let thousands = inp.pick(&THOUSANDS);
    let currency = inp.pick(&CURRENCIES);
    let fmt = inp.byte() as usize;

    let patterns = dates::patterns();
    let date_format = if fmt >= 200 {
        DATE_ODDITIES[fmt % DATE_ODDITIES.len()].to_string()
    } else {
        patterns[fmt % patterns.len()].to_string()
    };
    // Without COIN_HEAVEN_FUZZ_KNOWN the separator is one of the ASCII ones.
    let decimal_pool: &[char] = if known_findings() {
        &DECIMALS
    } else {
        &[',', '.', ';', '5', '-']
    };
    BankMapping {
        name: "fuzz".into(),
        delimiter: ';',
        encoding: "utf-8".into(),
        has_header: flags & 1 != 0,
        skip_rows,
        date_col,
        date_format,
        description_col,
        amount_col: (amount_col < 6).then_some(amount_col),
        counterparty_col: (counterparty_col < 6 && flags & 2 != 0).then_some(counterparty_col),
        decimal_separator: decimal_pool[decimal % decimal_pool.len()],
        thousands_separator: thousands.to_string(),
        invert_sign: flags & 4 != 0,
        currency: currency.to_string(),
    }
}

/// A grid read straight from text: lines, then tabs. No quoting, no decoding,
/// so the parser sees exactly the cells the fuzzer wrote.
pub fn grid(text: &str) -> Table {
    let rows: Vec<Vec<String>> = text
        .lines()
        .map(|l| l.split('\t').map(|c| c.trim().to_string()).collect())
        .collect();
    Table {
        rows,
        source: Source::Delimited {
            delimiter: '\t',
            encoding: "utf-8".into(),
        },
    }
}

/// What holds of every [`parser::parse`] result, for any table and mapping.
///
/// The one that matters most is the first: a row is never dropped silently.
/// A year of statements must not lose a line without it showing up in the
/// rejected list.
pub fn check_parse(t: &Table, m: &BankMapping, r: &ParseResult) {
    let skip = m.skip_rows + usize::from(m.has_header);
    let blank = |row: &Vec<String>| row.iter().all(|c| c.trim().is_empty());

    let expected = t.rows.iter().skip(skip).filter(|r| !blank(r)).count();
    assert_eq!(
        r.rows.len() + r.rejected.len(),
        expected,
        "every non-blank row after the header is either read or rejected"
    );

    let mut lines: Vec<usize> = r
        .rows
        .iter()
        .map(|p| p.line)
        .chain(r.rejected.iter().map(|j| j.line))
        .collect();
    lines.sort_unstable();
    for w in lines.windows(2) {
        assert!(w[0] < w[1], "line {} appears twice", w[0]);
    }
    for &l in &lines {
        assert!(l > skip && l <= t.rows.len(), "line {l} outside the data");
        assert!(!blank(&t.rows[l - 1]), "a blank line {l} was reported");
    }

    let exponent = coin_heaven_lib::currency::lookup(&m.currency).map_or(2, |c| u32::from(c.exponent));
    for p in &r.rows {
        let day = chrono::NaiveDate::parse_from_str(&p.booked_on, "%Y-%m-%d");
        assert!(day.is_ok(), "booked_on {:?} is not a date", p.booked_on);
        assert!(!p.description.is_empty() && p.description == p.description.trim());
        // The amount is what `parse_amount` makes of the cell, signed once.
        let cell = t.cell(p.line - 1, m.amount_col.expect("a row was read without an amount column"));
        let mut want = parser::parse_amount(cell.trim(), m.decimal_separator, &m.thousands_separator, exponent)
            .expect("the amount parsed once and not again");
        if m.invert_sign {
            want = -want;
        }
        assert_eq!(p.amount_minor, want, "line {}", p.line);
        assert!(p.counterparty.as_deref().is_none_or(|c| !c.is_empty()));
    }
    for j in &r.rejected {
        assert_eq!(j.raw, t.rows[j.line - 1].join(" | "), "line {}", j.line);
        assert!(!j.reason.is_empty());
    }

    if m.has_header && m.skip_rows < t.rows.len() {
        let want: Vec<String> = t.rows[m.skip_rows].iter().map(|c| c.trim().to_string()).collect();
        assert_eq!(r.headers, want);
    } else {
        assert!(r.headers.is_empty());
    }
}

/// What holds of every [`sniff::detect`] result, for any loaded table.
pub fn check_detection(t: &Table, d: &Detection) {
    let width = t.width().max(1);
    let m = &d.mapping;
    assert!(d.confidence <= 100);
    assert!(m.date_col < width, "date column {} of {width}", m.date_col);
    assert!(m.description_col < width, "description column {} of {width}", m.description_col);
    if let Some(a) = m.amount_col {
        assert!(a < width, "amount column {a} of {width}");
        assert_ne!(a, m.date_col, "the date column cannot also be the amount");
    }
    assert!(d.first_data_row < t.rows.len().max(1));
    if let Some(h) = d.header_row {
        assert!(h < t.rows.len(), "header row {h} of {}", t.rows.len());
        assert!(h < d.first_data_row || t.data_rows().next().is_none());
    }
    assert_eq!(m.has_header, d.header_row.is_some());
    assert_eq!(m.skip_rows, d.header_row.unwrap_or(d.first_data_row));
    assert!(
        dates::is_known(&m.date_format),
        "date format {:?} is not in the catalogue",
        m.date_format
    );
    assert!(matches!(m.decimal_separator, ',' | '.'));
    assert_ne!(m.thousands_separator, m.decimal_separator.to_string());
    assert!(!m.name.is_empty());
    assert_eq!(m.currency, coin_heaven_lib::currency::DEFAULT);
    match &t.source {
        Source::Delimited { delimiter, encoding } => {
            assert_eq!((m.delimiter, &m.encoding), (*delimiter, encoding));
        }
        Source::Spreadsheet { .. } => {}
    }
}

/// A JSON document from bytes, or `None` for anything that is not one.
pub fn json(data: &[u8]) -> Option<serde_json::Value> {
    serde_json::from_slice(data).ok()
}

/// Percent-decoding as a browser would, used to check `market::encode`.
pub fn percent_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    Some(out)
}

/* ---------------------------------------------------------- masking -- */

/// What a masked text still gives away.
#[derive(Debug, PartialEq, Eq)]
pub enum Leak {
    /// An e-mail address, in the shape the mask promises to remove.
    Email(String),
    /// A number of eight digits or more standing on its own.
    Number(String),
    /// A card-length run of digits with a letter against it. Only looked for
    /// with [`known_findings`]: `\b` finds no boundary between `TARJ` and
    /// `4539…`, so such a number is not masked.
    Glued(String),
}

/// The first thing a masked text still gives away, if anything.
///
/// The patterns are written out again here on purpose: they are what a person
/// reading "e-mail addresses and long numbers are removed" would expect, so
/// they are the contract and not a copy to keep in step.
pub fn leak(text: &str) -> Option<Leak> {
    use regex::Regex;
    static EMAIL: OnceLock<Regex> = OnceLock::new();
    static NUMBER: OnceLock<Regex> = OnceLock::new();
    static GLUED: OnceLock<Regex> = OnceLock::new();
    let email = EMAIL.get_or_init(|| {
        Regex::new(r"(?i)\b[a-z0-9._%+-]+@[a-z0-9.-]+\.[a-z]{2,}\b").expect("email pattern")
    });
    let number = NUMBER.get_or_init(|| Regex::new(r"\b\d{8,}\b").expect("number pattern"));
    let glued = GLUED.get_or_init(|| Regex::new(r"[0-9]{13,}").expect("glued pattern"));

    if let Some(m) = email.find(text) {
        return Some(Leak::Email(m.as_str().into()));
    }
    if let Some(m) = number.find(text) {
        return Some(Leak::Number(m.as_str().into()));
    }
    if known_findings() {
        if let Some(m) = glued.find(text) {
            return Some(Leak::Glued(m.as_str().into()));
        }
    }
    None
}

/* ------------------------------------------------------------- xlsx -- */

/// A workbook built around sheet XML the fuzzer controls.
pub mod xlsx {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        !crc
    }

    /// A ZIP with every entry stored uncompressed and a correct checksum.
    pub fn stored_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let offset = out.len() as u32;
            let crc = crc32(data);
            let head = |v: &mut Vec<u8>, central: bool| {
                v.extend_from_slice(if central {
                    &[0x50, 0x4b, 0x01, 0x02]
                } else {
                    &[0x50, 0x4b, 0x03, 0x04]
                });
                if central {
                    v.extend_from_slice(&20u16.to_le_bytes()); // made by
                }
                v.extend_from_slice(&20u16.to_le_bytes()); // needed
                v.extend_from_slice(&0u16.to_le_bytes()); // flags
                v.extend_from_slice(&0u16.to_le_bytes()); // stored
                v.extend_from_slice(&0u16.to_le_bytes()); // time
                v.extend_from_slice(&0x21u16.to_le_bytes()); // date
                v.extend_from_slice(&crc.to_le_bytes());
                v.extend_from_slice(&(data.len() as u32).to_le_bytes());
                v.extend_from_slice(&(data.len() as u32).to_le_bytes());
                v.extend_from_slice(&(name.len() as u16).to_le_bytes());
                v.extend_from_slice(&0u16.to_le_bytes()); // extra
            };
            head(&mut out, false);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);

            head(&mut central, true);
            central.extend_from_slice(&0u16.to_le_bytes()); // comment
            central.extend_from_slice(&0u16.to_le_bytes()); // disk
            central.extend_from_slice(&0u16.to_le_bytes()); // internal
            central.extend_from_slice(&0u32.to_le_bytes()); // external
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    const CONTENT_TYPES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>"#;
    const ROOT_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;
    const WORKBOOK: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Movimientos" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    const WORKBOOK_RELS: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#;
    const STYLES: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="dd/mm/yyyy"/></numFmts><fonts count="1"><font><sz val="11"/></font></fonts><fills count="1"><fill><patternFill patternType="none"/></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="2"><xf/><xf numFmtId="164" applyNumberFormat="1"/></cellXfs></styleSheet>"#;
    const SHARED: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="3" uniqueCount="3"><si><t>Fecha</t></si><si><t>Concepto</t></si><si><t>Importe</t></si></sst>"#;
    pub const SHEET: &[u8] = br#"<?xml version="1.0" encoding="UTF-8"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c></row><row r="2"><c r="A2" s="1"><v>46027</v></c><c r="B2" t="inlineStr"><is><t>MERCADONA</t></is></c><c r="C2"><v>-45.2</v></c></row></sheetData></worksheet>"#;

    /// A workbook with one sheet. An empty part is replaced by a plain one, so
    /// the fuzzer can change one part and leave the rest valid.
    pub fn workbook(sheet: &[u8], shared: &[u8], styles: &[u8], book: &[u8]) -> Vec<u8> {
        let or = |part: &'static [u8], given: &'_ [u8]| -> Vec<u8> {
            if given.is_empty() {
                part.to_vec()
            } else {
                given.to_vec()
            }
        };
        let (sheet, shared, styles, book) = (
            or(SHEET, sheet),
            or(SHARED, shared),
            or(STYLES, styles),
            or(WORKBOOK, book),
        );
        stored_zip(&[
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("xl/workbook.xml", &book),
            ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
            ("xl/styles.xml", &styles),
            ("xl/sharedStrings.xml", &shared),
            ("xl/worksheets/sheet1.xml", &sheet),
        ])
    }
}

/* -------------------------------------------------------- allocator -- */

/// One allocation above this is a memory bomb, not a bank statement.
const MAX_ONE: usize = 256 << 20;
/// So is a gigabyte held at once.
const MAX_LIVE: usize = 1 << 30;

/// The system allocator, aborting on a runaway request.
///
/// A small file that makes the program ask for hundreds of gigabytes is a real
/// defect — the app would be killed — but left alone it also takes the
/// machine the fuzzer runs on to the edge of swap before libFuzzer's own
/// limit notices. Aborting here is a crash libFuzzer saves like any other, and
/// the message says what was asked for.
pub struct Guarded;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static ABORTING: AtomicBool = AtomicBool::new(false);

/// `size` is the request, `growth` how much more is held once it is granted.
fn admit(size: usize, growth: usize) {
    let live = LIVE.load(Relaxed);
    if size > MAX_ONE || live.saturating_add(growth) > MAX_LIVE {
        if !ABORTING.swap(true, Relaxed) {
            eprintln!(
                "coin-heaven-fuzz: refusing an allocation of {size} bytes ({live} already held)"
            );
        }
        std::process::abort();
    }
}

unsafe impl GlobalAlloc for Guarded {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        admit(layout.size(), layout.size());
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            LIVE.fetch_add(layout.size(), Relaxed);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        admit(layout.size(), layout.size());
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            LIVE.fetch_add(layout.size(), Relaxed);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        admit(new_size, new_size.saturating_sub(layout.size()));
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            LIVE.fetch_add(new_size, Relaxed);
            LIVE.fetch_sub(layout.size(), Relaxed);
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Guarded = Guarded;
