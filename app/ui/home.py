"""Main page: source input, favourites and the per-track selection list."""

from __future__ import annotations

from typing import Optional

from PySide6.QtCore import QTimer, Qt, Signal
from PySide6.QtGui import QFont, QGuiApplication
from PySide6.QtWidgets import (
    QHBoxLayout,
    QLabel,
    QScrollArea,
    QSizePolicy,
    QVBoxLayout,
    QWidget,
)
from yandex_music import Client, Track

from .. import yandex
from ..config import Config
from . import theme
from .widgets import (
    Badge,
    Button,
    Card,
    Checkbox,
    Divider,
    IconButton,
    TextField,
    run_async,
)

RENDER_CHUNK = 60


class TrackRow(QWidget):
    toggled = Signal(int, bool)

    def __init__(self, index: int, track: Track, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._index = index
        self._track = track
        self.setCursor(Qt.CursorShape.PointingHandCursor)

        layout = QHBoxLayout(self)
        layout.setContentsMargins(4, 6, 4, 6)
        layout.setSpacing(12)

        self.checkbox = Checkbox()
        self.checkbox.setChecked(bool(track.available))
        self.checkbox.toggled.connect(self._on_toggle)
        layout.addWidget(self.checkbox, 0, Qt.AlignmentFlag.AlignVCenter)

        text = QVBoxLayout()
        text.setSpacing(1)
        title = QLabel(track.title or "Без названия")
        title.setFont(theme.font("body-large"))
        if not track.available:
            title.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
        text.addWidget(title)

        meta_parts = [yandex.track_artist(track)]
        album = yandex.track_album(track)
        if album:
            meta_parts.append(album)
        meta = QLabel(" · ".join(meta_parts))
        meta.setFont(theme.font("body-small"))
        meta.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
        text.addWidget(meta)
        layout.addLayout(text, 1)

        if not track.available:
            layout.addWidget(Badge("недоступен", "error"))

        duration = QLabel(yandex.format_duration(getattr(track, "duration_ms", None)))
        duration.setFont(theme.font("body-small"))
        duration.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
        duration.setFixedWidth(44)
        duration.setAlignment(Qt.AlignmentFlag.AlignRight | Qt.AlignmentFlag.AlignVCenter)
        layout.addWidget(duration)

    @property
    def track(self) -> Track:
        return self._track

    def is_checked(self) -> bool:
        return self.checkbox.isChecked()

    def set_checked(self, value: bool) -> None:
        self.checkbox.setChecked(value)

    def _on_toggle(self, value: bool) -> None:
        self.toggled.emit(self._index, value)

    def mousePressEvent(self, event) -> None:  # noqa: N802
        if self._track.available:
            self.checkbox.setChecked(not self.checkbox.isChecked())


class HomePage(QWidget):
    startRequested = Signal(list)
    stopRequested = Signal()
    progressHint = Signal(int, int)

    def __init__(self, config: Config, parent: QWidget | None = None) -> None:
        super().__init__(parent)
        self._config = config
        self._client: Optional[Client] = None
        self._source: Optional[yandex.Source] = None
        self._rows: list[TrackRow] = []
        self._pending: list[Track] = []
        self._render_index = 0
        self._busy = False
        self._downloading = False

        outer = QVBoxLayout(self)
        outer.setContentsMargins(36, 28, 36, 20)
        outer.setSpacing(18)

        header = QLabel("Главная")
        header.setFont(theme.font("headline", QFont.Weight.DemiBold))
        outer.addWidget(header)

        self.subtitle = QLabel("Вставьте ссылку или скачайте любимые треки")
        self.subtitle.setFont(theme.font("body"))
        self.subtitle.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
        outer.addWidget(self.subtitle)

        source_card = Card("low", theme.RADIUS["lg"])
        source_body = QVBoxLayout(source_card)
        source_body.setContentsMargins(20, 18, 20, 18)
        source_body.setSpacing(14)

        link_row = QHBoxLayout()
        link_row.setSpacing(10)
        self.link_field = TextField(
            "Ссылка на трек, альбом, плейлист или исполнителя",
            "https://music.yandex.ru/album/…",
        )
        self.link_field.set_text(self._config.settings.last_url)
        self.link_field.submitted.connect(self._resolve)
        link_row.addWidget(self.link_field, 1)

        self.paste_button = IconButton("content_paste", "Вставить из буфера", variant="tonal")
        self.paste_button.clicked.connect(self._paste)
        link_row.addWidget(self.paste_button, 0, Qt.AlignmentFlag.AlignBottom)

        self.load_button = Button("Показать треки", icon="search", kind="filled")
        self.load_button.clicked.connect(self._resolve)
        link_row.addWidget(self.load_button, 0, Qt.AlignmentFlag.AlignBottom)
        source_body.addLayout(link_row)

        source_body.addWidget(Divider())

        favourites_row = QHBoxLayout()
        favourites_row.setSpacing(10)
        self.favourites_button = Button("Скачать любимые", icon="favorite", kind="outlined")
        self.favourites_button.clicked.connect(self._load_favourites)
        favourites_row.addWidget(self.favourites_button)

        self.favourites_hint = QLabel()
        self.favourites_hint.setFont(theme.font("body-small"))
        self.favourites_hint.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
        favourites_row.addWidget(self.favourites_hint, 1)
        source_body.addLayout(favourites_row)

        self.status = QLabel("")
        self.status.setFont(theme.font("body-small"))
        self.status.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
        self.status.setWordWrap(True)
        source_body.addWidget(self.status)
        outer.addWidget(source_card)

        self.selection = Card("low", theme.RADIUS["lg"])
        selection_body = QVBoxLayout(self.selection)
        selection_body.setContentsMargins(20, 16, 20, 16)
        selection_body.setSpacing(12)

        head = QHBoxLayout()
        head.setSpacing(10)
        head_text = QVBoxLayout()
        head_text.setSpacing(2)
        self.source_title = QLabel("")
        self.source_title.setFont(theme.font("title", QFont.Weight.DemiBold))
        head_text.addWidget(self.source_title)
        self.source_meta = QLabel("")
        self.source_meta.setFont(theme.font("body-small"))
        self.source_meta.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
        head_text.addWidget(self.source_meta)
        head.addLayout(head_text, 1)

        self.select_all_button = Button("Выбрать все", kind="text", compact=True)
        self.select_all_button.clicked.connect(lambda: self._set_all(True))
        head.addWidget(self.select_all_button)
        self.select_none_button = Button("Снять все", kind="text", compact=True)
        self.select_none_button.clicked.connect(lambda: self._set_all(False))
        head.addWidget(self.select_none_button)
        selection_body.addLayout(head)

        self.scroll = QScrollArea()
        self.scroll.setWidgetResizable(True)
        self.scroll.setHorizontalScrollBarPolicy(Qt.ScrollBarPolicy.ScrollBarAlwaysOff)
        self.list_host = QWidget()
        self.list_layout = QVBoxLayout(self.list_host)
        self.list_layout.setContentsMargins(0, 0, 8, 0)
        self.list_layout.setSpacing(0)
        self.list_layout.addStretch(1)
        self.scroll.setWidget(self.list_host)
        selection_body.addWidget(self.scroll, 1)

        bottom = QHBoxLayout()
        bottom.setSpacing(12)
        self.folder_label = QLabel("")
        self.folder_label.setFont(theme.font("body-small"))
        self.folder_label.setStyleSheet(f"color: {theme.palette().on_surface_variant};")
        self.folder_label.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Preferred)
        bottom.addWidget(self.folder_label, 1)

        self.start_button = Button("Начать", icon="play_arrow", kind="filled")
        self.start_button.setMinimumWidth(180)
        self.start_button.setMinimumHeight(44)
        self.start_button.clicked.connect(self._start_or_stop)
        bottom.addWidget(self.start_button)
        selection_body.addLayout(bottom)

        self.selection.setVisible(False)
        outer.addWidget(self.selection, 1)

        self._render_timer = QTimer(self)
        self._render_timer.setInterval(0)
        self._render_timer.timeout.connect(self._render_chunk)
        self.progressHint.connect(self._show_progress_hint)
        self.refresh()

    # --- public API ----------------------------------------------------
    def set_client(self, client: Optional[Client]) -> None:
        self._client = client

    def refresh(self) -> None:
        settings = self._config.settings
        self.folder_label.setText(f"Папка: {settings.target_dir}")
        if settings.username:
            self.favourites_hint.setText(f"Любимые треки пользователя {settings.username}")
        else:
            self.favourites_hint.setText(
                "Любимые треки вашего аккаунта · имя пользователя задаётся в настройках"
            )
        p = theme.palette()
        for label in (self.subtitle, self.status, self.source_meta, self.folder_label, self.favourites_hint):
            label.setStyleSheet(f"color: {p.on_surface_variant};")
        self.link_field.refresh()

    def set_downloading(self, active: bool) -> None:
        self._downloading = active
        if active:
            self.start_button.set_text("Остановить")
            self.start_button.set_icon("stop_circle")
            self.start_button.set_kind("tonal")
        else:
            self.start_button.set_text("Начать")
            self.start_button.set_icon("play_arrow")
            self.start_button.set_kind("filled")

    def selected_tracks(self) -> list[Track]:
        return [row.track for row in self._rows if row.is_checked() and row.track.available]

    # --- actions -------------------------------------------------------
    def _paste(self) -> None:
        text = QGuiApplication.clipboard().text().strip()
        if text:
            self.link_field.set_text(text)

    def _show_progress_hint(self, done: int, total: int) -> None:
        self.status.setText(f"Загружаю список треков: {done} из {total}…")

    def _set_busy(self, busy: bool) -> None:
        self._busy = busy
        self.load_button.setEnabled(not busy)
        self.favourites_button.setEnabled(not busy)
        self.load_button.set_text("Загружаю…" if busy else "Показать треки")

    def _resolve(self) -> None:
        if self._busy or self._client is None:
            return
        url = self.link_field.text().strip()
        if not url:
            self._show_error("Вставьте ссылку на трек, альбом, плейлист или исполнителя.")
            return
        settings = self._config.settings
        self._set_busy(True)
        self.status.setText("Разбираю ссылку…")
        run_async(
            self,
            lambda: yandex.resolve_source(
                self._client,
                url,
                only_music=settings.only_music,
                stick_to_artist=settings.stick_to_artist,
                on_progress=self.progressHint.emit,
            ),
            lambda source: self._on_source(source, url),
            self._on_load_error,
        )

    def _load_favourites(self) -> None:
        if self._busy or self._client is None:
            return
        username = self._config.settings.username
        self._set_busy(True)
        self.status.setText("Загружаю любимые треки…")
        run_async(
            self,
            lambda: yandex.likes_source(
                self._client, username, on_progress=self.progressHint.emit
            ),
            lambda source: self._on_source(source, self._config.settings.last_url),
            self._on_load_error,
        )

    def _on_source(self, source: object, url: str) -> None:
        assert isinstance(source, yandex.Source)
        self._set_busy(False)
        self._source = source
        self.status.setText(f"{source.kind.capitalize()} · {source.subtitle}")
        if url:
            self._config.settings.last_url = url
            self._config.save()
        self._build_rows(source.tracks)

    def _on_load_error(self, message: str) -> None:
        self._set_busy(False)
        self._show_error(message)

    def _show_error(self, message: str) -> None:
        self.status.setText(message)
        self.status.setStyleSheet(f"color: {theme.palette().error};")

    def _clear_rows(self) -> None:
        for row in self._rows:
            row.setParent(None)
            row.deleteLater()
        self._rows.clear()
        while self.list_layout.count() > 1:
            item = self.list_layout.takeAt(0)
            widget = item.widget()
            if widget is not None:
                widget.deleteLater()

    def _build_rows(self, tracks: list[Track]) -> None:
        self._clear_rows()
        self._pending = tracks
        self._render_index = 0
        self.selection.setVisible(True)
        if self._source is not None:
            self.source_title.setText(self._source.title)
            self.source_meta.setText(self._source.subtitle)
        self.start_button.setEnabled(False)
        self._render_timer.start()

    def _render_chunk(self) -> None:
        end = min(self._render_index + RENDER_CHUNK, len(self._pending))
        for index in range(self._render_index, end):
            row = TrackRow(index, self._pending[index], self.list_host)
            row.toggled.connect(self._on_row_toggled)
            self.list_layout.insertWidget(self.list_layout.count() - 1, row)
            if self._rows:
                divider = Divider()
                self.list_layout.insertWidget(self.list_layout.count() - 1, divider)
                row._divider = divider
            self._rows.append(row)
        self._render_index = end
        self._update_count()
        if end >= len(self._pending):
            self._render_timer.stop()
            self.start_button.setEnabled(bool(self.selected_tracks()))

    def _on_row_toggled(self, index: int, value: bool) -> None:
        self._update_count()
        self.start_button.setEnabled(bool(self.selected_tracks()) or self._downloading)

    def _set_all(self, value: bool) -> None:
        for row in self._rows:
            if row.track.available:
                row.set_checked(value)
        self._update_count()
        self.start_button.setEnabled(bool(self.selected_tracks()) or self._downloading)

    def _update_count(self) -> None:
        total = len(self._pending)
        chosen = sum(1 for row in self._rows if row.is_checked() and row.track.available)
        available = sum(1 for track in self._pending if track.available)
        meta = f"выбрано {chosen} из {available} доступных"
        if available < total:
            meta += f" · {total - available} недоступны"
        if self._source is not None:
            self.source_meta.setText(f"{self._source.subtitle} · {meta}")
        else:
            self.source_meta.setText(meta)

    def _start_or_stop(self) -> None:
        if self._downloading:
            self.stopRequested.emit()
            return
        tracks = self.selected_tracks()
        if not tracks:
            self._show_error("Ничего не выбрано.")
            return
        self.startRequested.emit(tracks)
