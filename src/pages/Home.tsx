import { useEffect, useMemo, useState } from "react";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { api, type SearchResults } from "../lib/api";
import { linksIn, useApp } from "../lib/store";
import { useT } from "../lib/i18n";
import { Badge, Button, Checkbox, Icon, IconButton, TextField } from "../components/ui";

/**
 * Search results, laid out the way the reference client does it: the top tracks
 * in two columns, then a shelf for each other kind.
 *
 * The two-column split is what makes the tracks section feel like a list of
 * results rather than a table: eight rows become four lines.
 */
function SearchResultsPanel({
  results,
  onPick,
}: {
  results: SearchResults;
  onPick: (link: string) => void;
}) {
  const t = useT();
  const empty =
    results.tracks.length === 0 &&
    results.artists.length === 0 &&
    results.albums.length === 0 &&
    results.playlists.length === 0;

  if (empty) return <p className="lib-hint">{t("home.searchEmpty")}</p>;

  // Four rows per column, as in the reference: the rest is behind "all tracks".
  const columns = 2;
  const rows = Math.ceil(Math.min(results.tracks.length, 8) / columns);

  return (
    <div className="search-results animate-in">
      {results.tracks.length ? (
        <div className="search-section">
          <div className="search-section-head">
            <span className="md-title">{t("home.kind.track")}</span>
          </div>
          <div
            className="search-tracks"
            style={{ gridTemplateRows: `repeat(${rows}, auto)` }}
          >
            {results.tracks.slice(0, 8).map((track) => (
              <button
                key={track.id}
                type="button"
                className="search-track"
                onClick={() => onPick(track.link)}
              >
                {track.cover ? (
                  <img className="search-cover" src={track.cover} alt="" />
                ) : (
                  <span className="search-cover placeholder">
                    <Icon name="music_note" />
                  </span>
                )}
                <span className="lib-names">
                  <span className="lib-title">{track.title}</span>
                  <span className="lib-meta">{track.artist}</span>
                </span>
                <span className="lib-duration">{clock(track.durationMs)}</span>
              </button>
            ))}
          </div>
        </div>
      ) : null}

      {(["artists", "albums", "playlists"] as const).map((key) =>
        results[key].length ? (
          <div className="search-section" key={key}>
            <div className="search-section-head">
              <span className="md-title">{t(`home.shelf.${key}`)}</span>
            </div>
            <div className="search-shelf">
              {results[key].map((card) => (
                <button
                  key={card.link}
                  type="button"
                  className="shelf-card"
                  onClick={() => onPick(card.link)}
                >
                  {card.cover ? (
                    <img
                      className={`shelf-cover${card.round ? " round" : ""}`}
                      src={card.cover}
                      alt=""
                    />
                  ) : (
                    <span className={`shelf-cover placeholder${card.round ? " round" : ""}`}>
                      <Icon name={card.round ? "person" : "album"} size={26} />
                    </span>
                  )}
                  <span className="lib-title">{card.title}</span>
                  <span className="lib-meta">{card.subtitle}</span>
                </button>
              ))}
            </div>
          </div>
        ) : null,
      )}
    </div>
  );
}

/** mm:ss for a search row. */
function clock(ms: number): string {
  if (!ms) return "";
  const total = Math.round(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

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
  const queue = useApp((state) => state.queue);
  const enqueue = useApp((state) => state.enqueue);
  const dequeue = useApp((state) => state.dequeue);
  const clearQueue = useApp((state) => state.clearQueue);
  const moveQueueItem = useApp((state) => state.moveQueueItem);
  const addLinks = useApp((state) => state.addLinks);
  const toast = useApp((state) => state.toast);

  const [url, setUrl] = useState(settings?.lastUrl ?? "");
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchResults | null>(null);
  const [searching, setSearching] = useState(false);

  // Search as the user types, with a pause: the catalogue endpoint is cheap but
  // not free, and a request per keystroke is how a search box gets rate limited.
  useEffect(() => {
    const text = query.trim();
    // Nothing to search: the previous answer must not stay on screen under an
    // empty box, and the spinner has to go with it.
    if (text.length < 2) {
      setHits(null);
      setSearching(false);
      return;
    }
    // `alive` covers the request already in flight: the timer can only be
    // cleared, so without it a slow answer for "nir" overwrites the fresh one
    // for "nirvana" and stops the spinner while the second query still runs.
    let alive = true;
    const timer = window.setTimeout(async () => {
      setSearching(true);
      try {
        const found = await api.search(text);
        if (alive) setHits(found);
      } catch (error) {
        if (alive) toast(String((error as Error).message), "error");
      } finally {
        if (alive) setSearching(false);
      }
    }, 350);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [query, toast]);

  const available = useMemo(
    () => source?.tracks.filter((track) => track.available).length ?? 0,
    [source],
  );

  const queueTotal = useMemo(
    () => queue.reduce((sum, item) => sum + item.count, 0),
    [queue],
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

        {/* Search: without it the app only works if a link was copied from a
            browser first. */}
        <div className="search-box">
          <TextField
            value={query}
            icon="search"
            placeholder={t("home.searchPlaceholder")}
            onChange={setQuery}
            quiet
            trailing={
              query ? (
                <IconButton
                  icon="close"
                  label={t("home.clear")}
                  size="small"
                  onClick={() => setQuery("")}
                />
              ) : undefined
            }
          />
          {searching ? <p className="lib-hint">{t("home.searching")}</p> : null}
          {!searching && hits ? <SearchResultsPanel results={hits} onPick={(link) => {
            setUrl(link);
            setQuery("");
            setHits(null);
            void resolve(link);
          }} /> : null}
        </div>

        {/* What waits for "Начать": added but not downloading yet. */}
        {queue.length ? (
          <div className="queue-card animate-in">
            <div className="queue-head">
              <Icon name="queue_music" />
              <span className="md-title">{t("home.queue")}</span>
              <span className="queue-count">{queueTotal}</span>
              <span className="grow" />
              <Button variant="text" size="compact" icon="delete" onClick={clearQueue}>
                {t("home.queueClear")}
              </Button>
              {/* Without this the queue could only be started from a resolved
                  selection, which is not what "add first, start later" means. */}
              {downloading ? null : (
                <Button
                  variant="filled"
                  size="compact"
                  icon="play_arrow"
                  onClick={() => void start()}
                >
                  {t("home.start")}
                </Button>
              )}
            </div>
            <div className="queue-list">
              {queue.map((item, index) => (
                <div className="queue-item" key={item.id}>
                  <Icon name="music_note" />
                  <span className="queue-item-title truncate">{item.title}</span>
                  <span className="queue-item-count">
                    {t("home.trackCount", { count: item.count })}
                  </span>
                  <IconButton
                    icon="arrow_upward"
                    size="small"
                    label={t("home.queueUp")}
                    disabled={index === 0}
                    onClick={() => moveQueueItem(item.id, -1)}
                  />
                  <IconButton
                    icon="arrow_downward"
                    size="small"
                    label={t("home.queueDown")}
                    disabled={index === queue.length - 1}
                    onClick={() => moveQueueItem(item.id, 1)}
                  />
                  <IconButton
                    icon="close"
                    size="small"
                    label={t("home.queueRemove")}
                    onClick={() => dequeue(item.id)}
                  />
                </div>
              ))}
            </div>
            <p className="queue-hint">{t("home.queueHint")}</p>
          </div>
        ) : null}

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
                  const text = (await readText()) ?? "";
                  const links = linksIn(text);
                  // One link is something to look at before downloading; several
                  // are a batch, and filling the field with all of them would
                  // only leave the user to split them by hand.
                  if (links.length > 1) {
                    void addLinks(text);
                  } else if (links.length === 1) {
                    setUrl(links[0]);
                  } else if (text.trim()) {
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

          <p className="lib-hint">{t("home.linksHint")}</p>

          <div className="divider" />

          <div className="quick-row">
            <Button variant="outlined" icon="favorite" onClick={loadLikes} disabled={loading}>
              {t("home.likes")}
            </Button>
            <span className="md-body-sm muted">{t("home.likesHint")}</span>
          </div>

        </div>

        {/* What was resolved, before anything is downloaded. */}
        {source ? (
          <div className="preview-card animate-in">
            {source.cover ? (
              <img className="preview-cover" src={source.cover} alt="" />
            ) : (
              <span className="preview-cover placeholder">
                <Icon name="album" size={28} />
              </span>
            )}
            <div className="preview-text">
              <span className="md-title">
                {source.kind === "likes" ? t("home.likesOwn") : source.title}
              </span>
              <span className="lib-meta">{source.subtitle}</span>
              <span className="preview-tags">
                {source.year ? <Badge tone="neutral">{source.year}</Badge> : null}
                {source.quality ? (
                  <Badge tone={source.quality.startsWith("flac") ? "success" : "neutral"}>
                    {source.quality}
                  </Badge>
                ) : null}
                <Badge tone="neutral">{t("home.trackCount", { count: source.tracks.length })}</Badge>
              </span>
            </div>
          </div>
        ) : null}

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
                <>
                  <Button
                    variant="tonal"
                    icon="queue_music"
                    disabled={selected.size === 0}
                    onClick={enqueue}
                  >
                    {t("home.enqueue")}
                  </Button>
                  <Button
                    icon="play_arrow"
                    size="large"
                    disabled={selected.size === 0 && queue.length === 0}
                    onClick={start}
                  >
                    {queue.length ? t("home.startQueue") : t("home.start")}
                  </Button>
                </>
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
