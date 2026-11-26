import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  api,
  type LibrarySort,
  type LibraryTrack,
  type TagPatch,
  type UpgradeOutcome,
} from "../lib/api";
import { useApp } from "../lib/store";
import { useLang, useT } from "../lib/i18n";
import { Badge, Button, Icon, IconButton, Segmented, TextField } from "../components/ui";

/**
 * Covers are read from the file on demand and kept for the session: a list of a
 * few hundred rows would otherwise re-read the same tag block on every scroll.
 */
const coverCache = new Map<number, string | null>();

/** Formats a duration for the list. */
function clock(ms: number): string {
  if (!ms) return "—";
  const total = Math.round(ms / 1000);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

/**
 * The library: everything already on disk, with the actions that used to need a
 * file manager and a separate tag editor.
 */
export default function Library() {
  const t = useT();
  const lang = useLang();
  const toast = useApp((state) => state.toast);

  const [tracks, setTracks] = useState<LibraryTrack[]>([]);
  // The view choice lives in the store: the shell remounts a page on every
  // switch to replay its entrance, and a sort or a search word that resets on
  // every visit is the same as not having one.
  const sort = useApp((state) => state.librarySort);
  const descending = useApp((state) => state.libraryDescending);
  const filter = useApp((state) => state.libraryFilter);
  const setSort = useApp((state) => state.setLibrarySort);
  const setDescending = useApp((state) => state.setLibraryDescending);
  const setFilter = useApp((state) => state.setLibraryFilter);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<LibraryTrack | null>(null);
  const [busyId, setBusyId] = useState<number | null>(null);
  // Only the newest answer may touch the list: two queries in flight (a fast
  // one for "ba", a slow one for "ban") would otherwise finish out of order and
  // leave the rows of the older filter under the newer word.
  const sequence = useRef(0);

  const load = useCallback(async () => {
    const mine = ++sequence.current;
    setLoading(true);
    try {
      const list = await api.libraryList(sort, descending, filter);
      if (mine === sequence.current) setTracks(list);
    } catch (error) {
      if (mine === sequence.current) toast(String((error as Error).message), "error");
    } finally {
      if (mine === sequence.current) setLoading(false);
    }
  }, [sort, descending, filter, toast]);

  // A short delay so typing in the search box does not query on every keystroke.
  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 180);
    return () => window.clearTimeout(timer);
  }, [load]);

  const totalBytes = useMemo(
    () => tracks.reduce((sum, track) => sum + track.sizeBytes, 0),
    [tracks],
  );

  const scan = async () => {
    try {
      const report = await api.libraryScan();
      toast(
        t("lib.scanned", {
          scanned: report.scanned,
          added: report.added,
          updated: report.updated,
          removed: report.removed,
        }),
        report.skipped ? "info" : "success",
      );
      await load();
    } catch (error) {
      toast(String((error as Error).message), "error");
    }
  };

  const remove = async (track: LibraryTrack) => {
    try {
      await api.libraryDelete(track.id);
      coverCache.delete(track.id);
      toast(t("lib.deleted", { title: track.title }), "success");
      setTracks((current) => current.filter((entry) => entry.id !== track.id));
    } catch (error) {
      toast(String((error as Error).message), "error");
    }
  };

  const upgrade = async (track: LibraryTrack) => {
    setBusyId(track.id);
    try {
      const outcome: UpgradeOutcome = await api.libraryUpgrade(track.id);
      if (outcome.status === "upgraded") {
        toast(t("lib.upgraded", { title: track.title, detail: outcome.detail }), "success");
      } else {
        toast(t("lib.alreadyBest", { detail: outcome.detail }), "info");
      }
      await load();
    } catch (error) {
      toast(String((error as Error).message), "error");
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="page">
      <div className="page-narrow">
        <div className="page-header">
          <div>
            <h1>{t("lib.title")}</h1>
            <p>
              {tracks.length
                ? t("lib.subtitle", {
                    count: tracks.length,
                    size: formatSize(totalBytes, lang),
                  })
                : t("lib.subtitleEmpty")}
            </p>
          </div>
          <Button variant="tonal" size="compact" icon="search" onClick={() => void scan()}>
            {t("lib.scan")}
          </Button>
        </div>

        <div className="library-toolbar">
          <TextField
            value={filter}
            icon="search"
            placeholder={t("lib.filterPlaceholder")}
            onChange={setFilter}
            quiet
          />
          <Segmented
            value={sort}
            onChange={(value) => setSort(value as LibrarySort)}
            options={[
              { value: "added", label: t("lib.sortAdded") },
              { value: "artist", label: t("lib.sortArtist") },
              { value: "album", label: t("lib.sortAlbum") },
              { value: "year", label: t("lib.sortYear") },
              { value: "title", label: t("lib.sortTitle") },
            ]}
          />
          <IconButton
            icon={descending ? "arrow_downward" : "arrow_upward"}
            label={descending ? t("lib.desc") : t("lib.asc")}
            variant="tonal"
            onClick={() => setDescending(!descending)}
          />
        </div>

        {loading && !tracks.length ? (
          <p className="lib-hint">{t("lib.loading")}</p>
        ) : tracks.length === 0 ? (
          <div className="empty-state">
            <Icon name="library_music" size={30} />
            <div className="md-title">{t("lib.emptyTitle")}</div>
            <p className="md-body-sm muted">{t("lib.emptyBody")}</p>
            <Button variant="tonal" size="compact" icon="search" onClick={() => void scan()}>
              {t("lib.scan")}
            </Button>
          </div>
        ) : (
          <div className="library-list">
            {tracks.map((track) => (
              <LibraryRow
                key={track.id}
                track={track}
                busy={busyId === track.id}
                onPlay={() => useApp.getState().setPlaying(track)}
                onEdit={() => setEditing(track)}
                onUpgrade={() => void upgrade(track)}
                onDelete={() => void remove(track)}
              />
            ))}
          </div>
        )}
      </div>

      {editing ? (
        <TagEditor
          track={editing}
          onClose={() => setEditing(null)}
          onSaved={async () => {
            setEditing(null);
            await load();
          }}
        />
      ) : null}

    </div>
  );
}

/** The track's cover; clicking it replaces the picture in the file. */
function CoverButton({ track }: { track: LibraryTrack }) {
  const t = useT();
  const toast = useApp((state) => state.toast);
  const [cover, setCover] = useState<string | null | undefined>(coverCache.get(track.id));
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (cover !== undefined) return;
    let alive = true;
    api
      .libraryCover(track.id)
      .then((value) => {
        coverCache.set(track.id, value);
        if (alive) setCover(value);
      })
      .catch(() => {
        coverCache.set(track.id, null);
        if (alive) setCover(null);
      });
    return () => {
      alive = false;
    };
  }, [track.id, cover]);

  const pick = async () => {
    setBusy(true);
    try {
      const picked = await openDialog({
        multiple: false,
        filters: [{ name: t("lib.coverImage"), extensions: ["jpg", "jpeg", "png", "webp"] }],
      });
      if (typeof picked !== "string") return;
      await api.librarySetCover(track.id, picked);
      const fresh = await api.libraryCover(track.id);
      coverCache.set(track.id, fresh);
      setCover(fresh);
      toast(t("lib.coverSaved"), "success");
    } catch (error) {
      toast(String((error as Error).message), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      type="button"
      className="lib-cover"
      onClick={() => void pick()}
      disabled={busy}
      aria-label={t("lib.changeCover")}
      title={t("lib.changeCover")}
    >
      {cover ? (
        // A 400px cover in a 44px slot: decoding off the main thread and skipping
        // off-screen rows is what keeps a long list from stuttering while it
        // scrolls.
        <img src={cover} alt="" decoding="async" loading="lazy" />
      ) : (
        <Icon name="album" size={18} />
      )}
      <span className="lib-cover-edit">
        <Icon name="edit" size={14} />
      </span>
    </button>
  );
}

function LibraryRow({
  track,
  busy,
  onPlay,
  onEdit,
  onUpgrade,
  onDelete,
}: {
  track: LibraryTrack;
  busy: boolean;
  onPlay: () => void;
  onEdit: () => void;
  onUpgrade: () => void;
  onDelete: () => void;
}) {
  const t = useT();
  const lang = useLang();
  const [confirming, setConfirming] = useState(false);

  return (
    <div className="library-row">
      <button type="button" className="lib-play" onClick={onPlay} aria-label={t("lib.play")}>
        <Icon name="play_arrow" filled />
      </button>
      <CoverButton track={track} />

      <div className="lib-names">
        <span className="lib-title">{track.title || t("lib.untitled")}</span>
        <span className="lib-meta">
          {[track.artist, track.album, track.year ? String(track.year) : ""]
            .filter(Boolean)
            .join(" · ")}
        </span>
      </div>

      <Badge tone="neutral">
        {track.codec}
        {track.bitrate ? ` ${track.bitrate}` : ""}
      </Badge>
      <span className="lib-duration">{clock(track.durationMs)}</span>
      <span className="lib-size">{formatSize(track.sizeBytes, lang)}</span>

      <div className="lib-actions">
        <IconButton
          icon="system_update"
          label={t("lib.upgrade")}
          size="small"
          disabled={busy}
          onClick={onUpgrade}
        />
        <IconButton icon="edit" label={t("lib.edit")} size="small" onClick={onEdit} />
        {confirming ? (
          <>
            <Button variant="danger-tonal" size="compact" onClick={onDelete}>
              {t("lib.deleteConfirm")}
            </Button>
            <Button variant="text" size="compact" onClick={() => setConfirming(false)}>
              {t("lib.cancel")}
            </Button>
          </>
        ) : (
          <IconButton
            icon="delete"
            label={t("lib.delete")}
            size="small"
            onClick={() => setConfirming(true)}
          />
        )}
      </div>
    </div>
  );
}

/** Edits tags in the file and in the index, then closes. */
function TagEditor({
  track,
  onClose,
  onSaved,
}: {
  track: LibraryTrack;
  onClose: () => void;
  onSaved: () => void;
}) {
  const t = useT();
  const toast = useApp((state) => state.toast);
  const [saving, setSaving] = useState(false);
  const [draft, setDraft] = useState({
    title: track.title,
    artist: track.artist,
    albumArtist: track.albumArtist,
    album: track.album,
    genre: track.genre,
    year: track.year ? String(track.year) : "",
    trackNumber: track.trackNumber ? String(track.trackNumber) : "",
  });

  const save = async () => {
    setSaving(true);
    const patch: TagPatch = {
      title: draft.title,
      artist: draft.artist,
      albumArtist: draft.albumArtist,
      album: draft.album,
      genre: draft.genre,
    };
    // Empty numbers must not become 0: that would write a wrong track number.
    const year = Number.parseInt(draft.year, 10);
    if (Number.isFinite(year)) patch.year = year;
    const number = Number.parseInt(draft.trackNumber, 10);
    if (Number.isFinite(number)) patch.trackNumber = number;
    try {
      await api.libraryUpdate(track.id, patch);
      toast(t("lib.saved"), "success");
      onSaved();
    } catch (error) {
      toast(String((error as Error).message), "error");
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-scrim" role="dialog" aria-modal="true" aria-label={t("lib.edit")}>
      <div className="modal-card">
        <div className="modal-head">
          <span className="md-title">{t("lib.edit")}</span>
          <IconButton icon="close" label={t("lib.cancel")} size="small" onClick={onClose} />
        </div>

        <div className="modal-body">
          <TextField
            label={t("lib.fieldTitle")}
            value={draft.title}
            onChange={(value) => setDraft({ ...draft, title: value })}
          />
          <TextField
            label={t("lib.fieldArtist")}
            value={draft.artist}
            onChange={(value) => setDraft({ ...draft, artist: value })}
          />
          <TextField
            label={t("lib.fieldAlbumArtist")}
            value={draft.albumArtist}
            onChange={(value) => setDraft({ ...draft, albumArtist: value })}
          />
          <TextField
            label={t("lib.fieldAlbum")}
            value={draft.album}
            onChange={(value) => setDraft({ ...draft, album: value })}
          />
          <TextField
            label={t("lib.fieldGenre")}
            value={draft.genre}
            onChange={(value) => setDraft({ ...draft, genre: value })}
          />
          <div className="modal-grid">
            <TextField
              label={t("lib.fieldYear")}
              value={draft.year}
              onChange={(value) => setDraft({ ...draft, year: value })}
            />
            <TextField
              label={t("lib.fieldNumber")}
              value={draft.trackNumber}
              onChange={(value) => setDraft({ ...draft, trackNumber: value })}
            />
          </div>
          <p className="lib-hint">{t("lib.editNote")}</p>
        </div>

        <div className="modal-foot">
          <Button variant="text" onClick={onClose}>
            {t("lib.cancel")}
          </Button>
          <Button variant="filled" disabled={saving} onClick={() => void save()}>
            {saving ? t("lib.saving") : t("lib.save")}
          </Button>
        </div>
      </div>
    </div>
  );
}

/** Bytes as a person reads them. */
function formatSize(bytes: number, lang: string): string {
  if (!bytes) return "0";
  const units = lang === "en" ? ["B", "KB", "MB", "GB"] : ["Б", "КБ", "МБ", "ГБ"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}
