import type { LidConfig } from "@/bindings/LidConfig";
import type { SlapConfig } from "@/bindings/SlapConfig";
import type { SlapLevels } from "@/bindings/SlapLevels";

/** Same rules as `LidConfig::validate` (crates/spank-core/src/config.rs). */
export function lidConfigValid(l: LidConfig): boolean {
  const inRange = (v: number) => v >= 0 && v <= 180;
  if (![l.open_min_deg, l.close_threshold_deg, l.opening_start_deg, l.overbend_threshold_deg].every(inRange)) return false;
  if (l.close_threshold_deg >= l.overbend_threshold_deg) return false;
  if (l.opening_start_deg >= l.overbend_threshold_deg) return false;
  const gap = l.overbend_threshold_deg - l.close_threshold_deg;
  return l.hysteresis_deg > 0 && l.hysteresis_deg * 2 < gap;
}

/** Point on a circle around (cx, cy) at `deg` (0° = right, 90° = up). */
export function polar(cx: number, cy: number, r: number, deg: number): [number, number] {
  const a = (deg * Math.PI) / 180;
  return [cx + r * Math.cos(a), cy - r * Math.sin(a)];
}

/** SVG arc from `a0` to `a1` degrees (counter-clockwise on screen). */
export function arcPath(cx: number, cy: number, r: number, a0: number, a1: number): string {
  const [x0, y0] = polar(cx, cy, r, a0);
  const [x1, y1] = polar(cx, cy, r, a1);
  const large = a1 - a0 > 180 ? 1 : 0;
  return `M ${x0.toFixed(1)} ${y0.toFixed(1)} A ${r} ${r} 0 ${large} 0 ${x1.toFixed(1)} ${y1.toFixed(1)}`;
}

/** Angle (0–180, whole degrees) of a point relative to the pivot; below the hinge clamps to 0 / 180. */
export function angleAt(cx: number, cy: number, x: number, y: number): number {
  let a = (Math.atan2(cy - y, x - cx) * 180) / Math.PI;
  if (a < -90) a = 180;
  if (a < 0) a = 0;
  return Math.round(Math.min(180, a));
}

/**
 * Slap sensitivity as a slider position: 0–100, 50 = the calibrated (or default) levels.
 * Factor `f = 2^((pos − 50) / 50)` (0.5×–2×) divides every level, so right = easier to trigger.
 */
export function sensitivityPos(base: SlapLevels, current: SlapLevels): number {
  const f = base.light_g / current.light_g;
  return Math.round(Math.min(100, Math.max(0, 50 + 50 * Math.log2(f))));
}

export function sensitivityFactor(pos: number): number {
  return 2 ** ((pos - 50) / 50);
}

const round3 = (v: number) => Math.round(v * 1000) / 1000;

/** Slap settings at a sensitivity position; the trigger stays at or below the light level. */
export function withSensitivity(slap: SlapConfig, base: SlapLevels, defaultTrigger: number, pos: number): SlapConfig {
  const f = sensitivityFactor(pos);
  const levels = { light_g: round3(base.light_g / f), medium_g: round3(base.medium_g / f), hard_g: round3(base.hard_g / f) };
  return { ...slap, levels, trigger_g: Math.min(defaultTrigger, levels.light_g) };
}

/** Polyline points for a 0–1 g waveform in a `w` × `h` box (bottom padding `pad`). */
export function wavePoints(values: number[], w: number, h: number, pad = 4): string {
  const n = Math.max(1, values.length - 1);
  return values.map((v, i) => `${((i * w) / n).toFixed(1)},${waveY(v, h, pad)}`).join(" ");
}

export function waveY(g: number, h: number, pad = 4): string {
  return (h - pad - Math.min(1, Math.max(0, g)) * (h - 3 * pad)).toFixed(1);
}
