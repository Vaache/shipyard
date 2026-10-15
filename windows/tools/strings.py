import re
import sys
from pathlib import Path
from xml.sax.saxutils import escape

ROOT = Path(__file__).resolve().parent.parent
SOURCES = ROOT / "src" / "Shipyard"
OUTPUT = SOURCES / "Strings" / "en-US" / "Resources.resw"
EXTRA = {"App_Tagline": "Pull requests, without the browser"}

CALL = re.compile(r'\bL\.(?:T|F)\(\s*"((?:[^"\\]|\\.)*)"\s*,\s*"((?:[^"\\]|\\.)*)"')


def unescape(s):
    return bytes(s, "utf-8").decode("unicode_escape").encode("latin-1").decode("utf-8")


def collect():
    found = dict(EXTRA)
    conflicts = []
    for path in sorted(SOURCES.rglob("*.cs")):
        text = path.read_text(encoding="utf-8")
        for key, value in CALL.findall(text):
            value = unescape(value)
            if key in found and found[key] != value:
                conflicts.append(f"{key}: {found[key]!r} vs {value!r} ({path.name})")
                continue
            found[key] = value
    return found, conflicts


def render(strings):
    lines = [
        '<?xml version="1.0" encoding="utf-8"?>',
        "<root>",
        '  <resheader name="resmimetype"><value>text/microsoft-resx</value></resheader>',
        '  <resheader name="version"><value>2.0</value></resheader>',
        '  <resheader name="reader"><value>System.Resources.ResXResourceReader, System.Windows.Forms, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089</value></resheader>',
        '  <resheader name="writer"><value>System.Resources.ResXResourceWriter, System.Windows.Forms, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089</value></resheader>',
    ]
    for key in sorted(strings):
        lines.append(f'  <data name="{escape(key)}" xml:space="preserve"><value>{escape(strings[key])}</value></data>')
    lines.append("</root>")
    return "\n".join(lines) + "\n"


def main():
    strings, conflicts = collect()
    if conflicts:
        print("Conflicting defaults:\n  " + "\n  ".join(conflicts))
        sys.exit(1)
    content = render(strings)
    if "--check" in sys.argv:
        current = OUTPUT.read_text(encoding="utf-8") if OUTPUT.exists() else ""
        if current != content:
            print(f"{OUTPUT} is out of date; run python windows/tools/strings.py")
            sys.exit(1)
        print(f"{len(strings)} strings up to date")
        return
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(content, encoding="utf-8")
    print(f"wrote {len(strings)} strings to {OUTPUT}")


if __name__ == "__main__":
    main()
