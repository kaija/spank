"use client";

import { useTranslations } from "next-intl";
import { useAppStore } from "@/lib/edition";
import { useEffect, useState, type ReactNode } from "react";
import { useAccelWave, useBackend, useConfig, useProfiles, useRuntimeState, useSoundIndex, useSounds } from "@/backend/BackendProvider";
import type { CalStatus } from "@/bindings/CalStatus";
import type { CalStep } from "@/bindings/CalStep";
import type { CalibrationProgress } from "@/bindings/CalibrationProgress";
import type { EventKind } from "@/bindings/EventKind";
import type { FiredEvent } from "@/bindings/FiredEvent";
import type { Profile } from "@/bindings/Profile";
import { profileName, soundName } from "@/lib/names";
import { polar, wavePoints, waveY } from "@/lib/sensors";
import { usePreview } from "@/lib/usePreview";
import { Button } from "@/ui/controls";
import { Icon } from "@/ui/Icon";
import type { IconName } from "@/ui/iconPaths";
import s from "./OnboardingWizard.module.css";

export type WizardMode = "onboarding" | "calibration";

const STEPS = 6;
const CAL_STEP = 4;
const DONE_STEP = 5;
export const CAL_STEPS: readonly CalStep[] = ["light", "medium", "hard", "typing"];
const BUILTINS: { id: string; icon: IconName }[] = [
  { id: "builtin.destruction", icon: "evOver" },
  { id: "builtin.classic", icon: "evOpen" },
];
const LID_EVENTS: readonly EventKind[] = ["lid_opened", "lid_closing", "lid_overbent"];
/** Calibration starts detecting at 0.06 g (`CAL_TRIGGER_G`); each step must beat the last by 1.2×. */
const CAL_TRIGGER_G = 0.06;
const LEVEL_RATIO = 1.2;
const WAVE_W = 500;
const WAVE_H = 110;

/** What the current calibration step shows: the engine's status once it is on this step. */
export function calStatusFor(step: CalStep, progress: CalibrationProgress | null, typingStarted: boolean): CalStatus {
  if (step === "typing" && !typingStarted) return { status: "waiting" };
  if (!progress || progress.step !== step) return { status: "waiting" };
  return progress.status;
}

/** The line a slap has to clear on the waveform (g). */
export function calTarget(step: CalStep, p: CalibrationProgress | null): number | null {
  if (step === "light") return CAL_TRIGGER_G;
  if (step === "medium") return (p?.light_g ?? CAL_TRIGGER_G) * LEVEL_RATIO;
  if (step === "hard") return (p?.medium_g ?? p?.light_g ?? CAL_TRIGGER_G) * LEVEL_RATIO;
  return null;
}

/** First-run onboarding (mockup/Onboarding.dc.html), or just its calibration steps. */
export function OnboardingWizard({ mode }: { mode: WizardMode }) {
  const t = useTranslations();
  const backend = useBackend();
  const state = useRuntimeState();
  const calibrationOnly = mode === "calibration";
  const [step, setStep] = useState(calibrationOnly ? CAL_STEP : 0);
  const [tapSkipped, setTapSkipped] = useState(false);
  const [calStep, setCalStep] = useState(0);
  const [progress, setProgress] = useState<CalibrationProgress | null>(null);
  const [typingStarted, setTypingStarted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const appName = t("app.name");
  const appStore = useAppStore();
  // The slap and calibration steps don't exist in the App Store edition (ADR-0017).
  const order = appStore ? [0, 1, 2, DONE_STEP] : Array.from({ length: STEPS }, (_, i) => i);
  const pos = order.indexOf(step);

  useEffect(() => backend.on("calibration", setProgress), [backend]);
  useEffect(() => {
    if (calibrationOnly) void backend.startCalibration();
  }, [backend, calibrationOnly]);

  const accelUnsupported = state?.caps.accel.state === "unsupported";
  const current = CAL_STEPS[calStep];
  const status = calStatusFor(current, progress, typingStarted);
  const calDone = status.status === "success";

  const goCal = (i: number, reset = false) => {
    setCalStep(i);
    if (CAL_STEPS[i] === "typing") {
      if (reset) setTypingStarted(false);
      // Recording starts on the first keystroke; a finished typing step is just shown again.
      else if (typingStarted) void backend.calibrationStep("typing", false);
      return;
    }
    void backend.calibrationStep(CAL_STEPS[i], reset);
  };

  const enterCalibration = () => {
    setStep(CAL_STEP);
    setCalStep(0);
    setTypingStarted(false);
    setProgress(null);
    void backend.startCalibration();
  };

  const finish = async () => {
    setBusy(true);
    setError(null);
    try {
      await backend.finishCalibration();
      if (calibrationOnly) await backend.completeOnboarding();
      else setStep(DONE_STEP);
    } catch (e) {
      setError(t("calibration.failed", { message: String(e) }));
    } finally {
      setBusy(false);
    }
  };

  const next = () => {
    if (step === CAL_STEP) {
      if (calStep < CAL_STEPS.length - 1) goCal(calStep + 1);
      else void finish();
      return;
    }
    if (step === DONE_STEP) {
      void backend.completeOnboarding();
      return;
    }
    setStep(order[pos + 1] ?? step + 1);
  };

  const back = () => {
    if (step === CAL_STEP) {
      if (calStep > 0) goCal(calStep - 1);
      else if (!calibrationOnly) {
        void backend.cancelCalibration();
        setStep(CAL_STEP - 1);
      }
      return;
    }
    setStep(order[pos - 1] ?? step - 1);
  };

  const canBack = calibrationOnly ? calStep > 0 : step > 0 && step < DONE_STEP;
  const showNext = step !== CAL_STEP - 1 && !(step === CAL_STEP && accelUnsupported);
  const nextLabel =
    step === 0
      ? t("onboarding.start")
      : step === DONE_STEP
        ? t("onboarding.done")
        : step === CAL_STEP
          ? calStep === CAL_STEPS.length - 1
            ? t("calibration.finish")
            : t("calibration.nextStep")
          : t("onboarding.next");
  const nextDisabled = busy || (step === CAL_STEP && !calDone);

  return (
    <div
      className={s.window}
      role="dialog"
      aria-label={calibrationOnly ? t("calibration.window") : t("onboarding.window", { appName })}
    >
      <div className={s.titlebar} data-tauri-drag-region>
        {!calibrationOnly && (
          <span className={s.stepCount} data-tauri-drag-region>
            {t("onboarding.stepCount", { n: pos + 1, total: order.length })}
          </span>
        )}
      </div>

      <div className={s.body}>
        {step === 0 && <Welcome />}
        {step === 1 && <LidStep />}
        {step === 2 && <ProfileStep />}
        {step === 3 && (
          <Centered icon="hand" title={t("onboarding.tapTitle")}>
            {accelUnsupported ? (
              <p className={s.muted}>{t("calibration.unsupported")}</p>
            ) : (
              <p className={s.muted}>{t("onboarding.tapBody")}</p>
            )}
            <div className={s.row} style={{ marginTop: 10 }}>
              <Button
                onClick={() => {
                  setTapSkipped(true);
                  setStep(DONE_STEP);
                }}
              >
                {t("onboarding.later")}
              </Button>
              {!accelUnsupported && (
                <Button variant="primary" onClick={enterCalibration}>
                  {t("onboarding.enable")}
                </Button>
              )}
            </div>
            <p className={s.faint}>{t("onboarding.tapNote")}</p>
          </Centered>
        )}
        {step === CAL_STEP &&
          (accelUnsupported ? (
            <Centered icon="warn" title={t("calibration.window")}>
              <p className={s.muted}>{t("calibration.unsupported")}</p>
            </Centered>
          ) : (
            <CalibrationStep
              calStep={calStep}
              status={status}
              progress={progress}
              typingStarted={typingStarted}
              onType={() => {
                if (typingStarted) return;
                setTypingStarted(true);
                void backend.calibrationStep("typing", true);
              }}
              onRetry={() => goCal(calStep, true)}
            />
          ))}
        {step === DONE_STEP && <Done tapSkipped={tapSkipped} />}
        {error && (
          <p role="alert" className={s.error}>
            {error}
          </p>
        )}
      </div>

      <footer className={s.footer}>
        {!calibrationOnly && (
          <div className={s.dots} aria-hidden="true">
            {order.map((i, n) => (
              <span key={i} className={s.dot} data-current={i === step ? "" : undefined} data-done={n <= pos ? "" : undefined} />
            ))}
          </div>
        )}
        <div className={s.actions}>
          {canBack && <Button onClick={back}>{t("onboarding.back")}</Button>}
          {showNext && (
            <Button variant="primary" onClick={next} disabled={nextDisabled}>
              {nextLabel}
            </Button>
          )}
        </div>
      </footer>
    </div>
  );
}

function Centered({ icon, title, children }: { icon?: IconName; title: string; children: ReactNode }) {
  return (
    <div className={s.centered}>
      {icon && (
        <span className={s.bigIcon} aria-hidden="true">
          <Icon name={icon} size={34} />
        </span>
      )}
      <h1 className={s.h1}>{title}</h1>
      {children}
    </div>
  );
}

function Welcome() {
  const t = useTranslations();
  const appStore = useAppStore();
  const appName = t("app.name");
  return (
    <div className={s.centered}>
      <div aria-hidden="true" className={s.hero}>
        <div className={s.heroScreen}>
          <span>&gt;</span>
          <span>&lt;</span>
        </div>
        <div className={s.heroBase} />
        <div className={s.heroLine} style={{ right: 6, top: 4, width: 22, transform: "rotate(-30deg)" }} />
        <div className={s.heroLine} style={{ right: 2, top: 16, width: 18, transform: "rotate(-8deg)" }} />
        <div className={s.heroLine} style={{ right: 8, top: 28, width: 14, transform: "rotate(14deg)" }} />
      </div>
      <h1 className={s.hello}>{t("onboarding.hello", { appName })}</h1>
      <p className={s.lead}>{t(appStore ? "onboarding.helloBodyStore" : "onboarding.helloBody")}</p>
      <p className={s.faint} style={{ marginTop: 6 }}>
        {t("onboarding.privacy")}
      </p>
    </div>
  );
}

function LidStep() {
  const t = useTranslations();
  const appStore = useAppStore();
  const backend = useBackend();
  const state = useRuntimeState();
  const [sounds] = useSounds();
  const index = useSoundIndex(sounds);
  const [fired, setFired] = useState<FiredEvent | null>(null);

  useEffect(
    () =>
      backend.on("trigger_fired", (f) => {
        if (LID_EVENTS.includes(f.event.kind)) setFired(f);
      }),
    [backend],
  );

  const unsupported = state?.caps.lid.state === "unsupported";
  const angle = state?.lid_running ? state.lid_angle : null;
  const [lx, ly] = polar(120, 130, 92, angle ?? 0);

  let feedback = t("onboarding.waiting");
  if (fired) {
    const r = fired.result;
    feedback =
      r.type === "played"
        ? t("onboarding.detected", { sound: soundName(t, index.get(r.sound), r.sound) })
        : t("onboarding.detectedSilent", {
            reason:
              r.type === "muted" && r.reasons.length
                ? t(`reasons.${r.reasons[0]}`)
                : r.type === "paused"
                  ? t("status.paused")
                  : "—",
          });
  }

  return (
    <div className={s.stack}>
      <h1 className={s.h1}>{t("onboarding.lidTitle")}</h1>
      <p className={s.muted}>{t("onboarding.lidBody")}</p>
      {unsupported ? (
        <div className={s.notice}>
          <Icon name="info" size={15} />
          <span>{t(appStore ? "onboarding.noLidStore" : "onboarding.noLid")}</span>
        </div>
      ) : (
        <div className={s.lidRow}>
          <svg
            viewBox="0 0 240 150"
            width="240"
            height="150"
            role="img"
            aria-label={angle === null ? t("sensors.angleUnknownAria") : t("sensors.angleAria", { deg: Math.round(angle) })}
            className={s.lidSvg}
          >
            <path d="M 216 130 A 96 96 0 0 0 24 130" className={s.lidTrack} />
            <line x1="120" y1="130" x2="230" y2="130" className={s.lidBase} />
            {angle !== null && <line x1="120" y1="130" x2={lx.toFixed(1)} y2={ly.toFixed(1)} className={s.lidLine} />}
            <circle cx="120" cy="130" r="4.5" className={s.lidHinge} />
          </svg>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div role="status" aria-live="off" className={s.bigAngle}>
              {angle === null ? "—°" : `${Math.round(angle)}°`}
            </div>
            <div className={s.small}>{t("onboarding.liveAngle")}</div>
            <div role="status" aria-live="polite" className={s.feedback} data-ok={fired ? "" : undefined}>
              <Icon name={fired ? "check" : "sensors"} size={14} strokeWidth={2} />
              <span>{feedback}</span>
            </div>
          </div>
        </div>
      )}
      <div className={s.row}>
        <Button onClick={() => void backend.previewEvent("lid_opened")}>{t("onboarding.demo")}</Button>
        <span className={s.faint}>{t("onboarding.noSoundHint")}</span>
      </div>
    </div>
  );
}

/** A profile's most representative sound for the preview button. */
function sampleSound(p: Profile): string | undefined {
  for (const k of ["slap_medium", "slap_hard", "lid_overbent", "lid_opened"] as EventKind[]) {
    const b = p.mapping[k];
    if (b?.enabled && b.sounds.length) return b.sounds[0];
  }
  return undefined;
}

function ProfileStep() {
  const t = useTranslations();
  const appStore = useAppStore();
  const backend = useBackend();
  const [profiles] = useProfiles();
  const [config, , setConfig] = useConfig();
  const [sounds] = useSounds();
  const index = useSoundIndex(sounds);
  const { playing, toggle } = usePreview();

  const cards = BUILTINS.map((b) => ({ ...b, profile: profiles?.find((p) => p.id === b.id) })).filter(
    (c): c is { id: string; icon: IconName; profile: Profile } => !!c.profile,
  );
  const pick = (id: string) => {
    if (config) setConfig({ ...config, active_profile: id });
    void backend.setActiveProfile(id);
  };

  return (
    <div className={s.stack}>
      <h1 className={s.h1}>{t("onboarding.profileTitle")}</h1>
      <p className={s.muted}>{t("onboarding.profileBody")}</p>
      <div role="radiogroup" aria-label={t("onboarding.profilesAria")} className={s.cards}>
        {cards.map(({ id, icon, profile }) => {
          const sel = config?.active_profile === id;
          const name = profileName(t, profile);
          const sound = sampleSound(profile);
          const isPlaying = playing === id;
          return (
            <div
              key={id}
              role="radio"
              aria-checked={sel}
              tabIndex={0}
              className={s.card}
              data-selected={sel ? "" : undefined}
              onClick={() => pick(id)}
              onKeyDown={(e) => {
                if (e.key === " " || e.key === "Enter") {
                  e.preventDefault();
                  pick(id);
                }
              }}
            >
              <div className={s.cardHead}>
                <span className={s.cardIcon}>
                  <Icon name={icon} size={18} />
                </span>
                <span className={s.radio}>
                  <span />
                </span>
              </div>
              <div className={s.cardName}>{name}</div>
              <div className={s.cardDesc}>{t(`onboarding.desc_${id.replace("builtin.", "")}${appStore ? "_store" : ""}`)}</div>
              <button
                type="button"
                className={s.play}
                data-playing={isPlaying ? "" : undefined}
                aria-label={t("onboarding.previewAria", { name })}
                disabled={!sound}
                onClick={(e) => {
                  e.stopPropagation();
                  toggle(id, sound ? index.get(sound) : undefined);
                }}
              >
                <Icon name={isPlaying ? "stop" : "play"} size={10} strokeWidth={2} />
                <span>{isPlaying ? t("onboarding.playing") : t("onboarding.preview")}</span>
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
}

const PILL: Record<CalStatus["status"], { key: string; icon: IconName; tone: string }> = {
  waiting: { key: "pill_waiting", icon: "clock", tone: "muted" },
  recording: { key: "pill_recording", icon: "refresh", tone: "accent" },
  success: { key: "pill_success", icon: "check", tone: "success" },
  too_weak: { key: "pill_fail", icon: "x", tone: "danger" },
  too_strong: { key: "pill_fail", icon: "x", tone: "danger" },
  timeout: { key: "pill_timeout", icon: "clock", tone: "warning" },
};

function CalibrationStep({
  calStep,
  status,
  progress,
  typingStarted,
  onType,
  onRetry,
}: {
  calStep: number;
  status: CalStatus;
  progress: CalibrationProgress | null;
  typingStarted: boolean;
  onType: () => void;
  onRetry: () => void;
}) {
  const t = useTranslations();
  const wave = useAccelWave(90);
  const step = CAL_STEPS[calStep];
  const typing = step === "typing";
  const pill = PILL[status.status];
  const title = t(`calibration.title_${step}`);
  const target = calTarget(step, progress);

  const done = (i: number) => {
    const k = CAL_STEPS[i];
    if (k === "typing") return progress?.typing_g != null;
    return progress?.[`${k}_g`] != null;
  };

  let message: { tone: string; icon: IconName; text: string } | null = null;
  if (status.status === "success") {
    message = {
      tone: "success",
      icon: "check",
      text: typing
        ? t("calibration.successTyping")
        : t("calibration.success", { level: t(`calibration.step_${step}`), g: status.peak_g.toFixed(2) }),
    };
  } else if (status.status === "too_weak" || status.status === "too_strong") {
    const kind = status.status === "too_weak" ? "tooWeak" : "tooStrong";
    const key = `calibration.${kind}_${step}`;
    message = { tone: "danger", icon: "warn", text: t.has(key) ? t(key) : t(`calibration.${kind}`) };
  } else if (status.status === "timeout") {
    message = { tone: "warning", icon: "clock", text: t("calibration.timeout") };
  }
  const canRetry = message !== null;
  const typedPct =
    status.status === "recording" ? status.progress * 100 : status.status === "success" && typing ? 100 : 0;

  return (
    <div className={s.stack} style={{ gap: 10 }}>
      <div role="list" aria-label={t("calibration.stepsLabel")} className={s.calSteps}>
        {CAL_STEPS.map((k, i) => {
          const now = i === calStep;
          const ok = done(i);
          return (
            <div key={k} role="listitem" aria-current={now ? "step" : undefined} className={s.calStep} data-now={now ? "" : undefined} data-done={ok ? "" : undefined}>
              <span className={s.calNum}>{ok ? "✓" : i + 1}</span>
              <span className={s.ellipsis}>{t(`calibration.step_${k}`)}</span>
            </div>
          );
        })}
      </div>
      <div>
        <h1 className={s.h1} style={{ margin: "4px 0 2px" }}>
          {title}
        </h1>
        <p className={s.muted}>{t(`calibration.sub_${step}`)}</p>
      </div>
      <div role="img" aria-label={t("calibration.waveAria", { title, status: t(`calibration.${pill.key}`) })} className={s.wave} data-tone={pill.tone}>
        <svg viewBox={`0 0 ${WAVE_W} ${WAVE_H}`} preserveAspectRatio="none" width="100%" height={WAVE_H} aria-hidden="true">
          {target !== null && (
            <line x1="0" x2={WAVE_W} y1={waveY(target, WAVE_H, 6)} y2={waveY(target, WAVE_H, 6)} className={s.target} />
          )}
          <polyline points={wavePoints(wave, WAVE_W, WAVE_H, 6)} className={s.wavePath} />
        </svg>
        <div className={s.pill} data-tone={pill.tone}>
          <Icon name={pill.icon} size={12} strokeWidth={2.2} />
          <span>{t(`calibration.${pill.key}`)}</span>
        </div>
      </div>
      {typing && (
        <div className={s.stack} style={{ gap: 6, flex: "none" }}>
          <input
            aria-label={t("calibration.typeAria")}
            placeholder={t("calibration.typeHere")}
            className={s.typeInput}
            autoComplete="off"
            spellCheck={false}
            onKeyDown={onType}
            // Nothing typed is kept: the field only gives the fingers something to do.
            onChange={(e) => {
              if (e.target.value.length > 60) e.target.value = "";
            }}
          />
          <div className={s.progress}>
            <div style={{ width: `${typedPct}%` }} />
          </div>
          <span className={s.faint}>{t("calibration.typeNote")}</span>
        </div>
      )}
      <div role="status" aria-live="polite" className={s.msgRow}>
        {message && (
          <div className={s.msg} data-tone={message.tone}>
            <Icon name={message.icon} size={14} strokeWidth={2.2} />
            <span>{message.text}</span>
          </div>
        )}
        {canRetry && !(typing && !typingStarted) && (
          <button type="button" className={s.retry} onClick={onRetry}>
            <Icon name="refresh" size={13} strokeWidth={2} />
            {t("calibration.retry")}
          </button>
        )}
      </div>
    </div>
  );
}

function Done({ tapSkipped }: { tapSkipped: boolean }) {
  const t = useTranslations();
  const appName = t("app.name");
  return (
    <div className={s.centered}>
      <div aria-hidden="true" className={s.menubar}>
        <div className={s.menubarStrip}>
          <span className={s.menubarApp}>
            <Icon name="app" size={15} strokeWidth={1.8} />
          </span>
          <Icon name="wifi" size={13} />
          <span>14:05</span>
        </div>
        <div className={s.livesHere}>
          <Icon name="arrowUp" size={18} strokeWidth={2} />
          <span>{t("onboarding.livesHere")}</span>
        </div>
      </div>
      <h1 className={s.h1}>{t("onboarding.doneTitle")}</h1>
      <p className={s.lead} style={{ fontSize: 13 }}>
        {t("onboarding.doneBody", { appName })}
      </p>
      {tapSkipped && <p className={s.faint}>{t("onboarding.tapSkipped")}</p>}
    </div>
  );
}
