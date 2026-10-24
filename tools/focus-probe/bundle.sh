#!/usr/bin/env bash
# Build focus-probe into an ad-hoc signed .app and run it, capturing output.
# Usage: tools/focus-probe/bundle.sh [--watch-secs N]
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build -q --release -p focus-probe
APP=target/focus-probe/FocusProbe.app
rm -rf "$APP" && mkdir -p "$APP/Contents/MacOS"
cp target/release/focus-probe "$APP/Contents/MacOS/focus-probe"
cat >"$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>focus-probe</string>
  <key>CFBundleIdentifier</key><string>dev.spank.focusprobe</string>
  <key>CFBundleName</key><string>FocusProbe</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>LSMinimumSystemVersion</key><string>14.0</string>
  <key>NSFocusStatusUsageDescription</key><string>Spank mutes its sounds while Focus is on.</string>
</dict></plist>
PLIST
codesign --force --sign - --options runtime "$APP"
OUT=target/focus-probe/output.txt
rm -f "$OUT"
open -W --stdout "$OUT" --stderr "$OUT" "$APP" --args "$@"
cat "$OUT"
