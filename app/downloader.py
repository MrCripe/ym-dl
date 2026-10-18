"""Background download worker.

Reuses the `ymd` download pipeline (tags, covers, lyrics, path patterns) and
only swaps the transport so the UI can show real byte progress and stop a
download in the middle.
"""

from __future__ import annotations

import threading
import time
from dataclasses import dataclass
from pathlib import Path

from PySide6.QtCore import QThread, Signal
from yandex_music import Track

import ymd.core
from ymd.core import CoreTrackQuality, LyricsFormat

from . import flac, yandex
from .config import Settings


@dataclass
class TrackInfo:
    title: str
    artist: str
    album: str
    duration: str
    codec: str
    bitrate: int
    quality: str
    path: str


class DownloadWorker(QThread):
    trackStarted = Signal(int, int, object)
    trackProgress = Signal(int, int, int)
    trackFinished = Signal(int, str, str, str)
    overallProgress = Signal(int, int)
    finishedAll = Signal(bool, int, int)
    logMessage = Signal(str)

    def __init__(self, tracks: list[Track], settings: Settings, parent=None) -> None:
        super().__init__(parent)
        self._tracks = list(tracks)
        self._settings = settings
        self._stop = threading.Event()
        self._ok = 0
        self._failed = 0
        self._current_index = 0
        self._current_bytes = (0, 0)

    def request_stop(self) -> None:
        self._stop.set()

    @property
    def stopping(self) -> bool:
        return self._stop.is_set()

    # --- thread body ---------------------------------------------------
    def run(self) -> None:  # noqa: C901 - линейный конвейер загрузки
        settings = self._settings
        target_dir = settings.target_dir
        total = len(self._tracks)
        cancelled = False
        yandex.set_download_hooks(progress=self._on_bytes, stop=self._stop)
        covers_cache: dict = {}
        lyrics_format = LyricsFormat(settings.lyrics_format)
        quality = CoreTrackQuality(settings.quality)
        try:
            target_dir.mkdir(parents=True, exist_ok=True)
        except OSError as error:
            self.logMessage.emit(f"Не удалось создать папку {target_dir}: {error}")
            self.finishedAll.emit(False, 0, 0)
            return

        for index, track in enumerate(self._tracks):
            if self._stop.is_set():
                cancelled = True
                break
            self.overallProgress.emit(index, total)
            try:
                status = self._download_one(
                    index, track, target_dir, settings, quality, lyrics_format, covers_cache
                )
            except InterruptedError:
                cancelled = True
                self.trackFinished.emit(index, "cancelled", "остановлено", "")
                break
            except Exception as error:  # noqa: BLE001 - одна ошибка не рвёт очередь
                self._failed += 1
                self.trackFinished.emit(index, "failed", describe_error(error), "")
                continue
            if status == "ok":
                self._ok += 1
            if settings.delay > 0 and index < total - 1:
                self._sleep(settings.delay)

        self.overallProgress.emit(total - (1 if cancelled else 0), total)
        yandex.set_download_hooks()
        self.finishedAll.emit(cancelled, self._ok, self._failed)

    def _download_one(
        self,
        index: int,
        track: Track,
        target_dir: Path,
        settings: Settings,
        quality: CoreTrackQuality,
        lyrics_format: LyricsFormat,
        covers_cache: dict,
    ) -> str:
        if not track.available:
            self.trackFinished.emit(index, "failed", "трек недоступен в регионе", "")
            return "failed"

        self._current_index = index
        self._current_bytes = (0, 0)
        base_path = target_dir / ymd.core.prepare_base_path(
            Path(settings.path_pattern), track, settings.unsafe_path
        )
        if settings.skip_existing and any(
            Path(str(base_path) + suffix).is_file() for suffix in ymd.core.AUDIO_FILE_SUFFIXES
        ):
            self.trackFinished.emit(index, "skipped", "уже скачан", str(base_path))
            return "skipped"

        base_path.parent.mkdir(parents=True, exist_ok=True)
        downloadable = ymd.core.to_downloadable_track(track, quality, base_path)
        info = downloadable.download_info
        info_payload = TrackInfo(
            title=track.title or "",
            artist=yandex.track_artist(track),
            album=yandex.track_album(track),
            duration=yandex.format_duration(getattr(track, "duration_ms", None)),
            codec=getattr(info.file_format.codec, "name", ""),
            bitrate=int(info.bitrate or 0),
            quality=str(info.quality or ""),
            path=str(downloadable.path),
        )
        self.trackStarted.emit(index, len(self._tracks), info_payload)

        ymd.core.download_track(
            track_info=downloadable,
            lyrics_format=lyrics_format,
            embed_cover=settings.embed_cover,
            cover_resolution=settings.cover_size,
            covers_cache=covers_cache,
            compatibility_level=settings.compatibility_level,
        )

        final_path = downloadable.path
        if (
            settings.extract_flac
            and final_path.suffix.lower() == ".m4a"
            and info_payload.codec == "FLAC"
            and flac.available()
        ):
            outcome = flac.extract_flac(final_path)
            if outcome == "converted":
                flac.quarantine(final_path, target_dir)
                final_path = final_path.with_suffix(".flac")
                self.logMessage.emit(f"FLAC извлечён: {final_path.name}")
            elif outcome == "failed":
                self.logMessage.emit(f"Не удалось извлечь FLAC из {final_path.name}; оставлен .m4a")

        self.trackFinished.emit(index, "ok", f"{info_payload.codec} {info_payload.bitrate}".strip(), str(final_path))
        return "ok"

    def _on_bytes(self, done: int, total: int) -> None:
        self._current_bytes = (done, total)
        self.trackProgress.emit(self._current_index, done, total)

    def _sleep(self, seconds: int) -> None:
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if self._stop.is_set():
                return
            time.sleep(0.1)


def describe_error(error: Exception) -> str:
    text = str(error).strip()
    name = type(error).__name__
    if not text:
        return name
    if name in ("UnauthorizedError",):
        return "API отклонил запрос (401) — проверьте подписку и токен"
    return text[:180]
