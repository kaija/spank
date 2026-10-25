"use client";

import { useFormatter, useTranslations } from "next-intl";
import { useAppStore } from "@/lib/edition";
import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { useAccelWave, useBackend, useCalibration, useConfig, useRuntimeState } from "@/backend/BackendProvider";
import type { AppConfig } from "@/bindings/AppConfig";
import type { Capability } from "@/bindings/Capability";
import type { EventKind } from "@/bindings/EventKind";
import type { LidConfig } from "@/bindings/LidConfig";
import type { SlapConfig } from "@/bindings/SlapConfig";
import type { SlapLevels } from "@/bindings/SlapLevels";
import {
  angleAt,
  arcPath,
  lidConfigValid,
  polar,
  sensitivityFactor,
  sensitivityPos,
  wavePoints,
  waveY,
  withSensitivity,
} from "@/lib/sensors";
import { Badge, Button, Slider, Switch, type Tone } from "@/ui/controls";
import { Icon } from "@/ui/Icon";
import type { IconName } from "@/ui/iconPaths";
import s from "./SensorsPage.module.css";

type Handle = "open" | "close" | "over";
const FIELD: Record<Handle, "open_min_deg" | "close_threshold_deg" | "overbend_threshold_deg"> = {
  open: "open_min_deg",
  close: "close_threshold_deg",
  over: "overbend_threshold_deg",
};
const HANDLE_COLOR: Record<Handle, string> = {
  open: "var(--color-success)",
  close: "var(--color-warning)",
  over: "var(--color-danger)",
};
const SLAPS: readonly EventKind[] = ["slap_light", "slap_medium", "slap_hard", "slap_combo"];

// Gauge geometry (mockup: viewBox 380 × 186, hinge at 170,166).
const CX = 170;
const CY = 166;
const R = 128;
const LID = 116;
// Waveform box (mockup: 500 × 96).
const WAVE_W = 500;
const WAVE_H = 96;

export function SensorsPage() {
  const t = useTranslations();
  const appStore = useAppStore();
  const backend = useBackend();
  const [config, , setConfig] = useConfig();
  const state = useRuntimeState();
  const [calibration] = useCalibration();
  const [defaults, setDefaults] = useState<AppConfig["slap"] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    void backend.appInfo().then((i) => alive && setDefaults(i.defaultConfig.slap));
    void backend.setAccelMonitor(true);
    return () => {
      alive = false;
      void backend.setAccelMonitor(false);
    };
  }, [backend]);

  if (!config || !state) return null;

  const save = async (next: AppConfig, call: () => Promise<AppConfig>) => {
    setError(null);
    setConfig(next);
    try {
      setConfig(await call());
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className={s.page}>
      {error && (
        <div role="alert" className={s.error}>
          <Icon name="warn" size={14} />
          <span>{error}</span>
        </div>
      )}
      <LidSection
        config={config}
        angle={state.lid_running ? state.lid_angle : null}
        lidCap={state.caps.lid}
        onSave={(lid) => save({ ...config, lid }, () => backend.setLidConfig(lid))}
      />
      {!appStore && (
      <TapSection
        config={config}
        accelCap={state.caps.accel}
        accelRunning={state.accel_running}
        calibrating={state.calibrating}
        inputAccess={state.input_access}
        calibratedAt={calibration?.calibrated_unix_ms ?? null}
        base={calibration?.levels ?? defaults?.levels ?? null}
        defaultTrigger={defaults?.trigger_g ?? 0.15}
        onSlap={(slap) => save({ ...config, slap }, () => backend.setSlapConfig(slap))}
        hasCalibration={!!calibration}
      />
      )}
      <section aria-labelledby="sg-cap" className={s.card}>
        <h2 id="sg-cap" className={s.capsTitle}>
          {t("sensors.capsTitle")}
        </h2>
        <CapRow icon="sensors" name={t("sensors.capLid")} sub={capSub(t, state.caps.lid, "sensors.capLidSub")} {...capBadge(t, state.caps.lid)} />
        {!appStore && (
          <>
        <CapRow icon="cpu" name={t("sensors.capAccel")} sub={t("sensors.capAccelSub")} {...capBadge(t, state.caps.accel)} />
        <CapRow
          icon="keyboard"
          name={t("sensors.capInput")}
          sub={t("sensors.capInputSub")}
          {...(state.input_access === "granted"
            ? { tone: "success" as Tone, status: t("sensors.granted") }
            : state.input_access === "denied"
              ? { tone: "danger" as Tone, status: t("sensors.denied") }
              : { tone: "neutral" as Tone, status: t("sensors.notAsked") })}
        />
          </>
        )}
      </section>
    </div>
  );
}

type T = ReturnType<typeof useTranslations>;

function capBadge(t: T, c: Capability): { tone: Tone; status: string } {
  if (c.state === "available") return { tone: "success", status: t("sensors.supported") };
  if (c.state === "unsupported") return { tone: "warning", status: t("sensors.unsupportedShort") };
  return { tone: "neutral", status: t("sensors.checking") };
}

function capSub(t: T, c: Capability, key: string): string {
  return c.state === "unsupported" ? t("sensors.capLidMissing") : t(key);
}

function CapRow({ icon, name, sub, tone, status }: { icon: IconName; name: string; sub: string; tone: Tone; status: string }) {
  return (
    <div className={s.capRow}>
      <Icon name={icon} size={16} style={{ color: "var(--text-muted)" }} />
      <div className={s.grow}>
        <div>{name}</div>
        <div className={s.faint}>{sub}</div>
      </div>
      <Badge tone={tone} dot>
        {status}
      </Badge>
    </div>
  );
}

// ---- lid angle ------------------------------------------------------------------------------

function LidSection({
  config,
  angle,
  lidCap,
  onSave,
}: {
  config: AppConfig;
  angle: number | null;
  lidCap: Capability;
  onSave: (lid: LidConfig) => void;
}) {
  const t = useTranslations();
  const appStore = useAppStore();
  const [draft, setDraft] = useState<LidConfig>(config.lid);
  const [drag, setDrag] = useState<Handle | null>(null);
  const [advanced, setAdvanced] = useState(false);
  const svg = useRef<SVGSVGElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const editing = useRef(false);

  // Follow outside changes unless the user is mid-edit.
  useEffect(() => {
    if (!editing.current) setDraft(config.lid);
  }, [config.lid]);
  useEffect(() => () => clearTimeout(timer.current), []);

  const unsupported = lidCap.state === "unsupported";
  const valid = lidConfigValid(draft);

  const edit = (next: LidConfig) => {
    editing.current = true;
    setDraft(next);
    clearTimeout(timer.current);
    if (!lidConfigValid(next)) return;
    timer.current = setTimeout(() => {
      editing.current = false;
      onSave(next);
    }, 400);
  };
  const setHandle = (h: Handle, v: number) => edit({ ...draft, [FIELD[h]]: Math.max(0, Math.min(180, Math.round(v))) });

  const onMove = (e: PointerEvent<SVGSVGElement>) => {
    const el = svg.current;
    const ctm = el?.getScreenCTM?.();
    if (!drag || !el || !ctm) return;
    const pt = new DOMPoint(e.clientX, e.clientY).matrixTransform(ctm.inverse());
    setHandle(drag, angleAt(CX, CY, pt.x, pt.y));
  };
  const onKey = (h: Handle) => (e: KeyboardEvent) => {
    const d = ({ ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1 } as Record<string, number>)[e.key];
    if (!d) return;
    e.preventDefault();
    setHandle(h, draft[FIELD[h]] + d * (e.shiftKey ? 5 : 1));
  };

  const names: Record<Handle, string> = { open: t("sensors.open"), close: t("sensors.close"), over: t("sensors.over") };
  const shown = angle ?? null;
  const stateText =
    shown === null
      ? t("sensors.stateStopped")
      : shown > draft.overbend_threshold_deg
        ? t("sensors.stateOver")
        : shown < draft.close_threshold_deg
          ? t("sensors.stateClosing")
          : t("sensors.stateNormal");
  const [lx, ly] = polar(CX, CY, LID, shown ?? 0);

  return (
    <section aria-labelledby="sg-angle" className={s.card} data-pad="">
      <div className={s.head}>
        <h2 id="sg-angle" className={s.title}>
          {t("sensors.angleTitle")}
        </h2>
        {unsupported && <Badge tone="warning">{t("sensors.unsupported")}</Badge>}
      </div>
      {unsupported ? (
        <div className={s.unsupported}>
          <span className={s.unsupportedIcon}>
            <Icon name="sensors" size={22} strokeWidth={2} />
          </span>
          <p>{t(appStore ? "sensors.unsupportedBodyStore" : "sensors.unsupportedBody")}</p>
        </div>
      ) : (
        <>
          <div className={s.gaugeRow}>
            <div className={s.gauge}>
              <svg
                ref={svg}
                viewBox="0 0 380 186"
                width="100%"
                role="img"
                aria-label={shown === null ? t("sensors.angleUnknownAria") : t("sensors.angleAria", { deg: Math.round(shown) })}
                onPointerMove={onMove}
                onPointerUp={() => setDrag(null)}
                onPointerLeave={() => setDrag(null)}
                className={s.gaugeSvg}
              >
                <path d={arcPath(CX, CY, R, 0, 180)} className={s.track} />
                <path d={arcPath(CX, CY, R, 0, Math.max(1, draft.close_threshold_deg))} className={s.zone} style={{ stroke: HANDLE_COLOR.close }} />
                <path d={arcPath(CX, CY, R, Math.min(179, draft.overbend_threshold_deg), 180)} className={s.zone} style={{ stroke: HANDLE_COLOR.over }} />
                {shown !== null && <path d={arcPath(CX, CY, 40, 0, Math.max(1, shown))} className={s.angleArc} />}
                <line x1={CX} y1={CY} x2={330} y2={CY} className={s.base} />
                {shown !== null && <line x1={CX} y1={CY} x2={lx.toFixed(1)} y2={ly.toFixed(1)} className={s.lid} />}
                <circle cx={CX} cy={CY} r={5} className={s.hinge} />
                {(["open", "close", "over"] as Handle[]).map((h) => {
                  const v = draft[FIELD[h]];
                  const [x, y] = polar(CX, CY, R, v);
                  const [tx1, ty1] = polar(CX, CY, R - 9, v);
                  const [tx2, ty2] = polar(CX, CY, R + 9, v);
                  const [labx, laby] = polar(CX, CY, R + 22, v);
                  const c = Math.cos((v * Math.PI) / 180);
                  return (
                    <g key={h}>
                      <line x1={tx1} y1={ty1} x2={tx2} y2={ty2} style={{ stroke: HANDLE_COLOR[h], strokeWidth: 2 }} />
                      <circle
                        cx={x}
                        cy={y}
                        r={8}
                        tabIndex={0}
                        role="slider"
                        aria-label={t("sensors.thresholdAria", { name: names[h] })}
                        aria-valuemin={0}
                        aria-valuemax={180}
                        aria-valuenow={v}
                        className={s.handle}
                        style={{ stroke: HANDLE_COLOR[h] }}
                        onPointerDown={(e) => {
                          e.preventDefault();
                          setDrag(h);
                        }}
                        onKeyDown={onKey(h)}
                      />
                      <text
                        x={labx}
                        y={h === "open" ? laby + 8 : h === "close" ? laby - 6 : laby}
                        className={s.handleLabel}
                        style={{ fill: HANDLE_COLOR[h], textAnchor: c > 0.3 ? "start" : c < -0.3 ? "end" : "middle" }}
                      >
                        {`${names[h]} ${v}°`}
                      </text>
                    </g>
                  );
                })}
              </svg>
            </div>
            <div className={s.readout}>
              <div role="status" aria-live="off">
                <div className={s.angle}>{shown === null ? "—°" : `${Math.round(shown)}°`}</div>
                <div className={s.stateText} data-over={shown !== null && shown > draft.overbend_threshold_deg ? "" : undefined}>
                  {stateText}
                </div>
              </div>
              {(["open", "close", "over"] as Handle[]).map((h) => (
                <div key={h} className={s.thRow}>
                  <span className={s.faintMuted}>{names[h]}</span>
                  <input
                    type="range"
                    min={0}
                    max={180}
                    value={draft[FIELD[h]]}
                    aria-label={t("sensors.thresholdAria", { name: names[h] })}
                    className={s.range}
                    onChange={(e) => setHandle(h, Number(e.target.value))}
                  />
                  <span className={s.mono}>{draft[FIELD[h]]}°</span>
                </div>
              ))}
            </div>
          </div>
          {!valid && (
            <p role="alert" className={s.invalid}>
              {t("sensors.lidInvalid")}
            </p>
          )}
          <button type="button" aria-expanded={advanced} className={s.advToggle} onClick={() => setAdvanced(!advanced)}>
            <Icon name="chevR" size={13} strokeWidth={2} style={{ transform: advanced ? "rotate(90deg)" : "none" }} />
            {t("sensors.advanced")}
          </button>
          {advanced && (
            <div className={s.advanced}>
              <div>
                <div className={s.sliderLabel}>{t("sensors.hysteresis")}</div>
                <Slider
                  value={draft.hysteresis_deg}
                  min={1}
                  max={10}
                  label={t("sensors.hysteresis")}
                  format={(v) => `${v}°`}
                  onChange={(v) => edit({ ...draft, hysteresis_deg: v })}
                />
                <div className={s.hint}>{t("sensors.hysteresisHint")}</div>
              </div>
              <div>
                <div className={s.sliderLabel}>{t("sensors.cooldown")}</div>
                <Slider
                  value={draft.cooldown_ms}
                  min={200}
                  max={3000}
                  step={100}
                  label={t("sensors.cooldown")}
                  format={(v) => `${(v / 1000).toFixed(1)} s`}
                  onChange={(v) => edit({ ...draft, cooldown_ms: v })}
                />
                <div className={s.hint}>{t("sensors.cooldownHint")}</div>
              </div>
            </div>
          )}
        </>
      )}
    </section>
  );
}

// ---- slaps ----------------------------------------------------------------------------------

function TapSection({
  config,
  accelCap,
  accelRunning,
  calibrating,
  inputAccess,
  calibratedAt,
  base,
  defaultTrigger,
  hasCalibration,
  onSlap,
}: {
  config: AppConfig;
  accelCap: Capability;
  accelRunning: boolean;
  calibrating: boolean;
  inputAccess: string;
  calibratedAt: number | null;
  base: SlapLevels | null;
  defaultTrigger: number;
  hasCalibration: boolean;
  onSlap: (slap: SlapConfig) => void;
}) {
  const t = useTranslations();
  const format = useFormatter();
  const backend = useBackend();
  const slap = config.slap;
  const on = slap.enabled;
  const wave = useAccelWave(100, on);
  const [last, setLast] = useState<EventKind | null>(null);
  const [sens, setSens] = useState<number | null>(null);
  const unsupported = accelCap.state === "unsupported";
  const appName = t("app.name");

  useEffect(
    () =>
      backend.on("trigger_fired", (f) => {
        if (SLAPS.includes(f.event.kind)) setLast(f.event.kind);
      }),
    [backend],
  );

  const toggle = (v: boolean) => {
    if (v && !hasCalibration) {
      void backend.openCalibration();
      return;
    }
    onSlap({ ...slap, enabled: v });
  };

  const togglePrecise = async (v: boolean) => {
    if (v && inputAccess !== "granted") {
      try {
        await backend.requestInputMonitoring();
      } catch (e) {
        console.error(e);
      }
    }
    onSlap({ ...slap, precise_typing: v });
  };

  const live = calibrating
    ? t("sensors.calibrating")
    : !on
      ? t("sensors.tapOff")
      : last
        ? t("sensors.tapLast", { level: t(`events.${last}`) })
        : t("sensors.tapLive");
  const lv = slap.levels;
  const levels: { key: "light" | "medium" | "hard"; g: number; color: string }[] = [
    { key: "light", g: lv.light_g, color: "var(--color-accent)" },
    { key: "medium", g: lv.medium_g, color: "var(--color-warning)" },
    { key: "hard", g: lv.hard_g, color: "var(--color-danger)" },
  ];
  const sensPos = sens ?? (base ? sensitivityPos(base, lv) : 50);

  return (
    <section aria-labelledby="sg-tap" className={s.card} data-pad="">
      <div className={s.head}>
        <h2 id="sg-tap" className={s.title}>
          {t("sensors.tapTitle")}
        </h2>
        <span className={s.faint} role="status" aria-live="polite">
          {live}
        </span>
        <span style={{ marginLeft: "auto" }}>
          <Switch checked={on} disabled={unsupported} label={t("sensors.tapSwitch")} onChange={toggle} />
        </span>
      </div>
      {unsupported ? (
        <p className={s.faint}>{t("sensors.accelUnsupported")}</p>
      ) : (
        <>
          <div className={s.wave} style={{ opacity: on ? 1 : 0.4 }} aria-hidden="true">
            <svg viewBox={`0 0 ${WAVE_W} ${WAVE_H}`} preserveAspectRatio="none" width="100%" height={WAVE_H}>
              {levels.map((l, i) => (
                <line
                  key={l.key}
                  x1={0}
                  x2={WAVE_W}
                  y1={waveY(l.g, WAVE_H)}
                  y2={waveY(l.g, WAVE_H)}
                  style={{ stroke: l.color, strokeWidth: i ? 1 : 1.5, strokeDasharray: i ? "3 4" : "6 4", opacity: i ? 0.6 : 1 }}
                />
              ))}
              <polyline points={wavePoints(on && accelRunning ? wave : wave.map(() => 0.01), WAVE_W, WAVE_H)} className={s.wavePath} />
            </svg>
            <span className={s.trigger} style={{ top: `${Number(waveY(lv.light_g, WAVE_H)) - 16}px` }}>
              {t("sensors.triggerLine")}
            </span>
          </div>
          <div className={s.levels}>
            {levels.map((l) => (
              <div key={l.key} className={s.level}>
                <span className={s.dot} style={{ background: l.color }} />
                <span>{t(`sensors.${l.key}`)}</span>
                <span className={s.mono}>{l.g.toFixed(2)} g</span>
              </div>
            ))}
            {calibratedAt !== null && (
              <span className={s.faint}>
                {t("sensors.calibratedOn", { date: format.dateTime(new Date(calibratedAt), { month: "long", day: "numeric" }) })}
              </span>
            )}
            <span style={{ marginLeft: "auto" }}>
              <Button onClick={() => void backend.openCalibration()}>
                {hasCalibration ? t("sensors.recalibrate") : t("sensors.calibrate")}
              </Button>
            </span>
          </div>
          {!hasCalibration && (
            <div className={s.notice} data-tone="warning">
              <Icon name="info" size={15} style={{ color: "var(--color-warning)" }} />
              <span>{t("sensors.notCalibrated")}</span>
            </div>
          )}
          <div className={s.sensitivity}>
            <div className={s.sliderLabel}>{t("sensors.sensitivity")}</div>
            <Slider
              value={sensPos}
              label={t("sensors.sensitivity")}
              disabled={!base}
              format={(v) => t("sensors.sensitivityValue", { x: sensitivityFactor(v).toFixed(2) })}
              onChange={setSens}
              onCommit={(v) => {
                setSens(null);
                if (base) onSlap(withSensitivity(slap, base, defaultTrigger, v));
              }}
            />
            <div className={s.hint}>{t("sensors.sensitivityHint")}</div>
          </div>
        </>
      )}
      <div className={s.precise}>
        <Icon name="keyboard" size={18} style={{ color: "var(--text-muted)", marginTop: 1, flex: "none" }} />
        <div className={s.grow}>
          <div className={s.preciseTitle}>{t("sensors.precise")}</div>
          <div className={s.faintMuted}>{t("sensors.preciseDesc")}</div>
          {slap.precise_typing && inputAccess !== "granted" && (
            <div className={s.notice} data-tone="warning">
              <Icon name="warn" size={15} style={{ color: "var(--color-warning)", flex: "none" }} />
              <span className={s.grow}>{t("sensors.precisePending", { appName })}</span>
              <Button onClick={() => void backend.openInputMonitoringSettings()}>{t("sensors.openSettings")}</Button>
            </div>
          )}
        </div>
        <Switch
          checked={slap.precise_typing}
          disabled={unsupported}
          label={t("sensors.precise")}
          onChange={(v) => void togglePrecise(v)}
        />
      </div>
    </section>
  );
}
