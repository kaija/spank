#!/usr/bin/env bash
# Build the lid-only Mac App Store edition "Creaky" (sandboxed, no updater, no accelerometer)
# as a signed installer package, and optionally validate + upload it to App Store Connect.
# The Developer ID / DMG flow is tools/release.sh and is not affected by this script.
#
# Usage: tools/release-appstore.sh [--upload | --local-test] [--skip-build] [--out <dir>]
#
#   (default)     App Store build signed with the 3rd Party Mac Developer identities →
#                 <out>/Creaky.app + <out>/Creaky.pkg
#   --upload      also `altool --validate-app`, then `--upload-app` the package
#   --local-test  ad-hoc signed, still sandboxed, runnable on this Mac (no profile, no pkg).
#                 Drops the entitlements that need the App Store profile (Communication
#                 Notifications, application/team identifiers), so Focus status reads "off".
#   --skip-build  reuse the last tauri build output (re-sign / re-package only)
#
# Environment:
#   APPSTORE_APP_IDENTITY        default "3rd Party Mac Developer Application: Chang KaiChieh (H2ZM466J6A)"
#   APPSTORE_INSTALLER_IDENTITY  default "3rd Party Mac Developer Installer: Chang KaiChieh (H2ZM466J6A)"
#   CREAKY_PROVISION_PROFILE     Mac App Store profile for dev.spank.app
#                                (default signing/Creaky_App_Store.provisionprofile, gitignored)
#   CREAKY_BUILD_NUMBER          CFBundleVersion (default: YYMMDDHHMM now; must grow per upload)
#   With --upload: ASC_KEY_ID, ASC_ISSUER (App Store Connect API key; altool reads
#   ~/.appstoreconnect/private_keys/AuthKey_<ASC_KEY_ID>.p8, which is never printed)
#
# Output: target/appstore-dist/ (override with --out).
set -euo pipefail
cd "$(dirname "$0")/.."

UPLOAD=0
LOCAL=0
BUILD=1
OUT="target/appstore-dist"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --upload) UPLOAD=1; shift ;;
    --local-test) LOCAL=1; shift ;;
    --skip-build) BUILD=0; shift ;;
    --out) OUT="$2"; shift 2 ;;
    -h|--help) awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

die() { echo "error: $*" >&2; exit 1; }
[[ $UPLOAD == 1 && $LOCAL == 1 ]] && die "--upload and --local-test cannot be combined"

TEAM="H2ZM466J6A"
BUNDLE_ID="dev.spank.app"
APP_ID="$TEAM.$BUNDLE_ID"
APP_IDENTITY="${APPSTORE_APP_IDENTITY:-3rd Party Mac Developer Application: Chang KaiChieh ($TEAM)}"
INSTALLER_IDENTITY="${APPSTORE_INSTALLER_IDENTITY:-3rd Party Mac Developer Installer: Chang KaiChieh ($TEAM)}"
PROFILE_DEST="signing/Creaky_App_Store.provisionprofile"   # where tauri.appstore.conf.json looks
PROFILE="${CREAKY_PROVISION_PROFILE:-$PROFILE_DEST}"
ENTITLEMENTS_SRC="apps/desktop/src-tauri/Creaky.appstore.entitlements"
TARGET="aarch64-apple-darwin"
APP_BUILT="target/$TARGET/release/bundle/macos/Creaky.app"
APP="$OUT/Creaky.app"
PKG="$OUT/Creaky.pkg"
BUILD_NUMBER="${CREAKY_BUILD_NUMBER:-$(date +%y%m%d%H%M)}"
[[ "$BUILD_NUMBER" =~ ^[0-9]+(\.[0-9]+){0,2}$ ]] || die "CREAKY_BUILD_NUMBER must be up to three dot-separated integers"
# --skip-build re-signs / re-packages the last build, so it keeps that build's number.
if [[ $BUILD == 0 && -z "${CREAKY_BUILD_NUMBER:-}" ]]; then
  for prev in "target/aarch64-apple-darwin/release/bundle/macos/Creaky.app" "$APP"; do
    if [[ -f "$prev/Contents/Info.plist" ]]; then
      BUILD_NUMBER="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$prev/Contents/Info.plist")"
      break
    fi
  done
fi

# ---- preflight: fail before the (slow) build when anything is missing --------------------------

echo "==> Preflight"
if [[ $UPLOAD == 1 ]]; then
  [[ -n "${ASC_KEY_ID:-}" ]] || die "--upload needs ASC_KEY_ID (App Store Connect API key id)"
  [[ -n "${ASC_ISSUER:-}" ]] || die "--upload needs ASC_ISSUER (App Store Connect API issuer id)"
  [[ -f "$HOME/.appstoreconnect/private_keys/AuthKey_$ASC_KEY_ID.p8" ]] \
    || die "API key file for ASC_KEY_ID not found in ~/.appstoreconnect/private_keys/ (expected AuthKey_<ASC_KEY_ID>.p8)"
  xcrun --find altool >/dev/null 2>&1 || die "xcrun altool not found (install Xcode)"
fi
if [[ $LOCAL == 0 ]]; then
  # Capture first: `grep -q` closing the pipe early would fail the pipeline under pipefail.
  IDENTITIES="$(security find-identity -v 2>/dev/null)"
  [[ "$IDENTITIES" == *"\"$APP_IDENTITY\""* ]] \
    || die "signing identity not in the keychain: $APP_IDENTITY (set APPSTORE_APP_IDENTITY)"
  [[ "$IDENTITIES" == *"\"$INSTALLER_IDENTITY\""* ]] \
    || die "installer identity not in the keychain: $INSTALLER_IDENTITY (set APPSTORE_INSTALLER_IDENTITY)"

  [[ -f "$PROFILE" ]] || die "provisioning profile not found: $PROFILE
  Create a 'Mac App Store Connect' profile for App ID $APP_ID (with Communication Notifications)
  at developer.apple.com › Profiles, save it as $PROFILE_DEST or set CREAKY_PROVISION_PROFILE."
  PROFILE_PLIST="$(security cms -D -i "$PROFILE" 2>/dev/null)" || die "cannot decode provisioning profile: $PROFILE"
  PROFILE_PROBLEM="$(python3 -I -c '
import datetime, plistlib, sys
p = plistlib.loads(sys.stdin.buffer.read())
app_id = p.get("Entitlements", {}).get("com.apple.application-identifier")
if app_id != sys.argv[1]:
    print(f"it is for {app_id}, not {sys.argv[1]}")
elif "ProvisionedDevices" in p or p.get("ProvisionsAllDevices"):
    # Development and Developer ID profiles list devices or allow all; App Store ones do neither.
    print("it is not a Mac App Store distribution profile")
elif (expiry := p["ExpirationDate"]) < datetime.datetime.now(datetime.timezone.utc).replace(tzinfo=None):
    print(f"it expired on {expiry:%Y-%m-%d}")
' "$APP_ID" <<<"$PROFILE_PLIST")" || die "cannot read provisioning profile: $PROFILE"
  [[ -z "$PROFILE_PROBLEM" ]] || die "provisioning profile $PROFILE: $PROFILE_PROBLEM"
  if [[ "$PROFILE" != "$PROFILE_DEST" ]]; then
    mkdir -p signing
    cp "$PROFILE" "$PROFILE_DEST"
  fi
fi

METADATA="$(cargo metadata --no-deps --format-version 1)"
read -r VERSION HAS_FEATURE < <(python3 -I -c '
import json, sys
p = next(p for p in json.load(sys.stdin)["packages"] if p["name"] == "spank-desktop")
print(p["version"], int("appstore" in p["features"]))' <<<"$METADATA")
[[ $HAS_FEATURE == 1 ]] || die "spank-desktop has no 'appstore' Cargo feature yet (apps/desktop/src-tauri/Cargo.toml)"
echo "==> Creaky $VERSION ($BUILD_NUMBER)$([[ $LOCAL == 1 ]] && echo ', local test')"

mkdir -p "$OUT"

# ---- build -------------------------------------------------------------------------------------

# Overlay applied after tauri.appstore.conf.json: the build number, no second frontend build
# (the App Store frontend is built just below, so its beforeBuildCommand is skipped), and for
# --local-test no embedded profile and our own ad-hoc signature.
OVERLAY="$(python3 -I -c '
import json, sys
local = sys.argv[2] == "1"
mac = {"bundleVersion": sys.argv[1]}
if local:
    mac["files"] = {"embedded.provisionprofile": None}  # merge patch: null removes the key
print(json.dumps({"build": {"beforeBuildCommand": None}, "bundle": {"macOS": mac}}))' "$BUILD_NUMBER" "$LOCAL")"

if [[ $BUILD == 1 ]]; then
  echo "==> Frontend (App Store edition)"
  # Build-time constant read by apps/desktop/src/lib/edition.ts: hides the slap screens.
  NEXT_PUBLIC_EDITION=appstore pnpm --dir apps/desktop build

  echo "==> tauri build (appstore features)"
  SIGN_ARGS=()
  if [[ $LOCAL == 1 ]]; then
    SIGN_ARGS=(--no-sign)
  fi
  # Tauri signs with APPLE_SIGNING_IDENTITY using the entitlements and embedded profile from
  # tauri.appstore.conf.json. Unset Tauri's own notarization variables: App Store builds are
  # not notarized.
  env -u APPLE_ID -u APPLE_PASSWORD -u APPLE_API_KEY -u APPLE_API_ISSUER -u APPLE_API_KEY_PATH \
    APPLE_SIGNING_IDENTITY="$APP_IDENTITY" \
    pnpm --dir apps/desktop tauri build --bundles app --target "$TARGET" \
      ${SIGN_ARGS[@]+"${SIGN_ARGS[@]}"} \
      --config src-tauri/tauri.appstore.conf.json --config "$OVERLAY" \
      --features appstore -- --no-default-features
fi
[[ -d "$APP_BUILT" ]] || die "no build output at $APP_BUILT (run without --skip-build)"

rm -rf "$APP" "$PKG"
ditto "$APP_BUILT" "$APP"

if [[ $LOCAL == 1 ]]; then
  echo "==> Ad-hoc sign for local testing"
  LOCAL_ENTITLEMENTS="$OUT/Creaky.local.entitlements"
  cp "$ENTITLEMENTS_SRC" "$LOCAL_ENTITLEMENTS"
  for key in com.apple.developer.usernotifications.communication \
             com.apple.application-identifier com.apple.developer.team-identifier; do
    /usr/libexec/PlistBuddy -c "Delete :$key" "$LOCAL_ENTITLEMENTS" >/dev/null
  done
  rm -f "$APP/Contents/embedded.provisionprofile"
  codesign --force --deep --options runtime --entitlements "$LOCAL_ENTITLEMENTS" --sign - "$APP"
fi

# ---- verify ------------------------------------------------------------------------------------

echo "==> Verify"
plist_get() { /usr/libexec/PlistBuddy -c "Print :$1" "$APP/Contents/Info.plist" 2>/dev/null || true; }
codesign --verify --deep --strict --verbose=2 "$APP"
ENTITLEMENTS="$(codesign -d --entitlements :- "$APP" 2>/dev/null)"
SIGNATURE="$(codesign -dvv "$APP" 2>&1)"
[[ "$ENTITLEMENTS" == *"com.apple.security.app-sandbox"* ]] || die "app sandbox entitlement missing"
[[ "$ENTITLEMENTS" == *"com.apple.security.device.usb"* ]] || die "USB entitlement (lid sensor) missing"
# WKWebView needs it in the sandbox (blank windows otherwise); the app itself makes no requests.
[[ "$ENTITLEMENTS" == *"com.apple.security.network.client"* ]] || die "network.client entitlement missing (WebKit renders blank without it)"
[[ "$ENTITLEMENTS" != *"com.apple.security.network.server"* ]] || die "unexpected network.server entitlement"
[[ "$(plist_get CFBundleIdentifier)" == "$BUNDLE_ID" ]] || die "bundle id is not $BUNDLE_ID"
[[ "$(plist_get CFBundleVersion)" == "$BUILD_NUMBER" ]] || die "CFBundleVersion is not $BUILD_NUMBER"
[[ "$(plist_get LSApplicationCategoryType)" == "public.app-category.entertainment" ]] \
  || die "LSApplicationCategoryType is not Entertainment"
[[ "$(plist_get ITSAppUsesNonExemptEncryption)" == "false" ]] || die "ITSAppUsesNonExemptEncryption missing"
[[ -f "$APP/Contents/Resources/PrivacyInfo.xcprivacy" ]] || die "PrivacyInfo.xcprivacy missing"
if [[ $LOCAL == 0 ]]; then
  [[ "$ENTITLEMENTS" == *"$APP_ID"* ]] || die "application identifier entitlement missing"
  [[ "$SIGNATURE" == *"Authority=3rd Party Mac Developer Application"* ]] \
    || die "not signed with the 3rd Party Mac Developer Application identity"
  [[ -f "$APP/Contents/embedded.provisionprofile" ]] || die "embedded.provisionprofile missing"
  cmp -s "$APP/Contents/embedded.provisionprofile" "$PROFILE_DEST" || die "embedded profile is not $PROFILE_DEST"
fi

if [[ $LOCAL == 1 ]]; then
  echo "==> $APP (ad-hoc, sandboxed)"
  echo "    open \"$APP\"   # data lives in ~/Library/Containers/$BUNDLE_ID"
  exit 0
fi

# ---- package + upload --------------------------------------------------------------------------

echo "==> Package"
xcrun productbuild --sign "$INSTALLER_IDENTITY" --component "$APP" /Applications "$PKG"
pkgutil --check-signature "$PKG"

if [[ $UPLOAD == 1 ]]; then
  ASC_ARGS=(--type macos --apiKey "$ASC_KEY_ID" --apiIssuer "$ASC_ISSUER")
  echo "==> Validate"
  xcrun altool --validate-app -f "$PKG" "${ASC_ARGS[@]}"
  echo "==> Upload"
  xcrun altool --upload-app -f "$PKG" "${ASC_ARGS[@]}"
  echo "(uploaded build $BUILD_NUMBER: it appears in App Store Connect › TestFlight after processing)"
fi

shasum -a 256 "$PKG" | tee "$PKG.sha256"
echo "==> $PKG"
