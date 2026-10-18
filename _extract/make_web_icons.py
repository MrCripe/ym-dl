"""Builds the bundled Material Symbols subset for the web UI.

Collects every icon name the frontend asks for, then writes
src/assets/icons.woff2 plus src/styles/icons.css with a matching @font-face.
Run from the repo root:  python _extract/make_web_icons.py
"""

import re
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

ROOT = Path(__file__).resolve().parent.parent
SRC = Path(__import__("os").environ["TEMP"]) / "msr.ttf"
CODEPOINTS = Path(__import__("os").environ["TEMP"]) / "msr.codepoints"
OUT_FONT = ROOT / "src" / "assets" / "icons.woff2"
OUT_CSS = ROOT / "src" / "styles" / "icons.css"

# Names that appear only in dynamic strings still need to be shipped.
EXTRA = """
graphic_eq remove expand_content close check chevron_right arrow_forward arrow_back
""".split()

def collect() -> list[str]:
    """Every quoted snake_case identifier in the frontend that names a real glyph.

    Being generous is cheap (a few KB) and removes the risk of a missing icon
    after a rename, which is invisible until someone opens that screen.
    """
    names: set[str] = set(EXTRA)
    quoted = re.compile(r'"([a-z][a-z0-9_]{2,})"')
    for path in (ROOT / "src").rglob("*.tsx"):
        names.update(quoted.findall(path.read_text(encoding="utf-8")))
    return sorted(names)


def main() -> None:
    mapping = {}
    for line in CODEPOINTS.read_text(encoding="utf-8").splitlines():
        parts = line.split()
        if len(parts) == 2:
            mapping[parts[0]] = int(parts[1], 16)

    names = [name for name in collect() if name in mapping]
    missing = [name for name in collect() if name not in mapping]
    if missing:
        print("unknown icon names:", ", ".join(missing))

    font = TTFont(SRC)
    options = subset.Options()
    # Material Symbols ships its icons as required ligatures ("home" -> one
    # glyph), so the GSUB lookups must survive. Their input is the plain Latin
    # letters of the icon name: without those glyphs fontTools prunes every
    # ligature rule and the UI shows the name as text.
    options.layout_features = ["*"]
    options.name_IDs = ["*"]
    options.notdef_outline = True
    options.glyph_names = True
    # Layout closure would pull in every icon glyph reachable from the ligature
    # rules, turning a 17 KB subset into a 1.2 MB full font.
    options.layout_closure = False
    subsetter = subset.Subsetter(options=options)
    letters = {ord(char) for name in names for char in name}
    subsetter.populate(unicodes=[mapping[name] for name in names] + sorted(letters))
    subsetter.subset(font)
    font = instancer.instantiateVariableFont(font, {"wght": 400, "opsz": 24}, inplace=True)

    OUT_FONT.parent.mkdir(parents=True, exist_ok=True)
    font.flavor = "woff2"
    font.save(OUT_FONT)

    OUT_CSS.write_text(
        "@font-face {\n"
        '  font-family: "Material Symbols Rounded";\n'
        "  font-style: normal;\n"
        "  font-weight: 100 700;\n"
        "  font-display: block;\n"
        f'  src: url("../assets/icons.woff2") format("woff2");\n'
        "}\n\n"
        ".material-symbols-rounded {\n"
        '  font-family: "Material Symbols Rounded";\n'
        "  font-weight: normal;\n"
        "  font-style: normal;\n"
        "  font-size: 24px;\n"
        "  line-height: 1;\n"
        "  letter-spacing: normal;\n"
        "  text-transform: none;\n"
        "  display: inline-block;\n"
        "  white-space: nowrap;\n"
        "  word-wrap: normal;\n"
        "  direction: ltr;\n"
        "  -webkit-font-feature-settings: 'rlig';\n  font-feature-settings: 'rlig';\n"
        "  -webkit-font-smoothing: antialiased;\n"
        "  font-variation-settings: 'FILL' 0, 'wght' 400, 'GRAD' 0, 'opsz' 24;\n"
        "  user-select: none;\n"
        "}\n",
        encoding="utf-8",
    )
    print(f"{len(names)} icons -> {OUT_FONT.relative_to(ROOT)} ({OUT_FONT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
