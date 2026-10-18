#requires -Version 5.1
[CmdletBinding()]
param(
    [string]$Url = '',
    [string]$Dir = '',
    [ValidateRange(-1, 2)]
    [int]$Quality = -1,
    [string]$PathPattern = ''
)
$ErrorActionPreference = 'Stop'
if (Test-Path variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}
try { [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false) } catch {}
$env:PYTHONUTF8 = '1'
$env:PYTHONIOENCODING = 'utf-8'
$env:PYTHONUNBUFFERED = '1'
$ScriptDir = if ($env:YM_DL_APP_DIR -and [IO.Directory]::Exists($env:YM_DL_APP_DIR)) { [IO.Path]::GetFullPath($env:YM_DL_APP_DIR) } elseif ($PSCommandPath) { Split-Path -Parent $PSCommandPath } else { (Get-Location).Path }
$ConfigPath = Join-Path $ScriptDir 'ym-dl.config.json'
$TokenFile = Join-Path $env:USERPROFILE '.ym-dl-token'
$LoaderZip = 'https://github.com/llistochek/yandex-music-downloader/archive/main.zip'
$ProgressRunnerCode = @'
import os,sys,time
from urllib.parse import urlsplit
for _s in (sys.stdout, sys.stderr):
    try: _s.reconfigure(errors="replace")
    except Exception: pass
_LOG_PATH = os.path.join(os.path.dirname(os.path.abspath(__file__)), "ym-dl-debug.log")
def _trace(message):
    line = str(message)
    print(line, flush=True)
    try:
        with open(_LOG_PATH, "a", encoding="utf-8") as f: f.write(line + "\n")
    except OSError: pass
_trace("[RUNNER] Python запущен; секреты не записываются в журнал.")
try:
    import requests
    import ymd.api
    from yandex_music import Client
    from yandex_music.exceptions import NotFoundError
except BaseException as exc:
    _trace("[RUNNER-IMPORT-ERROR] " + type(exc).__name__ + ": " + str(exc)[:180])
    raise
_trace("[RUNNER] Импорты библиотек завершены.")
_original_client_playlist = Client.playlist
def _compatible_client_playlist(self, playlist_uuid, *args, **kwargs):
    if isinstance(playlist_uuid, str) and "/" in playlist_uuid:
        owner, kind = playlist_uuid.rsplit("/", 1)
        print(f"[PLAYLIST] Запрашиваю плейлист: владелец={owner}, ID={kind}", flush=True)
        playlist = self.users_playlists(kind, user_id=owner)
        if playlist is None:
            raise NotFoundError({"name": "playlist-not-found", "message": ""})
        count = getattr(playlist, "track_count", None)
        print(f"[PLAYLIST] Получен ответ API; треков по счётчику: {count}", flush=True)
        return playlist
    return _original_client_playlist(self, playlist_uuid, *args, **kwargs)
Client.playlist = _compatible_client_playlist
_original_request = requests.request
_downloading_track = False
_current_audio_urls = set()
_progress_active = False
def _render_progress(done, total, elapsed):
    elapsed = max(elapsed, 0.001)
    mib = done / (1024 * 1024)
    speed = mib / elapsed
    if total > 0:
        frac = min(done / total, 1.0)
        percent = int(frac * 100)
        width = 24
        filled = int(width * frac)
        bar = "#" * filled + "-" * (width - filled)
        return f"[{bar}] {percent:3d}%  {mib:.1f}/{total / (1024 * 1024):.1f} MiB  {speed:.1f} MiB/s"
    return f"{mib:.1f} MiB скачано  {speed:.1f} MiB/s (сервер не сообщил общий размер)"

def _progress_request(method, url, *args, **kwargs):
    global _progress_active
    is_audio = (_downloading_track and str(method).upper() == "GET" and
                (not _current_audio_urls or str(url) in _current_audio_urls))
    if not is_audio:
        try: endpoint = (urlsplit(str(url)).netloc + urlsplit(str(url)).path)[:160]
        except Exception: endpoint = "API endpoint"
        started = time.monotonic()
        _trace(f"[HTTP] -> {str(method).upper()} {endpoint}")
        try: response = _original_request(method, url, *args, **kwargs)
        except BaseException as exc:
            _trace(f"[HTTP-ERROR] {endpoint}: {type(exc).__name__} after {time.monotonic()-started:.1f}s")
            raise
        _trace(f"[HTTP] <- {endpoint}: HTTP {response.status_code}, {time.monotonic()-started:.1f}s")
        return response

    kwargs["stream"] = True
    _trace("[DOWNLOAD] Соединяюсь с аудиосервером; прогресс появится после первого ответа.")
    sys.stderr.write("\n    Прогресс загрузки: соединение с аудиосервером...\n")
    sys.stderr.flush()
    with requests.Session() as session:
        response = session.request(method, url, *args, **kwargs)
        _trace(f"[DOWNLOAD] Аудиосервер ответил HTTP {response.status_code}; Content-Length={response.headers.get('Content-Length', '?')}")
        if not (200 <= response.status_code < 300):
            response._content = response.content
            response._content_consumed = True
            response.close()
            return response

        try:
            total = int(response.headers.get("Content-Length", "0"))
        except (TypeError, ValueError):
            total = 0
        if total < 0:
            total = 0

        body = bytearray()
        started = time.monotonic()
        last_render = 0.0
        _progress_active = True
        sys.stderr.write("\r    Прогресс загрузки: подключено, получаю данные...")
        sys.stderr.flush()
        try:
            for chunk in response.iter_content(chunk_size=64 * 1024):
                if not chunk:
                    continue
                body.extend(chunk)
                now = time.monotonic()
                if now - last_render >= 0.12:
                    sys.stderr.write("\r    Прогресс загрузки: " + _render_progress(len(body), total, now - started) + "   ")
                    sys.stderr.flush()
                    last_render = now

            response._content = bytes(body)
            response._content_consumed = True
            elapsed = max(time.monotonic() - started, 0.001)
            sys.stderr.write("\r    Прогресс загрузки: " + _render_progress(len(body), total, elapsed) + "\n")
            sys.stderr.flush()
            _progress_active = False
            response.close()
            return response
        except BaseException:
            sys.stderr.write("\n    Загрузка прервана во время получения данных.\n")
            sys.stderr.flush()
            _progress_active = False
            response.close()
            raise

requests.request = _progress_request

from ymd.cli import main
from ymd import core as ymd_core
from yandex_music import Playlist

_original_init_client = ymd_core.init_client
def _logged_init_client(*args, **kwargs):
    _trace("[API] Инициализирую клиент...")
    try: result = _original_init_client(*args, **kwargs)
    except BaseException as exc:
        _trace(f"[API-INIT-ERROR] {type(exc).__name__}: {str(exc)[:180]}")
        raise
    _trace("[API] Клиент готов.")
    return result
ymd_core.init_client = _logged_init_client

ymd_core.get_download_info = ymd.api.get_download_info
print("[RUNNER] Обработчик get-file-info подключён к исправленной API-функции.", flush=True)

_original_fetch_tracks = Playlist.fetch_tracks
def _logged_fetch_tracks(self, *args, **kwargs):
    print(f"[PLAYLIST] Получаю список треков: {getattr(self, 'title', 'без названия')}...", flush=True)
    tracks = _original_fetch_tracks(self, *args, **kwargs)
    try:
        count = len(tracks)
    except TypeError:
        count = "?"
    print(f"[PLAYLIST] Список треков получен: {count}", flush=True)
    return tracks
Playlist.fetch_tracks = _logged_fetch_tracks

_requested_quality = "0"
for _i, _arg in enumerate(sys.argv[:-1]):
    if _arg == "--quality":
        _requested_quality = sys.argv[_i + 1]
        break

_original_to_downloadable = ymd_core.to_downloadable_track
def _trace_to_downloadable(track, quality, base_path, *args, **kwargs):
    item = _original_to_downloadable(track, quality, base_path, *args, **kwargs)
    info = item.download_info
    codec = getattr(getattr(info.file_format, "codec", None), "name", "UNKNOWN")
    container = getattr(getattr(info.file_format, "container", None), "name", "UNKNOWN")
    quality_returned = getattr(info, "quality", "unknown")
    bitrate = getattr(info, "bitrate", 0)
    print(f"[API-CODEC] requested={quality} returned_quality={quality_returned} codec={codec} container={container} bitrate={bitrate}kbps", flush=True)
    return item
ymd_core.to_downloadable_track = _trace_to_downloadable

_original_core_download_track = ymd_core.download_track
_lossy_skipped = 0
def _guarded_download_track(track_info, *args, **kwargs):
    global _lossy_skipped, _downloading_track, _current_audio_urls
    codec = getattr(getattr(track_info.download_info.file_format, "codec", None), "name", "UNKNOWN")
    info = track_info.download_info
    if codec != "FLAC":
        _lossy_skipped += 1
        print(f"[AAC-FALLBACK] {codec}: FLAC недоступен, качаю лучшее доступное.", flush=True)
    urls = getattr(info, "urls", None) or []
    _current_audio_urls = set(map(str, urls))
    _downloading_track = True
    print("[DOWNLOAD] Получаю аудиоданные; дальше появится байтовый прогресс.", flush=True)
    try:
        result = _original_core_download_track(track_info, *args, **kwargs)
        print("[DOWNLOAD] Аудиоданные получены.", flush=True)
        return result
    except Exception as exc:
        print(f"[DOWNLOAD-ERROR] {type(exc).__name__}: {str(exc)[:300]}", flush=True)
        raise
    finally:
        _downloading_track = False
        _current_audio_urls = set()
ymd_core.download_track = _guarded_download_track

if __name__ == "__main__":
    try:
        print("[RUNNER] Запущен. Проверяю доступ к плейлисту/треку...", flush=True)
        main()
        print("[RUNNER] Работа загрузчика завершена.", flush=True)
        if _lossy_skipped:
            print(f"\nИТОГ: {_lossy_skipped} трек(ов) в AAC — FLAC не выдан.", flush=True)
    except NotFoundError:
        playlist_arg = None
        for i, arg in enumerate(sys.argv[:-1]):
            if arg == "--playlist-id":
                playlist_arg = sys.argv[i + 1]
                break
        if playlist_arg and "/" in playlist_arg:
            print("\nПлейлист не найден через API Яндекс.Музыки: " + playlist_arg)
            print("Проверь, что имя владельца и номер плейлиста верны, а плейлист доступен по ссылке.")
            sys.exit(1)
        raise

'@
$LosslessApiPatchCode = @'
import os,re,sys,shutil,time
from pathlib import Path

try:
    import ymd.api as api
    import yandex_music.utils.sign_request as signer
except Exception as exc:
    print("ERROR|Не удалось импортировать ymd или yandex_music: " + type(exc).__name__)
    sys.exit(2)

if not all(hasattr(signer, name) for name in ("DESKTOP_CLIENT", "get_file_info_sign")):
    print("ERROR|В установленной yandex-music нет desktop signer; обнови yandex-music и повтори.")
    sys.exit(3)

path = Path(api.__file__).resolve()
source = path.read_text(encoding="utf-8-sig")
patched_marker = "sign = get_file_info_sign(track.id, quality_value, codecs, transport)"
patched_v2_marker = "[LOSSLESS-API] requested="

if patched_marker in source and "request.headers[header_name] = DESKTOP_CLIENT" in source and patched_v2_marker in source:
    print("PATCH|already_patched|" + str(path))
else:
    import_pattern = re.compile(r"(?m)^from yandex_music\.utils\.sign_request import [^\r\n]*$")
    matches = list(import_pattern.finditer(source))
    if len(matches) != 1:
        print("ERROR|Не удалось однозначно найти импорт sign_request в " + str(path))
        sys.exit(4)
    source = import_pattern.sub(
        "from yandex_music.utils.sign_request import DESKTOP_CLIENT, get_file_info_sign",
        source, count=1
    )

    start = source.find("def get_download_info(")
    end = source.find("\ndef download_track(", start)
    if start < 0 or end < 0:
        print("ERROR|Структура ymd/api.py отличается от ожидаемой; файл не изменён.")
        sys.exit(5)

    replacement = '''def get_download_info(track: Track, quality: ApiTrackQuality) -> CustomDownloadInfo:
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
    header_name = "X-Yandex-Music-Client"
    original_client = request.headers.get(header_name)
    request.headers[header_name] = DESKTOP_CLIENT
    try:
        resp = request.get("https://api.music.yandex.net/get-file-info", params=params)
    finally:
        if original_client is None:
            request.headers.pop(header_name, None)
        else:
            request.headers[header_name] = original_client

    e = resp["downloadInfo"]
    raw_codec = e["codec"]
    print("[LOSSLESS-API] requested=" + quality_value + " returned=" + str(e.get("quality", "unknown")) + " codec=" + str(raw_codec) + " bitrate=" + str(e.get("bitrate", "unknown")) + "kbps", flush=True)
    file_format = FILE_FORMAT_MAPPING.get(raw_codec)
    if file_format is None:
        raise ValueError("Unknown codec: " + str(raw_codec))

    return CustomDownloadInfo(
        quality=e["quality"],
        file_format=file_format,
        urls=e["urls"],
        bitrate=e["bitrate"],
        decryption_key=e.get("key"),
    )
'''
    source = source[:start] + replacement + source[end:]
    compile(source, str(path), "exec")
    stamp = time.strftime("%Y%m%d-%H%M%S")
    backup = path.with_name(path.name + ".bak-ym-dl-" + stamp)
    suffix = 1
    while backup.exists():
        backup = path.with_name(path.name + ".bak-ym-dl-" + stamp + "-" + str(suffix))
        suffix += 1
    shutil.copy2(path, backup)
    temp = path.with_suffix(path.suffix + ".ym-dl-tmp")
    with temp.open("w", encoding="utf-8", newline="\n") as stream:
        stream.write(source)
    os.replace(temp, path)
    print("PATCH|applied|" + str(path))
    print("BACKUP|" + str(backup))
'@

$ProgressRunnerPath = Join-Path $ScriptDir 'ymd-progress-runner.py'
$Defaults = [ordered]@{
    Url = 'https://music.yandex.ru/users/MrCriper10/playlists/3'
    Dir = ''
    Quality = 2
    PathPattern = '#track-artist - #title'
}

function Say {
    param([AllowEmptyString()][string]$Text, [string]$Color = 'Gray')
    Write-Host $Text -ForegroundColor $Color
}

function Format-Quality {
    param([int]$Q)
    switch ($Q) {
        0 { 'AAC 64 (0)' }
        1 { 'AAC 192 (1)' }
        2 { 'Lossless → FLAC (2)' }
        default { "неизвестно ($Q)" }
    }
}

function Get-Settings {
    $cfg = [ordered]@{}
    foreach ($key in $Defaults.Keys) { $cfg[$key] = $Defaults[$key] }
    if (Test-Path -LiteralPath $ConfigPath) {
        try {
            $saved = Get-Content -LiteralPath $ConfigPath -Raw -Encoding UTF8 | ConvertFrom-Json
            foreach ($key in $Defaults.Keys) {
                $prop = $saved.PSObject.Properties[$key]
                if ($null -ne $prop -and $null -ne $prop.Value -and "$($prop.Value)" -ne '') {
                    $cfg[$key] = $prop.Value
                }
            }
        } catch {
            Say "Не удалось прочитать конфиг; использую значения по умолчанию. Причина: $($_.Exception.Message)" 'Yellow'
        }
    }
    $cfg.Quality = [int]$cfg.Quality
    return $cfg
}

function Save-Settings {
    param($Settings)
    $json = $Settings | ConvertTo-Json -Depth 4
    [IO.File]::WriteAllText($ConfigPath, $json, [Text.UTF8Encoding]::new($false))
}

function Get-Token {
    if (Test-Path -LiteralPath $TokenFile) {
        $raw = Get-Content -LiteralPath $TokenFile -Raw -ErrorAction SilentlyContinue
        if ($null -ne $raw) {
            $value = $raw.Trim()
            if ($value) { return $value }
        }
    }
    if ($env:YM_TOKEN) { return $env:YM_TOKEN.Trim() }
    return ''
}

function Save-Token {
    param([string]$Token)
    [IO.File]::WriteAllText($TokenFile, $Token.Trim(), [Text.UTF8Encoding]::new($false))
}

function Get-PythonExe {
    foreach ($name in @('python', 'python3')) {
        $cmd = Get-Command $name -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($cmd -and $cmd.Source) { return $cmd.Source }
    }
    return ''
}

function Test-PythonModule {
    param([string]$Module = 'ymd')
    $python = Get-PythonExe
    if (-not $python) { return $false }
    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $python -c "import importlib.util,sys; sys.exit(0 if importlib.util.find_spec('$Module') else 1)" 2>$null | Out-Null
        return ($LASTEXITCODE -eq 0)
    } catch { return $false }
    finally { $ErrorActionPreference = $oldPreference }
}

function Find-YmdExe {
    $cmd = Get-Command yandex-music-downloader -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($cmd -and $cmd.Source) { return $cmd.Source }
    return ''
}

function Repair-YmdLosslessApi {
    param([string]$Python)
    if (-not $Python -or -not (Test-PythonModule -Module 'ymd')) { return $false }

    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = @($LosslessApiPatchCode | & $Python - 2>&1 | ForEach-Object { "$_" })
        $exitCode = $LASTEXITCODE
    } catch {
        Say "Не удалось обновить обработчик lossless API: $($_.Exception.Message)" 'Red'
        return $false
    } finally {
        $ErrorActionPreference = $oldPreference
    }

    foreach ($line in $output) {
        if ($line -like 'ERROR|*') { Say $line 'Red' }
        elseif ($line -like 'PATCH|applied|*') { Say 'Исправлена подпись lossless API: Windows desktop signer + согласованный заголовок.' 'Green' }
        elseif ($line -like 'PATCH|already_patched|*') { Say 'Подпись lossless API уже исправлена.' 'Green' }
        elseif ($line -like 'BACKUP|*') { Say "Резервная копия: $($line.Substring(7))" 'Gray' }
        else { Say $line 'Gray' }
    }
    if ($exitCode -ne 0 -or ($output | Where-Object { $_ -like 'ERROR|*' })) { return $false }
    return $true
}

function Invoke-Ymd {
    param([string[]]$Arguments)
    $python = Get-PythonExe
    $wantsLossless = $false
    for ($i = 0; $i -lt ($Arguments.Count - 1); $i++) {
        if ($Arguments[$i] -eq '--quality' -and $Arguments[$i + 1] -eq '2') { $wantsLossless = $true; break }
    }

    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        if ($python -and (Test-PythonModule -Module 'ymd')) {
            if ($wantsLossless -and -not (Repair-YmdLosslessApi -Python $python)) {
                Say 'Загрузка отменена: не удалось исправить подпись запроса lossless. Так мы не будем снова молча скачивать AAC/MP3.' 'Red'
                Say 'Проверь, что в этом Python установлена yandex-music с desktop signer, затем повтори запуск.' 'Yellow'
                return 1
            }
            [IO.File]::WriteAllText($ProgressRunnerPath, $ProgressRunnerCode, [Text.UTF8Encoding]::new($false))
            Say "Запускаю загрузчик через Python: $ProgressRunnerPath" 'Cyan'
            Say "Журнал: $(Join-Path $ScriptDir 'ym-dl-debug.log')" 'Gray'
            & $python -u $ProgressRunnerPath @Arguments | Write-Host
            $runnerCode = $LASTEXITCODE
            Say "Python-загрузчик завершился с кодом $runnerCode" $(if ($runnerCode -eq 0) { 'Green' } else { 'Yellow' })
            return $runnerCode
        }
        $exe = Find-YmdExe
        if ($exe) {
            if ($wantsLossless) {
                Say 'Lossless отменён: найден только отдельный EXE загрузчика, а для ремонта нужен модуль ymd в доступном Python.' 'Red'
                Say 'Открой меню 5 → 1, установи загрузчик в найденный Python и повтори.' 'Yellow'
                return 1
            }
            Say 'Запускаю установленную команду загрузчика; байтовый индикатор доступен, когда модуль ymd установлен в найденном Python.' 'Yellow'
            & $exe @Arguments
            return $LASTEXITCODE
        }
        Say 'Загрузчик не найден. Открой меню 5 → 1 и установи его.' 'Red'
        return 127
    } catch {
        Say "Не удалось запустить загрузчик: $($_.Exception.Message)" 'Red'
        return 1
    } finally {
        $ErrorActionPreference = $oldPreference
    }
}
function Test-Ffmpeg { return [bool](Get-Command ffmpeg -ErrorAction SilentlyContinue) }
function Test-Ffprobe { return [bool](Get-Command ffprobe -ErrorAction SilentlyContinue) }

function Update-SessionPath {
    $parts = [Collections.Generic.List[string]]::new()
    foreach ($scope in @('Machine', 'User')) {
        $stored = [Environment]::GetEnvironmentVariable('Path', $scope)
        if ($stored) {
            foreach ($part in ($stored -split ';')) {
                $p = $part.Trim()
                if ($p -and -not $parts.Contains($p)) { $parts.Add($p) }
            }
        }
    }
    foreach ($part in ($env:Path -split ';')) {
        $p = $part.Trim()
        if ($p -and -not $parts.Contains($p)) { $parts.Add($p) }
    }
    $env:Path = $parts -join ';'
}

$TokenProbeCode = @'
import os
from yandex_music import Client
try:
    c = Client(os.environ["YM_DL_TOKEN_CHECK"]).init()
    s = c.account_status()
    plus = bool(s and getattr(getattr(s, "plus", None), "has_plus", False))
    account = getattr(s, "account", None) if s else None
    uid = getattr(account, "uid", "?")
    print("OK|plus=" + str(plus).lower() + "|uid=" + str(uid))
except Exception as e:
    message = (type(e).__name__ + ":" + str(e)).encode("ascii", "replace").decode()
    print("FAIL|" + message[:140])
'@

function Invoke-PythonSnippet {
    param([string]$Code, [string]$Token)
    $python = Get-PythonExe
    if (-not $python) { return @() }
    $oldPreference = $ErrorActionPreference
    $hadOldToken = Test-Path Env:\YM_DL_TOKEN_CHECK
    $oldToken = $env:YM_DL_TOKEN_CHECK
    $ErrorActionPreference = 'Continue'
    $env:YM_DL_TOKEN_CHECK = $Token
    try {
        return @($Code | & $python - 2>&1 | ForEach-Object { "$_" })
    } catch {
        return @("PowerShell: $($_.Exception.Message)")
    } finally {
        if ($hadOldToken) { $env:YM_DL_TOKEN_CHECK = $oldToken }
        else { Remove-Item Env:\YM_DL_TOKEN_CHECK -ErrorAction SilentlyContinue }
        $ErrorActionPreference = $oldPreference
    }
}

function Get-TokenProbe {
    param([string]$Token)
    if (-not (Get-PythonExe)) {
        return [pscustomobject]@{ State = 'unknown'; HasPlus = $null; Text = 'Python не найден, проверить токен нечем.' }
    }
    if (-not (Test-PythonModule -Module 'yandex_music')) {
        return [pscustomobject]@{ State = 'unknown'; HasPlus = $null; Text = 'В выбранном Python не установлен пакет yandex-music. Переустанови загрузчик через меню 5 → 1.' }
    }
    $output = @(Invoke-PythonSnippet -Code $TokenProbeCode -Token $Token)
    $line = $output | Where-Object { $_ -like 'OK|*' -or $_ -like 'FAIL|*' } | Select-Object -First 1
    if (-not $line) {
        return [pscustomobject]@{ State = 'unknown'; HasPlus = $null; Text = "Нет ответа проверки: $($output -join ' ')" }
    }
    if ($line -like 'FAIL|*') {
        $problem = $line.Substring(5)
        if ($problem -match '(?i)Unauthorized|InvalidToken|TokenExpired|AuthenticationError|\b401\b') {
            return [pscustomobject]@{ State = 'invalid'; HasPlus = $null; Text = "API отклонил токен: $problem" }
        }
        return [pscustomobject]@{ State = 'unknown'; HasPlus = $null; Text = "Проверка API не завершилась успешно (это не доказывает, что токен неверный): $problem" }
    }
    $hasPlus = $line -match '\|plus=true\|'
    $uid = if ($line -match '\|uid=([^|]+)') { $Matches[1] } else { '?' }
    return [pscustomobject]@{
        State = 'ok'
        HasPlus = [bool]$hasPlus
        Text = "ОК, аккаунт UID $uid; Plus: $([bool]$hasPlus)"
    }
}

function Get-AudioCodec {
    param([string]$Path)
    if (-not (Test-Ffprobe)) { return '' }
    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & ffprobe -v error -select_streams a:0 -show_entries stream=codec_name -of default=noprint_wrappers=1:nokey=1 $Path 2>$null
        if ($LASTEXITCODE -ne 0 -or -not $output) { return '' }
        return ("$($output | Select-Object -First 1)").Trim().ToLowerInvariant()
    } catch { return '' }
    finally { $ErrorActionPreference = $oldPreference }
}

function Get-M4aFiles {
    param([string]$Dir, [string]$ExcludeDir)
    $excluded = [IO.Path]::GetFullPath($ExcludeDir).TrimEnd([char[]]@('\', '/')) + [IO.Path]::DirectorySeparatorChar
    Get-ChildItem -LiteralPath $Dir -Filter '*.m4a' -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object {
            $full = [IO.Path]::GetFullPath($_.FullName)
            -not $full.StartsWith($excluded, [StringComparison]::OrdinalIgnoreCase)
        }
}

function Get-ExistingAudioFiles {
    param([string]$Dir, [string]$ExcludeDir)
    $excluded = [IO.Path]::GetFullPath($ExcludeDir).TrimEnd([char[]]@('\', '/')) + [IO.Path]::DirectorySeparatorChar
    Get-ChildItem -LiteralPath $Dir -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object {
            $_.Extension.ToLowerInvariant() -in @('.m4a', '.mp3') -and
            -not ([IO.Path]::GetFullPath($_.FullName).StartsWith($excluded, [StringComparison]::OrdinalIgnoreCase))
        }
}

function Move-ToBackup {
    param([string]$Path, [string]$BackupDir)
    try {
        [void][IO.Directory]::CreateDirectory($BackupDir)
        $unique = [guid]::NewGuid().ToString('N').Substring(0, 8)
        $target = Join-Path $BackupDir "$unique-$(Split-Path -Leaf $Path)"
        [IO.File]::Move($Path, $target)
        return $true
    } catch {
        Say "Не удалось переместить $(Split-Path -Leaf $Path) в резервную папку: $($_.Exception.Message)" 'Yellow'
        return $false
    }
}

function Invoke-FlacRemux {
    param([string]$Source)
    $target = [IO.Path]::ChangeExtension($Source, '.flac')
    $part = [IO.Path]::ChangeExtension($Source, '.part.flac')
    if (Test-Path -LiteralPath $target) {
        if ((Get-AudioCodec -Path $target) -eq 'flac') {
            Say "  FLAC уже существует, не перезаписываю: $(Split-Path -Leaf $target)" 'Yellow'
            return 'exists'
        }
        Say "  Целевой файл уже существует, но не подтверждён как FLAC; не перезаписываю: $(Split-Path -Leaf $target)" 'Yellow'
        return 'failed'
    }

    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & ffmpeg -y -hide_banner -loglevel error -nostdin -i $Source `
            -map '0:a:0' -map '0:v?' -map_metadata 0 -c:a copy -c:v copy `
            -disposition:v attached_pic -f flac $part 2>$null | Out-Null
        $ok = ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $part) -and (Get-Item -LiteralPath $part).Length -gt 0)
        if (-not $ok) {
            Remove-Item -LiteralPath $part -Force -ErrorAction SilentlyContinue
            & ffmpeg -y -hide_banner -loglevel error -nostdin -i $Source `
                -map '0:a:0' -map_metadata 0 -vn -c:a copy -f flac $part 2>$null | Out-Null
            $ok = ($LASTEXITCODE -eq 0 -and (Test-Path -LiteralPath $part) -and (Get-Item -LiteralPath $part).Length -gt 0)
        }
        if (-not $ok) {
            Remove-Item -LiteralPath $part -Force -ErrorAction SilentlyContinue
            return 'failed'
        }
        $codec = Get-AudioCodec -Path $part
        if ($codec -ne 'flac') {
            Remove-Item -LiteralPath $part -Force -ErrorAction SilentlyContinue
            return 'failed'
        }
        [IO.File]::Move($part, $target)
        return 'converted'
    } catch {
        Remove-Item -LiteralPath $part -Force -ErrorAction SilentlyContinue
        Say "  Ошибка перепаковки $(Split-Path -Leaf $Source): $($_.Exception.Message)" 'Yellow'
        return 'failed'
    } finally { $ErrorActionPreference = $oldPreference }
}

function Invoke-M4aMaintenance {
    param([string]$Dir, [switch]$OfferAacQuarantine)
    if (-not ((Test-Ffmpeg) -and (Test-Ffprobe))) {
        Say 'Для определения кодека и извлечения FLAC нужны ffmpeg и ffprobe.' 'Yellow'
        return $false
    }
    $backup = Join-Path $Dir '_old_m4a'
    $items = @(Get-ExistingAudioFiles -Dir $Dir -ExcludeDir $backup | ForEach-Object {
        [pscustomobject]@{ Path = $_.FullName; Extension = $_.Extension.ToLowerInvariant(); Codec = (Get-AudioCodec -Path $_.FullName) }
    })
    $flac = @($items | Where-Object { $_.Extension -eq '.m4a' -and $_.Codec -eq 'flac' })
    $aac = @($items | Where-Object { $_.Codec -in @('aac', 'mp3') })
    $unknown = @($items | Where-Object { $_.Codec -eq '' -or $_.Codec -notin @('flac', 'aac', 'mp3') })

    $remuxFailed = $false
    foreach ($file in $flac) {
        $result = Invoke-FlacRemux -Source $file.Path
        if ($result -eq 'converted') {
            if (Move-ToBackup -Path $file.Path -BackupDir $backup) {
                Say "  FLAC готов: $(Split-Path -Leaf ([IO.Path]::ChangeExtension($file.Path, '.flac')))" 'Green'
            }
        } elseif ($result -eq 'exists') {
            [void](Move-ToBackup -Path $file.Path -BackupDir $backup)
        } else {
            $remuxFailed = $true
            Say "  Не удалось извлечь FLAC из: $(Split-Path -Leaf $file.Path)" 'Yellow'
        }
    }

    $safeToSkip = ($unknown.Count -eq 0 -and -not $remuxFailed)
    if ($unknown.Count -gt 0) {
        Say "Не распознан кодек у $($unknown.Count) файлов .m4a/.mp3. Чтобы загрузчик не пропустил их по --skip-existing, повторную загрузку запущу без этого флага." 'Yellow'
    }
    if ($aac.Count -gt 0) {
        Say ''
        Say "Найдено $($aac.Count) старых файлов AAC/MP3 (.m4a или .mp3). Они могут блокировать перекачивание из-за --skip-existing." 'Yellow'
        Say "Если согласишься, все такие файлы в выбранной папке будут перемещены в '$backup'; оригиналы не удаляются." 'Yellow'
        if ($OfferAacQuarantine) {
            $answer = Read-Host 'Переместить их в резервную папку и затем пропускать остальные готовые треки? [y/N]'
            if ($answer -match '^(y|yes|д|да)$') {
                $allMoved = $true
                foreach ($file in $aac) {
                    if (-not (Move-ToBackup -Path $file.Path -BackupDir $backup)) { $allMoved = $false }
                }
                $safeToSkip = $safeToSkip -and $allMoved
                if ($allMoved) { Say 'Старые файлы перенесены в _old_m4a.' 'Green' }
            } else {
                $safeToSkip = $false
                Say 'Старые файлы не трогаю; отключаю --skip-existing, поэтому загрузчик повторно запросит все треки по выбранной ссылке.' 'Yellow'
            }
        } else {
            $safeToSkip = $false
        }
    }
    return [bool]$safeToSkip
}

function Invoke-FlacPostProcess {
    param([string]$Dir)
    if (-not ((Test-Ffmpeg) -and (Test-Ffprobe))) {
        Say 'Постобработка пропущена: нужен ffmpeg и ffprobe. Файлы с FLAC внутри могут остаться .m4a.' 'Yellow'
        return
    }
    $backup = Join-Path $Dir '_old_m4a'
    $items = @(Get-ExistingAudioFiles -Dir $Dir -ExcludeDir $backup | ForEach-Object {
        [pscustomobject]@{ Path = $_.FullName; Extension = $_.Extension.ToLowerInvariant(); Codec = (Get-AudioCodec -Path $_.FullName) }
    })
    $flac = @($items | Where-Object { $_.Extension -eq '.m4a' -and $_.Codec -eq 'flac' })
    $lossy = @($items | Where-Object { $_.Codec -in @('aac', 'mp3') })
    $unknown = @($items | Where-Object { $_.Codec -eq '' -or $_.Codec -notin @('flac', 'aac', 'mp3') })

    foreach ($file in $flac) {
        $result = Invoke-FlacRemux -Source $file.Path
        if ($result -eq 'converted') {
            if (Move-ToBackup -Path $file.Path -BackupDir $backup) {
                Say "  Извлечён настоящий FLAC: $(Split-Path -Leaf ([IO.Path]::ChangeExtension($file.Path, '.flac')))" 'Green'
            }
        } elseif ($result -eq 'exists') {
            [void](Move-ToBackup -Path $file.Path -BackupDir $backup)
        } else {
            Say "  Оставил .m4a: не удалось перепаковать $(Split-Path -Leaf $file.Path)" 'Yellow'
        }
    }
    if ($lossy.Count -gt 0) {
        Say "Остались файлы с кодеком не FLAC: $($lossy.Count). Например: $(Split-Path -Leaf $lossy[0].Path). Не переименовываю их в .flac и не выдаю за lossless." 'Yellow'
    }
    if ($unknown.Count -gt 0) {
        Say "У $($unknown.Count) файлов .m4a/.mp3 не удалось определить кодек; оставил их без изменений." 'Yellow'
    }
}

function Start-Download {
    param([string]$DownloadUrl, [string]$DownloadDir, [int]$DownloadQuality, [string]$Pattern)
    $token = Get-Token
    if (-not $token) {
        Say 'Токен Яндекс.Музыки не найден.' 'Red'
        Say "Сохрани его через меню 6 или в файл: $TokenFile" 'Yellow'
        return 1
    }
    if ([string]::IsNullOrWhiteSpace($DownloadUrl)) { Say 'Не задана ссылка.' 'Red'; return 1 }
    if ($DownloadQuality -notin @(0, 1, 2)) { Say 'Качество должно быть 0, 1 или 2.' 'Red'; return 1 }

    if ([string]::IsNullOrWhiteSpace($DownloadDir)) {
        $music = [Environment]::GetFolderPath('MyMusic')
        if ([string]::IsNullOrWhiteSpace($music)) { $music = Join-Path $env:USERPROFILE 'Music' }
        $DownloadDir = Join-Path $music 'lmao'
    }
    try { [void][IO.Directory]::CreateDirectory($DownloadDir) }
    catch { Say "Не удалось создать папку '$DownloadDir': $($_.Exception.Message)" 'Red'; return 1 }

    $skipExisting = $true
    if ($DownloadQuality -eq 2) {
        $probe = Get-TokenProbe -Token $token
        if ($probe.State -eq 'ok' -and -not $probe.HasPlus) {
            Say 'У этого аккаунта не обнаружен активный Яндекс Плюс. Lossless может быть недоступен.' 'Yellow'
            $answer = Read-Host 'Продолжить и проверить фактический кодек каждого трека? [y/N]'
            if ($answer -notmatch '^(y|yes|д|да)$') { return 1 }
        } elseif ($probe.State -eq 'invalid') {
            Say $probe.Text 'Red'
            return 1
        } elseif ($probe.State -eq 'unknown') {
            Say "Проверка токена не дала результата: $($probe.Text)" 'Yellow'
            $answer = Read-Host 'Всё равно попробовать загрузку? [y/N]'
            if ($answer -notmatch '^(y|yes|д|да)$') { return 1 }
        }

        if ((Test-Ffmpeg) -and (Test-Ffprobe)) {
            $skipExisting = Invoke-M4aMaintenance -Dir $DownloadDir -OfferAacQuarantine
        } else {
            Say 'ffmpeg/ffprobe не найдены. Без них не получится проверить кодек и извлечь настоящий .flac из .m4a.' 'Yellow'
            Say 'Если продолжить, lossless-файлы могут остаться .m4a, а выбранные треки будут перекачаны без --skip-existing.' 'Yellow'
            $answer = Read-Host 'Продолжить без гарантии итогового .flac и с повторной загрузкой выбранных треков? [y/N]'
            if ($answer -notmatch '^(y|yes|д|да)$') { Say 'Отменено. Установи ffmpeg/ffprobe в меню 5 → 2.' 'Yellow'; return 1 }
            $skipExisting = $false
        }
    }

    Say ''
    Say "Ссылка:   $DownloadUrl"
    Say "Папка:    $DownloadDir"
    Say "Качество: $(Format-Quality $DownloadQuality)"
    Say "Пропуск готовых треков: $skipExisting"
    Say ''

    $resourceArgs = @('-u', $DownloadUrl)
    try {
        $parsedUrl = [Uri]$DownloadUrl
        $legacyPlaylist = [regex]::Match(
            $parsedUrl.AbsolutePath,
            '^/users/([^/]+)/playlists/([^/]+)/?$',
            [System.Text.RegularExpressions.RegexOptions]::IgnoreCase
        )
        if ($legacyPlaylist.Success) {
            $owner = [Uri]::UnescapeDataString($legacyPlaylist.Groups[1].Value)
            $kind = [Uri]::UnescapeDataString($legacyPlaylist.Groups[2].Value)
            $resourceArgs = @('--playlist-id', "$owner/$kind")
            Say "Распознан плейлист старого формата: владелец=$owner, ID=$kind" 'Cyan'
        }
    } catch {
    }

    $ymdArgs = @('--quality', "$DownloadQuality", '--only-music', '--embed-cover', '--delay', '0', '--timeout', '15', '--tries', '1', '--retry-delay', '1', '--path-pattern', $Pattern)
    $ymdArgs += $resourceArgs
    $ymdArgs += @('--dir', $DownloadDir, '--token', $token)
    if ($skipExisting) { $ymdArgs += '--skip-existing' }

    $code = Invoke-Ymd -Arguments $ymdArgs
    if ($DownloadQuality -eq 2) { Invoke-FlacPostProcess -Dir $DownloadDir }

    Say ''
    if ($code -eq 0) {
        Say 'Загрузка завершена. Сверь строки выше: настоящий FLAC должен иметь расширение .flac; AAC останется .m4a.' 'Green'
    } else {
        Say "Загрузчик завершился с кодом $code. Постобработка уже скачанных файлов выполнена, если были доступны ffmpeg и ffprobe." 'Yellow'
    }
    return $code
}

function Install-Loader {
    $python = Get-PythonExe
    if (-not $python) {
        Say 'Python не найден. Установи Python 3.9+ с python.org или через winget, затем открой новое окно терминала.' 'Red'
        return
    }
    Say "Устанавливаю/обновляю llistochek/yandex-music-downloader через: $python" 'Cyan'
    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $python -m pip install --upgrade $LoaderZip
        $pipCode = $LASTEXITCODE
        if ($pipCode -ne 0) {
            Say 'Обычная установка не удалась; пробую установку в профиль пользователя.' 'Yellow'
            & $python -m pip install --user --upgrade $LoaderZip
            $pipCode = $LASTEXITCODE
        }
        if ($pipCode -eq 0 -and (Test-PythonModule -Module 'ymd')) {
            Say 'Загрузчик установлен, модуль ymd импортируется.' 'Green'
        } else {
            Say "Установка не подтверждена (код $pipCode). Проверь сообщения pip выше." 'Red'
        }
    } finally { $ErrorActionPreference = $oldPreference }
}

function Install-Ffmpeg {
    Update-SessionPath
    if ((Test-Ffmpeg) -and (Test-Ffprobe)) { Say 'ffmpeg и ffprobe уже доступны.' 'Green'; return }
    $winget = Get-Command winget -ErrorAction SilentlyContinue
    if (-not $winget) {
        Say 'winget не найден. Установи ffmpeg вручную из https://www.gyan.dev/ffmpeg/builds/ и добавь папку bin в PATH.' 'Red'
        return
    }
    Say 'Устанавливаю ffmpeg через winget...' 'Cyan'
    $oldPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        & $winget.Source install --id Gyan.FFmpeg --exact --accept-source-agreements --accept-package-agreements
        if ($LASTEXITCODE -eq 0) {
            Update-SessionPath
            if ((Test-Ffmpeg) -and (Test-Ffprobe)) { Say 'ffmpeg и ffprobe доступны в этом окне.' 'Green' }
            else { Say 'Установка завершилась. Если команды не видны, открой новый терминал и проверь PATH.' 'Yellow' }
        } else { Say "winget завершился с кодом $LASTEXITCODE." 'Red' }
    } finally { $ErrorActionPreference = $oldPreference }
}

function Show-Environment {
    Say ''
    Say '--- Окружение ---' 'Cyan'
    $python = Get-PythonExe
    if ($python) {
        $oldPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            $ver = & $python -c "import sys; print(sys.version.split()[0])" 2>$null
            Say "Python:    $ver ($python)" 'Green'
            if (Test-PythonModule -Module 'ymd') { Say 'Загрузчик: модуль ymd найден в этом Python.' 'Green' }
            else { Say 'Загрузчик: в текущем Python модуль ymd не найден.' 'Yellow' }
            if (Test-PythonModule -Module 'yandex_music') { Say 'yandex-music: пакет доступен.' 'Green' }
            else { Say 'yandex-music: пакет не найден.' 'Yellow' }
        } finally { $ErrorActionPreference = $oldPreference }
    } else { Say 'Python: не найден.' 'Red' }
    $exe = Find-YmdExe
    if ($exe) { Say "Команда загрузчика: $exe" 'Gray' }
    Say "ffmpeg:  $(if (Test-Ffmpeg) { (Get-Command ffmpeg).Source } else { 'не найден' })" $(if (Test-Ffmpeg) { 'Green' } else { 'Yellow' })
    Say "ffprobe: $(if (Test-Ffprobe) { (Get-Command ffprobe).Source } else { 'не найден' })" $(if (Test-Ffprobe) { 'Green' } else { 'Yellow' })
    $token = Get-Token
    if (-not $token) { Say "Токен: не найден (меню 6; файл $TokenFile)." 'Red' }
    else { Say "Токен: найден ($($token.Length) символов); проверяю API..." 'Gray'; $result = Get-TokenProbe -Token $token; Say $result.Text $(if ($result.State -eq 'ok') { 'Green' } else { 'Yellow' }) }
}

function Show-SettingsMenu {
    param($Settings)
    while ($true) {
        Say ''
        Say '--- Настройки ---' 'Cyan'
        Say "1. Ссылка:       $($Settings.Url)"
        Say "2. Папка:        $(if ($Settings.Dir) { $Settings.Dir } else { '(Музыка\lmao)' })"
        Say "3. Качество:     $(Format-Quality ([int]$Settings.Quality))"
        Say "4. Шаблон имени: $($Settings.PathPattern)"
        Say '0. Назад'
        switch ("$(Read-Host 'Выбор')".Trim()) {
            '1' { $v = (Read-Host 'Ссылка (Enter — без изменений)').Trim(); if ($v) { $Settings.Url = $v; Save-Settings $Settings; Say 'Сохранено.' 'Green' } }
            '2' { $v = (Read-Host 'Путь к папке (Enter — без изменений)').Trim().Trim('"'); if ($v) { $Settings.Dir = $v; Save-Settings $Settings; Say 'Сохранено.' 'Green' } }
            '3' { $v = (Read-Host 'Качество: 0 = AAC 64, 1 = AAC 192, 2 = lossless → FLAC').Trim(); if ($v -match '^[012]$') { $Settings.Quality = [int]$v; Save-Settings $Settings; Say 'Сохранено.' 'Green' } else { Say 'Введи 0, 1 или 2.' 'Yellow' } }
            '4' { Say 'Подстановки: #title, #album, #album-artist, #track-artist, #number, #number-padded, #year, #disc-number'; $v = (Read-Host 'Шаблон (Enter — без изменений)').Trim(); if ($v) { $Settings.PathPattern = $v; Save-Settings $Settings; Say 'Сохранено.' 'Green' } }
            '0' { return }
            default { Say 'Не понял выбор.' 'Yellow' }
        }
    }
}

function Show-TokenMenu {
    while ($true) {
        $token = Get-Token
        Say ''
        Say '--- Токен ---' 'Cyan'
        Say "Файл: $TokenFile"
        Say "Текущий: $(if ($token) { 'есть, ' + $token.Length + ' символов' } else { 'не найден' })"
        Say '1. Проверить текущий токен через API'
        Say '2. Ввести/заменить токен'
        Say '0. Назад'
        switch ("$(Read-Host 'Выбор')".Trim()) {
            '1' { if ($token) { $result = Get-TokenProbe -Token $token; Say $result.Text $(if ($result.State -eq 'ok') { 'Green' } else { 'Yellow' }) } else { Say 'Токена нет.' 'Red' } }
            '2' {
                $newToken = (Read-Host 'Токен Яндекс.Музыки').Trim()
                if (-not $newToken) { Say 'Пустой токен; ничего не изменил.' 'Yellow'; continue }
                Save-Token $newToken
                Say 'Токен сохранён в пользовательский файл.' 'Green'
                $result = Get-TokenProbe -Token $newToken
                Say $result.Text $(if ($result.State -eq 'ok') { 'Green' } else { 'Yellow' })
            }
            '0' { return }
            default { Say 'Не понял выбор.' 'Yellow' }
        }
    }
}

function Show-InstallMenu {
    while ($true) {
        Say ''
        Say '--- Установка / обновление ---' 'Cyan'
        Say '1. Установить/обновить загрузчик llistochek'
        Say '2. Установить ffmpeg + ffprobe через winget'
        Say '0. Назад'
        switch ("$(Read-Host 'Выбор')".Trim()) {
            '1' { Install-Loader }
            '2' { Install-Ffmpeg }
            '0' { return }
            default { Say 'Не понял выбор.' 'Yellow' }
        }
    }
}

function Show-MainMenu {
    param($Settings)
    while ($true) {
        Say ''
        Say '============================================================' 'DarkCyan'
        Say '  ym-dl — Яндекс.Музыка → lossless FLAC' 'Cyan'
        Say '  Версия: 2026-10-15-fix6 (журнал API + прогресс)' 'DarkGray'
        Say '============================================================' 'DarkCyan'
        Say "Ссылка:    $($Settings.Url)"
        Say "Папка:     $(if ($Settings.Dir) { $Settings.Dir } else { '(Музыка\lmao)' })"
        Say "Качество:  $(Format-Quality ([int]$Settings.Quality))"
        Say "Загрузчик: $(if ((Test-PythonModule -Module 'ymd') -or (Find-YmdExe)) { 'найден' } else { 'не найден' })"
        Say "ffmpeg:    $(if ((Test-Ffmpeg) -and (Test-Ffprobe)) { 'готов для извлечения FLAC' } else { 'не найден/неполный' })"
        Say "Токен:     $(if (Get-Token) { 'есть' } else { 'не найден' })"
        Say '------------------------------------------------------------' 'DarkCyan'
        Say '1. Скачать ссылку по умолчанию'
        Say '2. Скачать другую ссылку'
        Say '3. Настройки'
        Say '4. Проверить окружение'
        Say '5. Установить / обновить'
        Say '6. Токен'
        Say '0. Выход'
        switch ("$(Read-Host 'Выбор')".Trim()) {
            '1' { [void](Start-Download -DownloadUrl $Settings.Url -DownloadDir $Settings.Dir -DownloadQuality ([int]$Settings.Quality) -Pattern $Settings.PathPattern) }
            '2' { $u = (Read-Host 'Ссылка на исполнителя / альбом / трек / плейлист').Trim(); if ($u) { [void](Start-Download -DownloadUrl $u -DownloadDir $Settings.Dir -DownloadQuality ([int]$Settings.Quality) -Pattern $Settings.PathPattern) } else { Say 'Пустая ссылка.' 'Yellow' } }
            '3' { Show-SettingsMenu -Settings $Settings }
            '4' { Show-Environment }
            '5' { Show-InstallMenu }
            '6' { Show-TokenMenu }
            '0' { return }
            default { Say 'Не понял выбор.' 'Yellow' }
        }
    }
}

Update-SessionPath
$settings = Get-Settings
if ($PSBoundParameters.Count -gt 0) {
    if ($Url) { $settings.Url = $Url }
    if ($Dir) { $settings.Dir = $Dir }
    if ($Quality -ge 0) { $settings.Quality = $Quality }
    if ($PathPattern) { $settings.PathPattern = $PathPattern }
    exit (Start-Download -DownloadUrl $settings.Url -DownloadDir $settings.Dir -DownloadQuality ([int]$settings.Quality) -Pattern $settings.PathPattern)
}
Show-MainMenu -Settings $settings
exit 0                                                         