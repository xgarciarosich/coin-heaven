# 4. One simulation engine, in Rust, called over IPC

**Status:** accepted

## Context
Sliders want instant feedback, which argues for a TypeScript engine. Correctness
and testability argue for Rust.

## Decision
The engine lives in Rust as a pure function and is called over IPC on every
parameter change, debounced 80 ms. Results are stale-guarded by a sequence
number so an older run cannot overwrite a newer one.

## Consequences
- One implementation of the financial model. Two copies would diverge — and the
  divergence would be silent, in money.
- A 120-month run is well under a millisecond; the debounce dominates.
- Being pure (no I/O, no clock, no randomness) is what makes the 40-test suite
  possible and what lets the same code back a future CLI or batch comparison.
