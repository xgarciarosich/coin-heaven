# Distribution

Coin Heaven is distributed as source and as signed installers on GitHub Releases.
Because it is open source, anyone can also build it themselves — that path is
always available and needs no certificates.

## macOS — signed and notarised

Releases are built by [`release.yml`](../.github/workflows/release.yml) and
notarised through Apple's service. Notarisation requires a paid Apple Developer
account (currently $99/year); without it Gatekeeper refuses to open the app with
a message most people read as "this app is broken".

Set these repository secrets. If they are absent the workflow still builds, but
the output is unsigned.

| Secret | What it is |
|---|---|
| `APPLE_CERTIFICATE` | base64 of your Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | password for that `.p12` |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: NAME (TEAMID)` |
| `APPLE_ID` | your Apple account email |
| `APPLE_PASSWORD` | an **app-specific** password, not your account password |
| `APPLE_TEAM_ID` | your 10-character team id |

Export the certificate with:

```bash
security find-identity -v -p codesigning   # find the identity name
base64 -i DeveloperID.p12 | pbcopy         # value for APPLE_CERTIFICATE
```

Verify a finished build:

```bash
spctl -a -vvv -t install /Applications/Coin Heaven.app
```

`source=Notarized Developer ID` means it is done.

### Building it yourself

No certificate needed:

```bash
pnpm install
pnpm app:build     # produces Coin Heaven.app
pnpm dmg           # wraps it in a verified .dmg
```

**Why `pnpm dmg` instead of Tauri's own dmg target.** Tauri shells out to a
vendored `create-dmg` script, and on some macOS versions its `hdiutil convert`
step fails with `convert failed - File exists`. Worse than failing, it leaves
behind a truncated image that does not pass `hdiutil verify` — a broken
installer that looks like a finished one.

This was reproduced here from a completely clean `target/`, with 49 GB free and
with Finder scripting available, so it is neither disk space, a stale artefact,
nor the AppleScript step. The root cause is unresolved; it is most likely a
mismatch between the vendored script and a newer macOS.

`scripts/make-dmg.sh` sidesteps it with a single `hdiutil create` and — the
part that matters — **runs `hdiutil verify` and fails loudly if the image is
bad**, so a corrupt installer can never be published by accident.

CI is unaffected: `tauri-action` builds on a clean GitHub runner.

macOS quarantines apps you build and then move around. If you hit
"Coin Heaven is damaged", clear the quarantine flag:

```bash
xattr -dr com.apple.quarantine /Applications/Coin Heaven.app
```

## Windows — SmartScreen, and why the warning appears

The Windows installer is **not** signed with an EV certificate. An EV
certificate costs several hundred dollars a year and is issued to a legal
entity, which is a poor fit for a project at this stage. The consequence is
honest and worth stating plainly: **Windows SmartScreen will warn on first
run.**

That warning does not mean the file is malicious. It means Microsoft has not yet
seen enough installations of this exact binary to have built a reputation for
it. A standard (non-EV) certificate would not remove the warning either — only
accumulated reputation or EV does.

### Running the installer

1. Download `Coin Heaven_x.y.z_x64-setup.exe` from the release page.
2. Windows shows **"Windows protected your PC"**.
3. Click **More info**.
4. Confirm the publisher line and click **Run anyway**.

### Verify what you downloaded first

Checksums are published in the release notes. Before running the installer:

```powershell
Get-FileHash .\Coin Heaven_x.y.z_x64-setup.exe -Algorithm SHA256
```

Compare the result to the release notes. If they differ, do not run it.

### When to buy an EV certificate

When download volume makes the SmartScreen warning a real barrier to adoption.
Until then the checksum route is more honest and costs nothing: a verifiable
hash against a public, reproducible build is a stronger assurance than a
certificate that only proves someone paid for one.

## What ships

| Platform | Artefact |
|---|---|
| macOS Apple silicon | `Coin Heaven_x.y.z_aarch64.dmg` |
| macOS Intel | `Coin Heaven_x.y.z_x64.dmg` |
| Windows | `Coin Heaven_x.y.z_x64-setup.exe`, `.msi` |

Linux is not built. Nothing in the codebase prevents it — `keyring` is
configured with `sync-secret-service` — but it is untested, so it is not
advertised.

## Releasing

```bash
git tag v1.0.0
git push origin v1.0.0
```

The workflow builds all targets and opens a **draft** release. Add the
checksums, then publish.
