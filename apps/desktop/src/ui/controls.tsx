"use client";

import { useEffect, useRef, type ButtonHTMLAttributes, type ComponentPropsWithRef, type CSSProperties, type ReactNode } from "react";
import { Icon } from "./Icon";
import type { IconName } from "./iconPaths";
import s from "./controls.module.css";

type ButtonProps = ComponentPropsWithRef<"button"> & { variant?: "default" | "primary" | "danger" };

export function Button({ variant = "default", className, type = "button", ...rest }: ButtonProps) {
  const v = variant === "primary" ? s.primary : variant === "danger" ? s.danger : "";
  return <button type={type} className={[s.btn, v, className].filter(Boolean).join(" ")} {...rest} />;
}

export function IconButton({
  icon,
  label,
  size = 15,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { icon: IconName; label: string; size?: number }) {
  return (
    <button type="button" className={s.iconBtn} aria-label={label} title={label} {...rest}>
      <Icon name={icon} size={size} />
    </button>
  );
}

export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <label className={s.switch}>
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        disabled={disabled}
        aria-label={label}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className={s.track}>
        <span className={s.knob} />
      </span>
    </label>
  );
}

/** 0–100 slider with a "NN%" readout. `onCommit` fires on release (to avoid a save per pixel). */
export function Slider({
  value,
  onChange,
  onCommit,
  label,
  disabled,
  format = (v) => `${Math.round(v)}%`,
}: {
  value: number;
  onChange: (v: number) => void;
  onCommit?: (v: number) => void;
  label: string;
  disabled?: boolean;
  format?: (v: number) => string;
}) {
  return (
    <div className={s.sliderRow}>
      <input
        type="range"
        min={0}
        max={100}
        step={1}
        value={value}
        disabled={disabled}
        aria-label={label}
        className={s.slider}
        style={{ "--fill": `${value}%` } as CSSProperties}
        onChange={(e) => onChange(Number(e.target.value))}
        onPointerUp={(e) => onCommit?.(Number((e.target as HTMLInputElement).value))}
        onKeyUp={(e) => onCommit?.(Number((e.target as HTMLInputElement).value))}
      />
      <span className={s.sliderValue}>{format(value)}</span>
    </div>
  );
}

export type Tone = "accent" | "neutral" | "success" | "warning" | "danger";

export function Badge({ tone = "accent", dot, children }: { tone?: Tone; dot?: boolean; children: ReactNode }) {
  return (
    <span className={s.badge} data-tone={tone}>
      {dot && <span className={s.badgeDot} />}
      {children}
    </span>
  );
}

/** Sound chip: play button, name, optional remove (×). */
export function Tag({
  children,
  accent,
  playing,
  onPlay,
  playLabel,
  onRemove,
  removeLabel,
}: {
  children: ReactNode;
  accent?: boolean;
  playing?: boolean;
  onPlay?: () => void;
  playLabel?: string;
  onRemove?: () => void;
  removeLabel?: string;
}) {
  return (
    <span className={s.tag} data-tone={accent || playing ? "accent" : undefined}>
      {onPlay && (
        <button type="button" className={s.tagPlay} onClick={onPlay} aria-label={playLabel}>
          <Icon name={playing ? "stop" : "play"} size={11} strokeWidth={2} />
        </button>
      )}
      <span>{children}</span>
      {onRemove && (
        <button type="button" className={s.tagRemove} onClick={onRemove} aria-label={removeLabel}>
          <Icon name="x" size={12} strokeWidth={2} />
        </button>
      )}
    </span>
  );
}

/** Confirmation sheet (mockup Dialogs). Escape cancels; focus starts on Cancel. */
export function Sheet({
  title,
  body,
  extra,
  confirmLabel,
  cancelLabel,
  onConfirm,
  onCancel,
}: {
  title: string;
  body: string;
  extra?: ReactNode;
  confirmLabel: string;
  cancelLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    cancel.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onCancel();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);
  return (
    <div className={s.scrim}>
      <div role="alertdialog" aria-modal="true" aria-labelledby="sheet-title" className={s.sheet}>
        <div className={s.sheetBody}>
          <span className={s.sheetIcon}>
            <Icon name="trash" size={18} />
          </span>
          <div style={{ flex: 1, minWidth: 0 }}>
            <h2 id="sheet-title" className={s.sheetTitle}>
              {title}
            </h2>
            <p className={s.sheetText}>{body}</p>
            {extra}
          </div>
        </div>
        <div className={s.sheetActions}>
          <Button ref={cancel} onClick={onCancel}>
            {cancelLabel}
          </Button>
          <Button variant="danger" onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </div>
      </div>
    </div>
  );
}
