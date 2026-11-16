import { useMemo, useState } from "react";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { useApp } from "../lib/store";
import { useT } from "../lib/i18n";
import { Badge, Button, Checkbox, Icon, IconButton, TextField } from "../components/ui";

export default function Home() {
  const t = useT();
  const source = useApp((state) => state.source);
  const selected = useApp((state) => state.selected);
  const loading = useApp((state) => state.loadingSource);
  const resolveProgress = useApp((state) => state.resolveProgress);
  const downloading = useApp((state) => state.downloading);
  const settings = useApp((state) => state.settings);
  const account = useApp((state) => state.account);
  const resolve = useApp((state) => state.resolve);
  const loadLikes = useApp((state) => state.loadLikes);
  const toggleTrack = useApp((state) => state.toggleTrack);
  const setAllTracks = useApp((state) => state.setAllTracks);
  const start = useApp((state) => state.start);
  const stop = useApp((state) => state.stop);
  const toast = useApp((state) => state.toast);

  const [url, setUrl] = useState(settings?.lastUrl ?? "");

  const available = useMemo(
    () => source?.tracks.filter((track) => track.available).length ?? 0,
    [source],
  );

  const targetPath = settings?.downloadDir?.trim() ? settings.downloadDir : "~/Music";

  return (
    <div className="page">
      <div className="page-narrow">
        <div className="page-header">
          <div>
            <h1>{t("home.title")}</h1>
            <p>{t("home.subtitle")}</p>
          </div>
          {account?.name || account?.login ? (
            <Badge tone="secondary" icon="person">
              {account.name || account.login}
            </Badge>
          ) : null}
        </div>

        <div className="source-card animate-in">
          <div className="source-input">
            <TextField
              value={url}
              icon="link"
              placeholder="https://music.yandex.ru/album/…"
              onChange={setUrl}
              onEnter={() => url.trim() && resolve(url.trim())}
              disabled={loading}
              quiet
              trailing={
                url ? (
                  <IconButton
                    icon="close"
                    label={t("home.clear")}
                    size="small"
                    onClick={() => setUrl("")}
                  />
                ) : undefined
              }
            />
            <IconButton
              icon="content_paste"
              label={t("home.paste")}
              variant="tonal"
              onClick={async () => {
                try {
                  const text = await readText();
                  if (text?.trim()) {
                    setUrl(text.trim());
                  } else {
                    // Silence after a click reads as "nothing happened".
                    toast(t("home.pasteEmpty"), "info");
                  }
                } catch {
                  toast(t("home.pasteFailed"), "error");
                }
              }}
            />
            <Button
              icon="search"
              onClick={() => url.trim() && resolve(url.trim())}
              disabled={loading || !url.trim()}
            >
              {loading ? t("home.loading") : t("home.show")}
            </Button>
          </div>

          <div className="divider" />

          <div className="quick-row">
            <Button variant="outlined" icon="favorite" onClick={loadLikes} disabled={loading}>
              {t("home.likes")}
            </Button>
            <span className="md-body-sm muted">{t("home.likesHint")}</span>
          </div>

          {source ? (
            <div className="md-body-sm muted">
              {source.kind === "likes" ? t("home.likesOwn") : source.title} · {source.subtitle}
            </div>
          ) : null}
        </div>

        {loading ? (
          <div className="selection" style={{ marginTop: 20 }}>
            <div className="selection-head">
              <div className="titles">
                <h2>{t("home.loadingList")}</h2>
                <span className="sub">
                  {resolveProgress && resolveProgress.total > 0
                    ? `${resolveProgress.done} / ${resolveProgress.total}`
                    : t("home.loadingHint")}
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
                  {source.subtitle} · {t("home.selected")}{" "}
                  <span key={selected.size} className="count-pop">
                    {selected.size}
                  </span>{" "}
                  {t("home.of")} {available} {t("home.available")}
                </span>
              </div>
              <Button
                variant="text"
                size="compact"
                icon="select_all"
                onClick={() => setAllTracks(true)}
              >
                {t("home.selectAll")}
              </Button>
              <Button
                variant="text"
                size="compact"
                icon="deselect"
                onClick={() => setAllTracks(false)}
              >
                {t("home.selectNone")}
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
                    {!track.available ? (
                      <Badge tone="error">{t("home.unavailable")}</Badge>
                    ) : null}
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
                  {t("home.stop")}
                </Button>
              ) : (
                <Button
                  icon="play_arrow"
                  size="large"
                  disabled={selected.size === 0}
                  onClick={start}
                >
                  {t("home.start")}
                </Button>
              )}
            </div>
          </div>
        ) : (
          <div className="empty-state animate-in">
            <Icon name="queue_music" />
            <div className="md-title">{t("home.emptyTitle")}</div>
            <div className="md-body-sm">{t("home.emptyBody")}</div>
          </div>
        )}
      </div>
    </div>
  );
}
