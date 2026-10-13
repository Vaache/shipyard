#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

VERSION="${1:?Usage: scripts/release.sh <version> [--publish]}"
VERSION="${VERSION#v}"
BUILD_NUMBER="$VERSION"

plutil -replace CFBundleShortVersionString -string "$VERSION" Resources/Info.plist
plutil -replace CFBundleVersion -string "$BUILD_NUMBER" Resources/Info.plist

UNIVERSAL=1 ./scripts/build.sh

APP="build/Shipyard.app"
DMG="build/Shipyard-$VERSION.dmg"
LATEST="build/Shipyard.dmg"
STAGE="build/dmg"
rm -rf "$STAGE" "$DMG"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "Shipyard $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"
cp "$DMG" "$LATEST"
echo "DMG: $DMG ($(du -h "$DMG" | cut -f1))"

if [ "${2:-}" = "--publish" ]; then
    NOTES="Universal build (Apple Silicon + Intel), macOS 14+.

Install: open the DMG and drag **Shipyard** to Applications.

The app is not notarized. On first launch macOS will block it — open **System Settings → Privacy & Security** and click **Open Anyway**, or run:
\`\`\`
xattr -dr com.apple.quarantine /Applications/Shipyard.app
\`\`\`"
    gh release create "v$VERSION" "$DMG" "$LATEST" --title "Shipyard $VERSION" --notes "$NOTES" --target "$(git rev-parse HEAD)"
fi
