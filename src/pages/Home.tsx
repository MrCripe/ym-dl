import { useMemo, useState } from "react";
import { useApp } from "../lib/store";
import { Badge, Button, Checkbox, Icon, IconButton, TextField } from "../components/ui";

export default function Home() {
  const source = useApp((state) => state.source);
  const selected = useApp((state) => state.selected);
  const loading = useApp((state) => state.loadingSource);
  const resolveProgress = useApp((state) => state.resolveProgress);
  const downloading = useApp((state) => state.downloading);
  const settings = useApp((state) => state.settings);
  const resolve = useApp((state) => state.resolve);
  const loadLikes = useApp((state) => state.loadLikes);
  const toggleTrack = useApp((state) => state.toggleTrack);
  const setAllTracks = useApp((state) => state.setAllTracks);
  const start = useApp((state) => state.start);
  const stop = useApp((state) => state.stop);

  const [url, setUrl] = useState(settings?.lastUrl ?? "");

  const available = useMemo(
    () => source?.tracks.filter((track) => track.available).length ?? 0,
    [source],
  );

  const targetPath = settings?.downloadDir?.trim()
    ? settings.downloadDir
    : "~/Music/lmao";

  return (
    <div className="page">
      <div className="page-narrow">
        <div className="page-header">
          <div>
            <h1>Главная</h1>
            <p>Вставьте ссылку или скачайте любимые треки</p>
          </div>
          {settings?.username ? (
            <Badge tone="secondary" icon="person">
              {settings.username}
            </Badge>
          ) : null}
        </div>

        <div className="source-card animate-in">
          <div className="source-input">
            <TextField
              label="Ссылка на трек, альбом, плейлист или исполнителя"
              value={url}
              icon="link"
              placeholder="https://music.yandex.ru/album/…"
              onChange={setUrl}
              onEnter={() => url.trim() && resolve(url.trim())}
              disabled={loading}
              trailing={
                url ? (
                  <IconButton
                    icon="close"
                    label="Очистить"
                    size="small"
                    onClick={() => setUrl("")}
                  />
                ) : undefined
              }
            />
            <IconButton
              icon="content_paste"
              label="Вставить из буфера"
              variant="tonal"
              onClick={async () => {
                try {
                  const text = await navigator.clipboard.readText();
                  if (text.trim()) setUrl(text.trim());
                } catch {
                  /* clipboard may be blocked; the field stays editable */
                }
              }}
            />
            <Button icon="search" onClick={() => url.trim() && resolve(url.trim())} disabled={loading || !url.trim()}>
              {loading ? "Загружаю…" : "Показать треки"}
            </Button>
          </div>

          <div className="divider" />

          <div className="quick-row">
            <Button
              variant="outlined"
              icon="favorite"
              onClick={loadLikes}
              disabled={loading}
            >
              Скачать любимые
            </Button>
            <span className="md-body-sm muted">
              {settings?.username
                ? `Любимые треки пользователя ${settings.username}`
                : "Любимые треки вашего аккаунта · имя пользователя задаётся в настройках"}
            </span>
          </div>

          {source ? (
            <div className="md-body-sm muted">
              {source.kind === "likes" ? "Мне нравится" : source.title} · {source.subtitle}
            </div>
          ) : null}
        </div>

        {loading ? (
          <div className="selection" style={{ marginTop: 20 }}>
            <div className="selection-head">
              <div className="titles">
                <h2>Загружаю список…</h2>
                <span className="sub">
                  {resolveProgress && resolveProgress.total > 0
                    ? `Альбом ${resolveProgress.done} из ${resolveProgress.total}`
                    : "Это может занять несколько секунд"}
                </span>
              </div>
            </div>
            {[0, 1, 2, 3, 4, 5].map((key) => (
              <div key={key} className="skeleton-row" />
            ))}
          </div>
        ) : source ? (
          <div className="selection animate-in">
            <div className="selection-head">
              <div className="titles">
                <h2 className="truncate">{source.title}</h2>
                <span className="sub">
                  {source.subtitle} · выбрано{" "}
                  <span key={selected.size} className="count-pop">
                    {selected.size}
                  </span>{" "}
                  из {available} доступных
                </span>
              </div>
              <Button variant="text" size="compact" icon="select_all" onClick={() => setAllTracks(true)}>
                Выбрать все
              </Button>
              <Button variant="text" size="compact" icon="deselect" onClick={() => setAllTracks(false)}>
                Снять все
              </Button>
            </div>

            <div className="selection-list">
              {source.tracks.map((track, index) => {
                const checked = selected.has(track.id);
                return (
                  <div
                    key={`${track.id}-${index}`}
                    className={`track-row ${track.available ? "" : "disabled"}`}
                    style={{ ["--i" as string]: Math.min(index, 24) }}
                    onClick={() => track.available && toggleTrack(track.id)}
                  >
                    <Checkbox
                      checked={checked}
                      disabled={!track.available}
                      label={track.title}
                      onChange={() => toggleTrack(track.id)}
                    />
                    <span className="index">{index + 1}</span>
                    <div className="grow stack" style={{ minWidth: 0 }}>
                      <span className="title truncate">{track.title}</span>
                      <span className="meta truncate">
                        {track.artist}
                        {track.album ? ` · ${track.album}` : ""}
                      </span>
                    </div>
                    {!track.available ? <Badge tone="error">недоступен</Badge> : null}
                    <span className="duration">{track.duration}</span>
                  </div>
                );
              })}
            </div>

            <div className="selection-foot">
              <Icon name="folder" />
              <span className="path grow">{targetPath}</span>
              {downloading ? (
                <Button variant="danger-tonal" icon="stop_circle" onClick={stop}>
                  Остановить
                </Button>
              ) : (
                <Button
                  icon="play_arrow"
                  size="large"
                  disabled={selected.size === 0}
                  onClick={start}
                >
                  Начать
                </Button>
              )}
            </div>
          </div>
        ) : (
          <div className="empty-state animate-in">
            <Icon name="queue_music" />
            <div className="md-title">Пока ничего не выбрано</div>
            <div className="md-body-sm">
              Вставьте ссылку на альбом, плейлист или трек — появится список с галочками.
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
