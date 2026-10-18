"""ffmpeg helpers: pull the real FLAC stream out of an .m4a container.

Yandex Music serves lossless as FLAC inside an MP4 container, so a finished
download can still be named .m4a. `extract_flac` remuxes the audio stream into
a .flac file and verifies the result with ffprobe before replacing anything.
"""

from __future__ import annotations

import shutil
import subprocess
import sys
import uuid
from pathlib import Path
from typing import Optional

AUDIO_SUFFIXES = {".mp3", ".flac", ".m4a"}
BACKUP_DIR_NAME = "_old_m4a"


def _flags() -> int:
    return 0x08000000 if sys.platform == "win32" else 0  # CREATE_NO_WINDOW


def tools() -> tuple[Optional[str], Optional[str]]:
    return shutil.which("ffmpeg"), shutil.which("ffprobe")


def available() -> bool:
    ffmpeg, ffprobe = tools()
    return bool(ffmpeg and ffprobe)


def codec_of(path: Path) -> str:
    _, ffprobe = tools()
    if not ffprobe:
        return ""
    try:
        result = subprocess.run(
            [ffprobe, "-v", "error", "-select_streams", "a:0", "-show_entries",
             "stream=codec_name", "-of", "default=noprint_wrappers=1:nokey=1", str(path)],
            capture_output=True,
            text=True,
            timeout=60,
            creationflags=_flags(),
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    if result.returncode != 0:
        return ""
    return (result.stdout or "").strip().splitlines()[0].lower() if result.stdout.strip() else ""


def extract_flac(source: Path) -> str:
    """Returns 'converted', 'exists' or 'failed'."""
    ffmpeg, _ = tools()
    if not ffmpeg:
        return "failed"
    target = source.with_suffix(".flac")
    if target.exists():
        return "exists" if codec_of(target) == "flac" else "failed"

    part = source.with_name(f".{source.stem}.part.flac")
    attempts = (
        ["-map", "0:a:0", "-map", "0:v?", "-map_metadata", "0", "-c:a", "copy",
         "-c:v", "copy", "-disposition:v", "attached_pic", "-f", "flac"],
        ["-map", "0:a:0", "-map_metadata", "0", "-vn", "-c:a", "copy", "-f", "flac"],
    )
    for extra in attempts:
        command = [ffmpeg, "-y", "-hide_banner", "-loglevel", "error", "-nostdin",
                   "-i", str(source), *extra, str(part)]
        try:
            result = subprocess.run(command, capture_output=True, timeout=300, creationflags=_flags())
        except (OSError, subprocess.SubprocessError):
            result = None
        ok = (
            result is not None
            and result.returncode == 0
            and part.exists()
            and part.stat().st_size > 0
            and codec_of(part) == "flac"
        )
        if ok:
            part.replace(target)
            return "converted"
        part.unlink(missing_ok=True)
    return "failed"


def quarantine(source: Path, directory: Path) -> bool:
    backup = directory / BACKUP_DIR_NAME
    try:
        backup.mkdir(parents=True, exist_ok=True)
        source.replace(backup / f"{uuid.uuid4().hex[:8]}-{source.name}")
        return True
    except OSError:
        return False


def post_process(directory: Path, quarantine_lossy: bool) -> dict[str, int]:
    """Converts FLAC-in-MP4 files in `directory` and optionally moves lossy ones."""
    stats = {"converted": 0, "quarantined": 0, "failed": 0}
    if not directory.is_dir():
        return stats
    backup = directory / BACKUP_DIR_NAME
    for path in sorted(directory.rglob("*")):
        if not path.is_file() or path.suffix.lower() not in AUDIO_SUFFIXES:
            continue
        if backup in path.parents:
            continue
        codec = codec_of(path)
        if path.suffix.lower() == ".m4a" and codec == "flac":
            outcome = extract_flac(path)
            if outcome == "converted":
                stats["converted"] += 1
                quarantine(path, directory)
            elif outcome == "exists":
                quarantine(path, directory)
            else:
                stats["failed"] += 1
        elif quarantine_lossy and codec in ("aac", "mp3"):
            if quarantine(path, directory):
                stats["quarantined"] += 1
    return stats
