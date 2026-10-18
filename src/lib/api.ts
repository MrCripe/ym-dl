import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Settings = {
  username: string;
  downloadDir: string;
  pathPattern: string;
  quality: number;
  skipExisting: boolean;
  embedCover: boolean;
  coverOriginal: boolean;
  coverResolution: number;
  lyricsFormat: "none" | "text" | "lrc";
  compatibilityLevel: number;
  onlyMusic: boolean;
  stickToArtist: boolean;
  unsafePath: boolean;
  delay: number;
  timeout: number;
  tries: number;
  retryDelay: number;
  extractFlac: boolean;
  quarantineLossy: boolean;
  theme: "system" | "light" | "dark";
  accent: string;
  checkUpdates: boolean;
  lastUrl: string;
  concurrent: number;
};

export type AccountInfo = {
  uid: string;
  login: string;
  name: string;
  hasPlus: boolean;
  familyMember: boolean;
  productId: string;
  permissions: string;
  expires: string;
  losslessOk: boolean | null;
  probeCodec: string;
  probeQuality: string;
  probeBitrate: number;
  probeTrack: string;
  note: string;
};

export type TrackItem = {
  id: string;
  title: string;
  artist: string;
  album: string;
  duration: string;
  available: boolean;
  year: number | null;
};

export type SourceInfo = {
  kind: "track" | "album" | "playlist" | "artist" | "likes";
  title: string;
  subtitle: string;
  tracks: TrackItem[];
};

export type TrackState = {
  index: number;
  total: number;
  id: string;
  title: string;
  artist: string;
  album: string;
  status: "pending" | "downloading" | "ok" | "failed" | "skipped" | "cancelled";
  codec: string;
  bitrate: number;
  doneBytes: number;
  totalBytes: number;
  path: string;
  message: string;
};

export type JobProgress = {
  done: number;
  total: number;
  ok: number;
  failed: number;
  skipped: number;
  running: boolean;
  stopping: boolean;
  cancelled: boolean;
  error: boolean;
};

export type JobLog = { level: "info" | "ok" | "warn" | "error"; message: string };

export type UpdateEntry = {
  name: string;
  installed: string;
  latest: string;
  newer: boolean;
  detail: string;
};

export type EnvironmentInfo = {
  ffmpeg: boolean;
  ffprobe: boolean;
  ffmpegPath: string;
  appVersion: string;
  configPath: string;
  tokenPath: string;
  os: string;
};

export type PostProcessStats = { converted: number; quarantined: number; failed: number };

const call = <T>(command: string, args?: Record<string, unknown>) =>
  invoke<T>(command, args).catch((error) => {
    throw new Error(typeof error === "string" ? error : String(error));
  });

export const api = {
  settings: () => call<Settings>("get_settings"),
  saveSettings: (settings: Settings) => call<Settings>("save_settings", { settings }),
  hasToken: () => call<boolean>("has_token"),
  clearToken: () => call<void>("clear_token"),
  checkToken: (token: string) => call<AccountInfo>("check_token", { token }),
  resolve: (url: string) => call<SourceInfo>("resolve_source", { url }),
  likes: () => call<SourceInfo>("load_likes"),
  start: (trackIds: string[]) => call<void>("start_download", { trackIds }),
  stop: () => call<boolean>("stop_download"),
  updates: (appVersion: string, repo: string) =>
    call<UpdateEntry[]>("check_updates", { appVersion, repo }),
  environment: () => call<EnvironmentInfo>("environment"),
  postProcess: () => call<PostProcessStats>("post_process"),
  probeLossless: () => call<AccountInfo>("probe_lossless"),
};

export type JobEvents = {
  onTrack: (state: TrackState) => void;
  onProgress: (progress: JobProgress) => void;
  onLog: (log: JobLog) => void;
  onFinished: (progress: JobProgress) => void;
  onResolve: (progress: { done: number; total: number }) => void;
};

export async function subscribe(handlers: JobEvents): Promise<UnlistenFn> {
  const unlisteners = await Promise.all([
    listen<TrackState>("job://track", (event) => handlers.onTrack(event.payload)),
    listen<JobProgress>("job://progress", (event) => handlers.onProgress(event.payload)),
    listen<JobLog>("job://log", (event) => handlers.onLog(event.payload)),
    listen<JobProgress>("job://finished", (event) => handlers.onFinished(event.payload)),
    listen<{ done: number; total: number }>("job://resolve", (event) =>
      handlers.onResolve(event.payload),
    ),
  ]);
  return () => unlisteners.forEach((off) => off());
}
