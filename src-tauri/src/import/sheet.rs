//! Reading a workbook outside the app's own process.
//!
//! A `.xlsx` is a zip of XML written by whoever made the file, and the library
//! that reads it builds a dense grid sized by the *largest* cell reference it
//! meets: one cell at the far corner of a sheet asks for billions of cells and
//! the allocator aborts the process. That is not an error that can be caught,
//! so — like a PDF — the workbook is read by a copy of the app started with
//! `--sheet-json`, which turns one file into rows on its standard output and
//! exits. If it crashes, hangs or runs out of memory the importer reports a
//! file it could not read and nothing else is affected.

use super::table::{self, Table};
use crate::error::{AppError, AppResult};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The flag the app is re-launched with to read one workbook from stdin.
pub const CHILD_FLAG: &str = "--sheet-json";

const TIMEOUT: Duration = Duration::from_secs(30);
/// More rows than any statement holds; past this the file is something else.
const MAX_OUTPUT: usize = 128 * 1024 * 1024;

/// What the child does: a workbook on stdin, its rows as JSON on stdout.
pub fn child_main() -> i32 {
    let mut bytes = Vec::new();
    if std::io::stdin().read_to_end(&mut bytes).is_err() {
        return 2;
    }
    match table::from_spreadsheet(&bytes) {
        Ok(t) => match serde_json::to_writer(std::io::stdout().lock(), &t) {
            Ok(()) => 0,
            Err(_) => 2,
        },
        Err(e) => {
            eprint!("{e}");
            2
        }
    }
}

/// Read a workbook through `exe`, a copy of this program.
pub fn via(exe: &Path, bytes: &[u8]) -> AppResult<Table> {
    let unreadable = |why: &str| AppError::Invalid(format!("cannot read this spreadsheet: {why}"));
    let mut child = Command::new(exe)
        .arg(CHILD_FLAG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| unreadable(&e.to_string()))?;

    // Each pipe on its own thread, so a child that fills one cannot deadlock
    // against a parent that is only waiting for it to exit.
    let mut stdin = child.stdin.take().ok_or_else(|| unreadable("no input"))?;
    let input = bytes.to_vec();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut stdout = child.stdout.take().ok_or_else(|| unreadable("no output"))?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stdout)
            .take(MAX_OUTPUT as u64 + 1)
            .read_to_end(&mut buf);
        buf
    });
    let mut stderr = child.stderr.take().ok_or_else(|| unreadable("no output"))?;
    let errors = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut stderr).take(4_096).read_to_end(&mut buf);
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
    let _ = writer.join();
    let out = reader.join().unwrap_or_default();
    let err = errors.join().unwrap_or_default();

    if !status.success() {
        // A refusal the child could put in words is passed on as it said it;
        // a crash has no words.
        let said = String::from_utf8_lossy(&err).trim().to_string();
        return Err(if status.code() == Some(2) && !said.is_empty() {
            AppError::Invalid(said)
        } else {
            unreadable("the file is damaged or far too large")
        });
    }
    if out.len() > MAX_OUTPUT {
        return Err(unreadable("it holds far more than a statement can"));
    }
    serde_json::from_slice(&out).map_err(|_| unreadable("the file is damaged"))
}

/// Whether this process is the app itself, and so safe to re-launch. Test
/// binaries and tools that embed the library read in-process instead.
fn is_the_app() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let stem = exe.file_stem()?.to_string_lossy().to_lowercase();
    (stem == "coin-heaven" || stem == "coin heaven").then_some(exe)
}

/// Read a workbook, outside this process when this process is the app.
pub fn read(bytes: &[u8]) -> AppResult<Table> {
    match is_the_app() {
        Some(exe) => via(&exe, bytes),
        None => table::from_spreadsheet(bytes),
    }
}
