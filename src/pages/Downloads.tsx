import { useEffect, useMemo, useRef } from "react";
import { useApp } from "../lib/store";
import { useT, useLang } from "../lib/i18n";
import type { TrackState } from "../lib/api";
import { Badge, Button, Icon, Progress } from "../components/ui";

const STATUS_ICON: Record<TrackState["status"], { icon: string; spin?: boolean }> = {
  pending: { icon: "schedule" },
  downloading: { icon: "downloading", spin: true },
  ok: { icon: "check_circle" },
  failed: { icon: "error" },
  skipped: { icon: "skip_next" },
  cancelled: { icon: "block" },
};

function formatBytes(value: number, lang: "ru" | "en"): string {
  if (!value) return "";
  const units =
    lang === "en" ? ["B", "KiB", "MiB", "GiB"] : ["Б", "КиБ", "МиБ", "ГиБ"];
  let size = value;
  let unit = 0;
  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${size.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export default function Downloads() {
  const t = useT();
  const lang = useLang();
  const tracks = useApp((state) => state.tracks);
  const progress = useApp((state) => state.progress);
  const logs = useApp((state) => state.logs);
  const downloading = useApp((state) => state.downloading);
  const stop = useApp((state) => state.stop);
  const setPage = useApp((state) => state.setPage);
  const openFolder = useApp((state) => state.openFolder);
  const toast = useApp((state) => state.toast);
  const logRef = useRef<HTMLDivElement>(null);

  const visible = useMemo(() => tracks.filter(Boolean), [tracks]);
  const percent = progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0;

  useEffect(() => {
    logRef.current?.scrollTo({ top: logRef.current.scrollHeight, behavior: "smooth" });
  }, [logs.length]);

  const folderButton = (
    <Button
      variant="tonal"
      icon="folder_open"
      onClick={() => {
        void openFolder().catch((error) => toast(String((error as Error).message), "error"));
      }}
    >
      {t("dl.openFolder")}
    </Button>
  );

  if (!visible.length && !downloading) {
    return (
      <div className="page">
        <div className="page-narrow">
          <div className="page-header">
            <div>
              <h1>{t("dl.title")}</h1>
              <p>{t("dl.subtitle")}</p>
            </div>
            {folderButton}
          </div>
          <div className="empty-state">
            <Icon name="download" />
            <div className="md-title">{t("dl.emptyTitle")}</div>
            <div className="md-body-sm">{t("dl.emptyBody")}</div>
            <Button variant="tonal" icon="home" onClick={() => setPage("home")}>
              {t("dl.toHome")}
            </Button>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="page">
      <div className="page-narrow">
        <div className="page-header">
          <div>
            <h1>{t("dl.title")}</h1>
            <p>
              {progress
                ? `${progress.done} / ${progress.total} ${t("dl.processed")}`
                : t("dl.preparing")}
            </p>
          </div>
          <div className="row">
            {folderButton}
            {downloading ? (
              <Button variant="danger-tonal" icon="stop_circle" onClick={stop}>
                {t("dl.stop")}
              </Button>
            ) : (
              <Button variant="tonal" icon="home" onClick={() => setPage("home")}>
                {t("dl.toHome")}
              </Button>
            )}
          </div>
        </div>

        <div className="job-header animate-in">
          <div className="job-ring">
            <svg width="72" height="72" viewBox="0 0 72 72">
              <circle
                cx="36"
                cy="36"
                r="30"
                fill="none"
                stroke="var(--surface-highest)"
                strokeWidth="6"
              />
              <circle
                cx="36"
                cy="36"
                r="30"
                fill="none"
                stroke="var(--primary)"
                strokeWidth="6"
                strokeLinecap="round"
                strokeDasharray={2 * Math.PI * 30}
                strokeDashoffset={2 * Math.PI * 30 * (1 - percent / 100)}
                style={{ transition: "stroke-dashoffset 400ms var(--ease-emphasized)" }}
              />
            </svg>
            <span className="value">{Math.round(percent)}%</span>
          </div>
          <div className="job-stats grow">
            <div className="job-stat">
              <span
                className="num count-pop"
                key={`ok-${progress?.ok ?? 0}`}
                style={{ color: "var(--success)" }}
              >
                {progress?.ok ?? 0}
              </span>
              <span className="cap">{t("dl.done")}</span>
            </div>
            <div className="job-stat">
              <span
                className="num count-pop"
                key={`skip-${progress?.skipped ?? 0}`}
                style={{ color: "var(--warning)" }}
              >
                {progress?.skipped ?? 0}
              </span>
              <span className="cap">{t("dl.skipped")}</span>
            </div>
            <div className="job-stat">
              <span
                className="num count-pop"
                key={`fail-${progress?.failed ?? 0}`}
                style={{ color: "var(--error)" }}
              >
                {progress?.failed ?? 0}
              </span>
              <span className="cap">{t("dl.failed")}</span>
            </div>
            <div className="job-stat">
              <span className="num">{progress?.total ?? visible.length}</span>
              <span className="cap">{t("dl.total")}</span>
            </div>
          </div>
          {downloading ? (
            <Badge tone="primary" icon="bolt">
              {progress?.stopping ? t("dl.stopping") : t("dl.working")}
            </Badge>
          ) : progress?.error ? (
            <Badge tone="error" icon="error">
              {t("dl.interrupted")}
            </Badge>
          ) : progress?.cancelled ? (
            <Badge tone="warning" icon="block">
              {t("dl.stopped")}
            </Badge>
          ) : (progress?.failed ?? 0) > 0 ? (
            <Badge tone="error" icon="error">
              {t("dl.withErrors")}
            </Badge>
          ) : (
            <Badge tone="success" icon="task_alt">
              {t("dl.finished")}
            </Badge>
          )}
        </div>

        <Progress value={percent} />

        <div className="download-list" style={{ marginTop: 16 }}>
          {visible.map((track) => {
            const state = STATUS_ICON[track.status] ?? STATUS_ICON.pending;
            const bytes =
              track.status === "downloading" && track.totalBytes > 0
                ? (track.doneBytes / track.totalBytes) * 100
                : track.status === "ok"
                  ? 100
                  : 0;
            return (
              <div key={`${track.id}-${track.index}`} className={`download-row ${track.status}`}>
                <span className="state-icon">
                  <Icon
                    name={state.icon}
                    className={state.spin ? "spin" : ""}
                    filled={track.status === "ok"}
                  />
                </span>
                <div className="grow stack" style={{ minWidth: 0 }}>
                  <span className="truncate md-body">{track.title || track.id}</span>
                  <span className="md-body-sm muted truncate">
                    {track.artist}
                    {track.message ? ` · ${track.message}` : ""}
                    {track.status === "downloading" && track.totalBytes > 0
                      ? ` · ${formatBytes(track.doneBytes, lang)} / ${formatBytes(track.totalBytes, lang)}`
                      : ""}
                  </span>
                  {track.status === "downloading" ? (
                    <div className="bar-wrap">
                      <Progress value={bytes} indeterminate={track.totalBytes === 0} />
                    </div>
                  ) : null}
                </div>
                <span className="md-body-sm muted">{t(`dl.status.${track.status}`)}</span>
              </div>
            );
          })}
        </div>

        {logs.length ? (
          <div className="log-panel" ref={logRef}>
            {logs.map((line, index) => (
              <div key={index} className={`log-line ${line.level}`}>
                <span className="time">
                  {new Date(line.time).toLocaleTimeString(undefined, { hour12: false })}
                </span>
                <span>{line.message}</span>
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}
