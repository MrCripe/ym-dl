"""Material Design 3 tokens, fonts and the global stylesheet."""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path

from PySide6.QtCore import Qt
from PySide6.QtGui import QColor, QFont, QFontDatabase, QGuiApplication, QIcon, QPainter, QPixmap

from .icons import ICONS

ASSETS = Path(__file__).resolve().parent.parent.parent / "assets"

FONT_STACK = (
    "Segoe UI Variable Display",
    "Segoe UI",
    "Inter",
    "Roboto",
    "Noto Sans",
    "DejaVu Sans",
    "sans-serif",
)

TYPE_SCALE = {
    "display": 34,
    "headline": 24,
    "title-large": 20,
    "title": 16,
    "title-small": 14,
    "body-large": 15,
    "body": 13,
    "body-small": 12,
    "label": 13,
    "label-small": 11,
}

RADIUS = {"xs": 4, "sm": 8, "md": 12, "lg": 16, "xl": 24, "full": 999}


@dataclass(frozen=True)
class Palette:
    primary: str
    on_primary: str
    primary_container: str
    on_primary_container: str
    secondary: str
    on_secondary: str
    secondary_container: str
    on_secondary_container: str
    tertiary: str
    on_tertiary: str
    tertiary_container: str
    on_tertiary_container: str
    error: str
    on_error: str
    error_container: str
    on_error_container: str
    surface: str
    on_surface: str
    surface_variant: str
    on_surface_variant: str
    surface_lowest: str
    surface_low: str
    surface_container: str
    surface_high: str
    surface_highest: str
    outline: str
    outline_variant: str
    inverse_surface: str
    inverse_on_surface: str
    inverse_primary: str
    success: str
    warning: str
    dark: bool

    def color(self, name: str) -> QColor:
        return QColor(getattr(self, name))


LIGHT = Palette(
    primary="#6750A4",
    on_primary="#FFFFFF",
    primary_container="#EADDFF",
    on_primary_container="#21005D",
    secondary="#625B71",
    on_secondary="#FFFFFF",
    secondary_container="#E8DEF8",
    on_secondary_container="#1D192B",
    tertiary="#7D5260",
    on_tertiary="#FFFFFF",
    tertiary_container="#FFD8E4",
    on_tertiary_container="#31111D",
    error="#B3261E",
    on_error="#FFFFFF",
    error_container="#F9DEDC",
    on_error_container="#410E0B",
    surface="#FEF7FF",
    on_surface="#1D1B20",
    surface_variant="#E7E0EC",
    on_surface_variant="#49454F",
    surface_lowest="#FFFFFF",
    surface_low="#F7F2FA",
    surface_container="#F3EDF7",
    surface_high="#ECE6F0",
    surface_highest="#E6E0E9",
    outline="#79747E",
    outline_variant="#CAC4D0",
    inverse_surface="#322F35",
    inverse_on_surface="#F5EFF7",
    inverse_primary="#D0BCFF",
    success="#1B6B3A",
    warning="#8A5300",
    dark=False,
)

DARK = Palette(
    primary="#D0BCFF",
    on_primary="#381E72",
    primary_container="#4F378B",
    on_primary_container="#EADDFF",
    secondary="#CCC2DC",
    on_secondary="#332D41",
    secondary_container="#4A4458",
    on_secondary_container="#E8DEF8",
    tertiary="#EFB8C8",
    on_tertiary="#492532",
    tertiary_container="#633B48",
    on_tertiary_container="#FFD8E4",
    error="#F2B8B5",
    on_error="#601410",
    error_container="#8C1D18",
    on_error_container="#F9DEDC",
    surface="#141218",
    on_surface="#E6E0E9",
    surface_variant="#49454F",
    on_surface_variant="#CAC4D0",
    surface_lowest="#0F0D13",
    surface_low="#1D1B20",
    surface_container="#211F26",
    surface_high="#2B2930",
    surface_highest="#36343B",
    outline="#938F99",
    outline_variant="#494F54",
    inverse_surface="#E6E0E9",
    inverse_on_surface="#322F35",
    inverse_primary="#6750A4",
    success="#7FD69B",
    warning="#FFB95C",
    dark=True,
)

_current: Palette = LIGHT
_font_family = "Segoe UI"
_icon_family = ""
_icon_family_filled = ""


def palette() -> Palette:
    return _current


def font_family() -> str:
    return _font_family


def load_fonts() -> None:
    global _font_family, _icon_family, _icon_family_filled
    available = set(QFontDatabase.families())
    _font_family = next((name for name in FONT_STACK if name in available), "sans-serif")

    for key, filename in (("icon", "icons.ttf"), ("icon_filled", "icons_filled.ttf")):
        path = ASSETS / filename
        family = ""
        if path.exists():
            font_id = QFontDatabase.addApplicationFont(str(path))
            families = QFontDatabase.applicationFontFamilies(font_id) if font_id >= 0 else []
            family = families[0] if families else ""
        if key == "icon":
            _icon_family = family
        else:
            _icon_family_filled = family


def font(size_key: str = "body", weight: int = QFont.Weight.Normal) -> QFont:
    value = QFont(_font_family)
    value.setPixelSize(TYPE_SCALE.get(size_key, 13))
    value.setWeight(weight)
    return value


def icon_char(name: str) -> str:
    return ICONS.get(name, "")


def icon_font(size: int, filled: bool = False) -> QFont:
    family = _icon_family_filled if filled else _icon_family
    value = QFont(family or _font_family)
    value.setPixelSize(size)
    return value


def make_icon(name: str, color: str | QColor, size: int = 20, filled: bool = False) -> QIcon:
    pixmap = QPixmap(size, size)
    pixmap.fill(QColor(0, 0, 0, 0))
    painter = QPainter(pixmap)
    painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
    painter.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
    painter.setFont(icon_font(size, filled))
    painter.setPen(QColor(color))
    painter.drawText(pixmap.rect(), Qt.AlignmentFlag.AlignCenter, icon_char(name))
    painter.end()
    return QIcon(pixmap)


def apply_theme(app: QGuiApplication, name: str) -> Palette:
    """`name` is system/light/dark; returns the palette actually applied."""
    global _current
    dark = name == "dark"
    if name == "system":
        hints = QGuiApplication.styleHints()
        dark = bool(getattr(hints, "colorScheme", lambda: None)() == 2) if hints else False
    _current = DARK if dark else LIGHT
    app.setFont(font("body"))
    app.setStyleSheet(stylesheet(_current))
    return _current


def stylesheet(p: Palette) -> str:
    return f"""
    * {{
        font-family: "{_font_family}";
        outline: none;
    }}
    QWidget {{
        color: {p.on_surface};
        background: transparent;
    }}
    QMainWindow, #Root {{
        background: {p.surface};
    }}
    QLabel {{
        background: transparent;
    }}
    QToolTip {{
        background: {p.inverse_surface};
        color: {p.inverse_on_surface};
        border: none;
        border-radius: 6px;
        padding: 6px 10px;
        font-size: {TYPE_SCALE['body-small']}px;
    }}
    QScrollArea {{
        border: none;
        background: transparent;
    }}
    QScrollBar:vertical {{
        background: transparent;
        width: 10px;
        margin: 2px;
    }}
    QScrollBar::handle:vertical {{
        background: {p.outline_variant};
        min-height: 36px;
        border-radius: 5px;
    }}
    QScrollBar::handle:vertical:hover {{
        background: {p.outline};
    }}
    QScrollBar::add-line:vertical, QScrollBar::sub-line:vertical,
    QScrollBar::add-page:vertical, QScrollBar::sub-page:vertical {{
        height: 0px;
        background: transparent;
    }}
    QScrollBar:horizontal {{
        background: transparent;
        height: 10px;
        margin: 2px;
    }}
    QScrollBar::handle:horizontal {{
        background: {p.outline_variant};
        min-width: 36px;
        border-radius: 5px;
    }}
    QScrollBar::add-line:horizontal, QScrollBar::sub-line:horizontal,
    QScrollBar::add-page:horizontal, QScrollBar::sub-page:horizontal {{
        width: 0px;
        background: transparent;
    }}
    QLineEdit, QSpinBox {{
        background: {p.surface_highest};
        color: {p.on_surface};
        border: none;
        border-bottom: 1px solid {p.on_surface_variant};
        border-top-left-radius: {RADIUS['xs']}px;
        border-top-right-radius: {RADIUS['xs']}px;
        padding: 10px 14px;
        selection-background-color: {p.primary};
        selection-color: {p.on_primary};
        font-size: {TYPE_SCALE['body-large']}px;
    }}
    QLineEdit:focus, QSpinBox:focus {{
        border-bottom: 2px solid {p.primary};
        padding-bottom: 9px;
    }}
    QLineEdit:disabled, QSpinBox:disabled {{
        color: {p.on_surface_variant};
    }}
    QComboBox {{
        background: {p.surface_highest};
        color: {p.on_surface};
        border: 1px solid {p.outline_variant};
        border-radius: {RADIUS['sm']}px;
        padding: 8px 12px;
        min-height: 22px;
        font-size: {TYPE_SCALE['body']}px;
    }}
    QComboBox:hover {{
        border-color: {p.outline};
    }}
    QComboBox::drop-down {{
        border: none;
        width: 28px;
    }}
    QComboBox QAbstractItemView {{
        background: {p.surface_container};
        color: {p.on_surface};
        border: 1px solid {p.outline_variant};
        border-radius: {RADIUS['sm']}px;
        padding: 6px;
        selection-background-color: {p.secondary_container};
        selection-color: {p.on_secondary_container};
        outline: none;
    }}
    QSlider::groove:horizontal {{
        height: 4px;
        background: {p.surface_highest};
        border-radius: 2px;
    }}
    QSlider::sub-page:horizontal {{
        background: {p.primary};
        border-radius: 2px;
    }}
    QSlider::handle:horizontal {{
        background: {p.primary};
        width: 8px;
        height: 8px;
        margin: -8px 0;
        border-radius: 8px;
    }}
    QSlider::handle:horizontal:hover {{
        width: 10px;
        height: 10px;
        margin: -9px 0;
        border-radius: 9px;
    }}
    QProgressBar {{
        background: {p.surface_highest};
        border: none;
        border-radius: 2px;
        height: 4px;
        text-align: center;
        color: transparent;
    }}
    QProgressBar::chunk {{
        background: {p.primary};
        border-radius: 2px;
    }}
    QMenu {{
        background: {p.surface_container};
        color: {p.on_surface};
        border: 1px solid {p.outline_variant};
        border-radius: {RADIUS['sm']}px;
        padding: 6px;
    }}
    QMenu::item {{
        padding: 8px 18px;
        border-radius: 6px;
    }}
    QMenu::item:selected {{
        background: {p.secondary_container};
        color: {p.on_secondary_container};
    }}
    """
