#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
. "$here/brand.out"
prefix="${PREFIX:-$HOME/.local}"
install -Dm755 "$here/bin/${BIN_NAME}" "$prefix/bin/${BIN_NAME}"
mkdir -p "$prefix/share"
cp -r "$here/share/." "$prefix/share/"
sed -i "s|^Exec=${BIN_NAME} |Exec=$prefix/bin/${BIN_NAME} |" "$prefix/share/applications/${APP_ID}.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" || true
echo "Installed ${APP_NAME} to $prefix (run: ${BIN_NAME})"
