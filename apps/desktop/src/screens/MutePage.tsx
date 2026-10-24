"use client";

import { useTranslations } from "next-intl";
import { useEffect, useRef, useState } from "react";
import { useBackend, useConfig, useRuntimeState } from "@/backend/BackendProvider";
import type { MuteReason } from "@/bindings/MuteReason";
import type { MuteRules } from "@/bindings/MuteRules";
import type { QuietHours } from "@/bindings/QuietHours";
import { rangeMinutes, toMinutes } from "@/lib/events";
import { Badge, Button, Switch } from "@/ui/controls";
import { Icon } from "@/ui/Icon";
import type { IconName } from "@/ui/iconPaths";
import s from "./MutePage.module.css";

type RuleId = "meeting" | "focus" | "quiet_hours_enabled" | "follow_system";

const RULES: { id: RuleId; reason: MuteReason; icon: IconName; name: string; desc: string }[] = [
  { id: "meeting", reason: "meeting", icon: "mic", name: "mute.meeting", desc: "mute.meetingDesc" },
  { id: "focus", reason: "focus", icon: "moon", name: "mute.focus", desc: "mute.focusDesc" },
  { id: "quiet_hours_enabled", reason: "quiet_hours", icon: "clock", name: "mute.quiet", desc: "mute.quietDesc" },
  { id: "follow_system", reason: "system_muted", icon: "volX", name: "mute.system", desc: "mute.systemDesc" },
];

export type Piece = { key: string; left: string; width: string; label: string; cut: "none" | "left" | "right" };

/** Timeline pieces in % of the day; a range crossing midnight is drawn as two pieces. */
export function timelinePieces(ranges: QuietHours[]): Piece[] {
  const pc = (m: number) => `${((m / 1440) * 100).toFixed(2)}%`;
  return ranges.flatMap((r, i): Piece[] => {
    const a = toMinutes(r.start);
    const b = toMinutes(r.end);
    if (b > a) return [{ key: `${i}`, left: pc(a), width: pc(b - a), label: r.label, cut: "none" }];
    return [
      { key: `${i}a`, left: pc(a), width: pc(1440 - a), label: `${r.label} →`, cut: "right" },
      { key: `${i}b`, left: "0%", width: pc(b), label: `→ ${r.label}`, cut: "left" },
    ];
  });
}

function nowMinutes() {
  const d = new Date();
  return d.getHours() * 60 + d.getMinutes();
}

export function MutePage() {
  const t = useTranslations();
  const backend = useBackend();
  const [config, , setConfig] = useConfig();
  const state = useRuntimeState();
  const [ranges, setRanges] = useState<QuietHours[] | null>(null);
  const [now, setNow] = useState(nowMinutes);
  const saveTimer = useRef<ReturnType<typeof setTimeout>>(undefined);

  useEffect(() => {
    if (config && ranges === null) setRanges(config.mute.quiet_hours);
  }, [config, ranges]);
  useEffect(() => {
    const id = setInterval(() => setNow(nowMinutes()), 30_000);
    return () => {
      clearInterval(id);
      clearTimeout(saveTimer.current);
    };
  }, []);

  if (!config || !state || ranges === null) return null;
  const rules = config.mute;
  const auth = state.focus_authorization;
  const reasons = state.mute.reasons;
  const appName = t("app.name");

  const save = async (next: MuteRules) => {
    setConfig({ ...config, mute: next });
    try {
      setConfig(await backend.setMuteRules(next));
    } catch (e) {
      console.error(e);
    }
  };

  const toggle = (id: RuleId, on: boolean) => void save({ ...rules, quiet_hours: ranges, [id]: on });

  /** Edit ranges locally; save after a pause, and only when every range is valid. */
  const editRanges = (next: QuietHours[], delay = 400) => {
    setRanges(next);
    clearTimeout(saveTimer.current);
    if (next.some((r) => r.start === r.end)) return;
    saveTimer.current = setTimeout(() => void save({ ...rules, quiet_hours: next }), delay);
  };

  const len = (r: QuietHours) => {
    const m = rangeMinutes(r.start, r.end);
    const h = Math.floor(m / 60);
    return m % 60 ? t("mute.hoursMinutes", { h, m: m % 60 }) : t("mute.hours", { h });
  };

  return (
    <div className={s.page}>
      <div role="status" aria-live="polite" className={s.banner} data-muted={reasons.length ? "" : undefined}>
        <Icon name={reasons.length ? "volX" : "vol"} size={18} style={{ color: "var(--banner-color)" }} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className={s.bannerTitle}>
            {reasons.length
              ? t("mute.mutedTitle", { reasons: reasons.map((r) => t(`reasons.${r}`)).join("、") })
              : t("mute.activeTitle")}
          </div>
          <div className={s.bannerSub}>{reasons.length ? t("mute.mutedSub") : t("mute.activeSub")}</div>
        </div>
        <span className={s.anyRule}>{t("mute.anyRule")}</span>
      </div>

      <div className={s.card}>
        {RULES.map((r) => {
          const on = rules[r.id];
          const focusDenied = r.id === "focus" && (auth === "denied" || auth === "restricted");
          const firing = on && reasons.includes(r.reason);
          return (
            <div key={r.id} className={s.rule}>
              <div className={s.ruleHead}>
                <span className={s.ruleIcon}>
                  <Icon name={r.icon} size={15} />
                </span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className={s.ruleName}>
                    <span>{t(r.name)}</span>
                    {firing && (
                      <Badge tone="warning" dot>
                        {t("mute.firing")}
                      </Badge>
                    )}
                  </div>
                  <div className={s.ruleDesc}>{t(r.desc)}</div>
                </div>
                <Switch
                  checked={on && !focusDenied}
                  disabled={focusDenied}
                  label={t(r.name)}
                  onChange={(v) => toggle(r.id, v)}
                />
              </div>
              {r.id === "focus" && on && auth === "not_determined" && (
                <div className={s.perm}>
                  <span className={s.permText}>{t("mute.focusAsk", { appName })}</span>
                  <Button variant="primary" onClick={() => void backend.requestFocusAuthorization()}>
                    {t("mute.focusAllow")}
                  </Button>
                </div>
              )}
              {focusDenied && (
                <div className={s.perm} data-tone="danger">
                  <Icon name="warn" size={15} style={{ color: "var(--color-danger)" }} />
                  <span className={s.permText}>{t("mute.focusDenied", { appName })}</span>
                  <Button onClick={() => void backend.openFocusSettings()}>{t("mute.openSettings")}</Button>
                </div>
              )}
              {r.id === "focus" && on && auth === "authorized" && state.mute.focus_unavailable && (
                <div className={s.perm} data-tone="neutral">
                  <Icon name="info" size={15} style={{ color: "var(--text-faint)" }} />
                  <span className={s.permText}>{t("mute.focusUnsigned")}</span>
                </div>
              )}
            </div>
          );
        })}
      </div>

      <section aria-labelledby="qh-title" className={s.quiet} style={{ opacity: rules.quiet_hours_enabled ? 1 : 0.5 }}>
        <div className={s.quietHead}>
          <h2 id="qh-title" className={s.quietTitle}>
            {t("mute.quietTitle")}
          </h2>
          <span className={s.faint}>{t("mute.daily")}</span>
          <span style={{ marginLeft: "auto" }}>
            <Button
              onClick={() =>
                editRanges([...ranges, { start: "09:00", end: "10:00", label: t("mute.newRange") }], 0)
              }
            >
              {t("mute.addRange")}
            </Button>
          </span>
        </div>
        <div aria-hidden="true" className={s.timeline}>
          {timelinePieces(ranges).map((p) => (
            <div key={p.key} className={s.piece} data-cut={p.cut} style={{ left: p.left, width: p.width }}>
              {p.label}
            </div>
          ))}
          <div className={s.now} style={{ left: `${(now / 1440) * 100}%` }} />
        </div>
        <div aria-hidden="true" className={s.ticks}>
          <span>00</span>
          <span>06</span>
          <span>12</span>
          <span>18</span>
          <span>24</span>
        </div>
        <div className={s.ranges}>
          {ranges.length === 0 && <p className={s.faint}>{t("mute.noRanges")}</p>}
          {ranges.map((r, i) => {
            const set = (patch: Partial<QuietHours>, delay?: number) =>
              editRanges(
                ranges.map((x, j) => (j === i ? { ...x, ...patch } : x)),
                delay,
              );
            return (
              <div key={i} className={s.range}>
                <input
                  className={s.label}
                  aria-label={t("mute.rangeName")}
                  value={r.label}
                  onChange={(e) => set({ label: e.target.value })}
                />
                <input
                  type="time"
                  className={s.time}
                  aria-label={t("mute.from")}
                  value={r.start}
                  onChange={(e) => e.target.value && set({ start: e.target.value }, 0)}
                />
                <span className={s.faint}>→</span>
                <input
                  type="time"
                  className={s.time}
                  aria-label={t("mute.to")}
                  value={r.end}
                  onChange={(e) => e.target.value && set({ end: e.target.value }, 0)}
                />
                {toMinutes(r.end) <= toMinutes(r.start) && r.start !== r.end && <Badge tone="neutral">{t("mute.overnight")}</Badge>}
                <span className={s.len}>{len(r)}</span>
                <button
                  type="button"
                  className={s.trash}
                  aria-label={t("mute.removeRange", { name: r.label })}
                  onClick={() => editRanges(ranges.filter((_, j) => j !== i), 0)}
                >
                  <Icon name="trash" size={14} />
                </button>
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
