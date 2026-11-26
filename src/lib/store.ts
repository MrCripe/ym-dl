import { create } from "zustand";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { api, subscribe, type AccountInfo, type Batch, type JobProgress, type LibraryTrack, type LibrarySort, type Settings, type SourceInfo, type TrackState } from "./api";
import { findUpdate, installUpdate, type AvailableUpdate } from "./update";
import { resolveLang, translate } from "./i18n";

export type Page = "home" | "library" | "downloads" | "settings";

/// One selection waiting in the download queue, with the label it shows.
export type QueuedBatch = { id: number; title: string; count: number; batch: Batch };
export type Toast = { id: number; text: string; tone: "info" | "error" | "success"; action?: string; page?: Page };

type LogLine = { level: string; message: string; time: number };

type State = {
  ready: boolean;
  initError: string;
  needsToken: boolean;
  settings: Settings | null;
  /** Unsaved edits. Lives in the store so leaving the page does not drop them. */
  settingsDraft: Settings | null;
  settingsDirty: boolean;
  account: AccountInfo | null;
  page: Page;
  source: SourceInfo | null;
  selected: Set<string>;
  loadingSource: boolean;
  resolveProgress: { done: number; total: number } | null;
  downloading: boolean;
  progress: JobProgress | null;
  tracks: TrackState[];
  logs: LogLine[];
  toasts: Toast[];
  /** The version the startup check found, so any screen can offer the update. */
  availableUpdate: AvailableUpdate | null;
  updateProgress: number | null;
  /** Portable unpack: the built-in updater cannot replace the binary in place. */
  portable: boolean;
  /** The library track currently in the player, if any. */
  playing: LibraryTrack | null;
  /// Kept in the store so the player bar survives navigation without restarting.
  paused: boolean;
  /// Bumped on every "play this" click. Clicking the row that is already in the
  /// bar leaves `playing` unchanged, so the bar needs a second signal to restart.
  playNonce: number;
  /// How the library list is ordered and narrowed. Kept here because the shell
  /// remounts a page on every switch to replay its entrance animation.
  librarySort: LibrarySort;
  libraryDescending: boolean;
  libraryFilter: string;
  setLibrarySort: (sort: LibrarySort) => void;
  setLibraryDescending: (descending: boolean) => void;
  setLibraryFilter: (filter: string) => void;
  /// Selections added with "В очередь": remembered but not started until the
  /// user presses "Начать".
  queue: QueuedBatch[];
  enqueue: () => void;
  dequeue: (id: number) => void;
  clearQueue: () => void;
  setPlaying: (track: LibraryTrack | null) => void;
  setPaused: (paused: boolean) => void;

  init: () => Promise<void>;
  setPage: (page: Page) => void;
  patchSettings: (values: Partial<Settings>) => void;
  saveSettingsDraft: () => Promise<void>;
  flushSettings: () => Promise<void>;
  finishWizard: (account: AccountInfo) => Promise<void>;
  resolve: (url: string) => Promise<void>;
  loadLikes: () => Promise<void>;
  toggleTrack: (id: string) => void;
  setAllTracks: (value: boolean) => void;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  openFolder: () => Promise<void>;
  installAvailableUpdate: () => Promise<void>;
  dismissUpdate: () => void;
  resetToken: () => Promise<void>;
  toast: (text: string, tone?: Toast["tone"], action?: string, page?: Page) => void;
  dismissToast: (id: number) => void;
};

let toastId = 0;
let saveTimer = 0;
/// The live event subscription, so `init` can replace it instead of stacking.
let unsubscribe: UnlistenFn | null = null;
/// Order of settings writes: two overlapping saves must not land out of order.
let saveSequence = 0;

/**
 * Translation for code that runs outside a component.
 *
 * The store raises toasts from event handlers and timers, where a hook cannot be
 * used; it reads the current language from its own state instead.
 */
function tr(lang: string | undefined, key: string, vars?: Record<string, string | number>) {
  return translate(resolveLang(lang), key, vars);
}

export const useApp = create<State>((set, get) => ({
  ready: false,
  initError: "",
  needsToken: true,
  settings: null,
  settingsDraft: null,
  settingsDirty: false,
  account: null,
  page: "home",
  source: null,
  selected: new Set<string>(),
  loadingSource: false,
  resolveProgress: null,
  downloading: false,
  progress: null,
  tracks: [],
  logs: [],
  toasts: [],
  availableUpdate: null,
  updateProgress: null,
  portable: false,
  playing: null,
  paused: false,
  playNonce: 0,
  queue: [],
  librarySort: "added",
  libraryDescending: true,
  libraryFilter: "",

  init: async () => {
    // A second init (the retry button, or StrictMode running the effect twice)
    // must not leave the previous set of listeners behind: every log line and
    // every toast would then arrive twice.
    unsubscribe?.();
    // Registered before anything can fail: the download page must hear the job
    // even when reading the settings below goes wrong.
    unsubscribe = await subscribe({
      onTrack: (state) =>
        set((current) => {
          const tracks = [...current.tracks];
          tracks[state.index] = state;
          return { tracks };
        }),
      onProgress: (progress) => set({ progress, downloading: progress.running }),
      onResolve: (resolveProgress) => set({ resolveProgress }),
      onLog: (log) =>
        set((current) => ({
          logs: [...current.logs.slice(-400), { ...log, time: Date.now() }],
        })),
      onFinished: (progress) => {
        set({ progress, downloading: false });
        const lang = get().settings?.language;
        get().toast(
          progress.error
            ? tr(lang, "toast.interrupted")
            : progress.cancelled
              ? tr(lang, "toast.stopped")
              : tr(lang, "toast.finished", {
                  ok: progress.ok,
                  skipped: progress.skipped,
                  failed: progress.failed,
                }),
          progress.error || progress.failed > 0
            ? "error"
            : progress.cancelled
              ? "info"
              : "success",
          tr(lang, "nav.downloads"),
          "downloads",
        );
      },
    });

    let settings: Settings;
    let hasToken: boolean;
    try {
      [settings, hasToken] = await Promise.all([api.settings(), api.hasToken()]);
    } catch (error) {
      // Without this the shell stays on "Запускаю ym-dl…" forever.
      set({ ready: true, needsToken: false, initError: String((error as Error).message) });
      return;
    }
    set({ settings, settingsDraft: settings, needsToken: !hasToken, ready: true });

    if (hasToken) {
      api
        .probeLossless()
        .then(async (account) => {
          set({ account, needsToken: false });
          // The backend fills the likes username from the account, so pull the
          // settings again or the field stays empty until a restart. A draft the
          // user is already editing wins: replacing it would erase their input.
          const fresh = await api.settings().catch(() => null);
          if (fresh) {
            set((current) =>
              current.settingsDirty
                ? { settings: fresh }
                : { settings: fresh, settingsDraft: fresh },
            );
          }
        })
        .catch((error) => {
          // A rejected token must send the user back to the token screen
          // instead of leaving an empty, unusable shell.
          if (String((error as Error).message).includes("401")) {
            set({ needsToken: true, account: null });
          }
        });
    }

    // Startup update check: a found update becomes a banner with its own button,
    // so the user does not have to hunt for "Проверить" in the settings to install
    // what the app already knows about.
    // Whether this is a portable unpack matters for the update banner regardless
    // of the auto-check setting.
    api
      .environment()
      .then((env) => set({ portable: env.portable }))
      .catch(() => undefined);

    if (settings.checkUpdates) {
      findUpdate()
        .then((update) => {
          if (update) set({ availableUpdate: update });
        })
        .catch(() => undefined);
    }
  },

  setPage: (page) => set({ page }),

  setPlaying: (track) => {
    // A new track always starts from the top, playing.
    set((current) => ({ playing: track, paused: false, playNonce: current.playNonce + 1 }));
  },

  setPaused: (paused) => set({ paused }),

  setLibrarySort: (librarySort) => set({ librarySort }),
  setLibraryDescending: (libraryDescending) => set({ libraryDescending }),
  setLibraryFilter: (libraryFilter) => set({ libraryFilter }),

  enqueue: () => {
    const { source, selected } = get();
    if (!source || selected.size === 0) {
      get().toast(tr(get().settings?.language, "home.enqueueEmpty"), "info");
      return;
    }
    const trackIds = source.tracks
      .filter((track) => selected.has(track.id))
      .map((track) => track.id);
    const title = source.kind === "likes" ? tr(get().settings?.language, "home.likesOwn") : source.title;
    set((current) => ({
      queue: [
        ...current.queue,
        { id: Date.now() + current.queue.length, title, count: trackIds.length, batch: { trackIds, sourceKind: source.kind } },
      ],
    }));
    get().toast(tr(get().settings?.language, "home.enqueued", { title, count: trackIds.length }), "success");
  },

  dequeue: (id) => set((current) => ({ queue: current.queue.filter((item) => item.id !== id) })),

  clearQueue: () => set({ queue: [] }),

  patchSettings: (values) => {
    set((current) => {
      const base = current.settingsDraft ?? current.settings;
      if (!base) return {};
      return { settingsDraft: { ...base, ...values }, settingsDirty: true };
    });
    // Autosave: one write shortly after the last change, so dragging a slider
    // does not hammer the disk but nothing is ever left unsaved.
    window.clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => {
      void get()
        .saveSettingsDraft()
        .catch((error) => get().toast(String((error as Error).message), "error"));
    }, 400);
  },

  saveSettingsDraft: async () => {
    const draft = get().settingsDraft ?? get().settings;
    if (!draft) return;
    const mine = ++saveSequence;
    const saved = await api.saveSettings(draft);
    // An autosave and a flush can overlap; only the newest write may decide what
    // is stored, otherwise the older answer rolls the settings back.
    if (mine !== saveSequence) return;
    // Keep edits made while the request was in flight: replacing the draft with
    // the saved copy would silently discard whatever the user typed meanwhile.
    set((current) =>
      current.settingsDraft === draft
        ? { settings: saved, settingsDraft: saved, settingsDirty: false }
        : { settings: saved },
    );
  },

  flushSettings: async () => {
    window.clearTimeout(saveTimer);
    if (!get().settingsDirty) return;
    await get().saveSettingsDraft();
  },

  finishWizard: async (account) => {
    set({ account, needsToken: false });
    const settings = await api.settings();
    set({ settings, settingsDraft: settings });
  },

  resolve: async (url) => {
    set({ loadingSource: true, resolveProgress: null });
    try {
      const source = await api.resolve(url);
      set({
        source,
        selected: new Set(source.tracks.filter((track) => track.available).map((track) => track.id)),
        loadingSource: false,
        resolveProgress: null,
      });
      // Remember the link so the next launch starts where the user left off.
      // It goes through the draft: writing it straight to `settings` would be
      // undone by the next autosave, which persists the draft.
      const base = get().settingsDraft ?? get().settings;
      if (base && base.lastUrl !== url) {
        set({ settingsDraft: { ...base, lastUrl: url }, settingsDirty: true });
        void get()
          .flushSettings()
          .catch(() => undefined);
      }
    } catch (error) {
      set({ loadingSource: false, resolveProgress: null });
      get().toast(String((error as Error).message), "error");
    }
  },

  loadLikes: async () => {
    set({ loadingSource: true });
    try {
      const source = await api.likes();
      set({
        source,
        selected: new Set(source.tracks.filter((track) => track.available).map((track) => track.id)),
        loadingSource: false,
      });
    } catch (error) {
      set({ loadingSource: false });
      get().toast(String((error as Error).message), "error");
    }
  },

  toggleTrack: (id) =>
    set((current) => {
      const selected = new Set(current.selected);
      if (selected.has(id)) {
        selected.delete(id);
      } else {
        selected.add(id);
      }
      return { selected };
    }),

  setAllTracks: (value) =>
    set((current) => {
      if (!current.source) return {};
      return {
        selected: value
          ? new Set(current.source.tracks.filter((track) => track.available).map((track) => track.id))
          : new Set<string>(),
      };
    }),

  start: async () => {
    const { selected, source, queue } = get();
    // The queue runs first, then whatever is selected right now. A track can sit
    // in both — it was added to the queue and left selected — so the second copy
    // is dropped instead of downloading the same file twice.
    const batches: Batch[] = queue.map((item) => item.batch);
    if (source && selected.size > 0) {
      batches.push({
        trackIds: source.tracks.filter((track) => selected.has(track.id)).map((track) => track.id),
        sourceKind: source.kind,
      });
    }
    const seen = new Set<string>();
    const unique: Batch[] = [];
    for (const batch of batches) {
      const trackIds = batch.trackIds.filter((id) => !seen.has(id));
      trackIds.forEach((id) => seen.add(id));
      if (trackIds.length > 0) unique.push({ sourceKind: batch.sourceKind, trackIds });
    }
    if (unique.length === 0) return;
    // The queue is emptied only once the backend accepted the job: a failed
    // start must not throw away selections the user spent time collecting.
    set({ downloading: true, tracks: [], logs: [], page: "downloads", progress: null });
    try {
      // The source kind picks the path template: likes, an album and a loose
      // track each has its own idea of where the file belongs.
      await api.start(unique);
      set({ queue: [] });
    } catch (error) {
      set({ downloading: false });
      get().toast(String((error as Error).message), "error");
    }
  },

  stop: async () => {
    let accepted: boolean;
    try {
      accepted = await api.stop();
    } catch (error) {
      // A rejected stop is not a stopped job: say so instead of pretending.
      get().toast(String((error as Error).message), "error");
      return;
    }
    if (!accepted) {
      // Nothing was running: saying "stopping" would be a lie.
      set({ downloading: false });
      get().toast(tr(get().settings?.language, "toast.alreadyStopped"), "info");
      return;
    }
    get().toast(tr(get().settings?.language, "toast.stopping"));
  },

  openFolder: async () => {
    await api.openDownloadDir();
  },

  installAvailableUpdate: async () => {
    // A second click must not start a second download of the same installer: the
    // button is disabled only after a re-render, so two clicks in one frame would
    // otherwise both reach downloadAndInstall.
    if (get().updateProgress !== null) return;
    const lang = get().settings?.language;
    set({ updateProgress: 0 });
    try {
      await installUpdate(({ downloaded, total }) => {
        set({
          updateProgress:
            total && total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0,
        });
      });
      // installUpdate relaunches the app; reaching here means it did not.
      get().toast(tr(lang, "set.updateInstalled"), "success");
    } catch (error) {
      set({ updateProgress: null });
      get().toast(tr(lang, "set.updateError", { error: (error as Error).message }), "error");
    }
  },

  dismissUpdate: () => set({ availableUpdate: null }),

  resetToken: async () => {
    await api.clearToken();
    // Reset the in-memory state as well: reloading the webview used to be the
    // only trigger, and it left the old account and page behind when it failed.
    set({
      needsToken: true,
      account: null,
      source: null,
      selected: new Set<string>(),
      page: "home",
      downloading: false,
      progress: null,
      tracks: [],
      logs: [],
    });
    get().toast(tr(get().settings?.language, "toast.tokenCleared"), "success");
  },

  toast: (text, tone = "info", action, page) => {
    const id = ++toastId;
    set((current) => ({ toasts: [...current.toasts, { id, text, tone, action, page }] }));
    window.setTimeout(() => get().dismissToast(id), 6000);
  },

  dismissToast: (id) =>
    set((current) => ({ toasts: current.toasts.filter((toast) => toast.id !== id) })),
}));
