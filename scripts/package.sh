#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

APP_NAME="FocusLock"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
APP="target/${APP_NAME}.app"
DIST="dist"
DMG="$DIST/${APP_NAME}.dmg"
ZIP="$DIST/${APP_NAME}.zip"

scripts/bundle.sh

echo "==> architecture: $(lipo -archs "$APP/Contents/MacOS/$APP_NAME")"

rm -rf "$DIST"
mkdir -p "$DIST"

echo "==> creating $DMG"
STAGING="target/dmg-staging"
rm -rf "$STAGING"
mkdir -p "$STAGING"
ditto "$APP" "$STAGING/$APP_NAME.app"
ln -s /Applications "$STAGING/Applications"
hdiutil create \
    -volname "$APP_NAME $VERSION" \
    -srcfolder "$STAGING" \
    -fs HFS+ \
    -format UDZO \
    -ov \
    "$DMG" >/dev/null
hdiutil verify "$DMG" >/dev/null
rm -rf "$STAGING"

echo "==> creating $ZIP"
ditto -c -k --sequesterRsrc --keepParent "$APP" "$ZIP"
echo "$APP_NAME $VERSION" | zip -qz "$ZIP"

echo "==> writing checksums"
(cd "$DIST" && shasum -a 256 "$APP_NAME.dmg" "$APP_NAME.zip" > checksums.txt)

sha() { awk -v f="$1" '$2 == f { print $1 }' "$DIST/checksums.txt"; }
size() { stat -f%z "$DIST/$1"; }
cat > "$DIST/latest.json" <<JSON
{
  "name": "$APP_NAME",
  "version": "$VERSION",
  "minimum_macos": "14.0",
  "arch": "$(lipo -archs "$APP/Contents/MacOS/$APP_NAME")",
  "notarized": false,
  "files": {
    "$APP_NAME.dmg": { "sha256": "$(sha "$APP_NAME.dmg")", "bytes": $(size "$APP_NAME.dmg") },
    "$APP_NAME.zip": { "sha256": "$(sha "$APP_NAME.zip")", "bytes": $(size "$APP_NAME.zip") }
  }
}
JSON

echo "==> writing $DIST/install.sh"
if [[ -n "${FOCUSLOCK_BASE_URL:-}" ]]; then
    url_escaped="$(printf '%s' "${FOCUSLOCK_BASE_URL%/}" | sed 's/[&|\\]/\\&/g')"
    sed "s|__FOCUSLOCK_BASE_URL__|$url_escaped|" scripts/install.sh > "$DIST/install.sh"
    echo "    download URL: ${FOCUSLOCK_BASE_URL%/}"
else
    cp scripts/install.sh "$DIST/install.sh"
    echo "    (no FOCUSLOCK_BASE_URL set: install.sh has a placeholder URL)"
fi
chmod +x "$DIST/install.sh"

echo
echo "==> FocusLock $VERSION packaged:"
ls -lh "$DIST" | awk 'NR > 1 { printf "    %-16s %s\n", $9, $5 }'
echo
cat "$DIST/checksums.txt" | sed 's/^/    /'
