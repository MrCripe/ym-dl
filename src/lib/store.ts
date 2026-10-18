import { create } from "zustand";
import { api, subscribe, type AccountInfo, type JobProgress, type Settings, type SourceInfo, type TrackState } from "./api";

export type Page = "home" | "downloads" | "settings";
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

  init: () => Promise<void>;
  setPage: (page: Page) => void;
  setSettings: (settings: Settings) => Promise<void>;
  patchSettings: (values: Partial<Settings>) => void;
  saveSettingsDraft: () => Promise<void>;
  setAccount: (account: AccountInfo | null) => void;
  finishWizard: (account: AccountInfo) => Promise<void>;
  resolve: (url: string) => Promise<void>;
  loadLikes: () => Promise<void>;
  toggleTrack: (id: string) => void;
  setAllTracks: (value: boolean) => void;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  resetToken: () => Promise<void>;
  toast: (text: string, tone?: Toast["tone"], action?: string, page?: Page) => void;
  dismissToast: (id: number) => void;
};

let toastId = 0;

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

  init: async () => {
    // Registered before anything can fail: the download page must hear the job
    // even when reading the settings below goes wrong.
    await subscribe({
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
        get().toast(
          progress.error
            ? "Загрузка прервана — причина в логе"
            : progress.cancelled
              ? "Загрузка остановлена"
              : `Готово: ${progress.ok} скачано, ${progress.skipped} пропущено, ${progress.failed} с ошибкой`,
          progress.error || progress.failed > 0 ? "error" : progress.cancelled ? "info" : "success",
          "Загрузки",
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
        .then((account) => set({ account, needsToken: false }))
        .catch((error) => {
          // A rejected token must send the user back to the token screen
          // instead of leaving an empty, unusable shell.
          if (String((error as Error).message).includes("401")) {
            set({ needsToken: true, account: null });
          }
        });
    }

    // Quiet startup update check: only a real update produces a message.
    if (settings.checkUpdates) {
      api
        .environment()
        .then((env) => api.updates(env.appVersion, ""))
        .then((entries) => {
          const stale = entries.filter((entry) => entry.newer);
          if (stale.length) {
            get().toast(
              `Доступно обновление: ${stale.map((entry) => `${entry.name} ${entry.latest}`).join(", ")}`,
              "info",
              "Настройки",
              "settings",
            );
          }
        })
        .catch(() => undefined);
    }
  },

  setPage: (page) => set({ page }),

  setSettings: async (settings) => {
    const saved = await api.saveSettings(settings);
    set({ settings: saved, settingsDraft: saved, settingsDirty: false });
  },

  setAccount: (account) => set({ account }),

  patchSettings: (values) =>
    set((current) => {
      const base = current.settingsDraft ?? current.settings;
      if (!base) return {};
      return { settingsDraft: { ...base, ...values }, settingsDirty: true };
    }),

  saveSettingsDraft: async () => {
    const draft = get().settingsDraft ?? get().settings;
    if (!draft) return;
    const saved = await api.saveSettings(draft);
    set({ settings: saved, settingsDraft: saved, settingsDirty: false });
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
      const { settings } = get();
      if (settings && settings.lastUrl !== url) {
        api.saveSettings({ ...settings, lastUrl: url }).then(
          (saved) => set({ settings: saved }),
          () => undefined,
        );
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
    const { selected, source } = get();
    if (!source || selected.size === 0) return;
    const order = source.tracks.filter((track) => selected.has(track.id)).map((track) => track.id);
    set({ downloading: true, tracks: [], logs: [], page: "downloads", progress: null });
    try {
      await api.start(order);
    } catch (error) {
      set({ downloading: false });
      get().toast(String((error as Error).message), "error");
    }
  },

  stop: async () => {
    const accepted = await api.stop();
    if (!accepted) {
      // Nothing was running: saying "stopping" would be a lie.
      set({ downloading: false });
      get().toast("Загрузка уже не идёт", "info");
      return;
    }
    get().toast("Останавливаю после текущего трека…");
  },

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
    get().toast("Токен удалён — введите новый", "success");
  },

  toast: (text, tone = "info", action, page) => {
    const id = ++toastId;
    set((current) => ({ toasts: [...current.toasts, { id, text, tone, action, page }] }));
    window.setTimeout(() => get().dismissToast(id), 6000);
  },

  dismissToast: (id) =>
    set((current) => ({ toasts: current.toasts.filter((toast) => toast.id !== id) })),
}));
