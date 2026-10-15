# 11. Bank formats are detected, not configured

**Status:** accepted · replaces the spec's manual column presets

## Context

The original specification asked for "saving column index presets per bank
format", and that is what was built: the user picked a file and was then shown
a grid of fourteen fields — column indices, date format, decimal and thousands
separators, encoding, skip rows — and had to fill them in correctly before
anything would parse.

It works, and it is unusable. Someone who wants to see where their money went
should not have to describe their bank to a computer, and every one of those
fields is derivable from the file itself.

## Decision

Reading a file produces a format. `import_inspect` loads it, detects the layout
and returns both the mapping and a parsed preview in one call.

Detection is **local and deterministic**:

- **Delimiter** by field-count *consistency*, not frequency. A European file
  using `;` contains a comma in every amount, so counting occurrences picks the
  wrong one; what marks the real delimiter is the same field count line after
  line.
- **Encoding** from the BOM, then UTF-8 validity, falling back to Windows-1252
  — a bank file that is not valid UTF-8 is far more likely to be Latin-1 than
  corrupt.
- **Data start** at the first row carrying both a parseable date and a number;
  everything above it is letterhead. The row above that, if it has no date, is
  the header.
- **Columns** by classifying every cell over up to 200 rows, weighted by header
  words in Spanish, English, German and Italian.
- **Decimal mark** from where the last separator sits relative to the trailing
  digits: `1.234` is a thousand, `1,23` is one and a bit.

Where a real ambiguity exists it is reported rather than hidden. If every day
in a file is 12 or lower, `03/04/2026` is genuinely both 3 April and 4 March,
and the detector says so in `warnings` instead of quietly committing.

`.xlsx` support falls out of the same design: CSV, tab-separated text and Excel
workbooks all become one `Table` of trimmed strings, so a new format is one
loader rather than a branch threaded through the module.

## The assistant is a fallback, not the mechanism

The request that prompted this suggested the AI could do the detection. It can,
and it is offered — but only when local detection has already reported low
confidence, and only with the user's explicit consent on a previewed payload.

Doing it locally first matters for three reasons: the app must work with no
provider configured at all; sending bank data to a third party to learn which
column holds the date is a real privacy cost for something solvable offline;
and a deterministic detector can be unit-tested, which a model cannot.

When the assistant is used it sees a header row and up to four sample rows,
redacted — never the file. Its answer is treated as a *hint*: each field is
applied only if present and in range, and anything else keeps the local guess,
so a confused reply degrades to the local result rather than producing a
mapping that parses nothing.

## Consequences

- The common case is: choose file, see the rows, import.
- The manual grid still exists behind "Adjust columns", and opens by itself
  when confidence is below 70%.
- A running-balance column is explicitly excluded from amount candidates.
  Reading it as the amount would turn a statement into nonsense while still
  looking plausible — the most damaging thing detection could get wrong, and
  the reason it has its own test.
- Excel date serials are converted to ISO at load. Left as `46027` the date
  detector would never recognise the column.
- `tests/real_statement.rs` runs the whole pipeline over a file in `samples/`
  shaped like a real export, because unit tests on hand-written snippets are
  easy to tune to without noticing.
