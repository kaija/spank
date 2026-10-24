#!/usr/bin/env bash
# Wrap a workspace binary in a minimal signed .app with Hardened Runtime and launch it through
# LaunchServices (`open`), so it does not inherit the terminal's permissions (spike T09).
#
# Usage: tools/run-as-app.sh [--sign <identity|->] [--entitlements <plist>] [--bundle-id <id>]
#                            [--profile <file.provisionprofile>] <package> [-- <args>...]
#   --sign          codesign identity; "-" (default) = ad-hoc, or e.g. "Developer ID Application: …"
#   --entitlements  entitlements plist to embed
#   --bundle-id     CFBundleIdentifier (default dev.spank.<package without dashes>)
#   --profile       provisioning profile to embed as Contents/embedded.provisionprofile
#                   (needed for restricted entitlements; must match the bundle id and certificate)
# Output of the app is printed when it exits; the .app is left in target/run-as-app/.
set -euo pipefail
cd "$(dirname "$0")/.."
SIGN="-"
ENT=""
BUNDLE_ID=""
PROFILE=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --sign) SIGN="$2"; shift 2 ;;
    --entitlements) ENT="$2"; shift 2 ;;
    --bundle-id) BUNDLE_ID="$2"; shift 2 ;;
    --profile) PROFILE="$2"; shift 2 ;;
    *) break ;;
  esac
done
PKG="${1:?package name required}"; shift
[[ "${1:-}" == "--" ]] && shift

cargo build -q --release -p "$PKG"
NAME="$(echo "$PKG" | tr -d '-')"
BUNDLE_ID="${BUNDLE_ID:-dev.spank.$NAME}"
APP="target/run-as-app/$NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "target/release/$PKG" "$APP/Contents/MacOS/$PKG"
cat >"$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>$PKG</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleName</key><string>$NAME</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>LSUIElement</key><true/>
  <key>NSFocusStatusUsageDescription</key><string>Spank mutes its sounds while Focus is on.</string>
</dict></plist>
PLIST
[[ -n "$PROFILE" ]] && cp "$PROFILE" "$APP/Contents/embedded.provisionprofile"
SIGN_ARGS=(--force --options runtime --sign "$SIGN")
[[ "$SIGN" != "-" ]] && SIGN_ARGS+=(--timestamp)
[[ -n "$ENT" ]] && SIGN_ARGS+=(--entitlements "$ENT")
codesign "${SIGN_ARGS[@]}" "$APP"
codesign -dv "$APP" 2>&1 | grep -E "^(Authority|TeamIdentifier|Signature|flags|CodeDirectory)" || true
OUT="target/run-as-app/$NAME.out"
rm -f "$OUT"
set +e
open -W --stdout "$OUT" --stderr "$OUT" "$APP" --args "$@"
STATUS=$?
set -e
echo "--- app output (open exit $STATUS) ---"
cat "$OUT" 2>/dev/null || echo "(no output)"
