import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useApp, type Page } from "./lib/store";
import { useT } from "./lib/i18n";
import { Icon, Button, IconButton } from "./components/ui";
import Wizard from "./pages/Wizard";
import Home from "./pages/Home";
import Downloads from "./pages/Downloads";
import Library from "./pages/Library";
import PlayerBar from "./components/PlayerBar";
import SettingsPage from "./pages/SettingsPage";

const NAV: { key: Page; icon: string; labelKey: string }[] = [
  { key: "home", icon: "home", labelKey: "nav.home" },
  { key: "library", icon: "library_music", labelKey: "nav.library" },
  { key: "downloads", icon: "download", labelKey: "nav.downloads" },
  { key: "settings", icon: "settings", labelKey: "nav.settings" },
];

export default function App() {
  const t = useT();
  const ready = useApp((state) => state.ready);
  const initError = useApp((state) => state.initError);
  const needsToken = useApp((state) => state.needsToken);
  const page = useApp((state) => state.page);
  const setPage = useApp((state) => state.setPage);
  const settings = useApp((state) => state.settings);
  // The draft is what the settings page edits: applying it live makes a theme or
  // accent click visible immediately instead of only after a save.
  const settingsDraft = useApp((state) => state.settingsDraft);
  const appearance = settingsDraft ?? settings;
  const downloading = useApp((state) => state.downloading);
  const init = useApp((state) => state.init);
  const toasts = useApp((state) => state.toasts);
  const dismissToast = useApp((state) => state.dismissToast);
  const availableUpdate = useApp((state) => state.availableUpdate);
  const playing = useApp((state) => state.playing);

  useEffect(() => {
    init().catch(() => undefined);
  }, [init]);

  // The boot screen from index.html covers the window until the app is ready, so
  // the first thing on screen is the logo instead of an empty frame. It fades out
  // rather than being cut, which is what made the start feel abrupt.
  useEffect(() => {
    if (!ready) return;
    const boot = document.getElementById("boot");
    if (!boot) return;
    boot.classList.add("leaving");
    const timer = window.setTimeout(() => boot.remove(), 480);
    return () => window.clearTimeout(timer);
  }, [ready]);

  useEffect(() => {
    // Closing with the title bar button goes through flushSettings, but the OS
    // close button, Alt+F4 and a shutdown do not, so the pending write is also
    // flushed when the page is going away.
    const flush = () => {
      void useApp.getState().flushSettings();
    };
    window.addEventListener("beforeunload", flush);
    return () => window.removeEventListener("beforeunload", flush);
  }, []);

  useEffect(() => {
    if (!appearance) return;
    const root = document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const theme =
        appearance.theme === "system" ? (media.matches ? "dark" : "light") : appearance.theme;
      root.dataset.theme = theme;
      root.dataset.accent = appearance.accent;
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [appearance]);

  // The window is undecorated, so every screen needs the custom titlebar —
  // otherwise the token wizard and the error screen cannot be closed or dragged.
  if (!ready) {
    return (
      <div className="app-shell">
        <WindowTitlebar rail={false} />
        <div style={{ height: "100%", display: "grid", placeItems: "center" }}>
          <span className="md-body muted">{t("app.starting")}</span>
        </div>
      </div>
    );
  }

  if (initError) {
    return (
      <div className="app-shell">
        <WindowTitlebar rail={false} />
        <div style={{ height: "100%", display: "grid", placeItems: "center", padding: 32 }}>
          <div style={{ maxWidth: 520, textAlign: "center" }}>
            <Icon name="error" size={40} />
            <div className="md-title" style={{ marginTop: 12 }}>
              {t("app.settingsError")}
            </div>
            <div className="md-body-sm muted" style={{ marginTop: 8 }}>
              {initError}
            </div>
            <div style={{ marginTop: 20 }}>
              <Button icon="refresh" onClick={() => init().catch(() => undefined)}>
                {t("app.retry")}
              </Button>
            </div>
          </div>
        </div>
      </div>
    );
  }

  if (needsToken) {
    return (
      <div className="app-shell">
        <WindowTitlebar rail={false} />
        <Wizard />
      </div>
    );
  }

  return (
    <div className="app-shell" data-player={playing ? "true" : undefined}>
      <WindowTitlebar />
      <nav className="rail">
        <div style={{ height: 36 }} />
        {NAV.map((item) => (
          <button
            key={item.key}
            type="button"
            className="rail-item"
            aria-current={page === item.key ? "page" : undefined}
            onClick={() => setPage(item.key)}
          >
            <span className="indicator">
              <Icon name={item.icon} filled={page === item.key} />
              {item.key === "downloads" && downloading ? <span className="rail-dot" /> : null}
            </span>
            <span className="label">{t(item.labelKey)}</span>
          </button>
        ))}
        <div className="rail-spacer" />
      </nav>

      <div className="app-content">
        <div key={page} className="page-host animate-in" style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
          {page === "home" ? <Home /> : null}
          {page === "library" ? <Library /> : null}
          {page === "downloads" ? <Downloads /> : null}
          {page === "settings" ? <SettingsPage /> : null}
        </div>
      </div>

      {/* The banner is fixed to the bottom edge, so the toasts step up while it is
          visible instead of hiding behind it. */}
      <div
        className="toast-host"
        data-raised={availableUpdate ? "true" : undefined}
        // Toasts are the only feedback for a finished job; without this a screen
        // reader never announces them.
        aria-live="polite"
        role="status"
      >
        {toasts.map((toast) => (
          <div key={toast.id} className={`toast ${toast.tone === "info" ? "" : toast.tone}`}>
            <span className="grow md-body">{toast.text}</span>
            {toast.action ? (
              <button
                type="button"
                className="action"
                onClick={() => {
                  dismissToast(toast.id);
                  if (toast.page) setPage(toast.page);
                }}
              >
                {toast.action}
              </button>
            ) : null}
          </div>
        ))}
      </div>

      <PlayerBar />
      <UpdateBanner />
    </div>
  );
}

/**
 * One-click update, shown wherever the user is.
 *
 * The startup check already knows the new version; before this, that knowledge
 * only produced a toast pointing at the settings, where the "Обновить" button was
 * not even rendered until "Проверить" was pressed again. Installing what the app
 * already found is the whole point of a self-updating program.
 */
function UpdateBanner() {
  const t = useT();
  const update = useApp((state) => state.availableUpdate);
  const progress = useApp((state) => state.updateProgress);
  const portable = useApp((state) => state.portable);
  const install = useApp((state) => state.installAvailableUpdate);
  const dismiss = useApp((state) => state.dismissUpdate);

  if (!update) return null;
  const busy = progress !== null;

  return (
    <div className="update-banner animate-in" role="status">
      <Icon name="system_update" filled />
      <span className="grow">{t("banner.updateReady", { version: update.version })}</span>
      <Button
        variant="filled"
        size="compact"
        icon="download"
        disabled={busy}
        onClick={() => {
          // A portable unpack has no installer to replace: running it would put a
          // second copy on the machine, so that build is sent to the release page.
          if (portable) {
            void openUrl("https://github.com/MrCripe/ym-dl/releases/latest").catch(() => undefined);
            return;
          }
          void install();
        }}
      >
        {!busy
          ? portable
            ? t("banner.download")
            : t("set.updateBtn")
          : progress > 0
            ? `${progress}%`
            : t("set.downloading")}
      </Button>
      <IconButton
        icon="close"
        label={t("banner.later")}
        size="small"
        disabled={busy}
        onClick={dismiss}
      />
    </div>
  );
}

function WindowTitlebar({ rail = true }: { rail?: boolean }) {
  const t = useT();
  const win = getCurrentWindow();
  return (
    <div
      className="titlebar"
      data-tauri-drag-region
      style={{
        position: "absolute",
        top: 0,
        left: 0,
        right: 0,
        height: 44,
        zIndex: 30,
        paddingLeft: rail ? "calc(var(--rail-width) + 16px)" : 16,
      }}
    >
      <span className="brand" data-tauri-drag-region>
        <Icon name="graphic_eq" filled className="brand-glyph" />
        ym-dl
      </span>
      <span className="spacer" data-tauri-drag-region />
      <button type="button" className="window-button" aria-label={t("win.minimize")} onClick={() => win.minimize()}>
        <Icon name="remove" />
      </button>
      <button
        type="button"
        className="window-button"
        aria-label={t("win.maximize")}
        onClick={() => win.toggleMaximize()}
      >
        <Icon name="expand_content" />
      </button>
      <button
        type="button"
        className="window-button close"
        aria-label={t("win.close")}
        onClick={() => {
          // Autosave waits 400 ms; closing sooner would drop the last edit.
          void useApp.getState().flushSettings().finally(() => win.close());
        }}
      >
        <Icon name="close" />
      </button>
    </div>
  );
}
