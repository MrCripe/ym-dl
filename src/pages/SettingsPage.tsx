import { useEffect, useState, type ReactNode } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type EnvironmentInfo, type UpdateEntry } from "../lib/api";
import { useApp } from "../lib/store";
import { Badge, Button, Icon, Segmented, Slider, Switch, TextField } from "../components/ui";

const ACCENTS: { value: string; label: string; color: string }[] = [
  { value: "violet", label: "Фиолетовый", color: "#6750a4" },
  { value: "blue", label: "Синий", color: "#0061a4" },
  { value: "green", label: "Зелёный", color: "#006c4c" },
  { value: "teal", label: "Бирюзовый", color: "#00696e" },
  { value: "orange", label: "Оранжевый", color: "#8b5000" },
  { value: "pink", label: "Розовый", color: "#a4306b" },
];

export default function SettingsPage() {
  const draft = useApp((state) => state.settingsDraft);
  const dirty = useApp((state) => state.settingsDirty);
  const patch = useApp((state) => state.patchSettings);
  const resetToken = useApp((state) => state.resetToken);
  const toast = useApp((state) => state.toast);
  const account = useApp((state) => state.account);

  const [env, setEnv] = useState<EnvironmentInfo | null>(null);
  const [updates, setUpdates] = useState<UpdateEntry[] | null>(null);
  const [checking, setChecking] = useState(false);

  useEffect(() => {
    api.environment().then(setEnv).catch(() => undefined);
  }, []);

  const checkUpdates = async () => {
    setChecking(true);
    try {
      const list = await api.updates(env?.appVersion ?? "0.0.0");
      setUpdates(list);
    } catch (error) {
      toast(String((error as Error).message), "error");
    } finally {
      setChecking(false);
    }
  };

  if (!draft) return null;

  return (
    <div className="page">
      <div className="page-narrow settings-page">
        <div className="page-header">
          <div>
            <h1>Настройки</h1>
            <p>Всё, что влияет на загрузку, теги и внешний вид</p>
          </div>
          <div className="row">
            {dirty ? (
              <Badge tone="warning" icon="sync">
                сохраняю…
              </Badge>
            ) : (
              <Badge tone="success" icon="task_alt">
                сохранено
              </Badge>
            )}
          </div>
        </div>

        <Section icon="person" title="Аккаунт">
          <Row
            name="Имя пользователя"
            desc="Для кнопки «Скачать любимые»: чей плейлист «Мне нравится» брать. Пусто — ваш аккаунт."
            control={
              <div className="control field-holder">
                <TextField
                  value={draft.username}
                  placeholder="например, MrCriper10"
                  icon="person"
                  onChange={(value) => patch({ username: value })}
                />
              </div>
            }
          />
          <Row
            name="Аккаунт"
            desc={account ? `UID ${account.uid} · ${account.login}` : "Проверьте токен в настройках ниже"}
            control={
              account ? (
                <Badge tone={account.hasPlus ? "success" : "error"}>
                  {account.hasPlus ? "Плюс активен" : "Плюс не активен"}
                </Badge>
              ) : (
                <Badge tone="neutral">нет данных</Badge>
              )
            }
          />
          {account?.expires ? (
            <Row
              name="Подписка действует до"
              desc={account.productId ? `Продукт: ${account.productId}` : undefined}
              control={<span className="value-chip">{account.expires}</span>}
            />
          ) : null}
          <Row
            name="Lossless"
            desc={
              account?.losslessOk === false
                ? `Сервер отдаёт ${account.probeCodec} ${account.probeBitrate} kbps вместо FLAC`
                : "Проверка на реальном треке через get-file-info"
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
                      info.losslessOk ? "FLAC доступен" : `FLAC недоступен: ${info.probeCodec}`,
                      info.losslessOk ? "success" : "error",
                    );
                  } catch (error) {
                    toast(String((error as Error).message), "error");
                  }
                }}
              >
                Проверить
              </Button>
            }
          />
        </Section>

        <Section icon="folder" title="Куда и в каком качестве">
          <Row
            name="Папка загрузки"
            desc="Пусто — ~/Music/lmao"
            control={
              <div className="control field-holder">
                <TextField
                  value={draft.downloadDir}
                  placeholder="~/Music/lmao"
                  icon="folder_open"
                  onChange={(value) => patch({ downloadDir: value })}
                />
              </div>
            }
          />
          <Row
            name="Выбрать папку"
            desc="Открыть системный диалог"
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
                Обзор…
              </Button>
            }
          />
          <Row
            name="Качество"
            desc="Lossless сервер отдаёт не всем аккаунтам — проверка выше показывает реальный результат"
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
            name="Шаблон пути"
            desc="#track-artist, #album-artist, #title, #album, #year, #number, #number-padded, #disc-number, #track-id, #album-id, #artist-id"
            control={
              <div className="control field-holder">
                <TextField
                  value={draft.pathPattern}
                  icon="edit"
                  onChange={(value) => patch({ pathPattern: value })}
                />
              </div>
            }
          />
          <Row
            name="Пропускать уже скачанное"
            desc="Не перекачивать треки, которые уже лежат в папке"
            control={
              <Switch
                checked={draft.skipExisting}
                label="Пропускать уже скачанное"
                onChange={(value) => patch({ skipExisting: value })}
              />
            }
          />
        </Section>

        <Section icon="subtitles" title="Теги и обложки">
          <Row
            name="Встраивать обложку"
            desc="Обложка альбома попадёт внутрь файла"
            control={
              <Switch
                checked={draft.embedCover}
                label="Встраивать обложку"
                onChange={(value) => patch({ embedCover: value })}
              />
            }
          />
          <Row
            name="Обложка в оригинале"
            desc="Иначе — уменьшенная копия"
            control={
              <Switch
                checked={draft.coverOriginal}
                label="Обложка в оригинале"
                disabled={!draft.embedCover}
                onChange={(value) => patch({ coverOriginal: value })}
              />
            }
          />
          <Row
            name="Разрешение обложки"
            desc="Сторона квадрата в пикселях"
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.coverResolution}
                  min={100}
                  max={1200}
                  step={100}
                  label="Разрешение обложки"
                  disabled={!draft.embedCover || draft.coverOriginal}
                  onChange={(value) => patch({ coverResolution: value })}
                />
                <span className="value-chip">{draft.coverResolution}px</span>
              </div>
            }
          />
          <Row
            name="Текст песни"
            desc="LRC создаёт отдельный файл с временными метками"
            control={
              <Segmented
                value={draft.lyricsFormat}
                onChange={(value) => patch({ lyricsFormat: value })}
                options={[
                  { value: "none", label: "Не качать" },
                  { value: "text", label: "В теги" },
                  { value: "lrc", label: "LRC" },
                ]}
              />
            }
          />
          <Row
            name="Уровень совместимости"
            desc="0 — стандартные теги mutagen, 1 — исполнители через «; » для плееров"
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

        <Section icon="tune" title="Что качать">
          <Row
            name="Только музыка"
            desc="Пропускать подкасты и аудиокниги у исполнителя"
            control={
              <Switch
                checked={draft.onlyMusic}
                label="Только музыка"
                onChange={(value) => patch({ onlyMusic: value })}
              />
            }
          />
          <Row
            name="Только альбомы исполнителя"
            desc="Не брать сборники, где он лишь один из участников"
            control={
              <Switch
                checked={draft.stickToArtist}
                label="Только альбомы исполнителя"
                onChange={(value) => patch({ stickToArtist: value })}
              />
            }
          />
          <Row
            name="Не чистить имена файлов"
            desc="Опасный режим: недопустимые символы останутся в пути"
            control={
              <Switch
                checked={draft.unsafePath}
                label="Не чистить имена файлов"
                onChange={(value) => patch({ unsafePath: value })}
              />
            }
          />
          <Row
            name="Параллельных загрузок"
            desc="Больше — быстрее, но выше шанс поймать лимит запросов"
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.concurrent}
                  min={1}
                  max={6}
                  label="Параллельных загрузок"
                  onChange={(value) => patch({ concurrent: value })}
                />
                <span className="value-chip">{draft.concurrent}</span>
              </div>
            }
          />
        </Section>

        <Section icon="network_check" title="Сеть и лимиты">
          <Row
            name="Таймаут запроса"
            desc="Секунды ожидания ответа сервера"
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.timeout}
                  min={5}
                  max={120}
                  step={5}
                  label="Таймаут"
                  onChange={(value) => patch({ timeout: value })}
                />
                <span className="value-chip">{draft.timeout} с</span>
              </div>
            }
          />
          <Row
            name="Попыток при ошибке"
            desc="Сколько раз повторять запрос, прежде чем сдаться (максимум 6)"
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.tries}
                  min={1}
                  max={6}
                  label="Попытки"
                  onChange={(value) => patch({ tries: value })}
                />
                <span className="value-chip">{draft.tries}</span>
              </div>
            }
          />
          <Row
            name="Пауза между попытками"
            desc="Растёт экспоненциально при 429 и 5xx"
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.retryDelay}
                  min={0}
                  max={30}
                  label="Пауза между попытками"
                  onChange={(value) => patch({ retryDelay: value })}
                />
                <span className="value-chip">{draft.retryDelay} с</span>
              </div>
            }
          />
          <Row
            name="Пауза между треками"
            desc="Секунды после каждого успешного трека"
            control={
              <div className="control wide" style={{ display: "flex", gap: 12 }}>
                <Slider
                  value={draft.delay}
                  min={0}
                  max={30}
                  label="Пауза между треками"
                  onChange={(value) => patch({ delay: value })}
                />
                <span className="value-chip">{draft.delay} с</span>
              </div>
            }
          />
        </Section>

        <Section icon="audio_file" title="FLAC и постобработка">
          <Row
            name="Извлекать FLAC из .m4a"
            desc="Яндекс отдаёт lossless как FLAC внутри MP4; ffmpeg перепаковывает его в .flac"
            control={
              <Switch
                checked={draft.extractFlac}
                label="Извлекать FLAC"
                onChange={(value) => patch({ extractFlac: value })}
              />
            }
          />
          <Row
            name="Старые lossy — в _old_m4a"
            desc="Чтобы пропуск готовых файлов не блокировал перекачивание"
            control={
              <Switch
                checked={draft.quarantineLossy}
                label="Старые lossy в бэкап"
                onChange={(value) => patch({ quarantineLossy: value })}
              />
            }
          />
          <Row
            name="ffmpeg"
            desc={env?.ffmpeg ? env.ffmpegPath : "Не найден — FLAC останется .m4a"}
            control={
              <Badge tone={env?.ffmpeg && env?.ffprobe ? "success" : "error"} icon="terminal">
                {env?.ffmpeg && env?.ffprobe ? "готов" : "нет"}
              </Badge>
            }
          />
          <Row
            name="Перепаковать папку сейчас"
            desc="Пройти по папке и вытащить FLAC из уже скачанных .m4a"
            control={
              <Button
                variant="tonal"
                size="compact"
                icon="build"
                disabled={!env?.ffmpeg}
                onClick={async () => {
                  try {
                    const stats = await api.postProcess();
                    toast(
                      `FLAC извлечён: ${stats.converted}, в бэкап: ${stats.quarantined}, ошибок: ${stats.failed}`,
                      stats.failed ? "error" : "success",
                    );
                  } catch (error) {
                    toast(String((error as Error).message), "error");
                  }
                }}
              >
                Запустить
              </Button>
            }
          />
        </Section>

        <Section icon="palette" title="Внешний вид">
          <Row
            name="Тема"
            desc="Системная подхватывает оформление ОС"
            control={
              <Segmented
                value={draft.theme}
                onChange={(value) => patch({ theme: value })}
                options={[
                  { value: "system", label: "Системная" },
                  { value: "light", label: "Светлая" },
                  { value: "dark", label: "Тёмная" },
                ]}
              />
            }
          />
          <Row
            name="Акцент"
            desc="Цветовая схема Material You"
            control={
              <div className="accent-row">
                {ACCENTS.map((accent) => (
                  <button
                    key={accent.value}
                    type="button"
                    title={accent.label}
                    aria-label={accent.label}
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

        <Section icon="system_update" title="Обновления">
          <Row
            name="Проверять обновления при запуске"
            desc="Раз в сессию, тихо, без сетевых ошибок в лицо"
            control={
              <Switch
                checked={draft.checkUpdates}
                label="Проверять обновления"
                onChange={(value) => patch({ checkUpdates: value })}
              />
            }
          />
          <Row
            name="Проверить сейчас"
            desc="Приложение и ffmpeg"
            control={
              <Button variant="tonal" size="compact" icon="refresh" onClick={checkUpdates} disabled={checking}>
                {checking ? "Проверяю…" : "Проверить"}
              </Button>
            }
          />
          {updates?.map((entry) => (
            <div key={entry.name} className="update-row">
              <div className="names">
                <div className="md-body">{entry.name}</div>
                <div className="md-body-sm muted">{entry.detail}</div>
              </div>
              <Badge tone="neutral">у вас {entry.installed}</Badge>
              <Badge tone={entry.newer ? "warning" : "success"}>
                {entry.newer ? `доступно ${entry.latest}` : "актуально"}
              </Badge>
              {entry.newer && entry.name === "Приложение" && (
                <Button
                  variant="tonal"
                  size="compact"
                  icon="download"
                  onClick={() => {
                    void openUrl(`https://${entry.detail}/latest`).catch((error) =>
                      toast(String((error as Error).message), "error"),
                    );
                  }}
                >
                  Скачать
                </Button>
              )}
            </div>
          ))}
        </Section>

        <Section icon="key" title="Токен и файлы">
          <Row
            name="Путь к токену"
            desc="Лежит рядом с программой: папку можно перенести целиком"
            control={<span className="path-box">{env?.tokenPath ?? "—"}</span>}
          />
          <Row
            name="Путь к настройкам"
            desc="JSON, можно править руками"
            control={<span className="path-box">{env?.configPath ?? "—"}</span>}
          />
          <Row
            name="Заменить токен"
            desc="Сменить аккаунт Яндекс.Музыки"
            control={
              <Button
                variant="danger-tonal"
                size="compact"
                icon="logout"
                onClick={async () => {
                  try {
                    await resetToken();
                  } catch (error) {
                    toast(String((error as Error).message), "error");
                  }
                }}
              >
                Сбросить токен
              </Button>
            }
          />
          <Row
            name="Версия"
            desc={env?.os ?? ""}
            control={<Badge tone="primary">{env?.appVersion ?? "—"}</Badge>}
          />
        </Section>
      </div>
    </div>
  );
}

function Section({
  icon,
  title,
  children,
}: {
  icon: string;
  title: string;
  children: ReactNode;
}) {
  return (
    <>
      <div className="section-title">
        <Icon name={icon} filled />
        {title}
      </div>
      <div className="setting-card">{children}</div>
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
