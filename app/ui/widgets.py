"""Hand-painted Material 3 controls for Qt.

Qt has no Material Design 3 widget set, so the small subset this app needs is
drawn here: state layers, ripples, M3 shapes and colour roles all come from
`theme`. Everything else (fields, sliders, scroll bars) is styled by the global
stylesheet.
"""

from __future__ import annotations

from typing import Callable, Optional

from PySide6.QtCore import (
    QEasingCurve,
    QObject,
    QPoint,
    QPointF,
    QPropertyAnimation,
    QRect,
    QRectF,
    QSize,
    Qt,
    QVariantAnimation,
    Signal,
)
from PySide6.QtGui import (
    QColor,
    QFont,
    QFontMetrics,
    QPainter,
    QPainterPath,
    QPen,
)
from PySide6.QtWidgets import (
    QAbstractButton,
    QComboBox,
    QFrame,
    QGraphicsDropShadowEffect,
    QHBoxLayout,
    QLabel,
    QLineEdit,
    QProgressBar,
    QSizePolicy,
    QVBoxLayout,
    QWidget,
)

from . import theme
from .icons import ICONS

ALIGN_CENTER = Qt.AlignmentFlag.AlignCenter


def _path(rect: QRectF, radius: float) -> QPainterPath:
    path = QPainterPath()
    path.addRoundedRect(rect, radius, radius)
    return path


def _with_alpha(color: QColor, alpha: float) -> QColor:
    copy = QColor(color)
    copy.setAlphaF(max(0.0, min(1.0, alpha)))
    return copy


class Pressable(QAbstractButton):
    """Shared ripple + state layer behaviour for buttons and toggles."""

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setAttribute(Qt.WidgetAttribute.WA_Hover, True)
        self.setFocusPolicy(Qt.FocusPolicy.TabFocus)
        self._ripple = 0.0
        self._ripple_origin = QPointF(0, 0)
        self._ripple_radius = 0.0
        self._hovered = False
        self._anim = QVariantAnimation(self)
        self._anim.setDuration(360)
        self._anim.setEasingCurve(QEasingCurve.Type.OutCubic)
        self._anim.setStartValue(0.0)
        self._anim.setEndValue(1.0)
        self._anim.valueChanged.connect(self._on_ripple_value)
        self._anim.finished.connect(self._on_ripple_done)

    def _on_ripple_value(self, value: float) -> None:
        self._ripple = float(value)
        self.update()

    def _on_ripple_done(self) -> None:
        self._ripple = 0.0
        self.update()

    def mousePressEvent(self, event) -> None:  # noqa: N802
        if event.button() == Qt.MouseButton.LeftButton:
            self._ripple_origin = event.position()
            corners = (
                QPointF(0, 0),
                QPointF(self.width(), 0),
                QPointF(0, self.height()),
                QPointF(self.width(), self.height()),
            )
            self._ripple_radius = max(
                (corner - self._ripple_origin).manhattanLength() for corner in corners
            )
            self._anim.stop()
            self._anim.start()
        super().mousePressEvent(event)

    def enterEvent(self, event) -> None:  # noqa: N802
        self._hovered = True
        self.update()
        super().enterEvent(event)

    def leaveEvent(self, event) -> None:  # noqa: N802
        self._hovered = False
        self.update()
        super().leaveEvent(event)

    def _paint_state(
        self,
        painter: QPainter,
        rect: QRectF,
        radius: float,
        layer: QColor,
        ripple: QColor,
        clip: bool = True,
    ) -> None:
        painter.save()
        if clip:
            painter.setClipPath(_path(rect, radius))
        if self._hovered and self.isEnabled():
            painter.fillPath(_path(rect, radius), _with_alpha(layer, 0.08))
        if self.isDown():
            painter.fillPath(_path(rect, radius), _with_alpha(layer, 0.12))
        if self._ripple > 0:
            painter.setPen(Qt.PenStyle.NoPen)
            painter.setBrush(_with_alpha(ripple, 0.16 * (1 - self._ripple)))
            size = self._ripple_radius * self._ripple
            painter.drawEllipse(self._ripple_origin, size, size)
        painter.restore()


class Button(Pressable):
    """M3 button: filled / tonal / outlined / text / elevated."""

    KINDS = ("filled", "tonal", "outlined", "text", "elevated")

    def __init__(
        self,
        text: str = "",
        icon: str = "",
        kind: str = "filled",
        parent: QWidget | None = None,
        compact: bool = False,
    ) -> None:
        super().__init__(parent)
        self._text = text
        self._icon = icon
        self._kind = kind if kind in self.KINDS else "filled"
        self._compact = compact
        self._icon_size = 18
        self.setFont(theme.font("label", QFont.Weight.DemiBold))
        self.setSizePolicy(QSizePolicy.Policy.Minimum, QSizePolicy.Policy.Fixed)
        self.setMinimumHeight(32 if compact else 40)
        self._sync_shadow()

    def _sync_shadow(self) -> None:
        if self._kind == "elevated":
            effect = QGraphicsDropShadowEffect(self)
            effect.setBlurRadius(12)
            effect.setOffset(0, 3)
            effect.setColor(QColor(0, 0, 0, 60))
            self.setGraphicsEffect(effect)
        else:
            self.setGraphicsEffect(None)

    def set_kind(self, kind: str) -> None:
        self._kind = kind
        self._sync_shadow()
        self.update()

    def set_text(self, text: str) -> None:
        self._text = text
        self.updateGeometry()
        self.update()

    def set_icon(self, icon: str) -> None:
        self._icon = icon
        self.update()

    def sizeHint(self) -> QSize:  # noqa: N802
        metrics = QFontMetrics(self.font())
        text_width = metrics.horizontalAdvance(self._text) if self._text else 0
        icon_width = (self._icon_size + 8) if self._icon else 0
        pad = 16 if self._compact else 24
        return QSize(text_width + icon_width + pad * 2, 32 if self._compact else 40)

    def _colors(self) -> tuple[QColor, QColor, QColor, Optional[QColor]]:
        p = theme.palette()
        if self._kind == "filled":
            return QColor(p.primary), QColor(p.on_primary), QColor(p.on_primary), None
        if self._kind == "tonal":
            return QColor(p.secondary_container), QColor(p.on_secondary_container), QColor(p.on_secondary_container), None
        if self._kind == "elevated":
            return QColor(p.surface_low), QColor(p.primary), QColor(p.primary), None
        if self._kind == "outlined":
            return QColor(0, 0, 0, 0), QColor(p.primary), QColor(p.primary), QColor(p.outline)
        return QColor(0, 0, 0, 0), QColor(p.primary), QColor(p.primary), None

    def paintEvent(self, event) -> None:  # noqa: N802
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
        rect = QRectF(self.rect()).adjusted(0.5, 0.5, -0.5, -0.5)
        radius = rect.height() / 2
        background, foreground, layer, border = self._colors()

        if not self.isEnabled():
            background = _with_alpha(background, 0.12) if background.alpha() else background
            foreground = _with_alpha(QColor(theme.palette().on_surface), 0.38)

        if background.alpha():
            painter.setPen(Qt.PenStyle.NoPen)
            painter.setBrush(background)
            painter.drawPath(_path(rect, radius))
        if border is not None:
            pen = QPen(_with_alpha(border, 0.9 if self.isEnabled() else 0.4))
            pen.setWidthF(1.0)
            painter.setPen(pen)
            painter.setBrush(Qt.BrushStyle.NoBrush)
            painter.drawPath(_path(rect, radius))

        self._paint_state(painter, rect, radius, layer, layer)

        painter.setPen(foreground)
        painter.setFont(self.font())
        metrics = QFontMetrics(self.font())
        text_width = metrics.horizontalAdvance(self._text) if self._text else 0
        icon_width = (self._icon_size + 8) if self._icon else 0
        total = text_width + icon_width
        start_x = rect.center().x() - total / 2
        if self._icon:
            painter.setFont(theme.icon_font(self._icon_size))
            painter.drawText(
                QRectF(start_x, rect.top(), self._icon_size, rect.height()),
                ALIGN_CENTER,
                theme.icon_char(self._icon),
            )
            painter.setFont(self.font())
            start_x += self._icon_size + 8
        if self._text:
            painter.drawText(
                QRectF(start_x, rect.top(), text_width, rect.height()),
                ALIGN_CENTER,
                self._text,
            )
        painter.end()


class IconButton(Pressable):
    def __init__(
        self,
        icon: str,
        tooltip: str = "",
        parent: QWidget | None = None,
        size: int = 40,
        icon_size: int = 20,
        variant: str = "standard",
    ) -> None:
        super().__init__(parent)
        self._icon = icon
        self._size = size
        self._icon_size = icon_size
        self._variant = variant
        self.setFixedSize(size, size)
        self.setCheckable(False)
        if tooltip:
            self.setToolTip(tooltip)

    def set_icon(self, icon: str) -> None:
        self._icon = icon
        self.update()

    def paintEvent(self, event) -> None:  # noqa: N802
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
        rect = QRectF(self.rect()).adjusted(0.5, 0.5, -0.5, -0.5)
        radius = rect.height() / 2
        p = theme.palette()

        if self._variant == "filled":
            painter.setPen(Qt.PenStyle.NoPen)
            painter.setBrush(QColor(p.primary))
            painter.drawPath(_path(rect, radius))
            color = QColor(p.on_primary)
        elif self._variant == "tonal":
            painter.setPen(Qt.PenStyle.NoPen)
            painter.setBrush(QColor(p.secondary_container))
            painter.drawPath(_path(rect, radius))
            color = QColor(p.on_secondary_container)
        elif self._variant == "outlined":
            pen = QPen(QColor(p.outline))
            painter.setPen(pen)
            painter.setBrush(Qt.BrushStyle.NoBrush)
            painter.drawPath(_path(rect, radius))
            color = QColor(p.on_surface_variant)
        else:
            color = QColor(p.on_surface_variant)

        self._paint_state(painter, rect, radius, color, color)

        painter.setPen(color)
        painter.setFont(theme.icon_font(self._icon_size, filled=self.isChecked()))
        painter.drawText(rect, ALIGN_CENTER, theme.icon_char(self._icon))
        painter.end()


class Checkbox(Pressable):
    def __init__(self, parent: QWidget | None = None, size: int = 18) -> None:
        super().__init__(parent)
        self._box = size
        self.setCheckable(True)
        self.setFixedSize(size + 18, size + 18)

    def sizeHint(self) -> QSize:  # noqa: N802
        return QSize(self._box + 18, self._box + 18)

    def paintEvent(self, event) -> None:  # noqa: N802
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
        p = theme.palette()
        outer = QRectF(self.rect())
        box_rect = QRectF(
            outer.center().x() - self._box / 2,
            outer.center().y() - self._box / 2,
            self._box,
            self._box,
        )
        radius = 3.0

        self._paint_state(painter, outer, outer.width() / 2, QColor(p.on_surface), QColor(p.on_surface))

        if self.isChecked():
            painter.setPen(Qt.PenStyle.NoPen)
            painter.setBrush(QColor(p.primary))
            painter.drawPath(_path(box_rect, radius))
            painter.setPen(QColor(p.on_primary))
            painter.setFont(theme.icon_font(int(self._box + 4)))
            painter.drawText(box_rect.adjusted(-2, -2, 2, 2), ALIGN_CENTER, theme.icon_char("check"))
        else:
            pen = QPen(QColor(p.on_surface_variant))
            pen.setWidthF(2.0)
            painter.setPen(pen)
            painter.setBrush(Qt.BrushStyle.NoBrush)
            painter.drawPath(_path(box_rect.adjusted(1, 1, -1, -1), radius))
        painter.end()


class Switch(Pressable):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setCheckable(True)
        self.setFixedSize(52, 32)
        self._knob_anim = QPropertyAnimation(self, b"knob", self)
        self._knob_anim.setDuration(180)
        self._knob_anim.setEasingCurve(QEasingCurve.Type.OutCubic)
        self._knob = 0.0 if not self.isChecked() else 1.0
        self.toggled.connect(self._animate)

    def _get_knob(self) -> float:
        return self._knob

    def _set_knob(self, value: float) -> None:
        self._knob = float(value)
        self.update()

    knob = property(_get_knob, _set_knob)

    def _animate(self, checked: bool) -> None:
        self._knob_anim.stop()
        self._knob_anim.setStartValue(self._knob)
        self._knob_anim.setEndValue(1.0 if checked else 0.0)
        self._knob_anim.start()

    def sizeHint(self) -> QSize:  # noqa: N802
        return QSize(52, 32)

    def paintEvent(self, event) -> None:  # noqa: N802
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        p = theme.palette()
        rect = QRectF(self.rect())
        track = QRectF(4, (rect.height() - 22) / 2, 44, 22)
        radius = track.height() / 2
        on = self.isChecked()

        if on:
            painter.setPen(Qt.PenStyle.NoPen)
            painter.setBrush(QColor(p.primary))
        else:
            painter.setPen(QPen(QColor(p.outline), 2.0))
            painter.setBrush(QColor(p.surface_highest))
        painter.drawPath(_path(track, radius))

        knob_color = QColor(p.on_primary) if on else QColor(p.outline)
        diameter = 12 + 12 * self._knob
        travel = track.width() - 12 - diameter
        cx = track.left() + 6 + diameter / 2 + travel * self._knob
        knob_rect = QRectF(cx - diameter / 2, track.center().y() - diameter / 2, diameter, diameter)
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(knob_color)
        painter.drawEllipse(knob_rect)
        painter.end()


class Card(QFrame):
    """M3 card: surface container with optional elevation."""

    VARIANTS = {
        "filled": "surface_container",
        "low": "surface_low",
        "high": "surface_high",
        "outlined": "surface",
    }

    def __init__(
        self,
        variant: str = "filled",
        radius: int = theme.RADIUS["md"],
        elevation: bool = False,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        self._variant = variant
        self._radius = radius
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground, False)
        if elevation:
            effect = QGraphicsDropShadowEffect(self)
            effect.setBlurRadius(20)
            effect.setOffset(0, 4)
            effect.setColor(QColor(0, 0, 0, 46))
            self.setGraphicsEffect(effect)

    def paintEvent(self, event) -> None:  # noqa: N802
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        p = theme.palette()
        rect = QRectF(self.rect()).adjusted(0.5, 0.5, -0.5, -0.5)
        color = QColor(getattr(p, self.VARIANTS.get(self._variant, "surface_container")))
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(color)
        painter.drawPath(_path(rect, self._radius))
        if self._variant == "outlined":
            painter.setPen(QPen(QColor(p.outline_variant), 1.0))
            painter.setBrush(Qt.BrushStyle.NoBrush)
            painter.drawPath(_path(rect, self._radius))
        painter.end()


class Badge(QLabel):
    TONES = {
        "primary": ("primary_container", "on_primary_container"),
        "secondary": ("secondary_container", "on_secondary_container"),
        "tertiary": ("tertiary_container", "on_tertiary_container"),
        "error": ("error_container", "on_error_container"),
        "neutral": ("surface_highest", "on_surface_variant"),
    }

    def __init__(self, text: str = "", tone: str = "neutral", parent: QWidget | None = None) -> None:
        super().__init__(text, parent)
        self._tone = tone
        self.setFont(theme.font("label-small", QFont.Weight.DemiBold))
        self.setAlignment(ALIGN_CENTER)
        self.setContentsMargins(0, 0, 0, 0)
        self.setFixedHeight(22)
        self.setMinimumWidth(44)
        self.set_tone(tone)

    def set_tone(self, tone: str) -> None:
        self._tone = tone
        background, foreground = self.TONES.get(tone, self.TONES["neutral"])
        p = theme.palette()
        self.setStyleSheet(
            f"background: {getattr(p, background)}; color: {getattr(p, foreground)};"
            f"border-radius: 11px; padding: 2px 10px;"
        )

    def setText(self, text: str) -> None:  # noqa: N802
        super().setText(text)
        self.setVisible(bool(text))


class TextField(QWidget):
    """Label + M3 filled text field."""

    submitted = Signal()

    def __init__(
        self,
        label: str = "",
        placeholder: str = "",
        password: bool = False,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        layout = QVBoxLayout(self)
        layout.setContentsMargins(0, 0, 0, 0)
        layout.setSpacing(6)
        self._label = QLabel(label)
        self._label.setFont(theme.font("label-small", QFont.Weight.DemiBold))
        self._label.setVisible(bool(label))
        layout.addWidget(self._label)

        self._edit = QLineEdit()
        self._edit.setPlaceholderText(placeholder)
        self._edit.setFont(theme.font("body-large"))
        if password:
            self._edit.setEchoMode(QLineEdit.EchoMode.Password)
        self._edit.returnPressed.connect(self.submitted.emit)
        layout.addWidget(self._edit)
        self.refresh()

    def refresh(self) -> None:
        p = theme.palette()
        self._label.setStyleSheet(f"color: {p.on_surface_variant}; background: transparent;")

    def text(self) -> str:
        return self._edit.text()

    def set_text(self, value: str) -> None:
        self._edit.setText(value)

    def edit(self) -> QLineEdit:
        return self._edit

    def set_error(self, message: str) -> None:
        self._label.setText(message)
        self._label.setStyleSheet(f"color: {theme.palette().error}; background: transparent;")

    def clear_error(self, label: str = "") -> None:
        self._label.setText(label)
        self.refresh()

    def setEnabled(self, enabled: bool) -> None:  # noqa: N802
        super().setEnabled(enabled)
        self._edit.setEnabled(enabled)


class Progress(QProgressBar):
    def __init__(self, parent: QWidget | None = None, height: int = 4) -> None:
        super().__init__(parent)
        self.setTextVisible(False)
        self.setFixedHeight(height)
        self.setRange(0, 100)
        self.setValue(0)


class Divider(QFrame):
    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setFixedHeight(1)
        self.setStyleSheet(f"background: {theme.palette().outline_variant};")


class NavRail(QWidget):
    """Vertical navigation rail with the M3 pill indicator."""

    selected = Signal(str)

    def __init__(self, items: list[tuple[str, str, str]], parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._items = items
        self._current = items[0][0] if items else ""
        self._hover: Optional[str] = None
        self.setFixedWidth(96)
        self.setMouseTracking(True)
        self.setAttribute(Qt.WidgetAttribute.WA_Hover, True)

    def current(self) -> str:
        return self._current

    def set_current(self, key: str) -> None:
        if key != self._current:
            self._current = key
            self.selected.emit(key)
            self.update()

    def _item_rect(self, index: int) -> QRectF:
        return QRectF(8, 16 + index * 76, self.width() - 16, 68)

    def paintEvent(self, event) -> None:  # noqa: N802
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
        p = theme.palette()
        for index, (key, icon, label) in enumerate(self._items):
            rect = self._item_rect(index)
            active = key == self._current
            indicator = QRectF(rect.center().x() - 28, rect.top() + 4, 56, 32)
            if active:
                painter.setPen(Qt.PenStyle.NoPen)
                painter.setBrush(QColor(p.secondary_container))
                painter.drawPath(_path(indicator, 16))
            elif self._hover == key:
                painter.setPen(Qt.PenStyle.NoPen)
                painter.setBrush(_with_alpha(QColor(p.on_surface), 0.08))
                painter.drawPath(_path(indicator, 16))

            color = QColor(p.on_secondary_container) if active else QColor(p.on_surface_variant)
            painter.setPen(color)
            painter.setFont(theme.icon_font(22, filled=active))
            painter.drawText(indicator, ALIGN_CENTER, theme.icon_char(icon))

            painter.setFont(theme.font("label-small", QFont.Weight.DemiBold if active else QFont.Weight.Normal))
            painter.setPen(color)
            painter.drawText(QRectF(rect.left(), indicator.bottom() + 6, rect.width(), 16), ALIGN_CENTER, label)
        painter.end()

    def _hit(self, point: QPoint) -> Optional[str]:
        for index, (key, _icon, _label) in enumerate(self._items):
            if self._item_rect(index).contains(QPointF(point)):
                return key
        return None

    def mousePressEvent(self, event) -> None:  # noqa: N802
        key = self._hit(event.position().toPoint())
        if key:
            self.set_current(key)

    def mouseMoveEvent(self, event) -> None:  # noqa: N802
        hover = self._hit(event.position().toPoint())
        if hover != self._hover:
            self._hover = hover
            self.setCursor(
                Qt.CursorShape.PointingHandCursor if hover else Qt.CursorShape.ArrowCursor
            )
            self.update()

    def leaveEvent(self, event) -> None:  # noqa: N802
        self._hover = None
        self.update()
        super().leaveEvent(event)


class Segmented(QWidget):
    """M3 segmented button used for quality and theme choice."""

    changed = Signal(int)

    def __init__(self, options: list[str], current: int = 0, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._options = options
        self._current = max(0, min(current, len(options) - 1))
        self._hover = -1
        self.setMouseTracking(True)
        self.setFixedHeight(40)
        self.setMinimumWidth(120 * len(options))
        self.setCursor(Qt.CursorShape.PointingHandCursor)

    def current(self) -> int:
        return self._current

    def set_current(self, index: int, emit: bool = True) -> None:
        index = max(0, min(index, len(self._options) - 1))
        if index != self._current:
            self._current = index
            self.update()
            if emit:
                self.changed.emit(index)

    def _segment_width(self) -> float:
        return self.width() / max(1, len(self._options))

    def paintEvent(self, event) -> None:  # noqa: N802
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        painter.setRenderHint(QPainter.RenderHint.TextAntialiasing, True)
        p = theme.palette()
        full = QRectF(self.rect()).adjusted(0.5, 0.5, -0.5, -0.5)
        painter.setPen(QPen(QColor(p.outline), 1.0))
        painter.setBrush(Qt.BrushStyle.NoBrush)
        painter.drawPath(_path(full, full.height() / 2))

        width = self._segment_width()
        for index, option in enumerate(self._options):
            seg = QRectF(index * width, 0, width, self.height()).adjusted(2, 2, -2, -2)
            selected = index == self._current
            if selected:
                painter.setPen(Qt.PenStyle.NoPen)
                painter.setBrush(QColor(p.secondary_container))
                painter.drawPath(_path(seg, seg.height() / 2))
            elif self._hover == index:
                painter.setPen(Qt.PenStyle.NoPen)
                painter.setBrush(_with_alpha(QColor(p.on_surface), 0.06))
                painter.drawPath(_path(seg, seg.height() / 2))
            color = QColor(p.on_secondary_container) if selected else QColor(p.on_surface_variant)
            painter.setPen(color)
            painter.setFont(theme.font("label", QFont.Weight.DemiBold if selected else QFont.Weight.Normal))
            text = ("✓ " if selected else "") + option
            painter.drawText(seg, ALIGN_CENTER, text)
        painter.end()

    def mousePressEvent(self, event) -> None:  # noqa: N802
        width = self._segment_width() or 1
        index = int(event.position().x() // width)
        self.set_current(index)

    def mouseMoveEvent(self, event) -> None:  # noqa: N802
        width = self._segment_width() or 1
        index = int(event.position().x() // width)
        if index != self._hover:
            self._hover = index
            self.update()

    def leaveEvent(self, event) -> None:  # noqa: N802
        self._hover = -1
        self.update()
        super().leaveEvent(event)


class Section(QWidget):
    """Settings block: title, description and rows."""

    def __init__(self, title: str, description: str = "", parent: QWidget | None = None) -> None:
        super().__init__(parent)
        outer = QVBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(10)

        heading = QLabel(title)
        heading.setFont(theme.font("title", QFont.Weight.DemiBold))
        outer.addWidget(heading)
        if description:
            note = QLabel(description)
            note.setWordWrap(True)
            note.setFont(theme.font("body"))
            note.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
            outer.addWidget(note)

        self._card = Card("low", theme.RADIUS["md"])
        self._body = QVBoxLayout(self._card)
        self._body.setContentsMargins(18, 8, 18, 8)
        self._body.setSpacing(0)
        outer.addWidget(self._card)

    def add_row(self, widget: QWidget) -> None:
        if self._body.count():
            self._body.addWidget(Divider())
        self._body.addWidget(widget)

    def add_widget(self, widget: QWidget) -> None:
        self._body.addWidget(widget)


class SettingRow(QWidget):
    """Label + description on the left, a control on the right."""

    def __init__(
        self,
        title: str,
        description: str = "",
        control: QWidget | None = None,
        parent: QWidget | None = None,
    ) -> None:
        super().__init__(parent)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(0, 12, 0, 12)
        layout.setSpacing(18)

        text_column = QVBoxLayout()
        text_column.setSpacing(2)
        label = QLabel(title)
        label.setFont(theme.font("body-large", QFont.Weight.Medium))
        text_column.addWidget(label)
        if description:
            note = QLabel(description)
            note.setWordWrap(True)
            note.setFont(theme.font("body-small"))
            note.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
            text_column.addWidget(note)
        layout.addLayout(text_column, 1)
        if control is not None:
            control.setSizePolicy(QSizePolicy.Policy.Fixed, QSizePolicy.Policy.Fixed)
            layout.addWidget(control, 0, Qt.AlignmentFlag.AlignVCenter)


class _SnackAction(QLabel):
    clicked = Signal()

    def __init__(self, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setFont(theme.font("label", QFont.Weight.DemiBold))

    def mousePressEvent(self, event) -> None:  # noqa: N802
        self.clicked.emit()


class Snackbar(QWidget):
    """Floating message bar shown at the bottom of the window."""

    def __init__(self, parent: QWidget) -> None:
        super().__init__(parent)
        self.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, False)
        self._label = QLabel("")
        self._label.setWordWrap(True)
        self._action = _SnackAction()
        self._action.setVisible(False)
        layout = QHBoxLayout(self)
        layout.setContentsMargins(18, 10, 12, 10)
        layout.setSpacing(12)
        layout.addWidget(self._label, 1)
        layout.addWidget(self._action, 0)
        self._callback: Optional[Callable[[], None]] = None
        self._action.clicked.connect(self._run_action)
        self.setVisible(False)
        self._anim = QPropertyAnimation(self, b"pos", self)
        self._anim.setDuration(220)
        self._anim.setEasingCurve(QEasingCurve.Type.OutCubic)
        self._timer = None

    def paintEvent(self, event) -> None:  # noqa: N802
        painter = QPainter(self)
        painter.setRenderHint(QPainter.RenderHint.Antialiasing, True)
        p = theme.palette()
        rect = QRectF(self.rect())
        painter.setPen(Qt.PenStyle.NoPen)
        painter.setBrush(QColor(p.inverse_surface))
        painter.drawPath(_path(rect, 10))
        painter.end()

    def _run_action(self) -> None:
        if self._callback is not None:
            self._callback()
        self.hide()

    def show_message(
        self,
        text: str,
        action: str = "",
        callback: Optional[Callable[[], None]] = None,
        timeout: int = 4000,
    ) -> None:
        from PySide6.QtCore import QTimer

        p = theme.palette()
        self._label.setText(text)
        self._label.setFont(theme.font("body"))
        self._label.setStyleSheet(f"color: {p.inverse_on_surface}; background: transparent;")
        self._action.setVisible(bool(action))
        self._action.setText(action)
        self._action.setStyleSheet(
            f"color: {p.inverse_primary}; background: transparent; padding: 0 6px;"
        )
        self._callback = callback
        self.adjustSize()
        parent = self.parentWidget()
        width = min(max(320, self._label.sizeHint().width() + 160), max(320, parent.width() - 64))
        self.setFixedWidth(width)
        self.adjustSize()
        target = QPoint(
            (parent.width() - self.width()) // 2,
            parent.height() - self.height() - 24,
        )
        self.setVisible(True)
        self.raise_()
        self._anim.stop()
        self._anim.setStartValue(QPoint(target.x(), target.y() + 24))
        self._anim.setEndValue(target)
        self._anim.start()
        if self._timer is not None:
            self._timer.stop()
        if timeout > 0:
            self._timer = QTimer(self)
            self._timer.setSingleShot(True)
            self._timer.timeout.connect(self.hide)
            self._timer.start(timeout)


class _AsyncJob(QObject):
    done = Signal(object)
    failed = Signal(str)


def run_async(
    owner: QWidget,
    function: Callable[[], object],
    on_done: Callable[[object], None],
    on_error: Optional[Callable[[str], None]] = None,
) -> None:
    """Runs `function` in a worker thread and delivers the result on the UI thread."""
    import threading

    job = _AsyncJob(owner)
    job.done.connect(on_done, Qt.ConnectionType.QueuedConnection)
    if on_error is not None:
        job.failed.connect(on_error, Qt.ConnectionType.QueuedConnection)

    def worker() -> None:
        try:
            result = function()
        except Exception as error:  # noqa: BLE001
            if on_error is not None:
                job.failed.emit(str(error) or type(error).__name__)
            return
        job.done.emit(result)

    threading.Thread(target=worker, daemon=True).start()


def icon_label(name: str, size: int = 20, color: Optional[str] = None, filled: bool = False) -> QLabel:
    label = QLabel(theme.icon_char(name))
    label.setFont(theme.icon_font(size, filled=filled))
    label.setStyleSheet(f"color: {color or theme.palette().on_surface_variant}; background: transparent;")
    label.setAlignment(ALIGN_CENTER)
    label.setFixedSize(size + 8, size + 8)
    return label
