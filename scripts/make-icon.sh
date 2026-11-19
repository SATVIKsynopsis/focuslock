#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

SVG=assets/icon.svg
ICNS=assets/AppIcon.icns
PNG=assets/icon-1024.png

if [[ -f $ICNS && $ICNS -nt $SVG && "${1:-}" != "--force" ]]; then
    echo "==> icon up to date ($ICNS)"
    exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
MASTER="$TMP/master.png"

render_qlmanage() { qlmanage -t -s 1024 -o "$TMP" "$SVG" >/dev/null 2>&1 && mv "$TMP/icon.svg.png" "$MASTER"; }
render_appkit()   { swift scripts/render-svg.swift "$SVG" "$MASTER" 1024; }
render_magick()   { command -v magick >/dev/null && magick -background none -density 96 "$SVG" -resize 1024x1024 "$MASTER"; }

used=""
for renderer in qlmanage appkit magick; do
    echo "==> rendering icon with $renderer"
    rm -f "$MASTER"
    if render_$renderer && [[ -f $MASTER ]] && python3 scripts/png-alpha-check.py "$MASTER"; then
        used=$renderer
        break
    fi
    echo "    $renderer: no usable output, trying the next renderer"
done
if [[ -z $used ]]; then
    echo "error: no renderer produced an icon with transparent corners" >&2
    exit 1
fi
if [[ $used == magick ]]; then
    echo "warning: ImageMagick's SVG renderer may drop strokes; check $PNG by eye" >&2
fi

echo "==> building iconset"
ICONSET="$TMP/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
    sips -z $size $size "$MASTER" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
    sips -z $((size * 2)) $((size * 2)) "$MASTER" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$ICNS"
cp "$MASTER" "$PNG"
echo "==> wrote $ICNS and $PNG (renderer: $used)"
