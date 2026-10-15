#!/usr/bin/env bash
# Build a distributable .dmg from an already-built Coin Heaven.app.
#
# Tauri's own dmg step shells out to a vendored `create-dmg` script whose
# `hdiutil convert` call fails on some macOS versions with
# "convert failed - File exists", leaving a truncated image that does not pass
# `hdiutil verify`. This does the same job with a single hdiutil call and
# verifies the result before declaring success.
#
#   pnpm app:build --bundles app     # produce Coin Heaven.app
#   ./scripts/make-dmg.sh            # wrap it in a verified .dmg
#
# CI does not use this: tauri-action builds and notarises on a clean runner.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APP="$ROOT/src-tauri/target/release/bundle/macos/Coin Heaven.app"
OUT_DIR="$ROOT/src-tauri/target/release/bundle/dmg"
VERSION="$(node -p "require('$ROOT/package.json').version")"
ARCH="$(uname -m)"
DMG="$OUT_DIR/Coin-Heaven_${VERSION}_${ARCH}.dmg"

[ -d "$APP" ] || { echo "error: $APP not found — run 'pnpm app:build --bundles app' first" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"

mkdir -p "$OUT_DIR"
rm -f "$DMG"

echo "Building $DMG"
hdiutil create \
  -volname "Coin Heaven" \
  -srcfolder "$STAGE" \
  -ov -format UDZO \
  "$DMG" >/dev/null

# A dmg that does not verify is worse than no dmg: it fails at the user's end.
hdiutil verify "$DMG" >/dev/null 2>&1 || { echo "error: produced image failed verification" >&2; exit 1; }

echo "ok  $(du -h "$DMG" | cut -f1)  $DMG"
echo "sha256  $(shasum -a 256 "$DMG" | cut -d' ' -f1)"
