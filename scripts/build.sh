#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

APP="build/Shipyard.app"
SIGN_ID="${SIGN_ID:-$(security find-identity -v -p codesigning | awk -F'"' '/Apple Development/{print $2; exit}')}"

if [ "${UNIVERSAL:-0}" = "1" ]; then
    swift build -c release --arch arm64
    swift build -c release --arch x86_64
    mkdir -p build
    lipo -create .build/arm64-apple-macosx/release/Shipyard .build/x86_64-apple-macosx/release/Shipyard -output build/Shipyard-universal
    BIN=build/Shipyard-universal
else
    swift build -c release
    BIN=.build/release/Shipyard
fi
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/Shipyard"
cp Resources/Info.plist "$APP/Contents/Info.plist"

if [ ! -f Resources/AppIcon.icns ]; then
    ICONSET="build/AppIcon.iconset"
    rm -rf "$ICONSET"
    swift scripts/make_icon.swift Resources/Brand/icon.svg "$ICONSET"
    iconutil -c icns "$ICONSET" -o Resources/AppIcon.icns
fi
cp Resources/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"

codesign --force --options runtime --sign "${SIGN_ID:--}" "$APP"
echo "Built: $APP (signed with: ${SIGN_ID:-ad-hoc})"

if [ "${1:-}" = "--install" ]; then
    DEST="/Applications"
    [ -w "$DEST" ] || DEST="$HOME/Applications"
    mkdir -p "$DEST"
    pkill -x Shipyard 2>/dev/null || true
    rm -rf "$DEST/Shipyard.app"
    cp -R "$APP" "$DEST/"
    echo "Installed: $DEST/Shipyard.app"
fi
