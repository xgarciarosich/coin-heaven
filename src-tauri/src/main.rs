// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Re-launched by the importer to read one PDF outside the app's own
    // process: see `import::pdf`. It prints the text and exits; it never
    // opens a window or the database.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some(coin_heaven_lib::import::pdf::CHILD_FLAG) {
        let code = args
            .get(2)
            .map_or(2, |path| coin_heaven_lib::import::pdf::child_main(path));
        std::process::exit(code);
    }
    // The same idea for a workbook: bytes on stdin, rows on stdout. See
    // `import::sheet`.
    if args.get(1).map(String::as_str) == Some(coin_heaven_lib::import::sheet::CHILD_FLAG) {
        std::process::exit(coin_heaven_lib::import::sheet::child_main());
    }
    coin_heaven_lib::run()
}
