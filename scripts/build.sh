#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

APP="build/GitHub Lite.app"
SIGN_ID="${SIGN_ID:-$(security find-identity -v -p codesigning | awk -F'"' '/Apple Development/{print $2; exit}')}"

if [ "${UNIVERSAL:-0}" = "1" ]; then
    swift build -c release --arch arm64 --arch x86_64
    BIN=.build/apple/Products/Release/GitHubLite
else
    swift build -c release
    BIN=.build/release/GitHubLite
fi
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/GitHubLite"
cp Resources/Info.plist "$APP/Contents/Info.plist"

if [ ! -f Resources/AppIcon.icns ]; then
    ICONSET="build/AppIcon.iconset"
    rm -rf "$ICONSET"
    swift scripts/make_icon.swift "$ICONSET"
    iconutil -c icns "$ICONSET" -o Resources/AppIcon.icns
fi
cp Resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

codesign --force --options runtime --sign "${SIGN_ID:--}" "$APP"
echo "Built: $APP (signed with: ${SIGN_ID:-ad-hoc})"

if [ "${1:-}" = "--install" ]; then
    DEST="/Applications"
    [ -w "$DEST" ] || DEST="$HOME/Applications"
    mkdir -p "$DEST"
    pkill -x GitHubLite 2>/dev/null || true
    rm -rf "$DEST/GitHub Lite.app"
    cp -R "$APP" "$DEST/"
    echo "Installed: $DEST/GitHub Lite.app"
fi
