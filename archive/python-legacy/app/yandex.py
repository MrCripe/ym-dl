"""Yandex Music integration: client, lossless signing, sources, token checks.

The downloader package (`ymd`) is reused as-is; the only thing replaced is the
`get-file-info` call, because the published one signs the request with the
mobile key while sending the desktop client header. The patch is applied in
memory — nothing under site-packages is modified.
"""

from __future__ import annotations

import random
import re
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable, Iterable, Optional
from urllib.parse import urlparse

import requests
from Crypto.Cipher import AES
from yandex_music import Client, Track
from yandex_music.exceptions import UnauthorizedError, YandexMusicError
from yandex_music.utils.sign_request import DESKTOP_CLIENT, get_file_info_sign

import ymd.api
import ymd.core
from ymd.api import FILE_FORMAT_MAPPING, ApiTrackQuality, CustomDownloadInfo

TRACK_BATCH = 100
PROBE_TRACK_ID = "53404"  # Smells Like Teen Spirit — есть в lossless у сервиса

_ARTIST_RE = re.compile(r"artist/(\d+)")
_ALBUM_TRACK_RE = re.compile(r"album/(\d+)/track/(\d+)")
_ALBUM_RE = re.compile(r"album/(\d+)")
_TRACK_RE = re.compile(r"track/(\d+)")
_USER_PLAYLIST_RE = re.compile(r"users/([^/?#]+)/playlists/([^/?#]+)")
_PLAYLIST_RE = re.compile(r"playlists/([^/?#]+)")


class SourceError(Exception):
    """Пользователь ввёл ссылку, которую не удалось разобрать."""


class TokenRejected(Exception):
    """API отклонил токен."""


@dataclass
class AccountInfo:
    uid: str = ""
    login: str = ""
    name: str = ""
    has_plus: bool = False
    family_member: bool = False
    product_id: str = ""
    permissions: list[str] = field(default_factory=list)
    lossless_ok: Optional[bool] = None
    probe_codec: str = ""
    probe_quality: str = ""
    probe_bitrate: int = 0
    probe_track: str = ""
    note: str = ""

    @property
    def display_name(self) -> str:
        return self.name or self.login or self.uid


@dataclass
class Source:
    kind: str
    title: str
    subtitle: str
    tracks: list[Track]

    @property
    def available_count(self) -> int:
        return sum(1 for track in self.tracks if track.available)


# --- lossless signing --------------------------------------------------
def _get_download_info(track: Track, quality: ApiTrackQuality) -> CustomDownloadInfo:
    client = track.client
    assert client

    quality_value = quality.value if hasattr(quality, "value") else str(quality)
    codecs = list(FILE_FORMAT_MAPPING.keys())
    transport = "encraw"
    sign = get_file_info_sign(track.id, quality_value, codecs, transport)
    params = {
        "ts": sign.timestamp,
        "trackId": track.id,
        "quality": quality_value,
        "codecs": ",".join(codecs),
        "transports": transport,
        "sign": sign.value,
    }

    request = client.request
    header = "X-Yandex-Music-Client"
    previous = request.headers.get(header)
    request.headers[header] = DESKTOP_CLIENT
    try:
        response = request.get("https://api.music.yandex.net/get-file-info", params=params)
    finally:
        if previous is None:
            request.headers.pop(header, None)
        else:
            request.headers[header] = previous

    info = response["downloadInfo"]
    file_format = FILE_FORMAT_MAPPING.get(info["codec"])
    if file_format is None:
        raise ValueError(f"Unknown codec: {info['codec']}")
    return CustomDownloadInfo(
        quality=info["quality"],
        file_format=file_format,
        urls=info["urls"],
        bitrate=info["bitrate"],
        decryption_key=info.get("key"),
    )


_progress_hook: Optional[Callable[[int, int], None]] = None
_stop_event: Optional[threading.Event] = None


def set_download_hooks(
    progress: Optional[Callable[[int, int], None]] = None,
    stop: Optional[threading.Event] = None,
) -> None:
    global _progress_hook, _stop_event
    _progress_hook = progress
    _stop_event = stop


def _download_track(client: Client, download_info: CustomDownloadInfo) -> bytes:
    """Streaming variant of `ymd.api.download_track` that reports bytes."""
    url = random.choice(download_info.urls)
    headers = dict(client.request.headers)
    timeout = getattr(client.request, "_timeout", 30) or 30
    cipher = None
    if download_info.decryption_key:
        cipher = AES.new(
            key=bytes.fromhex(download_info.decryption_key),
            nonce=bytes(12),
            mode=AES.MODE_CTR,
        )

    buffer = bytearray()
    with requests.get(url, headers=headers, stream=True, timeout=timeout) as response:
        response.raise_for_status()
        try:
            total = int(response.headers.get("Content-Length") or 0)
        except (TypeError, ValueError):
            total = 0
        last_emit = 0.0
        for chunk in response.iter_content(chunk_size=64 * 1024):
            if not chunk:
                continue
            if _stop_event is not None and _stop_event.is_set():
                raise InterruptedError("загрузка остановлена")
            buffer.extend(cipher.decrypt(chunk) if cipher else chunk)
            now = time.monotonic()
            if _progress_hook is not None and now - last_emit >= 0.1:
                _progress_hook(len(buffer), total)
                last_emit = now
    if _progress_hook is not None:
        _progress_hook(len(buffer), len(buffer))
    return bytes(buffer)


_patched = False


def install_patches() -> None:
    global _patched
    if _patched:
        return
    ymd.api.get_download_info = _get_download_info
    ymd.core.get_download_info = _get_download_info
    ymd.api.download_track = _download_track
    _patched = True


# --- client ------------------------------------------------------------
def create_client(token: str, timeout: int = 20, tries: int = 20, retry_delay: int = 5) -> Client:
    install_patches()
    return ymd.core.init_client(
        token=token,
        timeout=max(1, int(timeout)),
        max_try_count=max(0, int(tries)),
        retry_delay=max(0, int(retry_delay)),
    )


def check_token(token: str, timeout: int = 20) -> AccountInfo:
    token = (token or "").strip()
    if not token:
        raise TokenRejected("Токен пустой")
    client = Client(token)
    client.request.set_timeout(max(1, timeout))
    try:
        client.init()
        status = client.account_status()
    except UnauthorizedError as error:
        raise TokenRejected("API отклонил токен (401). Проверьте, что скопирован весь токен.") from error
    except YandexMusicError as error:
        raise TokenRejected(f"Не удалось проверить токен: {type(error).__name__}") from error

    info = AccountInfo()
    if status is None:
        return info
    account = status.account
    if account is not None:
        info.uid = str(account.uid or "")
        info.login = account.login or ""
        info.name = account.display_name or account.full_name or ""
        info.family_member = bool(account.non_owner_family_member)
    plus = status.plus
    info.has_plus = bool(plus and plus.has_plus)
    info.permissions = list(getattr(plus, "permissions", None) or []) if plus else []
    subscription = status.subscription
    products = getattr(subscription, "auto_renewable", None) or []
    if products:
        product = getattr(products[0], "product", None)
        info.product_id = str(getattr(product, "product_id", "") or "")

    track_id, track_name = _probe_track(client)
    info.probe_track = track_name
    try:
        details = probe_lossless(client, track_id)
    except Exception:  # noqa: BLE001 - проба не должна ломать проверку токена
        info.lossless_ok = None
        info.note = "Не удалось запросить lossless для проверочного трека."
        return info

    info.probe_codec = details["codec"]
    info.probe_quality = details["quality"]
    info.probe_bitrate = int(details.get("bitrate") or 0)
    info.lossless_ok = details["codec"] in ("flac", "flac-mp4")
    return info


def probe_lossless(client: Client, track_id: str) -> dict:
    """Asks the API for a lossless file and returns what it actually offered."""
    codecs = list(FILE_FORMAT_MAPPING.keys())
    sign = get_file_info_sign(track_id, "lossless", codecs, "encraw")
    params = {
        "ts": sign.timestamp,
        "trackId": track_id,
        "quality": "lossless",
        "codecs": ",".join(codecs),
        "transports": "encraw",
        "sign": sign.value,
    }
    request = client.request
    header = "X-Yandex-Music-Client"
    previous = request.headers.get(header)
    request.headers[header] = DESKTOP_CLIENT
    try:
        response = request.get("https://api.music.yandex.net/get-file-info", params=params)
    finally:
        if previous is None:
            request.headers.pop(header, None)
        else:
            request.headers[header] = previous
    return dict(response["downloadInfo"])


def _probe_track(client: Client) -> tuple[str, str]:
    try:
        likes = client.users_likes_tracks()
        shorts = getattr(likes, "tracks", None) or []
        for short in shorts[:5]:
            if getattr(short, "id", None):
                return str(short.id), "любимый трек"
    except Exception:  # noqa: BLE001
        pass
    return PROBE_TRACK_ID, "проверочный трек"


# --- sources -----------------------------------------------------------
def parse_link(text: str) -> tuple[str, tuple]:
    raw = (text or "").strip()
    if not raw:
        raise SourceError("Вставьте ссылку на трек, альбом, плейлист или исполнителя.")
    if raw.isdigit():
        return "track", (raw,)
    if "://" not in raw:
        raw = "https://" + raw.lstrip("/")
    parsed = urlparse(raw)
    if not parsed.netloc and parsed.path:
        parsed = urlparse("https://music.yandex.ru" + parsed.path)
    path = parsed.path

    if match := _ALBUM_TRACK_RE.search(path):
        return "track", (match.group(2),)
    if match := _USER_PLAYLIST_RE.search(path):
        return "playlist", (match.group(1), match.group(2))
    if match := _ALBUM_RE.search(path):
        return "album", (match.group(1),)
    if match := _TRACK_RE.search(path):
        return "track", (match.group(1),)
    if match := _ARTIST_RE.search(path):
        return "artist", (match.group(1),)
    if match := _PLAYLIST_RE.search(path):
        return "playlist_uuid", (match.group(1),)
    raise SourceError("Не понял ссылку. Нужна ссылка вида music.yandex.ru/album/… или /users/…/playlists/…")


def fetch_tracks(client: Client, ids: Iterable, on_progress: Optional[Callable[[int, int], None]] = None) -> list[Track]:
    id_list = [str(i) for i in ids]
    result: list[Track] = []
    total = len(id_list)
    for start in range(0, total, TRACK_BATCH):
        batch = id_list[start : start + TRACK_BATCH]
        fetched = client.tracks(batch) or []
        result.extend(fetched)
        if on_progress is not None:
            on_progress(min(start + len(batch), total), total)
    return result


def _album_tracks(client: Client, album_id: str) -> tuple[Album, list[Track]]:  # noqa: F821
    album = client.albums_with_tracks(album_id)
    if album is None:
        raise SourceError(f"Альбом {album_id} не найден или недоступен.")
    tracks: list[Track] = []
    volumes = album.volumes or []
    for volume in volumes:
        for track in volume:
            if track.albums:
                track.albums[0].volumes = volumes
            tracks.append(track)
    return album, tracks


def resolve_source(
    client: Client,
    text: str,
    only_music: bool = True,
    stick_to_artist: bool = False,
    on_progress: Optional[Callable[[int, int], None]] = None,
) -> Source:
    kind, args = parse_link(text)

    if kind == "track":
        tracks = client.tracks(args[0]) or []
        if not tracks:
            raise SourceError("Трек не найден или недоступен.")
        track = tracks[0]
        return Source("track", track.title or "Трек", artist_line(track), tracks)

    if kind == "album":
        album, tracks = _album_tracks(client, args[0])
        return Source("album", album.title or f"Альбом {args[0]}", artist_line(album), tracks)

    if kind == "playlist":
        owner, playlist_kind = args
        playlist = client.users_playlists(playlist_kind, user_id=owner)
        if playlist is None:
            raise SourceError(f"Плейлист {owner}/{playlist_kind} не найден.")
        shorts = playlist.fetch_tracks() or []
        ids = [short.id for short in shorts if getattr(short, "id", None)]
        tracks = fetch_tracks(client, ids, on_progress)
        subtitle = f"{owner} · {len(tracks)} треков"
        return Source("playlist", playlist.title or f"Плейлист {owner}/{playlist_kind}", subtitle, tracks)

    if kind == "playlist_uuid":
        playlist = client.playlist(args[0])
        if playlist is None:
            raise SourceError("Плейлист не найден.")
        shorts = playlist.fetch_tracks() or []
        ids = [short.id for short in shorts if getattr(short, "id", None)]
        tracks = fetch_tracks(client, ids, on_progress)
        return Source("playlist", playlist.title or "Плейлист", f"{len(tracks)} треков", tracks)

    if kind == "artist":
        artist_id = args[0]
        artists = client.artists(artist_id) or []
        name = artist_line(artists[0]) if artists else f"Исполнитель {artist_id}"
        album_ids: list[str] = []
        page = 0
        while True:
            albums_info = client.artists_direct_albums(artist_id, page)
            if albums_info is None:
                break
            for album in albums_info.albums or []:
                if not album.available or album.id is None:
                    continue
                if only_music and album.meta_type != "music":
                    continue
                if stick_to_artist and album.artists and str(album.artists[0].id) != str(artist_id):
                    continue
                album_ids.append(str(album.id))
            pager = albums_info.pager
            if pager is None:
                break
            page = pager.page + 1
            if pager.per_page * page >= pager.total:
                break
        tracks: list[Track] = []
        for index, album_id in enumerate(album_ids):
            _, album_tracks = _album_tracks(client, album_id)
            tracks.extend(album_tracks)
            if on_progress is not None:
                on_progress(index + 1, len(album_ids))
        return Source("artist", name, f"{len(album_ids)} альбомов · {len(tracks)} треков", tracks)

    raise SourceError("Неподдерживаемый тип ссылки.")


def likes_source(client: Client, username: str = "", on_progress: Optional[Callable[[int, int], None]] = None) -> Source:
    user_id = username.strip() or None
    likes = client.users_likes_tracks(user_id)
    shorts = getattr(likes, "tracks", None) or []
    ids = [short.id for short in shorts if getattr(short, "id", None)]
    if not ids:
        raise SourceError("В «Мне нравится» нет треков.")
    tracks = fetch_tracks(client, ids, on_progress)
    subtitle = f"{user_id} · {len(tracks)} треков" if user_id else f"{len(tracks)} треков"
    return Source("likes", "Мне нравится", subtitle, tracks)


# --- helpers -----------------------------------------------------------
def artist_line(obj) -> str:
    artists = getattr(obj, "artists", None) or []
    names = [a.name for a in artists if getattr(a, "name", None)]
    return ", ".join(names)


def track_artist(track: Track) -> str:
    return artist_line(track) or "Неизвестный исполнитель"


def track_album(track: Track) -> str:
    albums = track.albums or []
    if not albums:
        return ""
    return albums[0].title or ""


def format_duration(ms: int | None) -> str:
    if not ms:
        return ""
    seconds = int(ms) // 1000
    return f"{seconds // 60}:{seconds % 60:02d}"


def format_bytes(value: int) -> str:
    size = float(value)
    for unit in ("Б", "КиБ", "МиБ", "ГиБ"):
        if size < 1024 or unit == "ГиБ":
            return f"{size:.1f} {unit}" if unit != "Б" else f"{int(size)} Б"
        size /= 1024
    return f"{size:.1f} ГиБ"


def open_in_browser(url: str) -> None:
    from PySide6.QtCore import QUrl
    from PySide6.QtGui import QDesktopServices

    QDesktopServices.openUrl(QUrl(url))


def reveal_in_file_manager(path: Path) -> None:
    from PySide6.QtCore import QUrl
    from PySide6.QtGui import QDesktopServices

    QDesktopServices.openUrl(QUrl.fromLocalFile(str(path)))
