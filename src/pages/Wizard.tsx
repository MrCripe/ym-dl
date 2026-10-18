import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type AccountInfo } from "../lib/api";
import { useApp } from "../lib/store";
import { Badge, Button, Icon, TextField } from "../components/ui";

const TOKEN_HELP = "https://ym.marshal.dev/token/";

export default function Wizard() {
  const finishWizard = useApp((state) => state.finishWizard);
  const toast = useApp((state) => state.toast);

  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [account, setAccount] = useState<AccountInfo | null>(null);

  useEffect(() => {
    // Pre-fill from the clipboard only when it looks like a token: saves a
    // paste without ever guessing wrong.
    navigator.clipboard
      ?.readText()
      .then((text) => {
        const trimmed = text.trim();
        if (trimmed.length > 30 && !trimmed.includes(" ")) setToken(trimmed);
      })
      .catch(() => undefined);
  }, []);

  const verify = async () => {
    if (!token.trim()) {
      setError("Вставьте токен — без него Яндекс.Музыка не отдаст музыку.");
      return;
    }
    setBusy(true);
    setError("");
    try {
      const info = await api.checkToken(token);
      setAccount(info);
    } catch (problem) {
      setAccount(null);
      setError(String((problem as Error).message));
    } finally {
      setBusy(false);
    }
  };

  const proceed = async () => {
    if (!account) {
      await verify();
      return;
    }
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
          Загрузчик Яндекс.Музыки. Вставьте токен — приложение проверит аккаунт и сразу
          скажет, отдаёт ли сервис lossless именно вам.
        </p>

        <TextField
          label="Токен Яндекс.Музыки"
          value={token}
          type="password"
          icon="key"
          placeholder="Вставьте токен (OAuth)"
          onChange={(value) => {
            setToken(value);
            setAccount(null);
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
                  setAccount(null);
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

        {account ? <AccountStatus account={account} /> : null}

        <div style={{ marginTop: 22 }}>
          <Button block size="large" icon={account ? "arrow_forward" : "verified"} onClick={proceed} disabled={busy}>
            {busy ? "Проверяю…" : account ? "Продолжить" : "Проверить токен"}
          </Button>
        </div>

        <p className="wizard-note">
          Токен хранится локально в файле ~/.ym-dl-token и никуда не отправляется.
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
    </div>
  );
}
