"""Update checks for the downloader package and the Yandex Music library.

The downloader is installed from the GitHub archive (it is not published to
PyPI), so its newest version is read from the repository's pyproject.toml.
"""

from __future__ import annotations

import importlib.metadata as metadata
import re
import subprocess
import sys
from dataclasses import dataclass

import requests

from .constants import LOADER_REPO, LOADER_ZIP

PYPROJECT_URL = f"https://raw.githubusercontent.com/{LOADER_REPO}/main/pyproject.toml"
LOADER_DIST = "yandex-music-downloader"
LIBRARY_DIST = "yandex-music"
TIMEOUT = 12


@dataclass
class UpdateInfo:
    name: str
    installed: str
    latest: str
    newer: bool
    detail: str = ""


def installed(dist: str) -> str:
    try:
        return metadata.version(dist)
    except metadata.PackageNotFoundError:
        return ""


def _get(url: str) -> requests.Response:
    return requests.get(url, timeout=TIMEOUT, headers={"User-Agent": "ym-dl"})


def _version_tuple(text: str) -> tuple[int, ...]:
    parts = re.findall(r"\d+", text or "")
    return tuple(int(p) for p in parts[:4]) or (0,)


def check_updates() -> list[UpdateInfo]:
    result: list[UpdateInfo] = []

    installed_loader = installed(LOADER_DIST)
    try:
        response = _get(PYPROJECT_URL)
        response.raise_for_status()
        match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', response.text)
        latest_loader = match.group(1) if match else ""
    except requests.RequestException as error:
        latest_loader = ""
        result.append(
            UpdateInfo("Загрузчик", installed_loader or "—", "недоступно", False,
                       f"GitHub не ответил: {type(error).__name__}")
        )
    else:
        if latest_loader:
            result.append(
                UpdateInfo(
                    "Загрузчик",
                    installed_loader or "—",
                    latest_loader,
                    _version_tuple(latest_loader) > _version_tuple(installed_loader),
                    f"github.com/{LOADER_REPO}",
                )
            )

    installed_lib = installed(LIBRARY_DIST)
    try:
        response = _get(f"https://pypi.org/pypi/{LIBRARY_DIST}/json")
        response.raise_for_status()
        latest_lib = response.json()["info"]["version"]
    except (requests.RequestException, KeyError, ValueError) as error:
        result.append(
            UpdateInfo("Библиотека API", installed_lib or "—", "недоступно", False,
                       f"PyPI не ответил: {type(error).__name__}")
        )
    else:
        result.append(
            UpdateInfo(
                "Библиотека API",
                installed_lib or "—",
                latest_lib,
                _version_tuple(latest_lib) > _version_tuple(installed_lib),
                "pypi.org",
            )
        )
    return result


def _pip_install(target: str) -> tuple[bool, str]:
    if getattr(sys, "frozen", False):
        # ponytail: в собранном .exe пакеты вшиты, обновление только из исходников.
        return False, "Собранное приложение обновляется заменой сборки, а не через pip."
    command = [sys.executable, "-m", "pip", "install", "--upgrade", "--no-input", target]
    try:
        result = subprocess.run(command, capture_output=True, text=True, timeout=600)
    except (OSError, subprocess.SubprocessError) as error:
        return False, f"Не удалось запустить pip: {error}"
    tail = "\n".join((result.stdout or result.stderr or "").strip().splitlines()[-4:])
    return result.returncode == 0, tail


def update_loader() -> tuple[bool, str]:
    return _pip_install(LOADER_ZIP)


def update_library() -> tuple[bool, str]:
    return _pip_install(LIBRARY_DIST)
