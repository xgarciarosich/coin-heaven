# 10. No prototype freezing

**Status:** accepted · reverses a hardening option added in the initial scaffold

## Context

`app.security.freezePrototype` was set to `true` in `tauri.conf.json` as
defence-in-depth against prototype pollution. It freezes `Object.prototype`
before any application code runs.

React's initialisation assigns to a prototype property. With the prototype
frozen that assignment throws:

```
TypeError: Attempted to assign to readonly property. @ /assets/index-*.js
```

The throw happened during module evaluation, so React never mounted, `#root`
stayed empty, and the window rendered **blank with no error shown anywhere** —
no console the user could reach, nothing in stdout, no crash report. It looked
like a broken app rather than a broken setting, and cost three rounds of
guessing before it was measured.

## Decision

Remove the setting; Tauri's default is `false`.

Add a startup watchdog (`public/diag.js`) that reports to the Rust side when
`#root` is still empty four seconds in, or when any error or unhandled
rejection was captured. A healthy launch reports nothing.

## Consequences

- The UI mounts.
- Prototype pollution protection is given up. That is acceptable here: the
  webview executes only our own bundle, loads no third-party script, and the
  CSP (`script-src 'self'`) forbids any other source. There is no attacker-
  controlled JavaScript for the hardening to defend against.
- A silent blank window is now self-reporting. This class of failure —
  everything healthy at the process level, nothing rendered — is invisible to
  process checks, which is exactly how it survived earlier verification: a
  webview showing a blank page uses the same ~100MB as one showing the app.
