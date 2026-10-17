#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
src="$here/io.github.pullyard.PullYard.yml"
dst="${1:-$here/io.github.pullyard.PullYard.local.yml}"
python3 - "$src" "$dst" "${PULLYARD_VERSION:-}" "${PULLYARD_UPDATES:-}" <<'EOF'
import os, re, sys
src, dst, version, updates = sys.argv[1:]
text = open(src).read()
root = os.path.relpath(os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(src))))), os.path.dirname(os.path.abspath(dst)))
skip = [".git", ".github", ".flatpak-builder", "builddir", "repo", "docs", "Sources", "Tests", "windows", "linux/target", "linux/dist", "linux/build-dir", "linux/.flatpak-builder"]
def local(m):
    pad = m.group(1)
    lines = [f"- type: dir", f"{pad}path: {root}", f"{pad}skip:"] + [f"{pad}  - {s}" for s in skip]
    return "\n".join(lines)
text, n = re.subn(r"- type: git\n(\s+)url: https://github\.com/pullyard/pullyard\.git\n\s+tag: \S+\n\s+commit: \S+", local, text)
if n != 1:
    sys.exit("app source block not found")
text = text.replace("      - cargo-sources.json", "      - " + os.path.relpath(os.path.join(os.path.dirname(os.path.abspath(src)), "cargo-sources.json"), os.path.dirname(os.path.abspath(dst))))
if version:
    text = re.sub(r"(PULLYARD_VERSION: )\S+", lambda m: m.group(1) + version.lstrip("vV"), text)
if updates:
    text = re.sub(r"(PULLYARD_UPDATES: )\S+", lambda m: m.group(1) + updates, text)
open(dst, "w").write(text)
EOF
echo "wrote $dst"
