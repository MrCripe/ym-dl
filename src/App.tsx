import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useApp, type Page } from "./lib/store";
import { Icon, Button } from "./components/ui";
import Wizard from "./pages/Wizard";
import Home from "./pages/Home";
import Downloads from "./pages/Downloads";
import SettingsPage from "./pages/SettingsPage";

const NAV: { key: Page; icon: string; label: string }[] = [
  { key: "home", icon: "home", label: "Главная" },
  { key: "downloads", icon: "download", label: "Загрузки" },
  { key: "settings", icon: "settings", label: "Настройки" },
];

export default function App() {
  const ready = useApp((state) => state.ready);
  const initError = useApp((state) => state.initError);
  const needsToken = useApp((state) => state.needsToken);
  const page = useApp((state) => state.page);
  const setPage = useApp((state) => state.setPage);
  const settings = useApp((state) => state.settings);
  const downloading = useApp((state) => state.downloading);
  const init = useApp((state) => state.init);
  const toasts = useApp((state) => state.toasts);
  const dismissToast = useApp((state) => state.dismissToast);

  useEffect(() => {
    init().catch(() => undefined);
  }, [init]);

  useEffect(() => {
    if (!settings) return;
    const root = document.documentElement;
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const theme =
        settings.theme === "system" ? (media.matches ? "dark" : "light") : settings.theme;
      root.dataset.theme = theme;
      root.dataset.accent = settings.accent;
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [settings]);

  // The window is undecorated, so every screen needs the custom titlebar —
  // otherwise the token wizard and the error screen cannot be closed or dragged.
  if (!ready) {
    return (
      <div className="app-shell">
        <WindowTitlebar rail={false} />
        <div style={{ height: "100%", display: "grid", placeItems: "center" }}>
          <span className="md-body muted">Запускаю ym-dl…</span>
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
              Не удалось прочитать настройки
            </div>
            <div className="md-body-sm muted" style={{ marginTop: 8 }}>
              {initError}
            </div>
            <div style={{ marginTop: 20 }}>
              <Button icon="refresh" onClick={() => init().catch(() => undefined)}>
                Повторить
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
    <div className="app-shell">
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
            <span className="label">{item.label}</span>
          </button>
        ))}
        <div className="rail-spacer" />
      </nav>

      <div className="app-content">
        <div key={page} className="page-host animate-in" style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}>
          {page === "home" ? <Home /> : null}
          {page === "downloads" ? <Downloads /> : null}
          {page === "settings" ? <SettingsPage /> : null}
        </div>
      </div>

      <div className="toast-host">
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
    </div>
  );
}

function WindowTitlebar({ rail = true }: { rail?: boolean }) {
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
      <button type="button" className="window-button" aria-label="Свернуть" onClick={() => win.minimize()}>
        <Icon name="remove" />
      </button>
      <button
        type="button"
        className="window-button"
        aria-label="Развернуть"
        onClick={() => win.toggleMaximize()}
      >
        <Icon name="expand_content" />
      </button>
      <button type="button" className="window-button close" aria-label="Закрыть" onClick={() => win.close()}>
        <Icon name="close" />
      </button>
    </div>
  );
}
