import { useEffect, useState } from "react";
import { readText } from "@tauri-apps/plugin-clipboard-manager";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type AccountInfo } from "../lib/api";
import { useApp } from "../lib/store";
import { useT } from "../lib/i18n";
import { Badge, Button, Icon, TextField } from "../components/ui";

const TOKEN_HELP = "https://ym.marshal.dev/token/";

export default function Wizard() {
  const t = useT();
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
    readText()
      .then((text) => {
        const trimmed = (text ?? "").trim();
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
      toast(`${t("wiz.loggedIn")}: ${info.name || info.login || info.uid}`, "success");
    } catch (problem) {
      setAccount(null);
      setError(String((problem as Error).message));
    } finally {
      setBusy(null);
    }
  };

  const verify = async () => {
    if (!token.trim()) {
      setError(t("wiz.tokenEmpty"));
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
    toast(t("wiz.hello", { name: account.name || account.login || "" }), "success");
  };

  return (
    <div className="wizard">
      <div className="wizard-card">
        <div className="wizard-logo">
          <Icon name="graphic_eq" filled />
        </div>
        <h1>{t("wiz.title")}</h1>
        <p className="lede">{t("wiz.lede")}</p>

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
                {busy === "login" ? t("wiz.loggingIn") : t("wiz.login")}
              </Button>
            </div>
            <p className="wizard-note" style={{ marginTop: 10 }}>
              {t("wiz.loginHint")}
            </p>

            <div className="wizard-divider">
              <span>{t("wiz.or")}</span>
            </div>

            {manual ? (
              <>
                <TextField
                  label={t("wiz.tokenLabel")}
                  value={token}
                  type="password"
                  icon="key"
                  placeholder={t("wiz.tokenPlaceholder")}
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
                        const text = await readText();
                        if (text?.trim()) {
                          setToken(text.trim());
                          setError("");
                        }
                      } catch {
                        setError(t("wiz.clipboardBlocked"));
                      }
                    }}
                  >
                    {t("wiz.paste")}
                  </Button>
                  <Button
                    variant="text"
                    size="compact"
                    icon="open_in_new"
                    onClick={() => openUrl(TOKEN_HELP).catch(() => undefined)}
                  >
                    {t("wiz.whereToken")}
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
                    {busy === "check" ? t("wiz.verifying") : t("wiz.verify")}
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
                {t("wiz.haveToken")}
              </Button>
            )}
            {!manual && error ? <p className="wizard-error">{error}</p> : null}
          </>
        )}

        {account ? (
          <div style={{ marginTop: 22 }}>
            <Button block size="large" icon="arrow_forward" onClick={proceed}>
              {t("wiz.continue")}
            </Button>
          </div>
        ) : null}

        <p className="wizard-note">{t("wiz.tokenStored")}</p>
      </div>
    </div>
  );
}

function AccountStatus({ account }: { account: AccountInfo }) {
  const t = useT();
  const lossless = account.losslessOk;
  return (
    <div className="status-card">
      <div className="status-row">
        <span className="key">{t("wiz.account")}</span>
        <span className="value">{account.name || account.login || "—"}</span>
        <Badge tone="primary" icon="check_circle">
          {t("wiz.loggedIn")}
        </Badge>
      </div>
      <div className="status-row">
        <span className="key">{t("wiz.subscription")}</span>
        <span className="value">
          {account.hasPlus ? t("wiz.plusActive") : t("wiz.plusInactive")}
        </span>
        <Badge tone={account.hasPlus ? "success" : "error"}>
          {account.hasPlus ? "Plus" : "—"}
        </Badge>
      </div>
      <div className="status-row">
        <span className="key">{t("wiz.lossless")}</span>
        <span className="value">
          {lossless === null
            ? t("wiz.losslessUnchecked")
            : lossless
              ? t("wiz.losslessAvailable")
              : `${t("wiz.serverGives")} ${account.probeCodec || "AAC"}${
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
        <div className="wizard-explain">
          <div className="wizard-explain-title">{t("wiz.noFlacTitle")}</div>
          <p>{t("wiz.noFlacBody")}</p>
          <p>{t("wiz.noFlacHow")}</p>
          <p className="muted">{t("wiz.noFlacNote")}</p>
        </div>
      ) : null}
      {account.note ? <p className="wizard-note">{account.note}</p> : null}
    </div>
  );
}
