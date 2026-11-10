// In-app updates: the plugin downloads the new build, verifies its signature and
// installs it, so an update never means uninstalling and downloading by hand.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type AvailableUpdate = {
  version: string;
  currentVersion: string;
};

export type DownloadProgress = {
  downloaded: number;
  total: number | null;
};

/// The update the last check found, kept so pressing "Обновить" installs exactly
/// the version that was shown. Asking again would install whatever is published by
/// then — or nothing at all, while the UI reported success.
let pending: Update | null = null;

/// What the published manifest offers, or `null` when this build is the newest.
/// A missing manifest (a release without updater files) is not an error: it just
/// means there is nothing to install. A network failure is an error and must not
/// be reported as "up to date".
export async function findUpdate(): Promise<AvailableUpdate | null> {
  const update = await check();
  pending = update;
  if (!update) return null;
  return {
    version: update.version,
    currentVersion: update.currentVersion,
  };
}

/// Downloads and installs the version found by `findUpdate`, reporting progress in
/// bytes, then restarts the app so the new binary is the one running.
export async function installUpdate(onProgress: (progress: DownloadProgress) => void): Promise<void> {
  const update = pending;
  if (!update) {
    throw new Error("обновление не найдено — нажмите «Проверить» заново");
  }
  // `chunkLength` is the size of the chunk just received, not a running total.
  let downloaded = 0;
  let total: number | null = null;
  await update.downloadAndInstall((event) => {
    if (event.event === "Started") {
      total = event.data.contentLength ?? null;
      onProgress({ downloaded, total });
    } else if (event.event === "Progress") {
      downloaded += event.data.chunkLength;
      onProgress({ downloaded, total });
    } else {
      onProgress({ downloaded, total });
    }
  });
  pending = null;
  await relaunch();
}
