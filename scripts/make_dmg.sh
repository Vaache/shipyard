#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."

APP="${1:?Usage: scripts/make_dmg.sh <app> <output.dmg>}"
OUT="${2:?Usage: scripts/make_dmg.sh <app> <output.dmg>}"
VOLUME="PullYard"
README="Resources/DMG/How to Install.rtf"
WORK="$(mktemp -d)"
RW="$WORK/rw.dmg"
trap 'rm -rf "$WORK"' EXIT

sed 's/^format = "UDZO"/format = "UDRW"/' scripts/dmg_settings.py > "$WORK/settings.py"
python3 -m dmgbuild -s "$WORK/settings.py" -D app="$APP" -D readme="$README" "$VOLUME" "$RW" >/dev/null

MOUNT="$(hdiutil attach -readwrite -noverify -noautoopen "$RW" | awk -F'\t' '/Apple_HFS/ {print $NF}')"
detach() { hdiutil detach -quiet "$MOUNT" 2>/dev/null || hdiutil detach -quiet -force "$MOUNT" 2>/dev/null || true; }
trap 'detach; rm -rf "$WORK"' EXIT

mkdir -p "$MOUNT/.background"
cp "$MOUNT/.background.tiff" "$MOUNT/.background/background.tiff"

osascript - "$MOUNT" "$(basename "$APP")" "$(basename "$README")" <<'APPLESCRIPT'
on run argv
  set mountPath to item 1 of argv
  set appName to item 2 of argv
  set readmeName to item 3 of argv
  set volumeAlias to (POSIX file mountPath) as alias
  tell application "Finder"
    set theDisk to folder volumeAlias
    set theWindow to make new Finder window to theDisk
    set current view of theWindow to icon view
    set toolbar visible of theWindow to false
    set statusbar visible of theWindow to false
    set sidebar width of theWindow to 0
    set bounds of theWindow to {200, 120, 860, 632}
    set theOptions to icon view options of theWindow
    set arrangement of theOptions to not arranged
    set icon size of theOptions to 96
    set text size of theOptions to 13
    set label position of theOptions to bottom
    tell theDisk
      set background picture of theOptions to file ".background:background.tiff"
      set position of item appName to {165, 170}
      set position of item "Applications" to {495, 170}
      set position of item readmeName to {330, 268}
      update without registering applications
    end tell
    delay 2
    close theWindow
  end tell
end run
APPLESCRIPT

sync
sleep 1
python3 - "$MOUNT/.DS_Store" <<'PY'
import sys
data = open(sys.argv[1], "rb").read()
missing = [code for code in (b"pBBk", b"icvp", b"bwsp") if code not in data]
if missing:
    sys.exit("DMG layout incomplete, missing: " + ", ".join(c.decode() for c in missing))
PY
chflags hidden "$MOUNT/.background" 2>/dev/null || true
detach
trap 'rm -rf "$WORK"' EXIT

rm -f "$OUT"
hdiutil convert "$RW" -format UDZO -imagekey zlib-level=9 -o "$OUT" >/dev/null
