#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
set -a
. "$here/brand.env"
set +a
VERSION="${PULLYARD_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$here/Cargo.toml" | head -1)}"
VERSION="${VERSION#v}"
DATE="$(date -u +%Y-%m-%d)"
REPO_URL="https://github.com/${GITHUB_OWNER}/${GITHUB_REPO}"
OWNER_ID="$(printf %s "$GITHUB_OWNER" | tr "[:upper:]" "[:lower:]" | tr -c "a-z0-9_.\n-" "_")"
out="${1:-$here/dist}"
svg="$here/../Resources/Brand/icon.svg"
subst() {
  sed -e "s|@APP_NAME@|${APP_NAME}|g" \
      -e "s|@APP_ID@|${APP_ID}|g" \
      -e "s|@BIN_NAME@|${BIN_NAME}|g" \
      -e "s|@URL_SCHEME@|${URL_SCHEME}|g" \
      -e "s|@APP_SUMMARY@|${APP_SUMMARY}|g" \
      -e "s|@APP_DESCRIPTION@|${APP_DESCRIPTION}|g" \
      -e "s|@GITHUB_OWNER@|${GITHUB_OWNER}|g" \
      -e "s|@OWNER_ID@|${OWNER_ID}|g" \
      -e "s|@DEVELOPER_NAME@|${DEVELOPER_NAME}|g" \
      -e "s|@REPO_URL@|${REPO_URL}|g" \
      -e "s|@VERSION@|${VERSION}|g" \
      -e "s|@DATE@|${DATE}|g" "$1"
}
rm -rf "$out/share" "$out/flatpak"
mkdir -p "$out/share/applications" "$out/share/metainfo" "$out/share/icons/hicolor/scalable/apps" "$out/flatpak"
subst "$here/data/app.desktop.in" > "$out/share/applications/${APP_ID}.desktop"
subst "$here/data/app.metainfo.xml.in" > "$out/share/metainfo/${APP_ID}.metainfo.xml"
cp "$svg" "$out/share/icons/hicolor/scalable/apps/${APP_ID}.svg"
for size in 16 32 48 64 128 256 512; do
  dir="$out/share/icons/hicolor/${size}x${size}/apps"
  mkdir -p "$dir"
  rsvg-convert -w "$size" -h "$size" "$svg" -o "$dir/${APP_ID}.png"
done
subst "$here/packaging/flatpak.yml.in" > "$out/flatpak/${APP_ID}.yml"
{
  printf 'APP_NAME="%s"\n' "$APP_NAME"
  printf 'APP_ID="%s"\n' "$APP_ID"
  printf 'BIN_NAME="%s"\n' "$BIN_NAME"
  printf 'VERSION="%s"\n' "$VERSION"
} > "$out/brand.out"
echo "rendered packaging for ${APP_NAME} ${VERSION} into $out"
