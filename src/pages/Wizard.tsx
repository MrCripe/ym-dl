import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type AccountInfo } from "../lib/api";
import { useApp } from "../lib/store";
import { Badge, Button, Icon, TextField } from "../components/ui";

const TOKEN_HELP = "https://ym.marshal.dev/token/";

export default function Wizard() {
  const finishWizard = useApp((state) => state.finishWizard);
  const toast = useApp((state) => state.toast);

  // The sign-in button is the main path; the token field is the fallback for
  // anyone who already has a token and does not want to log in again.
  const [manual, setManual] = useState(false);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState<"login" | "check" | null>(null);
  const [error, setError] = useState("");
  const [account, setAccount] = useState<AccountInfo | null>(null);

  useEffect(() => {
    if (!manual) return;
    // Pre-fill from the clipboard only when it looks like a token: saves a paste
    // without ever guessing wrong.
    navigator.clipboard
      ?.readText()
      .then((text) => {
        const trimmed = text.trim();
        if (trimmed.length > 30 && !trimmed.includes(" ")) setToken(trimmed);
      })
      .catch(() => undefined);
  }, [manual]);

  const login = async () => {
    setBusy("login");
    setError("");
    try {
      const info = await api.loginYandex();
      setAccount(info);
      toast(`Вход выполнен: ${info.name || info.login || info.uid}`, "success");
    } catch (problem) {
      setAccount(null);
      setError(String((problem as Error).message));
    } finally {
      setBusy(null);
    }
  };

  const verify = async () => {
    if (!token.trim()) {
      setError("Вставьте токен — без него Яндекс.Музыка не отдаст музыку.");
      return;
    }
    setBusy("check");
    setError("");
    try {
      const info = await api.checkToken(token);
      setAccount(info);
    } catch (problem) {
      setAccount(null);
      setError(String((problem as Error).message));
    } finally {
      setBusy(null);
    }
  };

  const proceed = async () => {
    if (!account) return;
    await finishWizard(account);
    toast(`Привет, ${account.name || account.login}!`, "success");
  };

  return (
    <div className="wizard">
      <div className="wizard-card">
        <div className="wizard-logo">
          <Icon name="graphic_eq" filled />
        </div>
        <h1>Добро пожаловать в ym-dl</h1>
        <p className="lede">
          Загрузчик Яндекс.Музыки. Войдите в аккаунт — приложение само получит токен и
          сразу скажет, отдаёт ли сервис lossless именно вам.
        </p>

        {account ? (
          <AccountStatus account={account} />
        ) : (
          <>
            <div style={{ marginTop: 18 }}>
              <Button
                block
                size="large"
                icon="person"
                onClick={login}
                disabled={busy !== null}
              >
                {busy === "login" ? "Жду вход в Яндекс…" : "Войти в Яндекс"}
              </Button>
            </div>
            <p className="wizard-note" style={{ marginTop: 10 }}>
              Откроется окно Яндекса. Войдите в аккаунт — окно закроется само, токен
              подставится.
            </p>

            <div className="wizard-divider">
              <span>или</span>
            </div>

            {manual ? (
              <>
                <TextField
                  label="Токен Яндекс.Музыки"
                  value={token}
                  type="password"
                  icon="key"
                  placeholder="Вставьте токен (OAuth)"
                  onChange={(value) => {
                    setToken(value);
                    setError("");
                  }}
                  onEnter={verify}
                  error={error}
                  autoFocus
                />
                <div className="wizard-links">
                  <Button
                    variant="tonal"
                    size="compact"
                    icon="content_paste"
                    onClick={async () => {
                      try {
                        const text = await navigator.clipboard.readText();
                        if (text.trim()) {
                          setToken(text.trim());
                          setError("");
                        }
                      } catch {
                        setError("Буфер обмена недоступен — вставьте токен вручную.");
                      }
                    }}
                  >
                    Вставить из буфера
                  </Button>
                  <Button
                    variant="text"
                    size="compact"
                    icon="open_in_new"
                    onClick={() => openUrl(TOKEN_HELP).catch(() => undefined)}
                  >
                    Где взять токен
                  </Button>
                </div>
                <div style={{ marginTop: 14 }}>
                  <Button
                    block
                    variant="tonal"
                    icon="verified"
                    onClick={verify}
                    disabled={busy !== null}
                  >
                    {busy === "check" ? "Проверяю…" : "Проверить токен"}
                  </Button>
                </div>
                {error ? <p className="wizard-error">{error}</p> : null}
              </>
            ) : (
              <Button
                block
                variant="outlined"
                icon="key"
                onClick={() => setManual(true)}
                disabled={busy !== null}
              >
                У меня уже есть токен
              </Button>
            )}
            {!manual && error ? <p className="wizard-error">{error}</p> : null}
          </>
        )}

        {account ? (
          <div style={{ marginTop: 22 }}>
            <Button block size="large" icon="arrow_forward" onClick={proceed}>
              Продолжить
            </Button>
          </div>
        ) : null}

        <p className="wizard-note">
          Токен хранится локально, рядом с программой, и никуда не отправляется.
        </p>
      </div>
    </div>
  );
}

function AccountStatus({ account }: { account: AccountInfo }) {
  const lossless = account.losslessOk;
  return (
    <div className="status-card">
      <div className="status-row">
        <span className="key">Аккаунт</span>
        <span className="value">
          {account.name || account.login || "—"} · UID {account.uid}
        </span>
        <Badge tone="primary" icon="check_circle">
          вошёл
        </Badge>
      </div>
      <div className="status-row">
        <span className="key">Подписка</span>
        <span className="value">{account.hasPlus ? "Яндекс Плюс активен" : "Плюс не активен"}</span>
        <Badge tone={account.hasPlus ? "success" : "error"}>{account.hasPlus ? "Plus" : "нет"}</Badge>
      </div>
      <div className="status-row">
        <span className="key">Lossless</span>
        <span className="value">
          {lossless === null
            ? "не удалось проверить"
            : lossless
              ? "доступен — сервер отдаёт FLAC"
              : `сервер отдаёт ${account.probeCodec || "AAC"}${
                  account.probeBitrate ? ` ${account.probeBitrate} kbps` : ""
                }`}
        </span>
        <Badge
          tone={lossless === null ? "neutral" : lossless ? "success" : "warning"}
          icon={lossless ? "verified" : undefined}
        >
          {lossless === null ? "?" : lossless ? "FLAC" : "AAC"}
        </Badge>
      </div>
      {lossless === false ? (
        <p className="md-body-sm muted" style={{ margin: 0 }}>
          Проверка на реальном треке: API вернул не FLAC. Скачивание пойдёт в лучшем доступном
          качестве, файлы останутся .m4a. Обычно это значит, что музыкальная подписка не
          применяется к аккаунту.
        </p>
      ) : null}
      {account.note ? (
        <p className="md-body-sm muted" style={{ margin: 0 }}>
          {account.note}
        </p>
      ) : null}
    </div>
  );
}
