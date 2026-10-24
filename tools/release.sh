#!/usr/bin/env bash
# Build a signed (and optionally notarized) Spank.app and DMG for Apple Silicon (T40, design §12).
# Used by .github/workflows/release.yml and for local dry runs.
#
# Usage: tools/release.sh [--notarize] [--skip-build] [--out <dir>]
#
# Environment:
#   APPLE_SIGNING_IDENTITY   required, e.g. "Developer ID Application: Chang KaiChieh (H2ZM466J6A)"
#   SPANK_PROVISION_PROFILE  provisioning profile to embed
#                            (default signing/Spank_Developer_ID.provisionprofile, gitignored)
#   With --notarize (App Store Connect API key; not Tauri's APPLE_API_* names, so Tauri's own
#   notarization stays off and this script notarizes the app and the DMG itself):
#   NOTARY_KEY_ID, NOTARY_ISSUER, NOTARY_KEY_PATH (path to the AuthKey_*.p8 file)
#
# Steps: tauri build (sign, hardened runtime, entitlements, embedded profile) → verify →
# [notarize + staple app] → DMG → sign DMG → [notarize + staple DMG] → Gatekeeper check.
set -euo pipefail
cd "$(dirname "$0")/.."

NOTARIZE=0
BUILD=1
OUT="target/release-dist"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --notarize) NOTARIZE=1; shift ;;
    --skip-build) BUILD=0; shift ;;
    --out) OUT="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

: "${APPLE_SIGNING_IDENTITY:?set APPLE_SIGNING_IDENTITY to a Developer ID Application identity}"
PROFILE_DEST="signing/Spank_Developer_ID.provisionprofile"
PROFILE="${SPANK_PROVISION_PROFILE:-$PROFILE_DEST}"
[[ -f "$PROFILE" ]] || { echo "provisioning profile not found: $PROFILE" >&2; exit 1; }
if [[ "$PROFILE" != "$PROFILE_DEST" ]]; then
  mkdir -p signing
  cp "$PROFILE" "$PROFILE_DEST"
fi
if [[ $NOTARIZE == 1 ]]; then
  : "${NOTARY_KEY_ID:?}" "${NOTARY_ISSUER:?}" "${NOTARY_KEY_PATH:?}"
fi

VERSION="$(cargo metadata --no-deps --format-version 1 \
  | python3 -I -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="spank-desktop"))')"
APP="target/release/bundle/macos/Spank.app"
DMG="$OUT/Spank_${VERSION}_aarch64.dmg"
echo "==> Spank $VERSION"

echo "==> Build and sign"
# Tauri signs with APPLE_SIGNING_IDENTITY (hardened runtime from tauri.conf.json) and adds the
# release entitlements + embedded profile from the overlay config.
if [[ $BUILD == 1 ]]; then
  pnpm --dir apps/desktop tauri build --bundles app --config src-tauri/tauri.release.conf.json
fi

echo "==> Verify signature, entitlements and profile"
codesign --verify --deep --strict --verbose=2 "$APP"
# Capture first: `grep -q` would close the pipe early and fail the pipeline under pipefail.
ENTITLEMENTS="$(codesign -d --entitlements :- "$APP" 2>/dev/null)"
SIGNATURE="$(codesign -dvv "$APP" 2>&1)"
[[ "$ENTITLEMENTS" == *"H2ZM466J6A.dev.spank.app"* ]] || { echo "release entitlements missing" >&2; exit 1; }
[[ "$SIGNATURE" == *"(runtime)"* ]] || { echo "hardened runtime missing" >&2; exit 1; }
[[ "$SIGNATURE" == *"Authority=Developer ID Application"* ]] || { echo "not signed with Developer ID" >&2; exit 1; }
[[ -f "$APP/Contents/embedded.provisionprofile" ]] || { echo "embedded.provisionprofile missing" >&2; exit 1; }

notarize() {
  xcrun notarytool submit "$1" --key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER" \
    --wait --timeout 30m
}

mkdir -p "$OUT"
if [[ $NOTARIZE == 1 ]]; then
  echo "==> Notarize app"
  ZIP="$OUT/Spank-notarize.zip"
  ditto -c -k --keepParent "$APP" "$ZIP"
  notarize "$ZIP"
  rm -f "$ZIP"
  xcrun stapler staple "$APP"
fi

echo "==> DMG"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
ditto "$APP" "$STAGE/Spank.app"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -volname "Spank" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$DMG"

if [[ $NOTARIZE == 1 ]]; then
  echo "==> Notarize DMG"
  notarize "$DMG"
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"
  echo "==> Gatekeeper"
  spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"
  spctl --assess --type execute --verbose=2 "$APP"
else
  echo "(not notarized: Gatekeeper will warn on other Macs)"
fi

shasum -a 256 "$DMG" | tee "$DMG.sha256"
echo "==> $DMG"
