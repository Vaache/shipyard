#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${1:?Usage: scripts/release.sh <version> [--publish]}"
VERSION="${VERSION#v}"
BUILD_NUMBER="$VERSION"

plutil -replace CFBundleShortVersionString -string "$VERSION" Resources/Info.plist
plutil -replace CFBundleVersion -string "$BUILD_NUMBER" Resources/Info.plist

UNIVERSAL=1 ./scripts/build.sh

APP="build/PullYard.app"
DMG="build/PullYard-$VERSION.dmg"
LATEST="build/PullYard.dmg"
rm -f "$DMG"
./scripts/make_dmg.sh "$APP" "$DMG"
cp "$DMG" "$LATEST"
echo "DMG: $DMG ($(du -h "$DMG" | cut -f1))"

LEGACY_DIR="$(mktemp -d)"
trap 'rm -rf "$LEGACY_DIR"' EXIT
LEGACY="build/Shipyard.dmg"
ditto "$APP" "$LEGACY_DIR/Shipyard.app"
rm -f "$LEGACY"
./scripts/make_dmg.sh "$LEGACY_DIR/Shipyard.app" "$LEGACY"
echo "Legacy DMG for Shipyard updater: $LEGACY"

if [ "${2:-}" = "--publish" ]; then
    NOTES="Universal build (Apple Silicon + Intel), macOS 14+.

Install: open the DMG and drag **PullYard** to Applications. From now on, updates install right from **PullYard → Check for Updates…**

The app is not notarized. On first launch macOS will block it — open **System Settings → Privacy & Security** and click **Open Anyway** (the DMG has a *How to Install* guide), or run:
\`\`\`
xattr -dr com.apple.quarantine /Applications/PullYard.app
\`\`\`"
    gh release create "v$VERSION" "$DMG" "$LATEST" "$LEGACY" --title "PullYard $VERSION" --notes "$NOTES" --target "$(git rev-parse HEAD)"
fi
