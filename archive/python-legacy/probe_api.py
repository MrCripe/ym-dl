import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from yandex_music import Client
from yandex_music.utils.sign_request import DESKTOP_CLIENT, get_file_info_sign

token = Path(os.path.expanduser("~/.ym-dl-token")).read_text(encoding="utf-8").strip()
client = Client(token).init()
status = client.account_status()
print("uid:", status.account.uid, "plus:", bool(status.plus and status.plus.has_plus))

likes = client.users_likes_tracks()
print("likes:", len(likes.tracks) if likes and likes.tracks else 0)
if likes and likes.tracks:
    short = likes.tracks[0]
    print("first short:", type(short).__name__, short.id, short.album_id)

ids = [t.id for t in (likes.tracks or [])[:3]]
tracks = client.tracks(ids)
print("tracks fetched:", [(t.id, t.title, t.available) for t in tracks])

FILE_FORMAT_MAPPING = {"flac": "flac", "flac-mp4": "flac-mp4", "mp3": "mp3", "aac": "aac",
                       "he-aac": "he-aac", "aac-mp4": "aac-mp4", "he-aac-mp4": "he-aac-mp4"}

def probe(track):
    codecs = list(FILE_FORMAT_MAPPING)
    sign = get_file_info_sign(track.id, "lossless", codecs, "encraw")
    params = {"ts": sign.timestamp, "trackId": track.id, "quality": "lossless",
              "codecs": ",".join(codecs), "transports": "encraw", "sign": sign.value}
    req = client.request
    old = req.headers.get("X-Yandex-Music-Client")
    req.headers["X-Yandex-Music-Client"] = DESKTOP_CLIENT
    try:
        resp = req.get("https://api.music.yandex.net/get-file-info", params=params)
    finally:
        if old is None:
            req.headers.pop("X-Yandex-Music-Client", None)
        else:
            req.headers["X-Yandex-Music-Client"] = old
    e = resp["downloadInfo"]
    return {k: e.get(k) for k in ("codec", "quality", "bitrate", "key", "urls")} | {"urls": len(e.get("urls") or [])}

for track in tracks[:2]:
    print("probe", track.id, track.title, "->", probe(track))

pl = client.playlist("MrCriper10/3")
print("playlist:", pl.title if pl else None, "tracks:", pl.track_count if pl else None)
