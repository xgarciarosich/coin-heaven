# Fuzzing

Targets for everything that reads bytes from outside: statements, price
replies from a model, web search results, market data, update responses and the
text helpers behind them. Each target asserts real properties (a parsed amount
is never `i64::MIN`, detection points at columns that exist, parsing accounts
for every line), not only "does not crash".

## Run

On the stable toolchain the targets build as plain libFuzzer programs. They
mutate blindly (no coverage feedback), which is still enough to find the
crashes that matter in parsers:

```bash
cd src-tauri/fuzz
cargo build --release --bins
./target/release/table_load -max_total_time=60 -rss_limit_mb=2048
```

A crash is written to `artifacts/<target>/`; replay it with
`./target/release/<target> artifacts/<target>/crash-…`.

Seed inputs can go in `corpus/<target>/` (git-ignored). Good seeds are the
files in `../tests/fixtures` and `../../samples`.

## Coverage-guided (optional)

`cargo +nightly fuzz run <target>` is the usual route. The Tauri build script
does not link under the sanitizer flags `cargo-fuzz` sets on macOS at the time
of writing, so use the blind route above unless that has been resolved.

Workbooks are not fuzzed in-process in the app at all: they are read by a
separate copy of the program (`import::sheet`), because the reader can abort on
a hostile file. `xlsx_load` exercises the reader directly, so run it with the
expectation that it can abort; `tests/security_import_limits.rs` covers the
isolated path.
