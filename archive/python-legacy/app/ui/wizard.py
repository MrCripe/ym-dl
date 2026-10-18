"""First-run screen: token entry and verification."""

from __future__ import annotations

from PySide6.QtCore import Qt, Signal
from PySide6.QtGui import QFont, QGuiApplication
from PySide6.QtWidgets import QHBoxLayout, QLabel, QVBoxLayout, QWidget

from .. import yandex
from ..config import Config
from ..constants import APP_NAME, APP_TAGLINE, TOKEN_HELP_URL
from . import theme
from .widgets import Badge, Button, Card, Divider, TextField, run_async


class Wizard(QWidget):
    completed = Signal(object)

    def __init__(self, config: Config, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._config = config
        self._info: yandex.AccountInfo | None = None
        self._checked = False

        outer = QVBoxLayout(self)
        outer.setContentsMargins(32, 32, 32, 32)
        outer.addStretch(1)

        card = Card("low", theme.RADIUS["xl"], elevation=True)
        card.setFixedWidth(640)
        body = QVBoxLayout(card)
        body.setContentsMargins(32, 30, 32, 28)
        body.setSpacing(14)

        logo = QLabel(theme.icon_char("graphic_eq"))
        logo.setFont(theme.icon_font(40, filled=True))
        logo.setAlignment(Qt.AlignmentFlag.AlignCenter)
        logo.setFixedSize(76, 76)
        p = theme.palette()
        logo.setStyleSheet(
            f"color: {p.on_primary_container}; background: {p.primary_container};"
            f"border-radius: 38px;"
        )
        logo_row = QHBoxLayout()
        logo_row.addStretch(1)
        logo_row.addWidget(logo)
        logo_row.addStretch(1)
        body.addLayout(logo_row)

        title = QLabel(f"Добро пожаловать в {APP_NAME}")
        title.setFont(theme.font("headline", QFont.Weight.DemiBold))
        title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        body.addWidget(title)

        subtitle = QLabel(
            f"{APP_TAGLINE}. Вставьте токен Яндекс.Музыки — приложение проверит аккаунт "
            "и сразу скажет, отдаёт ли сервис lossless именно вам."
        )
        subtitle.setWordWrap(True)
        subtitle.setAlignment(Qt.AlignmentFlag.AlignCenter)
        subtitle.setFont(theme.font("body"))
        subtitle.setStyleSheet(f"color: {p.on_surface_variant};")
        body.addWidget(subtitle)

        body.addSpacing(4)
        self.token_field = TextField("Токен Яндекс.Музыки", "Вставьте токен (oauth)", password=True)
        self.token_field.set_text(Config.read_token())
        body.addWidget(self.token_field)

        links = QHBoxLayout()
        links.setSpacing(8)
        paste = Button("Вставить из буфера", icon="content_paste", kind="tonal", compact=True)
        paste.clicked.connect(self._paste)
        links.addWidget(paste)
        help_button = Button("Где взять токен", icon="open_in_new", kind="text", compact=True)
        help_button.clicked.connect(lambda: yandex.open_in_browser(TOKEN_HELP_URL))
        links.addWidget(help_button)
        links.addStretch(1)
        body.addLayout(links)

        self.status = QWidget()
        status_layout = QVBoxLayout(self.status)
        status_layout.setContentsMargins(0, 6, 0, 0)
        status_layout.setSpacing(10)
        self._status_body = status_layout
        self.status.setVisible(False)
        body.addWidget(self.status)

        self.error = QLabel("")
        self.error.setWordWrap(True)
        self.error.setFont(theme.font("body"))
        self.error.setStyleSheet(f"color: {p.error};")
        self.error.setVisible(False)
        body.addWidget(self.error)

        self.action = Button("Проверить и продолжить", icon="verified", kind="filled")
        self.action.setMinimumHeight(46)
        self.action.clicked.connect(self._primary)
        body.addWidget(self.action)

        note = QLabel("Токен хранится локально в файле ~/.ym-dl-token и никуда не отправляется.")
        note.setFont(theme.font("body-small"))
        note.setAlignment(Qt.AlignmentFlag.AlignCenter)
        note.setStyleSheet(f"color: {p.on_surface_variant};")
        body.addWidget(note)

        outer.addWidget(card, 0, Qt.AlignmentFlag.AlignHCenter)
        outer.addStretch(2)

    # --- actions -------------------------------------------------------
    def _paste(self) -> None:
        text = QGuiApplication.clipboard().text().strip()
        if text:
            self.token_field.set_text(text)
            self.token_field.clear_error("Токен Яндекс.Музыки")
            self.error.setVisible(False)

    def _primary(self) -> None:
        if self._checked and self._info is not None:
            self.completed.emit(self._info)
            return
        token = self.token_field.text().strip()
        if not token:
            self.error.setText("Вставьте токен — без него Яндекс.Музыка не отдаст музыку.")
            self.error.setVisible(True)
            return
        self.error.setVisible(False)
        self.action.setEnabled(False)
        self.action.set_text("Проверяю…")
        settings = self._config.settings
        run_async(
            self,
            lambda: yandex.check_token(token, settings.timeout),
            self._on_checked,
            self._on_error,
        )

    def _on_checked(self, info: object) -> None:
        assert isinstance(info, yandex.AccountInfo)
        self._info = info
        self._checked = True
        Config.write_token(self.token_field.text().strip())
        self.action.setEnabled(True)
        self.action.set_text("Продолжить")
        self.action.set_icon("arrow_forward")
        self._render_status(info)

    def _on_error(self, message: str) -> None:
        self.action.setEnabled(True)
        self.action.set_text("Проверить и продолжить")
        self._checked = False
        self._info = None
        self.status.setVisible(False)
        self.error.setText(message)
        self.error.setVisible(True)

    def _render_status(self, info: yandex.AccountInfo) -> None:
        while self._status_body.count():
            item = self._status_body.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()

        card = Card("filled", theme.RADIUS["md"])
        rows = QVBoxLayout(card)
        rows.setContentsMargins(16, 12, 16, 12)
        rows.setSpacing(10)

        rows.addLayout(self._row("Аккаунт", f"{info.display_name} · UID {info.uid}"))
        rows.addWidget(Divider())
        rows.addLayout(
            self._row(
                "Подписка",
                "Яндекс Плюс активен" if info.has_plus else "Плюс не активен",
                tone="primary" if info.has_plus else "error",
            )
        )
        rows.addWidget(Divider())

        if info.lossless_ok is None:
            lossless_text = "не удалось проверить"
            tone = "neutral"
        elif info.lossless_ok:
            lossless_text = "доступен — сервер отдаёт FLAC"
            tone = "primary"
        else:
            lossless_text = (
                f"сервер отдаёт {info.probe_codec or 'AAC'} "
                f"{(str(info.probe_bitrate) + ' kbps') if info.probe_bitrate else ''}".strip()
            )
            tone = "tertiary"
        rows.addLayout(self._row("Lossless", lossless_text, tone=tone))

        if info.lossless_ok is False:
            hint = QLabel(
                "Проверка на реальном треке: API вернул не FLAC. Скачивание пойдёт в лучшем "
                "доступном качестве, файлы останутся .m4a. Обычно это значит, что музыкальная "
                "подписка не применяется к аккаунту."
            )
            hint.setWordWrap(True)
            hint.setFont(theme.font("body-small"))
            hint.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
            rows.addWidget(hint)

        self._status_body.addWidget(card)
        self.status.setVisible(True)

    def _row(self, title: str, value: str, tone: str = "neutral") -> QHBoxLayout:
        row = QHBoxLayout()
        row.setSpacing(12)
        label = QLabel(title)
        label.setFont(theme.font("body"))
        label.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
        row.addWidget(label)
        row.addStretch(1)
        badge = Badge(value, tone)
        badge.setFont(theme.font("body-small", QFont.Weight.Medium))
        badge.setFixedHeight(24)
        row.addWidget(badge)
        return row

    def refresh_theme(self) -> None:
        p = theme.palette()
        self.error.setStyleSheet(f"color: {p.error};")
        self.token_field.refresh()
