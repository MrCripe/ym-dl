"""Persistent settings and token storage.

Settings live in a per-user config directory; the token keeps using the file
the old console launcher wrote (~/.ym-dl-token) so an existing token is picked
up without any migration.
"""

from __future__ import annotations

import json
import os
import sys
import tempfile
from dataclasses import asdict, dataclass, fields
from pathlib import Path

TOKEN_FILE = Path.home() / ".ym-dl-token"

QUALITY_CHOICES = (0, 1, 2)
LYRICS_CHOICES = ("none", "text", "lrc")
THEME_CHOICES = ("system", "light", "dark")


def config_dir() -> Path:
    if sys.platform == "win32":
        base = os.environ.get("APPDATA") or str(Path.home() / "AppData" / "Roaming")
    else:
        base = os.environ.get("XDG_CONFIG_HOME") or str(Path.home() / ".config")
    return Path(base) / "ym-dl"


def default_download_dir() -> Path:
    music = Path.home() / "Music"
    return music / "lmao"


@dataclass
class Settings:
    # account
    username: str = ""
    # library
    download_dir: str = ""
    path_pattern: str = "#track-artist - #title"
    quality: int = 2
    skip_existing: bool = True
    # metadata
    embed_cover: bool = True
    cover_original: bool = False
    cover_resolution: int = 400
    lyrics_format: str = "none"
    compatibility_level: int = 1
    only_music: bool = True
    stick_to_artist: bool = False
    unsafe_path: bool = False
    # network
    delay: int = 0
    timeout: int = 20
    tries: int = 20
    retry_delay: int = 5
    # post-processing
    extract_flac: bool = True
    quarantine_lossy: bool = False
    # ui
    theme: str = "system"
    check_updates: bool = True
    last_url: str = ""

    @property
    def target_dir(self) -> Path:
        raw = self.download_dir.strip()
        return Path(os.path.expanduser(raw)) if raw else default_download_dir()

    @property
    def cover_size(self) -> int:
        return -1 if self.cover_original else max(100, int(self.cover_resolution))


class Config:
    """Loads/saves Settings and keeps the token out of the JSON file."""

    def __init__(self, path: Path | None = None) -> None:
        self.path = path or (config_dir() / "config.json")
        self.settings = Settings()
        self.load()

    def load(self) -> None:
        try:
            raw = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return
        known = {f.name for f in fields(Settings)}
        for key, value in raw.items():
            if key in known:
                setattr(self.settings, key, value)
        self._sanitize()

    def _sanitize(self) -> None:
        s = self.settings
        if s.quality not in QUALITY_CHOICES:
            s.quality = 2
        if s.lyrics_format not in LYRICS_CHOICES:
            s.lyrics_format = "none"
        if s.theme not in THEME_CHOICES:
            s.theme = "system"
        s.compatibility_level = min(1, max(0, int(s.compatibility_level or 0)))
        s.timeout = max(1, int(s.timeout or 20))
        s.tries = max(0, int(s.tries or 0))
        s.retry_delay = max(0, int(s.retry_delay or 0))
        s.delay = max(0, int(s.delay or 0))
        s.cover_resolution = max(100, int(s.cover_resolution or 400))

    def save(self) -> None:
        self._sanitize()
        self.path.parent.mkdir(parents=True, exist_ok=True)
        payload = json.dumps(asdict(self.settings), ensure_ascii=False, indent=2)
        write_atomic(self.path, payload.encode("utf-8"))

    # --- token ---------------------------------------------------------
    @staticmethod
    def read_token() -> str:
        try:
            return TOKEN_FILE.read_text(encoding="utf-8").strip()
        except OSError:
            return os.environ.get("YM_TOKEN", "").strip()

    @staticmethod
    def write_token(token: str) -> None:
        token = token.strip()
        if not token:
            Config.clear_token()
            return
        write_atomic(TOKEN_FILE, token.encode("utf-8"), mode=0o600)

    @staticmethod
    def clear_token() -> None:
        try:
            TOKEN_FILE.unlink()
        except OSError:
            pass


def write_atomic(path: Path, data: bytes, mode: int | None = None) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp = tempfile.mkstemp(dir=str(path.parent), prefix=".tmp-", suffix=path.suffix)
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
        if mode is not None and os.name == "posix":
            os.chmod(tmp, mode)
        os.replace(tmp, path)
    except BaseException:
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise
