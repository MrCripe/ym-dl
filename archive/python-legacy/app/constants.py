"""Application-wide constants."""

APP_NAME = "ym-dl"
APP_TITLE = "ym-dl — Яндекс.Музыка"
APP_VERSION = "1.0.0"
APP_TAGLINE = "Яндекс.Музыка → FLAC"

TOKEN_HELP_URL = "https://ym.marshal.dev/token/"
LOADER_REPO = "llistochek/yandex-music-downloader"
LOADER_ZIP = f"https://github.com/{LOADER_REPO}/archive/main.zip"

QUALITY_LABELS = {
    0: "Низкое (AAC 64)",
    1: "Оптимальное (AAC 192)",
    2: "Лучшее (FLAC)",
}
