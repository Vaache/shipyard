#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/brand.out"
prefix="${PREFIX:-$HOME/.local}"
rm -f "$prefix/bin/${BIN_NAME}" "$prefix/share/applications/${APP_ID}.desktop" "$prefix/share/metainfo/${APP_ID}.metainfo.xml"
find "$prefix/share/icons/hicolor" -name "${APP_ID}.*" -delete 2>/dev/null || true
echo "Removed ${APP_NAME} from $prefix"
