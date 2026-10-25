"use client";

import { useTranslations } from "next-intl";
import { useAppStore } from "@/lib/edition";
import { useEffect, useRef, useState } from "react";
import { useBackend, useConfig } from "@/backend/BackendProvider";
import type { AppConfig } from "@/bindings/AppConfig";
import type { Language } from "@/bindings/Language";
import type { UpdateInfo } from "@/backend/types";
import { Button, Slider, Switch } from "@/ui/controls";
import { Icon } from "@/ui/Icon";
import s from "./GeneralPage.module.css";

type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "done"; info: UpdateInfo }
  | { kind: "installing"; progress: number }
  | { kind: "error"; message: string };

const LANGUAGES: { value: Language; key: string }[] = [
  { value: "system", key: "general.langSystem" },
  { value: "zh-hant", key: "general.langZh" },
  { value: "en", key: "general.langEn" },
];

/** Settings › General (mockup/Settings Window.dc.html, T38). */
export function GeneralPage() {
  const t = useTranslations();
  const appStore = useAppStore();
  const backend = useBackend();
  const [config, , setConfig] = useConfig();
  const [volume, setVolume] = useState<number | null>(null);
  const [version, setVersion] = useState("");
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const copiedTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const [update, setUpdate] = useState<UpdateState>({ kind: "idle" });

  useEffect(
    () => backend.on("update_progress", (p) => setUpdate({ kind: "installing", progress: p })),
    [backend],
  );

  useEffect(() => {
    let alive = true;
    void backend.appInfo().then((i) => alive && setVersion(i.version));
    return () => {
      alive = false;
      clearTimeout(copiedTimer.current);
    };
  }, [backend]);

  if (!config) return null;
  const appName = t("app.name");

  /** Optimistic update, then the saved config from the app (or the error). */
  const save = async (patch: Partial<AppConfig>, call: () => Promise<AppConfig>) => {
    setError(null);
    setConfig({ ...config, ...patch });
    try {
      setConfig(await call());
    } catch (e) {
      setConfig(config);
      setError(t("general.failed", { message: String(e) }));
    }
  };

  const copyDiag = async () => {
    try {
      await backend.copyDiagnostics();
      setCopied(true);
      clearTimeout(copiedTimer.current);
      copiedTimer.current = setTimeout(() => setCopied(false), 2000);
    } catch (e) {
      setError(String(e));
    }
  };

  const shownVolume = volume ?? Math.round(config.app_volume * 100);

  const checkUpdates = async () => {
    setUpdate({ kind: "checking" });
    try {
      setUpdate({ kind: "done", info: await backend.checkForUpdates() });
    } catch (e) {
      setUpdate({ kind: "error", message: t("general.checkFailed", { message: String(e) }) });
    }
  };
  const installUpdate = async () => {
    setUpdate({ kind: "installing", progress: 0 });
    try {
      // The app restarts when this succeeds.
      await backend.installUpdate();
    } catch (e) {
      setUpdate({ kind: "error", message: t("general.installFailed", { message: String(e) }) });
    }
  };
  const available = update.kind === "done" ? update.info.available : null;
  const updateMsg =
    update.kind === "checking"
      ? t("general.checking")
      : update.kind === "installing"
        ? t("general.downloading", { pct: Math.round(update.progress * 100) })
        : update.kind === "error"
          ? update.message
          : update.kind === "done"
            ? available
              ? t("general.available", { version: available.version })
              : t("general.upToDate")
            : "";

  return (
    <div className={s.page}>
      {error && (
        <div role="alert" className={s.error}>
          <Icon name="warn" size={14} />
          <span>{error}</span>
        </div>
      )}
      <div className={s.card}>
        <div className={s.row}>
          <span className={s.grow}>{t("general.launchAtLogin")}</span>
          <Switch
            checked={config.launch_at_login}
            label={t("general.launchAtLogin")}
            onChange={(v) => void save({ launch_at_login: v }, () => backend.setLaunchAtLogin(v))}
          />
        </div>
        <div className={s.row} data-wrap="">
          <span className={s.label}>{t("general.volume")}</span>
          <div className={s.volume}>
            <Icon name="volX" size={15} style={{ color: "var(--text-faint)" }} />
            <div style={{ flex: 1 }}>
              <Slider
                value={shownVolume}
                label={t("general.volume")}
                onChange={setVolume}
                onCommit={(v) => {
                  setVolume(null);
                  void save({ app_volume: v / 100 }, () => backend.setAppVolume(v / 100));
                }}
              />
            </div>
          </div>
        </div>
        <div className={s.hint}>{t("general.volumeHint")}</div>
        <div className={s.row} data-wrap="">
          <label htmlFor="language" className={s.grow}>
            {t("general.language")}
          </label>
          <select
            id="language"
            className={s.select}
            value={config.language}
            onChange={(e) => {
              const language = e.target.value as Language;
              void save({ language }, () => backend.setLanguage(language));
            }}
          >
            {LANGUAGES.map((l) => (
              <option key={l.value} value={l.value}>
                {t(l.key)}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className={s.card}>
        {/* The App Store edition is updated by the App Store (ADR-0017). */}
        {!appStore && (
        <div className={s.row}>
          <span className={s.grow}>{t("general.autoUpdate")}</span>
          <Switch
            checked={config.auto_check_updates}
            label={t("general.autoUpdate")}
            onChange={(v) => void save({ auto_check_updates: v }, () => backend.setAutoCheckUpdates(v))}
          />
        </div>
        )}
        <div className={s.row} data-wrap="">
          <div className={s.grow}>
            <div>{t("general.version")}</div>
            <div className={s.mono}>{version && t("general.versionLine", { version })}</div>
          </div>
          {!appStore && (
            <>
          <span role="status" aria-live="polite" className={s.updateMsg} data-error={update.kind === "error" ? "" : undefined}>
            {updateMsg}
          </span>
          {available ? (
            <Button variant="primary" onClick={() => void installUpdate()}>
              {t("general.install")}
            </Button>
          ) : (
            <Button
              disabled={update.kind === "checking" || update.kind === "installing"}
              onClick={() => void checkUpdates()}
            >
              {t("general.checkUpdates")}
            </Button>
          )}
            </>
          )}
        </div>
      </div>

      <div className={s.card}>
        <div className={s.row} data-wrap="">
          <div className={s.grow} style={{ minWidth: 180 }}>
            <div>{t("general.troubleshoot")}</div>
            <div className={s.faint}>{t("general.troubleshootSub")}</div>
          </div>
          <Button onClick={() => void copyDiag()}>
            {copied ? (
              <>
                <Icon name="check" size={13} /> {t("general.copied")}
              </>
            ) : (
              t("general.copyDiag")
            )}
          </Button>
          <Button onClick={() => void backend.openLogFolder()}>{t("general.openLogs")}</Button>
        </div>
      </div>

      <div className={s.privacy}>
        <Icon name="shield" size={18} style={{ color: "var(--color-success)", flex: "none", marginTop: 1 }} />
        <p>{t(appStore ? "general.privacyStore" : "general.privacy", { appName })}</p>
      </div>
    </div>
  );
}
