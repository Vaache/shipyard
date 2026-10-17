#!/usr/bin/env bash
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
tag="${1:?Usage: pin-release.sh <tag>}"
case "$tag" in v*) ;; *) tag="v$tag" ;; esac
manifest="$here/io.github.pullyard.PullYard.yml"
url="$(sed -n 's|^ *url: \(https://github.com/pullyard/pullyard.git\)$|\1|p' "$manifest" | head -1)"
commit="$(git ls-remote "$url" "refs/tags/${tag}^{}" | cut -f1)"
[ -n "$commit" ] || commit="$(git ls-remote "$url" "refs/tags/${tag}" | cut -f1)"
[ -n "$commit" ] || { echo "tag ${tag} not found at ${url}" >&2; exit 1; }
python3 - "$manifest" "$tag" "$commit" <<'EOF'
import re, sys
path, tag, commit = sys.argv[1:]
text = open(path).read()
block = re.compile(r"(url: https://github\.com/pullyard/pullyard\.git\n\s+tag: )\S+(\n\s+commit: )\S+")
text, n = block.subn(lambda m: f"{m.group(1)}{tag}{m.group(2)}{commit}", text)
if n != 1:
    sys.exit("app source block not found")
text, n = re.subn(r"(PULLYARD_VERSION: )\S+", lambda m: m.group(1) + tag[1:], text)
if n != 1:
    sys.exit("PULLYARD_VERSION not found")
open(path, "w").write(text)
EOF
echo "pinned ${tag} (${commit})"
