import { useEffect, useState, type ReactNode } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type EnvironmentInfo, type UpdateEntry } from "../lib/api";
import { findUpdate } from "../lib/update";
import { useApp } from "../lib/store";
import { useT, useLang } from "../lib/i18n";
import { Badge, Button, Icon, Segmented, Slider, Switch, TextField } from "../components/ui";

const ACCENTS: { value: string; key: string; color: string }[] = [
  { value: "violet", key: "set.violet", color: "#6750a4" },
  { value: "blue", key: "set.blue", color: "#0061a4" },
  { value: "green", key: "set.green", color: "#006c4c" },
  { value: "teal", key: "set.teal", color: "#00696e" },
  { value: "orange", key: "set.orange", color: "#8b5000" },
  { value: "pink", key: "set.pink", color: "#a4306b" },
];

/**
 * Ready-made file layouts, in the order a person is likely to want them: liked
 * tracks first, then singles, then albums, then the sorting variants.
 *
 * `example` is what the user actually sees: a template alone says nothing about
 * the folder that comes out of it.
 */
const LAYOUTS: { key: string; hint: string; pattern: string; example: string }[] = [
  {
    key: "layout.likes",
    hint: "layout.likesHint",
    pattern: "#title",
    example: "Smells Like Teen Spirit.flac",
  },
  {
    key: "layout.single",
    hint: "layout.singleHint",
    pattern: "#track-artist - #title",
    example: "Nirvana - Smells Like Teen Spirit.flac",
  },
  {
    key: "layout.album",
    hint: "layout.albumHint",
    pattern: "#album-artist/#album/#number-padded #title",
    example: "Nirvana/Nevermind/05 Smells Like Teen Spirit.flac",
  },
  {
    key: "layout.year",
    hint: "layout.yearHint",
    pattern: "#album-artist/#year - #album/#number-padded #title",
    example: "Nirvana/1991 - Nevermind/05 Smells Like Teen Spirit.flac",
  },
  {
    key: "layout.flat",
    hint: "layout.flatHint",
    pattern: "#number-padded #title",
    example: "05 Smells Like Teen Spirit.flac",
  },
];

export default function SettingsPage() {
  const t = useT();
  const lang = useLang();
  const draft = useApp((state) => state.settingsDraft);
  const patch = useApp((state) => state.patchSettings);
  const resetToken = useApp((state) => state.resetToken);
  const toast = useApp((state) => state.toast);
  const account = useApp((state) => state.account);

  const [env, setEnv] = useState<EnvironmentInfo | null>(null);
  const [updates, setUpdates] = useState<UpdateEntry[] | null>(null);
  const [checking, setChecking] = useState(false);
  const [repackAsk, setRepackAsk] = useState(false);

  useEffect(() => {
    api.environment().then(setEnv).catch(() => undefined);
  }, []);

  const checkUpdates = async () => {
    setChecking(true);
    try {
      const list = await api.updates();
      setUpdates(list);
      // The updater reads the release manifest and knows whether it can install
      // the new build in place, which the plain version check cannot say.
      const available = await findUpdate();
      // The banner is what installs it; keeping the two in sync means the same
      // version is offered everywhere instead of each screen asking on its own.
      useApp.setState({ availableUpdate: available });
      if (available) {
        toast(t("set.updateFound", { version: available.version }), "info");
      } else {
        toast(t("set.upToDate"), "success");
      }
    } catch (error) {
      // "Nothing to install" and "the check failed" are different answers; the
      // second one must not be shown as a green "you are up to date".
      useApp.setState({ availableUpdate: null });
      toast(t("set.updateFailed", { error: (error as Error).message }), "error");
    } finally {
      setChecking(false);
    }
  };

  if (!draft) return null;

  const expiry = account?.expires ? formatExpiry(account.expires, lang) : "";

  const activeLayout =
    LAYOUTS.find((layout) => layout.pattern === draft.pathPattern)?.key ?? "custom";

  return (
    <div className="page">
      <div className="page-narrow settings-page">
        <div className="page-header">
          <div>
            <h1>{t("set.title")}</h1>
          </div>
        </div>

        <Section icon="person" title={t("set.account")}>
          <Row
            name={t("wiz.account")}
            desc={account ? account.login : t("set.accountDesc")}
            control={
              account ? (
                <Badge tone={account.hasPlus ? "success" : "error"}>
                  {account.hasPlus ? t("set.plusActive") : t("set.plusInactive")}
                </Badge>
              ) : (
                <Badge tone="neutral">{t("set.unknown")}</Badge>
              )
            }
          />
          <Row
            name={t("set.subUntil")}
            control={
              <Badge tone={expiry ? "primary" : "neutral"}>{expiry || t("set.subNoData")}</Badge>
            }
          />
          {/* Only shown when the fallback is actually in use: a password store that
              answered is the normal case and needs no comment. */}
          {env?.tokenFile ? (
            <Row
              name={t("set.tokenFile")}
              desc={t("set.tokenFileDesc")}
              control={<Badge tone="warning" icon="warning">{t("set.tokenFileBadge")}</Badge>}
            />
          ) : null}
          <Row
            name={t("set.lossless")}
            desc={
              account?.losslessOk === false
                ? `${t("set.losslessNo")} ${account.probeCodec} ${account.probeBitrate} kbps`
                : account?.losslessOk
                  ? t("set.losslessOk")
                  : t("wiz.losslessUnchecked")
            }
            control={
              <Button
                variant="text"
                size="compact"
                icon="refresh"
                onClick={async () => {
                  try {
                    const info = await api.probeLossless();
                    useApp.setState({ account: info });
                    toast(
                      info.losslessOk
                        ? t("set.losslessFound")
                        : `${t("set.losslessMissing")} ${info.probeCodec}`,
                      info.losslessOk ? "success" : "error",
                    );
                  } catch (error) {
                    toast(String((error as Error).message), "error");
                  }
                }}
              >
                {t("set.check")}
              </Button>
            }
          />
        </Section>

        <Section icon="folder" title={t("set.files")}>
          <Row
            name={t("set.folder")}
            desc={t("set.folderDefault")}
            control={
              <div className="field-holder">
                <TextField
                  value={draft.downloadDir}
                  placeholder="~/Music"
                  icon="folder_open"
                  quiet
                  onChange={(value) => patch({ downloadDir: value })}
                />
              </div>
            }
          />
          <Row
            name={t("set.folderPick")}
            desc={t("set.folderPickDesc")}
            control={
              <Button
                variant="tonal"
                size="compact"
                icon="folder_open"
                onClick={async () => {
                  const picked = await openDialog({ directory: true, multiple: false });
                  if (typeof picked === "string") patch({ downloadDir: picked });
                }}
              >
                {t("set.browse")}
              </Button>
            }
          />
          <Row
            name={t("set.quality")}
            desc={t("set.qualityDesc")}
            control={
              <Segmented
                value={draft.quality}
                onChange={(value) => patch({ quality: value })}
                options={[
                  { value: 0, label: "AAC 64" },
                  { value: 1, label: "AAC 192" },
                  { value: 2, label: "Lossless" },
                ]}
              />
            }
          />
          <Row
            name={t("set.skipExisting")}
            desc={t("set.skipExistingDesc")}
            control={
              <Switch
                checked={draft.skipExisting}
                label={t("set.skipExisting")}
                onChange={(value) => patch({ skipExisting: value })}
              />
            }
          />
        </Section>

        <Section icon="queue_music" title={t("set.layout")}>
          <div className="layout-list">
            <p className="layout-intro">{t("set.layoutDesc")}</p>
            {LAYOUTS.map((layout) => (
              <button
                key={layout.key}
                type="button"
                className={`layout-option${activeLayout === layout.key ? " active" : ""}`}
                aria-pressed={activeLayout === layout.key}
                onClick={() => patch({ pathPattern: layout.pattern })}
              >
                <div className="layout-text">
                  <span className="layout-name">{t(layout.key)}</span>
                  <span className="layout-hint">{t(layout.hint)}</span>
                </div>
                <code className="layout-example">{layout.example}</code>
              </button>
            ))}
            <div className={`layout-option custom${activeLayout === "custom" ? " active" : ""}`}>
              <div className="layout-text">
                <span className="layout-name">{t("set.layoutCustom")}</span>
                <span className="layout-hint">{t("layout.customHint")}</span>
              </div>
              <div className="layout-input">
                <TextField
                  value={draft.pathPattern}
                  icon="edit"
                  quiet
                  onChange={(value) => patch({ pathPattern: value })}
                />
              </div>
            </div>
          </div>
        </Section>

        <Section icon="subtitles" title={t("set.tags")}>
          <Row
            name={t("set.cover")}
            desc={t("set.coverDesc")}
            control={
              <Switch
                checked={draft.embedCover}
                label={t("set.cover")}
                onChange={(value) => patch({ embedCover: value })}
              />
            }
          />
          <Row
            name={t("set.coverOriginal")}
            desc={t("set.coverOriginalDesc")}
            control={
              <Switch
                checked={draft.coverOriginal}
                label={t("set.coverOriginal")}
                disabled={!draft.embedCover}
                onChange={(value) => patch({ coverOriginal: value })}
              />
            }
          />
          <Row
            name={t("set.coverSize")}
            desc={t("set.coverSizeDesc")}
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.coverResolution}
                  min={100}
                  max={1200}
                  step={100}
                  label={t("set.coverSize")}
                  disabled={!draft.embedCover || draft.coverOriginal}
                  onChange={(value) => patch({ coverResolution: value })}
                />
                <span className="value-chip">{draft.coverResolution}px</span>
              </div>
            }
          />
          <Row
            name={t("set.lyrics")}
            desc={t("set.lyricsDesc")}
            control={
              <Segmented
                value={draft.lyricsFormat}
                onChange={(value) => patch({ lyricsFormat: value })}
                options={[
                  { value: "none", label: t("set.lyricsNone") },
                  { value: "text", label: t("set.lyricsPlain") },
                  { value: "lrc", label: t("set.lyricsLrc") },
                ]}
              />
            }
          />
          <Row
            name={t("set.lrclib")}
            desc={t("set.lrclibDesc")}
            control={
              <Switch
                checked={draft.lyricsFallback}
                label={t("set.lrclib")}
                disabled={draft.lyricsFormat === "none"}
                onChange={(value) => patch({ lyricsFallback: value })}
              />
            }
          />
          {draft.lyricsFallback && draft.lyricsFormat !== "none" ? (
            <div className="setting-note">
              <Badge tone="warning">{t("set.lrclibBadge")}</Badge>
            </div>
          ) : null}
          <Row
            name={t("set.compat")}
            desc={t("set.compatDesc")}
            control={
              <Segmented
                value={draft.compatibilityLevel}
                onChange={(value) => patch({ compatibilityLevel: value })}
                options={[
                  { value: 0, label: "0" },
                  { value: 1, label: "1" },
                ]}
              />
            }
          />
        </Section>

        <Section icon="tune" title={t("set.what")}>
          <Row
            name={t("set.onlyMusic")}
            desc={t("set.onlyMusicDesc")}
            control={
              <Switch
                checked={draft.onlyMusic}
                label={t("set.onlyMusic")}
                onChange={(value) => patch({ onlyMusic: value })}
              />
            }
          />
          <Row
            name={t("set.stickArtist")}
            desc={t("set.stickArtistDesc")}
            control={
              <Switch
                checked={draft.stickToArtist}
                label={t("set.stickArtist")}
                onChange={(value) => patch({ stickToArtist: value })}
              />
            }
          />
          <Row
            name={t("set.unsafePath")}
            desc={t("set.unsafePathDesc")}
            control={
              <Switch
                checked={draft.unsafePath}
                label={t("set.unsafePath")}
                onChange={(value) => patch({ unsafePath: value })}
              />
            }
          />
        </Section>

        <Section icon="network_check" title={t("set.network")} collapsible>
          <Row
            name={t("set.parallel")}
            desc={t("set.parallelDesc")}
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.concurrent}
                  min={1}
                  max={6}
                  label={t("set.parallel")}
                  onChange={(value) => patch({ concurrent: value })}
                />
                <span className="value-chip">{draft.concurrent}</span>
              </div>
            }
          />
          <Row
            name={t("set.timeout")}
            desc={t("set.timeoutDesc")}
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.timeout}
                  min={5}
                  max={120}
                  step={5}
                  label={t("set.timeout")}
                  onChange={(value) => patch({ timeout: value })}
                />
                <span className="value-chip">{draft.timeout} s</span>
              </div>
            }
          />
          <Row
            name={t("set.tries")}
            desc={t("set.triesDesc")}
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.tries}
                  min={1}
                  max={6}
                  label={t("set.tries")}
                  onChange={(value) => patch({ tries: value })}
                />
                <span className="value-chip">{draft.tries}</span>
              </div>
            }
          />
          <Row
            name={t("set.retryDelay")}
            desc={t("set.retryDelayDesc")}
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.retryDelay}
                  min={0}
                  max={30}
                  label={t("set.retryDelay")}
                  onChange={(value) => patch({ retryDelay: value })}
                />
                <span className="value-chip">{draft.retryDelay} s</span>
              </div>
            }
          />
          <Row
            name={t("set.trackDelay")}
            desc={t("set.trackDelayDesc")}
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.delay}
                  min={0}
                  max={30}
                  label={t("set.trackDelay")}
                  onChange={(value) => patch({ delay: value })}
                />
                <span className="value-chip">{draft.delay} s</span>
              </div>
            }
          />
        </Section>

        <Section icon="audio_file" title={t("set.flac")}>
          <Row
            name={t("set.extractFlac")}
            desc={t("set.extractFlacDesc")}
            control={
              <Switch
                checked={draft.extractFlac}
                label={t("set.extractFlac")}
                onChange={(value) => patch({ extractFlac: value })}
              />
            }
          />
          <Row
            name={t("set.ffmpeg")}
            desc={
              env?.ffmpeg && env?.ffprobe ? env.ffmpegPath : t("set.ffmpegMissing")
            }
            control={
              env?.ffmpeg && env?.ffprobe ? (
                <Badge tone="success" icon="check">
                  {t("set.ffmpegFound")}
                </Badge>
              ) : (
                <Button
                  variant="tonal"
                  size="compact"
                  icon="help"
                  onClick={() =>
                    void openUrl("https://www.gyan.dev/ffmpeg/builds/").catch(() => undefined)
                  }
                >
                  {t("set.ffmpegHow")}
                </Button>
              )
            }
          />
          <Row
            name={t("set.repack")}
            desc={repackAsk ? t("set.repackWarn") : t("set.repackDesc")}
            control={
              repackAsk ? (
                <>
                  <Button
                    variant="danger-tonal"
                    size="compact"
                    icon="build"
                    onClick={async () => {
                      setRepackAsk(false);
                      try {
                        const stats = await api.postProcess();
                        toast(
                          t("set.repackDone", {
                            converted: stats.converted,
                            failed: stats.failed,
                          }),
                          stats.failed ? "error" : "success",
                        );
                      } catch (error) {
                        toast(String((error as Error).message), "error");
                      }
                    }}
                  >
                    {t("set.repackConfirm")}
                  </Button>
                  <Button variant="text" size="compact" onClick={() => setRepackAsk(false)}>
                    {t("set.repackCancel")}
                  </Button>
                </>
              ) : (
                <Button
                  variant="tonal"
                  size="compact"
                  icon="build"
                  disabled={!env?.ffmpeg}
                  onClick={() => setRepackAsk(true)}
                >
                  {t("set.repackRun")}
                </Button>
              )
            }
          />
        </Section>

        <Section icon="palette" title={t("set.appearance")}>
          <Row
            name={t("set.theme")}
            desc={t("set.themeDesc")}
            control={
              <Segmented
                value={draft.theme}
                onChange={(value) => patch({ theme: value })}
                options={[
                  { value: "system", label: t("set.themeSystem") },
                  { value: "light", label: t("set.themeLight") },
                  { value: "dark", label: t("set.themeDark") },
                ]}
              />
            }
          />
          <Row
            name={t("set.language")}
            desc={t("set.languageDesc")}
            control={
              <Segmented
                value={draft.language}
                onChange={(value) => patch({ language: value })}
                options={[
                  { value: "auto", label: t("set.langAuto") },
                  { value: "ru", label: t("set.langRu") },
                  { value: "en", label: t("set.langEn") },
                ]}
              />
            }
          />
          <Row
            name={t("set.accent")}
            desc={t("set.accentDesc")}
            control={
              <div className="accent-row">
                {ACCENTS.map((accent) => (
                  <button
                    key={accent.value}
                    type="button"
                    title={t(accent.key)}
                    aria-label={t(accent.key)}
                    aria-pressed={draft.accent === accent.value}
                    className="accent-swatch"
                    style={{ background: accent.color }}
                    onClick={() => patch({ accent: accent.value })}
                  />
                ))}
              </div>
            }
          />
        </Section>

        <Section icon="system_update" title={t("set.updates")}>
          <Row
            name={t("set.autoCheck")}
            desc={t("set.autoCheckDesc")}
            control={
              <Switch
                checked={draft.checkUpdates}
                label={t("set.autoCheck")}
                onChange={(value) => patch({ checkUpdates: value })}
              />
            }
          />
          <Row
            name={t("set.checkNow")}
            desc={t("set.checkNowDesc")}
            control={
              <Button
                variant="tonal"
                size="compact"
                icon="refresh"
                onClick={checkUpdates}
                disabled={checking}
              >
                {checking ? t("set.checking") : t("set.check")}
              </Button>
            }
          />
          {updates?.map((entry) => (
            <div key={entry.kind} className="update-row">
              <div className="names">
                <div className="md-body">
                  {entry.kind === "app" ? t("set.appName") : "ffmpeg"}
                </div>
                <div className="md-body-sm muted">
                  {entry.kind === "app" ? entry.detail : t("set.ffmpegSeparate")}
                </div>
              </div>
              <Badge tone="neutral">
                {t("set.yours")} {entry.installed}
              </Badge>
              <Badge tone={!entry.checked ? "warning" : entry.newer ? "warning" : "success"}>
                {!entry.checked
                  ? t("set.notChecked")
                  : entry.newer
                    ? `${t("set.availableShort")} ${entry.latest}`
                    : t("set.actual")}
              </Badge>
            </div>
          ))}
        </Section>

        <Section icon="key" title={t("set.token")}>
          <Row
            name={t("set.configPath")}
            desc={t("set.configPathDesc")}
            control={<span className="path-box">{env?.configPath ?? "—"}</span>}
          />
          <Row
            name={t("set.resetToken")}
            desc={t("set.resetTokenDesc")}
            control={
              <Button
                variant="danger-tonal"
                size="compact"
                icon="key"
                onClick={async () => {
                  try {
                    await resetToken();
                  } catch (error) {
                    toast(String((error as Error).message), "error");
                  }
                }}
              >
                {t("set.resetBtn")}
              </Button>
            }
          />
        </Section>

        <Section icon="info" title={t("set.about")}>
          <Row
            name={t("set.aboutApp")}
            desc={t("set.aboutDesc")}
            control={<Badge tone="primary">{env?.appVersion ?? "—"}</Badge>}
          />
          <Row
            name={t("set.aboutSystem")}
            desc={env?.os ?? ""}
            control={
              <span className="path-box">
                {env?.portable ? t("set.aboutPortable") : t("set.aboutInstalled")}
              </span>
            }
          />
          <Row
            name={t("set.aboutLinks")}
            desc={t("set.aboutLinksDesc")}
            control={
              <div className="row-actions">
                <Button
                  variant="tonal"
                  size="compact"
                  icon="code"
                  onClick={() =>
                    void openUrl("https://github.com/MrCripe/ym-dl").catch(() => undefined)
                  }
                >
                  GitHub
                </Button>
                <Button
                  variant="tonal"
                  size="compact"
                  icon="history"
                  onClick={() =>
                    void openUrl("https://github.com/MrCripe/ym-dl/blob/master/CHANGELOG.md").catch(
                      () => undefined,
                    )
                  }
                >
                  {t("set.aboutChanges")}
                </Button>
              </div>
            }
          />
        </Section>
      </div>
    </div>
  );
}

/**
 * Turns the API's `2026-12-11T18:55:36+00:00` into a date a person reads.
 *
 * Yandex reports the end of the subscription in UTC; the date is shown in Moscow
 * time, which is what the account page uses.
 */
function formatExpiry(value: string, lang: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat(lang === "en" ? "en-GB" : "ru-RU", {
    day: "numeric",
    month: "long",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZone: "Europe/Moscow",
  }).format(date);
}

function Section({
  icon,
  title,
  children,
  collapsible = false,
}: {
  icon: string;
  title: string;
  children: ReactNode;
  /** Advanced settings start folded: they are rarely touched and the numbers
   *  mean nothing to a first-time user. */
  collapsible?: boolean;
}) {
  const [open, setOpen] = useState(!collapsible);
  return (
    <>
      <div className="section-title">
        <Icon name={icon} filled />
        {title}
        {collapsible ? (
          <button
            type="button"
            className={`section-toggle${open ? " open" : ""}`}
            aria-expanded={open}
            aria-label={open ? `− ${title}` : `+ ${title}`}
            onClick={() => setOpen((value) => !value)}
          >
            <span className="chevron" aria-hidden />
          </button>
        ) : null}
      </div>
      {open ? <div className="setting-card">{children}</div> : null}
    </>
  );
}

function Row({
  name,
  desc,
  control,
}: {
  name: string;
  desc?: string;
  control: ReactNode;
}) {
  return (
    <div className="setting-row">
      <div className="texts">
        <div className="name">{name}</div>
        {desc ? <div className="desc">{desc}</div> : null}
      </div>
      <div className="control">{control}</div>
    </div>
  );
}
