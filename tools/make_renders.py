"""Composes app screenshots into clean marketing renders.

Each capture in `docs/raw` becomes a card on the app's own gradient: rounded
corners, a soft shadow, a hairline edge. Raw window captures look like a
screenshot of a window; these look like the product.

Two things are fixed here rather than by hand:
  * the capture leaves a few pixels of desktop around the window, so the edges
    are cropped;
  * the account name is personal, so it is blurred out before the render is
    written.

Run from the repo root:  python tools/make_renders.py
"""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "docs" / "raw"
TARGET = ROOT / "docs" / "screens"

WIDTH, HEIGHT = 1600, 1000
PADDING = 56
RADIUS = 16
SHADOW_BLUR = 30

# Desktop pixels left around the window by the capture, in capture coordinates.
CROP = (0, 0, 1180, 772)

# Boxes holding the signed-in account name, blurred before the card is drawn.
# Coordinates are relative to the cropped image.
REDACTIONS = {
    "main.png": [(952, 100, 1165, 132)],
    "search.png": [(952, 100, 1165, 132)],
    "settings.png": [(140, 222, 250, 248)],
}

TOP = (34, 40, 44)
BOTTOM = (16, 18, 20)


def gradient(width: int, height: int) -> Image.Image:
    image = Image.new("RGB", (width, height))
    draw = ImageDraw.Draw(image)
    for y in range(height):
        ratio = y / max(height - 1, 1)
        color = tuple(round(TOP[i] + (BOTTOM[i] - TOP[i]) * ratio) for i in range(3))
        draw.line([(0, y), (width, y)], fill=color)
    return image


def rounded(image: Image.Image, radius: int) -> Image.Image:
    mask = Image.new("L", image.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([(0, 0), image.size], radius=radius, fill=255)
    out = image.convert("RGBA")
    out.putalpha(mask)
    return out


def redact(shot: Image.Image, boxes: list[tuple[int, int, int, int]]) -> None:
    """Blurs each box in place, so a name cannot be read back."""
    for left, top, right, bottom in boxes:
        box = shot.crop((left, top, right, bottom))
        # Downscale then upscale: a plain blur leaves the glyph shapes readable.
        small = box.resize((max(2, box.width // 12), max(2, box.height // 12)), Image.BILINEAR)
        shot.paste(small.resize(box.size, Image.BILINEAR), (left, top))


def compose(source: Path, target: Path) -> None:
    shot = Image.open(source).convert("RGB").crop(CROP)
    redact(shot, REDACTIONS.get(source.name, []))

    box_w = WIDTH - PADDING * 2
    box_h = round(shot.height * box_w / shot.width)
    shot = shot.resize((box_w, box_h), Image.LANCZOS)

    canvas = gradient(WIDTH, HEIGHT).convert("RGBA")
    x = PADDING
    y = (HEIGHT - box_h) // 2

    shadow = Image.new("RGBA", (WIDTH, HEIGHT), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle(
        [(x + 4, y + 12), (x + box_w + 4, y + box_h + 12)], radius=RADIUS, fill=(0, 0, 0, 140)
    )
    canvas.alpha_composite(shadow.filter(ImageFilter.GaussianBlur(SHADOW_BLUR)))
    canvas.alpha_composite(rounded(shot, RADIUS), (x, y))

    # A hairline keeps the card from melting into the background on dark screens.
    ImageDraw.Draw(canvas).rounded_rectangle(
        [(x, y), (x + box_w - 1, y + box_h - 1)], radius=RADIUS, outline=(255, 255, 255, 30)
    )

    target.parent.mkdir(parents=True, exist_ok=True)
    canvas.convert("RGB").save(target, optimize=True)
    print(f"{source.name} -> {target.relative_to(ROOT)} ({target.stat().st_size} bytes)")


def main() -> None:
    sources = sorted(SOURCE.glob("*.png"))
    if not sources:
        raise SystemExit(f"no screenshots in {SOURCE}")
    for source in sources:
        compose(source, TARGET / source.name)


if __name__ == "__main__":
    main()
