//! scratch: harness + probes (to be replaced by the real suite)
use coin_heaven_lib::import::parser::{self, BankMapping, parse_amount};
use coin_heaven_lib::import::{dates, pdf, sniff, table};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

/* ----------------------------------------------------------- allocator -- */

const MAX_ONE: usize = 1 << 30;
const MAX_LIVE: usize = 3 << 30;

struct Meter;

thread_local! {
    static LIVE: Cell<isize> = const { Cell::new(0) };
    static PEAK: Cell<isize> = const { Cell::new(0) };
    static LARGEST: Cell<usize> = const { Cell::new(0) };
}
static TOTAL: AtomicUsize = AtomicUsize::new(0);

fn admit(size: usize) {
    if size > MAX_ONE || TOTAL.load(Relaxed).saturating_add(size) > MAX_LIVE {
        eprintln!(
            "import-limits: refusing an allocation of {size} bytes ({} already held)",
            TOTAL.load(Relaxed)
        );
        std::process::abort();
    }
}

fn gained(size: usize) {
    TOTAL.fetch_add(size, Relaxed);
    let _ = LIVE.try_with(|live| {
        let now = live.get() + size as isize;
        live.set(now);
        let _ = PEAK.try_with(|p| {
            if now > p.get() {
                p.set(now)
            }
        });
        let _ = LARGEST.try_with(|l| {
            if size > l.get() {
                l.set(size)
            }
        });
    });
}

fn lost(size: usize) {
    TOTAL.fetch_sub(size, Relaxed);
    let _ = LIVE.try_with(|live| live.set(live.get() - size as isize));
}

unsafe impl GlobalAlloc for Meter {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        admit(l.size());
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            gained(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        admit(l.size());
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            gained(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        lost(l.size());
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        admit(new);
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            lost(l.size());
            gained(new);
        }
        q
    }
}

#[global_allocator]
static METER: Meter = Meter;

struct Measured<T> {
    value: T,
    took: Duration,
    peak: usize,
    largest: usize,
}

fn measure<T>(f: impl FnOnce() -> T) -> Measured<T> {
    let base = LIVE.get();
    PEAK.set(base);
    LARGEST.set(0);
    let t = Instant::now();
    let value = f();
    Measured {
        value,
        took: t.elapsed(),
        peak: (PEAK.get() - base).max(0) as usize,
        largest: LARGEST.get(),
    }
}

fn mb(n: usize) -> String {
    format!("{:.1} MB", n as f64 / 1_048_576.0)
}

/* ----------------------------------------------------- child process -- */

use std::io::Read;
use std::process::{Command, Stdio};

const CHILD: &str = "COIN_HEAVEN_IMPORT_LIMITS_CHILD";

fn is_child() -> bool {
    std::env::var_os(CHILD).is_some()
}

fn run_isolated(test: &str, deadline: Duration) {
    let exe = std::env::current_exe().unwrap();
    let mut child = Command::new(exe)
        .args([
            test,
            "--exact",
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let drain = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = r.read_to_end(&mut b);
            String::from_utf8_lossy(&b).into_owned()
        })
    };
    let out = drain(Box::new(child.stdout.take().unwrap()));
    let err = drain(Box::new(child.stderr.take().unwrap()));
    let started = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if started.elapsed() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{test}: still running after {deadline:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (out, err) = (out.join().unwrap(), err.join().unwrap());
    if !status.success() {
        let all = format!("{out}\n{err}");
        let tail: String = all
            .chars()
            .rev()
            .take(3000)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        panic!(
            "{test}: child ended with {status} after {:?}\n{tail}",
            started.elapsed()
        );
    }
    eprintln!("{test}: child ok in {:?}\n{out}\n{err}", started.elapsed());
}

/* --------------------------------------------------------------- zip -- */

fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    for (i, e) in t.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *e = c;
    }
    t
}

fn crc32(data: &[u8]) -> u32 {
    let t = crc_table();
    let mut c = !0u32;
    for &b in data {
        c = t[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

fn gf2_times(mat: &[u32; 32], mut v: u32) -> u32 {
    let mut sum = 0;
    let mut i = 0;
    while v != 0 {
        if v & 1 != 0 {
            sum ^= mat[i];
        }
        v >>= 1;
        i += 1;
    }
    sum
}

fn gf2_square(sq: &mut [u32; 32], m: &[u32; 32]) {
    for n in 0..32 {
        sq[n] = gf2_times(m, m[n]);
    }
}

/// CRC-32 of `a` followed by `b`, from the two CRCs and the length of `b`.
fn crc32_combine(mut crc1: u32, crc2: u32, mut len2: u64) -> u32 {
    if len2 == 0 {
        return crc1;
    }
    let mut even = [0u32; 32];
    let mut odd = [0u32; 32];
    odd[0] = 0xEDB8_8320;
    let mut row = 1u32;
    for o in odd.iter_mut().skip(1) {
        *o = row;
        row <<= 1;
    }
    gf2_square(&mut even, &odd);
    gf2_square(&mut odd, &even);
    loop {
        gf2_square(&mut even, &odd);
        if len2 & 1 != 0 {
            crc1 = gf2_times(&even, crc1);
        }
        len2 >>= 1;
        if len2 == 0 {
            break;
        }
        gf2_square(&mut odd, &even);
        if len2 & 1 != 0 {
            crc1 = gf2_times(&odd, crc1);
        }
        len2 >>= 1;
        if len2 == 0 {
            break;
        }
    }
    crc1 ^ crc2
}

fn crc32_repeat(byte: u8, n: u64) -> u32 {
    let mut acc = 0u32;
    let mut piece = crc32(&[byte]);
    let mut len = 1u64;
    let mut n = n;
    while n > 0 {
        if n & 1 == 1 {
            acc = crc32_combine(acc, piece, len);
        }
        piece = crc32_combine(piece, piece, len);
        len *= 2;
        n >>= 1;
    }
    acc
}

#[derive(Default)]
struct BitWriter {
    out: Vec<u8>,
    acc: u32,
    n: u32,
}

impl BitWriter {
    fn bits(&mut self, value: u32, nbits: u32) {
        for i in 0..nbits {
            self.acc |= ((value >> i) & 1) << self.n;
            self.n += 1;
            if self.n == 8 {
                self.out.push(self.acc as u8);
                self.acc = 0;
                self.n = 0;
            }
        }
    }
    fn code(&mut self, code: u32, nbits: u32) {
        for i in (0..nbits).rev() {
            self.bits((code >> i) & 1, 1);
        }
    }
    fn symbol(&mut self, s: u32) {
        match s {
            0..=143 => self.code(0x30 + s, 8),
            144..=255 => self.code(0x190 + s - 144, 9),
            256..=279 => self.code(s - 256, 7),
            _ => self.code(0xC0 + s - 280, 8),
        }
    }
    fn copy(&mut self, len: u32, dist1: bool) {
        const BASE: [u32; 29] = [
            3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99,
            115, 131, 163, 195, 227, 258,
        ];
        const EXTRA: [u32; 29] = [
            0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
        ];
        assert!(dist1 && (3..=258).contains(&len));
        let i = BASE.iter().rposition(|&b| b <= len).unwrap();
        self.symbol(257 + i as u32);
        self.bits(len - BASE[i], EXTRA[i]);
        self.code(0, 5); // distance 1
    }
    fn finish(mut self) -> Vec<u8> {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
        self.out
    }
}

/// A deflate stream (one fixed-Huffman block) for `head`, then `byte`
/// repeated `count` times, then `tail`, at about 160 to 1.
fn deflate_run(head: &[u8], byte: u8, count: u64, tail: &[u8]) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.bits(1, 1);
    w.bits(1, 2);
    for &b in head {
        w.symbol(u32::from(b));
    }
    if count > 0 {
        w.symbol(u32::from(byte));
        let mut left = count - 1;
        while left >= 258 {
            w.copy(258, true);
            left -= 258;
        }
        if left >= 3 {
            w.copy(left as u32, true);
        } else {
            for _ in 0..left {
                w.symbol(u32::from(byte));
            }
        }
    }
    for &b in tail {
        w.symbol(u32::from(b));
    }
    w.symbol(256);
    w.finish()
}

enum Body {
    Stored(Vec<u8>),
    Run {
        head: Vec<u8>,
        byte: u8,
        count: u64,
        tail: Vec<u8>,
    },
}

impl Body {
    fn text(s: &str) -> Body {
        Body::Stored(s.as_bytes().to_vec())
    }
    fn run(head: &str, byte: u8, count: u64, tail: &str) -> Body {
        Body::Run {
            head: head.as_bytes().to_vec(),
            byte,
            count,
            tail: tail.as_bytes().to_vec(),
        }
    }
}

fn zip(entries: &[(&str, Body)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, body) in entries {
        let (method, crc, size, packed): (u16, u32, u64, Vec<u8>) = match body {
            Body::Stored(d) => (0, crc32(d), d.len() as u64, d.clone()),
            Body::Run {
                head,
                byte,
                count,
                tail,
            } => {
                let crc = crc32_combine(
                    crc32_combine(crc32(head), crc32_repeat(*byte, *count), *count),
                    crc32(tail),
                    tail.len() as u64,
                );
                let size = head.len() as u64 + count + tail.len() as u64;
                (8, crc, size, deflate_run(head, *byte, *count, tail))
            }
        };
        let offset = out.len() as u32;
        let common = |v: &mut Vec<u8>| {
            v.extend_from_slice(&20u16.to_le_bytes()); // needed
            v.extend_from_slice(&0u16.to_le_bytes()); // flags
            v.extend_from_slice(&method.to_le_bytes());
            v.extend_from_slice(&0u16.to_le_bytes()); // time
            v.extend_from_slice(&0x21u16.to_le_bytes()); // date
            v.extend_from_slice(&crc.to_le_bytes());
            v.extend_from_slice(&(packed.len() as u32).to_le_bytes());
            v.extend_from_slice(&(size as u32).to_le_bytes());
            v.extend_from_slice(&(name.len() as u16).to_le_bytes());
            v.extend_from_slice(&0u16.to_le_bytes()); // extra
        };
        out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
        common(&mut out);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&packed);

        central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        common(&mut central);
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

const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>"#;
const NS: &str = r#"xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main""#;

fn xlsx(sheet: Body, shared: Body) -> Vec<u8> {
    let ct = format!(
        r#"{XML}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>"#
    );
    let rels = format!(
        r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let book = format!(
        r#"{XML}<workbook {NS} xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Movimientos" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let book_rels = format!(
        r#"{XML}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"#
    );
    zip(&[
        ("[Content_Types].xml", Body::text(&ct)),
        ("_rels/.rels", Body::text(&rels)),
        ("xl/workbook.xml", Body::text(&book)),
        ("xl/_rels/workbook.xml.rels", Body::text(&book_rels)),
        ("xl/sharedStrings.xml", shared),
        ("xl/worksheets/sheet1.xml", sheet),
    ])
}

fn small_sheet() -> Body {
    Body::text(&format!(
        r#"{XML}<worksheet {NS}><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Fecha</t></is></c><c r="B1" t="inlineStr"><is><t>Concepto</t></is></c><c r="C1" t="inlineStr"><is><t>Importe</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>2026-01-05</t></is></c><c r="B2" t="inlineStr"><is><t>MERCADONA</t></is></c><c r="C2"><v>-45.2</v></c></row></sheetData></worksheet>"#
    ))
}

fn small_shared() -> Body {
    Body::text(&format!(
        r#"{XML}<sst {NS} count="1" uniqueCount="1"><si><t>x</t></si></sst>"#
    ))
}

/* -------------------------------------------------------------- probes -- */

fn csv_rows(n: usize, row: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(n * row.len() + 20);
    v.extend_from_slice(b"Fecha;Concepto;Importe\n");
    for _ in 0..n {
        v.extend_from_slice(row.as_bytes());
    }
    v
}

#[test]
fn hostile_workbook_million_rows() {
    for (label, row) in [
        ("tiny a,b", "a;b\n"),
        ("valid", "05/01/2026;X;-1,00\n"),
        ("blank-ish", ";;\n"),
        ("single char", "x\n"),
    ] {
        let bytes = csv_rows(1_000_000, row);
        let m = measure(|| {
            let t = table::load("s.csv", &bytes, None, None).unwrap();
            let d = sniff::detect(&t);
            let p = parser::parse(&t, &d.mapping);
            (t.rows.len(), p.rows.len(), p.rejected.len())
        });
        eprintln!(
            "{label}: input {} -> {:?} in {:?}, peak {} ({}x), largest {}",
            mb(bytes.len()),
            m.value,
            m.took,
            mb(m.peak),
            m.peak / bytes.len(),
            mb(m.largest)
        );
    }
}

#[test]
fn hostile_workbook_wide_row() {
    for cols in [10_000usize, 100_000, 400_000, 1_600_000] {
        let mut s = String::new();
        s.push_str(&";".repeat(cols));
        s.push('\n');
        s.push_str("Fecha;Concepto;Importe\n");
        for _ in 0..200 {
            s.push_str("05/01/2026;X;-1.00\n");
        }
        let m = measure(|| {
            let t = table::load("s.csv", s.as_bytes(), None, None).unwrap();
            let d = sniff::detect(&t);
            (t.width(), d.confidence)
        });
        eprintln!(
            "cols {cols}: {:?} in {:?}, peak {}",
            m.value,
            m.took,
            mb(m.peak)
        );
    }
}

#[test]
fn hostile_workbook_big_cell() {
    for (label, head, tail) in [
        ("description", "05/01/2026;", ";-1,00\n"),
        ("date", "", ";X;-1,00\n"),
    ] {
        let mut s = String::from("Fecha;Concepto;Importe\n05/01/2026;OK;-1,00\n");
        s.push_str(head);
        s.push_str(&"a".repeat(50 * 1024 * 1024));
        s.push_str(tail);
        let m = measure(|| {
            let t = table::load("s.csv", s.as_bytes(), None, None).unwrap();
            let d = sniff::detect(&t);
            let p = parser::parse(&t, &d.mapping);
            (
                p.rows.len(),
                p.rejected.len(),
                p.rejected
                    .iter()
                    .map(|r| r.raw.len() + r.reason.len())
                    .sum::<usize>(),
            )
        });
        eprintln!(
            "{label}: input {} -> {:?} in {:?}, peak {}",
            mb(s.len()),
            m.value,
            m.took,
            mb(m.peak)
        );
    }
}

#[test]
fn hostile_workbook_misc() {
    let _ = (
        BankMapping::default(),
        parse_amount("1", ',', ".", 2),
        dates::patterns(),
        pdf::CHILD_FLAG,
    );
}

#[test]
fn hostile_workbook_xlsx_builder_sanity() {
    let t = table::load("b.xlsx", &xlsx(small_sheet(), small_shared()), None, None).unwrap();
    eprintln!("{:?}", t.rows);
    // deflate run with valid crc: 5 MB shared string
    let sst = Body::run(
        &format!(r#"{XML}<sst {NS} count="1" uniqueCount="1"><si><t>"#),
        b'a',
        5_000_000,
        "</t></si></sst>",
    );
    let sheet = Body::text(&format!(
        r#"{XML}<worksheet {NS}><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData></worksheet>"#
    ));
    let bytes = xlsx(sheet, sst);
    eprintln!("zip size {}", bytes.len());
    let m = measure(|| table::load("b.xlsx", &bytes, None, None));
    match &m.value {
        Ok(t) => eprintln!(
            "ok rows {} cell len {} in {:?} peak {}",
            t.rows.len(),
            t.cell(0, 0).len(),
            m.took,
            mb(m.peak)
        ),
        Err(e) => eprintln!("err {e}"),
    }
}

fn probe_case(name: &str, body: impl FnOnce() -> Vec<u8>) {
    if !is_child() {
        return run_isolated(name, Duration::from_secs(120));
    }
    let bytes = body();
    eprintln!("{name}: zip {} bytes", bytes.len());
    // The way the app reads a workbook: in a copy of itself, so a file that
    // makes the reader run out of memory ends that copy and nothing else.
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_coin-heaven"));
    let m = measure(|| coin_heaven_lib::import::sheet::via(exe, &bytes));
    match &m.value {
        Ok(t) => eprintln!(
            "{name}: ok rows {} width {} in {:?} peak {} largest {}",
            t.rows.len(),
            t.width(),
            m.took,
            mb(m.peak),
            mb(m.largest)
        ),
        Err(e) => eprintln!("{name}: err {e} in {:?} peak {}", m.took, mb(m.peak)),
    }
}

#[test]
fn hostile_workbook_shared_string_bomb() {
    probe_case("hostile_workbook_shared_string_bomb", || {
        let sst = Body::run(
            &format!(r#"{XML}<sst {NS} count="1" uniqueCount="1"><si><t>"#),
            b'a',
            300_000_000,
            "</t></si></sst>",
        );
        let sheet = Body::text(&format!(
            r#"{XML}<worksheet {NS}><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c></row></sheetData></worksheet>"#
        ));
        xlsx(sheet, sst)
    });
}

#[test]
fn hostile_workbook_sheet_bomb() {
    probe_case("hostile_workbook_sheet_bomb", || {
        let sheet = Body::run(
            &format!(
                r#"{XML}<worksheet {NS}><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>"#
            ),
            b'a',
            300_000_000,
            "</t></is></c></row></sheetData></worksheet>",
        );
        xlsx(sheet, small_shared())
    });
}

#[test]
fn hostile_workbook_far_corner() {
    probe_case("hostile_workbook_far_corner", || {
        let sheet = Body::text(&format!(
            r#"{XML}<worksheet {NS}><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row><row r="1048576"><c r="XFD1048576"><v>1</v></c></row></sheetData></worksheet>"#
        ));
        xlsx(sheet, small_shared())
    });
}

#[test]
fn hostile_workbook_declared_dimension() {
    probe_case("hostile_workbook_declared_dimension", || {
        let sheet = Body::text(&format!(
            r#"{XML}<worksheet {NS}><dimension ref="A1:XFD1048576"/><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>x</t></is></c></row></sheetData></worksheet>"#
        ));
        xlsx(sheet, small_shared())
    });
}

#[test]
fn hostile_workbook_reference_amplification() {
    probe_case("hostile_workbook_reference_amplification", || {
        // one 16 MB shared string referenced from 400 cells
        let sst = Body::run(
            &format!(r#"{XML}<sst {NS} count="400" uniqueCount="1"><si><t>"#),
            b'a',
            16 * 1024 * 1024,
            "</t></si></sst>",
        );
        let mut rows = String::new();
        for r in 1..=400 {
            rows.push_str(&format!(
                r#"<row r="{r}"><c r="A{r}" t="s"><v>0</v></c></row>"#
            ));
        }
        let sheet = Body::text(&format!(
            r#"{XML}<worksheet {NS}><sheetData>{rows}</sheetData></worksheet>"#
        ));
        xlsx(sheet, sst)
    });
}

fn ods(content: &str) -> Vec<u8> {
    zip(&[
        (
            "mimetype",
            Body::text("application/vnd.oasis.opendocument.spreadsheet"),
        ),
        ("content.xml", Body::text(content)),
        (
            "META-INF/manifest.xml",
            Body::text(
                r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.2"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#,
            ),
        ),
    ])
}

const ODS_HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.2"><office:body><office:spreadsheet><table:table table:name="Hoja1">"#;
const ODS_TAIL: &str =
    "</table:table></office:spreadsheet></office:body></office:document-content>";

#[test]
fn hostile_workbook_ods_small() {
    let bytes = ods(&format!(
        r#"{ODS_HEAD}<table:table-row><table:table-cell office:value-type="string"><text:p>Fecha</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>Concepto</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell office:value-type="string"><text:p>2026-01-05</text:p></table:table-cell><table:table-cell office:value-type="string"><text:p>X</text:p></table:table-cell></table:table-row>{ODS_TAIL}"#
    ));
    let t = table::load("b.ods", &bytes, None, None);
    eprintln!("{t:?}");
}

#[test]
fn hostile_workbook_ods_repeat_bomb() {
    probe_case_named("hostile_workbook_ods_repeat_bomb", "b.ods", || {
        ods(&format!(
            r#"{ODS_HEAD}<table:table-row table:number-rows-repeated="1000000000"><table:table-cell table:number-columns-repeated="1000000000" office:value-type="string"><text:p>x</text:p></table:table-cell></table:table-row>{ODS_TAIL}"#
        ))
    });
}

#[test]
fn hostile_workbook_ods_repeat_bomb_empty() {
    probe_case_named("hostile_workbook_ods_repeat_bomb_empty", "b.ods", || {
        ods(&format!(
            r#"{ODS_HEAD}<table:table-row><table:table-cell office:value-type="string"><text:p>x</text:p></table:table-cell></table:table-row><table:table-row table:number-rows-repeated="1000000000"><table:table-cell table:number-columns-repeated="1000000000"/></table:table-row><table:table-row><table:table-cell office:value-type="string"><text:p>y</text:p></table:table-cell></table:table-row>{ODS_TAIL}"#
        ))
    });
}

fn probe_case_named(name: &str, file: &str, body: impl FnOnce() -> Vec<u8>) {
    if !is_child() {
        return run_isolated(name, Duration::from_secs(120));
    }
    let bytes = body();
    eprintln!("{name}: file {} bytes", bytes.len());
    let m = measure(|| table::load(file, &bytes, None, None));
    match &m.value {
        Ok(t) => eprintln!(
            "{name}: ok rows {} width {} in {:?} peak {} largest {}",
            t.rows.len(),
            t.width(),
            m.took,
            mb(m.peak),
            mb(m.largest)
        ),
        Err(e) => eprintln!("{name}: err {e} in {:?} peak {}", m.took, mb(m.peak)),
    }
}
