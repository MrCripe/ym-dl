from fontTools.ttLib import TTFont
import pathlib, re
f = TTFont("src/assets/icons.woff2")
cmap = f.getBestCmap()
names = set()
for path in pathlib.Path("src").rglob("*.tsx"):
    names.update(re.findall(r'"([a-z][a-z0-9_]{2,})"', path.read_text(encoding="utf-8")))
cps = {}
for line in (pathlib.Path(__import__("os").environ["TEMP"]) / "msr.codepoints").read_text().splitlines():
    p = line.split()
    if len(p) == 2:
        cps[p[0]] = int(p[1], 16)
real = [n for n in names if n in cps]
print("icons:", len(real))
print("with digits:", [n for n in real if any(c.isdigit() for c in n)])
print("with j/z:", [n for n in real if set(n) & {"j", "z"}])
print("glyph for space:", 0x20 in cmap)
