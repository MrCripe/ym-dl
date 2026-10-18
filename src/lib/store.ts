import { create } from "zustand";
import { api, subscribe, type AccountInfo, type JobProgress, type Settings, type SourceInfo, type TrackState } from "./api";

export type Page = "home" | "downloads" | "settings";
export type Toast = { id: number; text: string; tone: "info" | "error" | "success"; action?: string };

type LogLine = { level: string; message: string; time: number };

type State = {
  ready: boolean;
  needsToken: boolean;
  settings: Settings | null;
  account: AccountInfo | null;
  page: Page;
  source: SourceInfo | null;
  selected: Set<string>;
  loadingSource: boolean;
  downloading: boolean;
  progress: JobProgress | null;
  tracks: TrackState[];
  logs: LogLine[];
  toasts: Toast[];

  init: () => Promise<void>;
  setPage: (page: Page) => void;
  setSettings: (settings: Settings) => Promise<void>;
  setAccount: (account: AccountInfo | null) => void;
  finishWizard: (account: AccountInfo) => Promise<void>;
  resolve: (url: string) => Promise<void>;
  loadLikes: () => Promise<void>;
  toggleTrack: (id: string) => void;
  setAllTracks: (value: boolean) => void;
  start: () => Promise<void>;
  stop: () => Promise<void>;
  toast: (text: string, tone?: Toast["tone"], action?: string) => void;
  dismissToast: (id: number) => void;
};

let toastId = 0;

export const useApp = create<State>((set, get) => ({
  ready: false,
  needsToken: true,
  settings: null,
  account: null,
  page: "home",
  source: null,
  selected: new Set<string>(),
  loadingSource: false,
  downloading: false,
  progress: null,
  tracks: [],
  logs: [],
  toasts: [],

  init: async () => {
    const [settings, hasToken] = await Promise.all([api.settings(), api.hasToken()]);
    set({ settings, needsToken: !hasToken, ready: true });

    if (hasToken) {
      api
        .probeLossless()
        .then((account) => set({ account, needsToken: false }))
        .catch(() => undefined);
    }

    await subscribe({
      onTrack: (state) =>
        set((current) => {
          const tracks = [...current.tracks];
          tracks[state.index] = state;
          return { tracks };
        }),
      onProgress: (progress) => set({ progress, downloading: progress.running }),
      onLog: (log) =>
        set((current) => ({
          logs: [...current.logs.slice(-400), { ...log, time: Date.now() }],
        })),
      onFinished: (progress) => {
        set({ progress, downloading: false });
        get().toast(
          progress.cancelled
            ? "Загрузка остановлена"
            : `Готово: ${progress.ok} скачано, ${progress.skipped} пропущено, ${progress.failed} с ошибкой`,
          progress.failed > 0 ? "error" : "success",
          "Загрузки",
        );
      },
    });
  },

  setPage: (page) => set({ page }),

  setSettings: async (settings) => {
    const saved = await api.saveSettings(settings);
    set({ settings: saved });
  },

  setAccount: (account) => set({ account }),

  finishWizard: async (account) => {
    set({ account, needsToken: false });
    const settings = await api.settings();
    set({ settings });
  },

  resolve: async (url) => {
    set({ loadingSource: true });
    try {
      const source = await api.resolve(url);
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
    await api.stop();
    get().toast("Останавливаю после текущего трека…");
  },

  toast: (text, tone = "info", action) => {
    const id = ++toastId;
    set((current) => ({ toasts: [...current.toasts, { id, text, tone, action }] }));
    window.setTimeout(() => get().dismissToast(id), 6000);
  },

  dismissToast: (id) =>
    set((current) => ({ toasts: current.toasts.filter((toast) => toast.id !== id) })),
}));
