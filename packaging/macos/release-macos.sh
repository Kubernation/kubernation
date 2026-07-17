#!/usr/bin/env bash
# Assemble, sign, notarize, and staple a distributable KuberNation.app, then
# wrap it in a drag-to-Applications .dmg that is also stapled. Runs in CI
# (.github/workflows/release.yml) and locally (given a Developer ID identity in
# your keychain + an App Store Connect API key).
#
# Why an .app in a .dmg: KuberNation is a windowed GUI, so a bare Unix binary
# double-clicked in Finder just opens Terminal. A .app is the real macOS app
# (Dock icon, window identity) and — unlike a bare Mach-O — a notarization
# ticket can be STAPLED to it, so Gatekeeper clears it on first launch even
# offline, with no `xattr` workaround for the user.
#
# Required env:
#   BIN_PATH        path to the (universal) `kubernation` Mach-O
#   VERSION         version string, no leading v (e.g. 1.0.0)
#   ICON_PNG        path to a square PNG >=512px for the .icns (mark.png is 256,
#                   upscaled; fine for an icon)
#   IDENTITY        codesign identity — the "Developer ID Application: NAME (TEAMID)"
#                   string or its SHA-1 hash
#   NOTARY_KEY      path to the App Store Connect API key .p8
#   NOTARY_KEY_ID   the key's Key ID
#   NOTARY_ISSUER   the key's Issuer ID
#   OUT_DMG         output path for the .dmg
# Optional env:
#   BIN             executable basename inside the bundle (default kubernation)
#   APP_NAME        .app display name (default KuberNation)
#   KEYCHAIN        keychain to search for the identity (default: the login/search list)
#   EXTRA_RESOURCES space-separated files to copy into Contents/Resources before
#                   signing (licenses / third-party notices — they must be inside
#                   the bundle so codesign seals them and they ship with the app)
#   NOTARY_TIMEOUT  how long to wait for Apple's notary verdict (default 45m;
#                   Apple's service is usually 1-5m but can back up on a busy day)
set -euo pipefail

BIN="${BIN:-kubernation}"
APP_NAME="${APP_NAME:-KuberNation}"
: "${BIN_PATH:?BIN_PATH is required}"
: "${VERSION:?VERSION is required}"
: "${ICON_PNG:?ICON_PNG is required}"
: "${IDENTITY:?IDENTITY is required}"
: "${NOTARY_KEY:?NOTARY_KEY is required}"
: "${NOTARY_KEY_ID:?NOTARY_KEY_ID is required}"
: "${NOTARY_ISSUER:?NOTARY_ISSUER is required}"
: "${OUT_DMG:?OUT_DMG is required}"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

APP="$WORK/$APP_NAME.app"
echo "==> Assembling $APP_NAME.app (v$VERSION)"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN_PATH" "$APP/Contents/MacOS/$BIN"
chmod +x "$APP/Contents/MacOS/$BIN"
sed "s/@VERSION@/$VERSION/g" "$HERE/Info.plist.template" > "$APP/Contents/Info.plist"

# .icns from the PNG mark: build the standard iconset sizes with sips, compile
# with iconutil. The 256px source is upscaled to the 512/1024 slots — acceptable
# for an app icon.
echo "==> Building icon"
ICONSET="$WORK/kubernation.iconset"
mkdir -p "$ICONSET"
for spec in "16:16x16" "32:16x16@2x" "32:32x32" "64:32x32@2x" \
            "128:128x128" "256:128x128@2x" "256:256x256" "512:256x256@2x" \
            "512:512x512" "1024:512x512@2x"; do
  px="${spec%%:*}"; label="${spec##*:}"
  sips -z "$px" "$px" "$ICON_PNG" --out "$ICONSET/icon_${label}.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/kubernation.icns"

# License / notice files travel inside the bundle (sealed by codesign).
for res in ${EXTRA_RESOURCES:-}; do
  [ -f "$res" ] && cp "$res" "$APP/Contents/Resources/"
done

# Sign the app with the hardened runtime (a notarization prerequisite) and a
# secure timestamp. The bundle has one Mach-O (the main binary), so no --deep is
# needed; signing the bundle signs its executable. No entitlements: the Rust GUI
# needs no JIT / no library-validation exceptions (everything is statically linked).
echo "==> Signing"
KC_ARGS=()
[ -n "${KEYCHAIN:-}" ] && KC_ARGS=(--keychain "$KEYCHAIN")
codesign --force --options runtime --timestamp \
  "${KC_ARGS[@]+"${KC_ARGS[@]}"}" --sign "$IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

# Notarize a submittable container (a .zip of the .app, or the .dmg) and fail
# loudly with Apple's log if it comes back anything but Accepted.
#
# LOAD-BEARING: a notarization ticket is keyed to the cdhash of the code that was
# SUBMITTED. The .dmg is its own separately-signed code object with its own
# cdhash, so notarizing the .app does NOT produce a ticket the .dmg can staple
# ("Record not found"). Each container we want STAPLED needs its own submission —
# hence two round-trips below. Stapling both means the .dmg verifies offline at
# mount, and the .app still verifies offline once dragged out to /Applications.
notarize() {
  local path="$1" out rc sid
  set +e
  out="$(xcrun notarytool submit "$path" \
    --key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER" \
    --wait --timeout "${NOTARY_TIMEOUT:-45m}" 2>&1)"
  rc=$?
  set -e
  echo "$out"
  if [ $rc -ne 0 ] || ! grep -q "status: Accepted" <<<"$out"; then
    sid="$(grep -m1 -Eo 'id: [0-9a-f-]+' <<<"$out" | head -1 | awk '{print $2}')"
    if [ -n "$sid" ]; then
      echo "==> Notarization not accepted; fetching log for $sid"
      xcrun notarytool log "$sid" \
        --key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER" || true
    fi
    echo "ERROR: notarization failed for $path" >&2
    exit 1
  fi
}

echo "==> Notarizing the app (this can take a few minutes)"
ZIP="$WORK/$APP_NAME.zip"
ditto -c -k --keepParent "$APP" "$ZIP"
notarize "$ZIP"

echo "==> Stapling the app"
xcrun stapler staple "$APP"

# Build the .dmg from the stapled app + an /Applications symlink for drag-install.
echo "==> Building .dmg"
DMG_SRC="$WORK/dmg"
mkdir -p "$DMG_SRC"
cp -R "$APP" "$DMG_SRC/"
ln -s /Applications "$DMG_SRC/Applications"
rm -f "$OUT_DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$DMG_SRC" \
  -fs HFS+ -format UDZO -ov "$OUT_DMG" >/dev/null
codesign --force --timestamp "${KC_ARGS[@]+"${KC_ARGS[@]}"}" --sign "$IDENTITY" "$OUT_DMG"

echo "==> Notarizing the .dmg (its own cdhash needs its own ticket)"
notarize "$OUT_DMG"
echo "==> Stapling the .dmg"
xcrun stapler staple "$OUT_DMG"

echo "==> Verifying Gatekeeper acceptance"
spctl -a -vvv -t install "$OUT_DMG" || true
spctl -a -vvv -t exec "$APP" || true
echo "==> Done: $OUT_DMG"
