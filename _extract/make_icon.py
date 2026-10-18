"""Generates the app icon (assets/icon.png) used by `tauri icon`."""

from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
SIZE = 1024
GLYPH = "\ue1b8"  # graphic_eq in Material Symbols Rounded


def vertical_gradient(size: int, top: tuple, bottom: tuple) -> Image.Image:
    image = Image.new("RGB", (1, size))
    draw = ImageDraw.Draw(image)
    for y in range(size):
        ratio = y / max(1, size - 1)
        color = tuple(int(top[i] + (bottom[i] - top[i]) * ratio) for i in range(3))
        draw.point((0, y), fill=color)
    return image.resize((size, size))


def main() -> None:
    base = vertical_gradient(SIZE, (0x8F, 0x7A, 0xD1), (0x4A, 0x34, 0x8F))
    mask = Image.new("L", (SIZE, SIZE), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, SIZE - 1, SIZE - 1), radius=int(SIZE * 0.235), fill=255)
    icon = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    icon.paste(base, (0, 0), mask)

    font = ImageFont.truetype(str(ROOT / "assets" / "icons.ttf"), int(SIZE * 0.62))
    layer = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    box = draw.textbbox((0, 0), GLYPH, font=font)
    x = (SIZE - (box[2] - box[0])) / 2 - box[0]
    y = (SIZE - (box[3] - box[1])) / 2 - box[1]
    draw.text((x, y), GLYPH, font=font, fill=(255, 255, 255, 255))
    icon = Image.alpha_composite(icon, layer)

    target = ROOT / "assets" / "icon.png"
    icon.save(target)
    print(f"{target}: {target.stat().st_size} bytes")


if __name__ == "__main__":
    main()
