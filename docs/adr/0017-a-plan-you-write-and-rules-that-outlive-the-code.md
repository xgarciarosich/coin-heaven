# 17. A property plan the person writes; rules that outlive the code

**Status:** accepted. Supersedes the simulator's purchase rule.

## Decision

**The simulator runs a plan, it does not make one.** Each purchase is entered
with what belongs to that property alone — month, price, renovation, rent,
the transfer tax of its region, and its own mortgage terms if it has them.
The engine checks each against cash and the bank's affordability test, tries
again each month if it fails, and reports the delay and its cause. Around the
plan sit the person's own numbers (pay, its yearly rise, what is put aside,
starting cash) and the general ones every purchase shares. Automatic buying
and per-year overrides are removed. The plan carries the calendar month it
starts in, so "month 18" keeps meaning the same real month, and it saves
itself.

**Whether a class counts toward FIRE is a tick box on the class**
(`asset_class.counts_fire`, migration 0013), not a consequence of its kind.
It starts where the old rule left it.

**An item is its class and its name, on every date.** Items recorded on any
date are offered on all of them; a rename applies to every date at once.

**A wished-for retirement age is checked, not obeyed.** The date comes out of
the numbers. The age is held against it, and when it does not fit, the plan
says by how much and what change in saving or spending would close the gap.

**Nothing the law or a vendor can change is a constant.**

- The savings-tax brackets live in the saved plan and are editable; the
  shipped ones are a starting point.
- The Claude web-search tool version is chosen by excluding known-old model
  families rather than listing new ones, and a 400 retries once with the
  other version, so a model released later needs no new build.
- The year list always includes the current year.
- No screen assumes a particular year; dates in prompts and defaults come from
  the clock.

## What still depends on the outside world

The AI features call third-party APIs whose endpoints and tool versions will
eventually be retired. Everything else — import, ledger, budgets, net worth,
portfolio arithmetic, FIRE, the simulator, backup and recovery — runs offline
on the local database and has no date after which it stops.
