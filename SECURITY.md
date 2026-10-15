# Security

Coin Heaven holds people's finances, so a vulnerability matters more than in most
projects. What it protects and what it does not is written down plainly in
[docs/THREAT-MODEL.md](docs/THREAT-MODEL.md).

## Reporting a vulnerability

Please report privately, not in a public issue: use GitHub's **"Report a
vulnerability"** on the Security tab of this repository. Include what you
found, how to reproduce it, and what you think the impact is.

You will get an acknowledgement within a few days. Coin Heaven is maintained by
volunteers, so a fix may take longer, but a report about data exposure is
treated first.

## What is in scope

- Anything that lets data leave the machine without the person having asked.
- Weaknesses in how the database key is derived, wrapped, stored or backed up.
- The webview gaining a capability it should not have (file system, shell,
  network).
- A crafted statement file (CSV, XLSX, PDF) that crashes or compromises the
  app.

## Supported versions

Only the latest release is supported.
